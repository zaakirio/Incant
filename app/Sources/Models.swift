import Foundation
import SwiftUI

struct HistoryEntry: Codable, Hashable {
    let text: String
    let at: Double
}

/// What a session is doing right now, as reported by agent lifecycle hooks.
enum SessionStatus: String {
    case idle
    case working
    case awaitingApproval = "awaiting_approval"
    case awaitingInput = "awaiting_input"

    /// The agent is blocked on the user (approval or an answer).
    var needsAttention: Bool { self == .awaitingApproval || self == .awaitingInput }

    var label: String {
        switch self {
        case .idle: return "Idle"
        case .working: return "Working"
        case .awaitingApproval: return "Needs approval"
        case .awaitingInput: return "Needs input"
        }
    }
}

struct Session: Codable, Identifiable, Hashable {
    let key: String
    let source: String
    let sessionId: String
    let cwd: String
    let project: String
    let pid: Int?
    let canKill: Bool
    let behavior: String
    let behaviorOverride: String?
    let unread: Bool
    let speaking: Bool
    // Optional so snapshots from an older engine still decode.
    let status: String?
    let statusDetail: String?
    let statusSince: Double?
    let parentKey: String?
    let subagents: Int?
    let lastSeen: Double
    let lastText: String?
    let history: [HistoryEntry]

    var id: String { key }

    var sessionStatus: SessionStatus { SessionStatus(rawValue: status ?? "") ?? .idle }
    var subagentCount: Int { subagents ?? 0 }

    enum CodingKeys: String, CodingKey {
        case key, source
        case sessionId = "session_id"
        case cwd, project, pid
        case canKill = "can_kill"
        case behavior
        case behaviorOverride = "behavior_override"
        case unread, speaking
        case status
        case statusDetail = "status_detail"
        case statusSince = "status_since"
        case parentKey = "parent_key"
        case subagents
        case lastSeen = "last_seen"
        case lastText = "last_text"
        case history
    }
}

struct DaemonConfig: Codable {
    var mode: String
    var behavior: String
    var voice: String
    var speed: Double
    var maxChars: Int
    var voices: [String: String]
    var ttsModel: String
    var providerBehaviors: [String: String]?

    enum CodingKeys: String, CodingKey {
        case mode, behavior, voice, speed
        case maxChars = "max_chars"
        case voices
        case ttsModel = "tts_model"
        case providerBehaviors = "provider_behaviors"
    }
}

/// One SSE frame. Fields are optional because the daemon multiplexes
/// several event shapes over the one stream (see daemon.py).
struct DaemonEvent: Decodable {
    let type: String
    let sessions: [Session]?
    let session: Session?
    let key: String?
    let muted: Bool?
    // turn.completed
    let source: String?
    let project: String?
    let text: String?
    // session.status
    let status: String?
    let detail: String?
}

enum AgentStyle {
    /// Asset-catalog image name for a provider's logo, or nil if we don't
    /// bundle one (falls back to an SF Symbol). Template-rendered, tinted
    /// to the row's foreground color for a cohesive monochrome look.
    static let logoNames: Set<String> = [
        "claude", "codex", "opencode", "copilot", "gemini", "cursor",
        "kimi", "minimax", "deepseek", "devin", "ollama", "mistral",
    ]

    static func logo(_ source: String) -> String? {
        logoNames.contains(source) ? source : nil
    }

    static func fallbackGlyph(_ source: String) -> String { "terminal.fill" }

    static func label(_ source: String) -> String {
        switch source {
        case "claude": return "Claude Code"
        case "codex": return "Codex"
        case "opencode": return "OpenCode"
        case "kimi": return "Kimi CLI"
        default: return source.capitalized
        }
    }

    /// A provider's brand color, or nil for marks that are intrinsically
    /// monochrome (they activate to full foreground instead).
    static func brandColor(_ source: String) -> Color? {
        switch source {
        case "claude": return Color(red: 0.851, green: 0.463, blue: 0.341)   // #D97757
        case "gemini": return Color(red: 0.192, green: 0.525, blue: 1.0)     // #3186FF
        case "kimi": return Color(red: 0.996, green: 0.376, blue: 0.235)     // #FE603C
        case "minimax": return Color(red: 0.906, green: 0.208, blue: 0.384)  // #E73562
        case "mistral": return Color(red: 0.980, green: 0.322, blue: 0.059)  // #FA520F
        case "deepseek": return Color(red: 0.302, green: 0.420, blue: 0.996) // #4D6BFE
        default: return nil // codex, opencode, copilot, cursor, devin, ollama
        }
    }
}

/// Decodes a Kokoro voice id (e.g. "af_heart", "bm_george") into a
/// human-readable description. The prefix is <language><gender>.
struct VoiceInfo: Identifiable {
    let id: String
    let name: String
    let language: String
    let flag: String
    let gender: String
    let languageKey: String

    var displayName: String { "\(name) · \(gender)" }
    var subtitle: String { "\(flag) \(language), \(gender)" }
}

enum VoiceCatalog {
    private static let languages: [Character: (String, String)] = [
        "a": ("American English", "🇺🇸"),
        "b": ("British English", "🇬🇧"),
        "e": ("Spanish", "🇪🇸"),
        "f": ("French", "🇫🇷"),
        "h": ("Hindi", "🇮🇳"),
        "i": ("Italian", "🇮🇹"),
        "j": ("Japanese", "🇯🇵"),
        "p": ("Portuguese", "🇧🇷"),
        "z": ("Chinese", "🇨🇳"),
    ]

    static func parse(_ id: String) -> VoiceInfo {
        let chars = Array(id)
        let langChar = chars.first ?? "a"
        let genderChar = chars.count > 1 ? chars[1] : "f"
        let (language, flag) = languages[langChar] ?? ("Other", "🎙")
        let gender = genderChar == "m" ? "Male" : "Female"
        let rawName = id.contains("_") ? String(id.split(separator: "_", maxSplits: 1)[1]) : id
        let name = rawName.prefix(1).uppercased() + rawName.dropFirst()
        return VoiceInfo(id: id, name: name, language: language, flag: flag,
                         gender: gender, languageKey: String(langChar))
    }

    /// Voices grouped by language, in a stable, sensible order.
    static func grouped(_ ids: [String]) -> [(language: String, flag: String, voices: [VoiceInfo])] {
        let order = Array("abefhijpz")
        func rank(_ key: String) -> Int { order.firstIndex(of: Character(key)) ?? 99 }
        let infos = ids.map(parse)
        let byLang = Dictionary(grouping: infos, by: \.languageKey)
        return byLang.keys
            .sorted { rank($0) < rank($1) }
            .map { key in
                let voices = byLang[key]!.sorted { $0.name < $1.name }
                return (voices[0].language, voices[0].flag, voices)
            }
    }
}
