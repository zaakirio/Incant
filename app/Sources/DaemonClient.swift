import Foundation
import Combine

/// Talks to the incant daemon over its HTTP + SSE contract. The daemon
/// is the engine and single source of truth; this app is a client that
/// renders live state and sends control commands. It never touches the
/// TTS server or config file directly.
@MainActor
final class DaemonClient: ObservableObject {
    @Published var sessions: [Session] = []
    @Published var muted = false
    @Published var connected = false
    @Published var config: DaemonConfig?
    @Published var availableVoices: [String] = []
    @Published var names: [String: String] = [:]

    let port: Int
    private var streamTask: Task<Void, Never>?
    private static let namesKey = "session.names"

    init(port: Int = 5111) {
        self.port = port
        if let data = UserDefaults.standard.data(forKey: Self.namesKey),
           let decoded = try? JSONDecoder().decode([String: String].self, from: data) {
            names = decoded
        }
    }

    // MARK: custom names (UI-only; keyed by provider+cwd so they persist)

    private func nameKey(_ s: Session) -> String { "\(s.source)|\(s.cwd)" }

    func displayName(_ s: Session) -> String {
        let custom = names[nameKey(s)]
        return (custom?.isEmpty == false) ? custom! : s.project
    }

    func rename(_ s: Session, to name: String) {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty { names[nameKey(s)] = nil } else { names[nameKey(s)] = trimmed }
        if let data = try? JSONEncoder().encode(names) {
            UserDefaults.standard.set(data, forKey: Self.namesKey)
        }
    }

    private func url(_ path: String) -> URL {
        let encoded = path.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? path
        return URL(string: "http://127.0.0.1:\(port)/\(encoded)")!
    }

    // MARK: lifecycle

    func start() {
        streamTask?.cancel()
        streamTask = Task { await self.runStream() }
        Task { await self.refreshConfig() }
        Task { await self.refreshVoices() }
    }

    func stop() { streamTask?.cancel() }

    private func runStream() async {
        while !Task.isCancelled {
            do {
                var req = URLRequest(url: url("events"))
                req.timeoutInterval = .infinity
                req.setValue("text/event-stream", forHTTPHeaderField: "Accept")
                let (bytes, response) = try await URLSession.shared.bytes(for: req)
                guard let http = response as? HTTPURLResponse, http.statusCode == 200 else {
                    throw URLError(.badServerResponse)
                }
                connected = true
                await refreshConfig()
                Task { await refreshVoices() }
                for try await line in bytes.lines {
                    guard line.hasPrefix("data: ") else { continue }
                    handle(String(line.dropFirst(6)))
                }
            } catch {
                // fall through to reconnect
            }
            connected = false
            if Task.isCancelled { break }
            try? await Task.sleep(nanoseconds: 1_500_000_000)
        }
    }

    private func handle(_ json: String) {
        guard let data = json.data(using: .utf8),
              let event = try? JSONDecoder().decode(DaemonEvent.self, from: data) else { return }
        switch event.type {
        case "snapshot":
            sessions = (event.sessions ?? []).sorted(by: sortOrder)
            muted = event.muted ?? false
        case "session.updated":
            if let s = event.session { upsert(s) }
        case "session.removed":
            if let key = event.key {
                sessions.removeAll { $0.key == key }
                NotificationManager.shared.withdrawAttention(sessionKey: key)
            }
        case "mute.changed":
            muted = event.muted ?? muted
        case "turn.completed":
            if let key = event.key {
                NotificationManager.shared.turnCompleted(
                    sessionKey: key,
                    agent: AgentStyle.label(event.source ?? "agent"),
                    project: friendlyName(key: key, fallback: event.project),
                    text: event.text
                )
            }
        case "session.status":
            if let key = event.key, let raw = event.status,
               let status = SessionStatus(rawValue: raw) {
                if status.needsAttention {
                    let session = sessions.first { $0.key == key }
                    // The key is "source:session_id"; the status event can
                    // precede the session snapshot, so fall back to the key.
                    let source = session?.source ?? String(key.split(separator: ":").first ?? "agent")
                    NotificationManager.shared.needsAttention(
                        sessionKey: key,
                        agent: AgentStyle.label(source),
                        project: friendlyName(key: key, fallback: session?.project),
                        status: status,
                        detail: event.detail
                    )
                } else {
                    // Resumed or finished: the banner is stale, pull it.
                    NotificationManager.shared.withdrawAttention(sessionKey: key)
                }
            }
        default:
            break
        }
    }

    /// The user's custom session name when set, else the project folder.
    private func friendlyName(key: String, fallback: String?) -> String {
        if let session = sessions.first(where: { $0.key == key }) {
            return displayName(session)
        }
        return fallback ?? "?"
    }

    private func upsert(_ s: Session) {
        if let i = sessions.firstIndex(where: { $0.key == s.key }) {
            sessions[i] = s
        } else {
            sessions.append(s)
        }
        sessions.sort(by: sortOrder)
    }

    private func sortOrder(_ a: Session, _ b: Session) -> Bool {
        if a.project != b.project { return a.project < b.project }
        return a.source < b.source
    }

    // MARK: reads

    func refreshConfig() async {
        guard let data = try? await get("config"),
              let cfg = try? JSONDecoder().decode(DaemonConfig.self, from: data) else { return }
        config = cfg
    }

    func refreshVoices() async {
        struct VoicesResponse: Decodable { let voices: [String] }
        // The TTS server warms up after the daemon starts, so /voices can
        // be empty for the first several seconds. Retry until populated.
        for _ in 0..<20 {
            if let data = try? await get("voices"),
               let resp = try? JSONDecoder().decode(VoicesResponse.self, from: data),
               !resp.voices.isEmpty {
                availableVoices = resp.voices
                return
            }
            try? await Task.sleep(nanoseconds: 3_000_000_000)
        }
    }

    private func get(_ path: String) async throws -> Data {
        let (data, _) = try await URLSession.shared.data(from: url(path))
        return data
    }

    // MARK: commands

    private func post(_ path: String, body: [String: Any]? = nil) {
        Task {
            var req = URLRequest(url: url(path))
            req.httpMethod = "POST"
            if let body {
                req.setValue("application/json", forHTTPHeaderField: "Content-Type")
                req.httpBody = try? JSONSerialization.data(withJSONObject: body)
            }
            _ = try? await URLSession.shared.data(for: req)
        }
    }

    func toggleMute() { muted ? post("unmute") : post("mute") }
    func skip() { post("skip") }

    func setSessionBehavior(_ key: String, _ behavior: String?) {
        post("sessions/\(key)/behavior", body: ["behavior": behavior as Any])
    }
    func markRead(_ key: String) { post("sessions/\(key)/read") }
    func replay(_ key: String) { post("sessions/\(key)/replay") }
    func kill(_ key: String) { post("sessions/\(key)/kill") }

    func setConfig(_ patch: [String: Any]) {
        post("config", body: patch)
        Task {
            try? await Task.sleep(nanoseconds: 200_000_000)
            await refreshConfig()
        }
    }

    func audition(voice: String) {
        post("say", body: ["text": "This is the \(voice) voice.", "voice": voice])
    }

    // MARK: engine bootstrap

    /// Start the daemon via a login shell so PATH resolves the installed
    /// `incant` binary (uv tool / pipx / pip install location).
    func startEngine() {
        let proc = Process()
        proc.executableURL = URL(fileURLWithPath: "/bin/zsh")
        proc.arguments = ["-lc", "incant serve >/dev/null 2>&1 &"]
        try? proc.run()
    }
}
