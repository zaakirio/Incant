import Foundation
import AppKit

enum CheckState { case ok, missing, unknown, checking }
enum CheckKind { case required, recommended, optional }

struct DependencyCheck: Identifiable {
    let id: String
    var title: String
    var state: CheckState
    var detail: String
    var command: String?        // shell command to install (copyable)
    var runnable: Bool = false  // the app can run this itself
    var kind: CheckKind = .required
}

/// A speech-to-text option the user can pair. incant is STT-agnostic;
/// these are suggestions, none required.
struct DictationOption: Identifiable {
    let id: String
    let name: String
    let detail: String
    let command: String?
    let appNames: [String]      // bundle display names to detect in /Applications
    let url: String
    var installed: Bool = false
    var free: Bool
}

@MainActor
final class DependencyChecker: ObservableObject {
    @Published var checks: [DependencyCheck] = []
    @Published var dictation: [DictationOption] = []
    @Published var lastChecked = Date.distantPast

    private let client: DaemonClient

    init(client: DaemonClient) {
        self.client = client
    }

    var allRequiredOk: Bool {
        checks.filter { $0.kind == .required }.allSatisfy { $0.state == .ok }
    }

    // MARK: shell helpers

    /// Run a command through a login shell so PATH matches the terminal
    /// (GUI apps otherwise get a minimal PATH and miss brew/uv/incant).
    private func loginShell(_ command: String) async -> (Int32, String) {
        await withCheckedContinuation { cont in
            DispatchQueue.global().async {
                let proc = Process()
                proc.executableURL = URL(fileURLWithPath: "/bin/zsh")
                proc.arguments = ["-lc", command]
                let pipe = Pipe()
                proc.standardOutput = pipe
                proc.standardError = pipe
                do { try proc.run() } catch { cont.resume(returning: (127, "")); return }
                let data = pipe.fileHandleForReading.readDataToEndOfFile()
                proc.waitUntilExit()
                cont.resume(returning: (proc.terminationStatus, String(data: data, encoding: .utf8) ?? ""))
            }
        }
    }

    private func which(_ tool: String) async -> String? {
        let (code, out) = await loginShell("command -v \(tool)")
        let path = out.trimmingCharacters(in: .whitespacesAndNewlines)
        return code == 0 && !path.isEmpty ? path : nil
    }

    private func appInstalled(_ names: [String]) -> Bool {
        let dirs = ["/Applications", ("~/Applications" as NSString).expandingTildeInPath]
        for dir in dirs {
            for name in names {
                if FileManager.default.fileExists(atPath: "\(dir)/\(name).app") { return true }
            }
        }
        return false
    }

    // MARK: checks

    func recheck() async {
        var result: [DependencyCheck] = []

        let brew = await which("brew")
        result.append(DependencyCheck(
            id: "brew", title: "Homebrew", state: brew != nil ? .ok : .missing,
            detail: brew ?? "Package manager used to install the rest",
            command: brew == nil ? #"/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)""# : nil,
            kind: .required))

        let uv = await which("uv")
        result.append(DependencyCheck(
            id: "uv", title: "uv", state: uv != nil ? .ok : .missing,
            detail: uv ?? "Installs the incant engine",
            command: uv == nil ? "brew install uv" : nil,
            kind: .required))

        let incant = await which("incant")
        let daemonUp = await pingDaemon()
        let engineOk = incant != nil || daemonUp
        result.append(DependencyCheck(
            id: "engine", title: "incant engine", state: engineOk ? .ok : .missing,
            detail: incant ?? (daemonUp ? "running" : "The narration engine"),
            command: engineOk ? nil : "uv tool install incant",
            kind: .required))

        // Hook + model status comes from the daemon when it is reachable.
        if daemonUp, let doctor = await fetchDoctor() {
            let hooksOk = doctor.filter { ["claude", "codex", "opencode"].contains($0.id) }.contains { $0.ok }
            result.append(DependencyCheck(
                id: "hooks", title: "Agent hooks", state: hooksOk ? .ok : .missing,
                detail: hooksOk ? "wired into your coding tools" : "not wired yet",
                command: hooksOk ? nil : "incant install",
                runnable: !hooksOk, kind: .required))
            if let model = doctor.first(where: { $0.id == "model" }) {
                result.append(DependencyCheck(
                    id: "model", title: "Voice model", state: model.ok ? .ok : .unknown,
                    detail: model.ok ? "downloaded" : "downloads on first narration (~300 MB)",
                    kind: .recommended))
            }
        } else {
            result.append(DependencyCheck(
                id: "hooks", title: "Agent hooks", state: .unknown,
                detail: "start the engine to check",
                command: "incant install", kind: .required))
        }

        checks = result
        dictation = await checkDictation()
        lastChecked = Date()
    }

    private func pingDaemon() async -> Bool {
        var req = URLRequest(url: URL(string: "http://127.0.0.1:\(client.port)/health")!)
        req.timeoutInterval = 2
        guard let (_, resp) = try? await URLSession.shared.data(for: req),
              let http = resp as? HTTPURLResponse else { return false }
        return http.statusCode == 200
    }

    private struct DoctorCheck: Decodable { let id: String; let ok: Bool }
    private func fetchDoctor() async -> [DoctorCheck]? {
        struct Response: Decodable { let checks: [DoctorCheck] }
        var req = URLRequest(url: URL(string: "http://127.0.0.1:\(client.port)/doctor")!)
        req.timeoutInterval = 5
        guard let (data, _) = try? await URLSession.shared.data(for: req),
              let resp = try? JSONDecoder().decode(Response.self, from: data) else { return nil }
        return resp.checks
    }

    private func checkDictation() async -> [DictationOption] {
        var options = [
            DictationOption(id: "hex", name: "Hex",
                            detail: "Free, open-source. Parakeet / Whisper on-device.",
                            command: "brew install --cask kitlangton-hex",
                            appNames: ["Hex"], url: "https://github.com/kitlangton/Hex", free: true),
            DictationOption(id: "superwhisper", name: "superwhisper",
                            detail: "Freemium. Local + cloud models.",
                            command: "brew install --cask superwhisper",
                            appNames: ["superwhisper"], url: "https://superwhisper.com", free: true),
            DictationOption(id: "wisprflow", name: "Wispr Flow",
                            detail: "Paid. Polished dictation across apps.",
                            command: nil,
                            appNames: ["Wispr Flow", "Flow"], url: "https://wisprflow.ai", free: false),
            DictationOption(id: "macwhisper", name: "MacWhisper",
                            detail: "Freemium. Whisper-based transcription.",
                            command: "brew install --cask macwhisper",
                            appNames: ["MacWhisper"], url: "https://goodsnooze.gumroad.com/l/macwhisper", free: true),
        ]
        for i in options.indices {
            options[i].installed = appInstalled(options[i].appNames)
        }
        return options
    }

    // MARK: actions

    func run(_ check: DependencyCheck) {
        guard let command = check.command, check.runnable else { return }
        Task {
            _ = await loginShell(command)
            await recheck()
        }
    }
}
