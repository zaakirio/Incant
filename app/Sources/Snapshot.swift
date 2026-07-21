import SwiftUI
import AppKit

/// Offscreen render of the popover with representative sample state, so
/// the layout can be checked without a running daemon or manual clicks.
@MainActor
enum Snapshot {
    static func sampleClient() -> DaemonClient {
        let client = DaemonClient()
        client.connected = true
        client.config = DaemonConfig(
            mode: "full",
            behavior: "auto",
            voice: "af_heart",
            speed: 1.1,
            maxChars: 700,
            voices: ["claude": "af_heart", "codex": "am_michael", "opencode": "bf_emma", "kimi": "bm_george"],
            ttsModel: "mlx-community/Kokoro-82M-bf16",
            providerBehaviors: nil
        )
        client.availableVoices = ["af_heart", "af_bella", "am_michael", "am_onyx", "bf_emma", "bm_george"]
        client.sessions = [
            Session(key: "claude:a1", source: "claude", sessionId: "a1",
                    cwd: "/Users/z/dev/incant", project: "incant", pid: 4201, canKill: true,
                    behavior: "auto", behaviorOverride: nil, unread: false, speaking: true,
                    status: "idle", statusDetail: nil, statusSince: 0, parentKey: nil, subagents: 0,
                    lastSeen: 0, lastText: "Refactor complete.", history: []),
            Session(key: "codex:b2", source: "codex", sessionId: "b2",
                    cwd: "/Users/z/dev/crucible", project: "crucible", pid: 4310, canKill: true,
                    behavior: "notify", behaviorOverride: "notify", unread: true, speaking: false,
                    status: "awaiting_approval", statusDetail: "wants to use Bash", statusSince: 0,
                    parentKey: nil, subagents: 0,
                    lastSeen: 0, lastText: "Tests pass.", history: []),
            Session(key: "kimi:d4", source: "kimi", sessionId: "d4",
                    cwd: "/Users/z/dev/swarm-lab", project: "swarm-lab", pid: 4402, canKill: true,
                    behavior: "auto", behaviorOverride: nil, unread: false, speaking: false,
                    status: "working", statusDetail: "running coder", statusSince: 0,
                    parentKey: nil, subagents: 5,
                    lastSeen: 0, lastText: nil, history: []),
            Session(key: "opencode:c3", source: "opencode", sessionId: "c3",
                    cwd: "/Users/z/dev/inf-eng", project: "inf-eng", pid: nil, canKill: false,
                    behavior: "off", behaviorOverride: "off", unread: false, speaking: false,
                    status: "idle", statusDetail: nil, statusSince: 0, parentKey: nil, subagents: 0,
                    lastSeen: 0, lastText: nil, history: []),
        ]
        return client
    }

    static func render(to dir: String) {
        write(PopoverRoot().environmentObject(sampleClient()), "popover.png", dir: dir)
    }

    static func renderOnboarding(to dir: String) {
        let client = sampleClient()
        let checker = DependencyChecker(client: client)
        checker.checks = [
            DependencyCheck(id: "brew", title: "Homebrew", state: .ok, detail: "/opt/homebrew/bin/brew", kind: .required),
            DependencyCheck(id: "uv", title: "uv", state: .ok, detail: "/opt/homebrew/bin/uv", kind: .required),
            DependencyCheck(id: "engine", title: "incant engine", state: .missing,
                            detail: "The narration engine", command: "uv tool install incant", kind: .required),
            DependencyCheck(id: "hooks", title: "Agent hooks", state: .unknown,
                            detail: "start the engine to check", command: "incant install", runnable: true, kind: .required),
        ]
        checker.dictation = [
            DictationOption(id: "hex", name: "Hex", detail: "Free, open-source. Parakeet / Whisper on-device.",
                            command: "brew install --cask kitlangton-hex", appNames: ["Hex"],
                            url: "https://github.com/kitlangton/Hex", installed: true, free: true),
            DictationOption(id: "wisprflow", name: "Wispr Flow", detail: "Paid. Polished dictation across apps.",
                            command: nil, appNames: ["Wispr Flow"], url: "https://wisprflow.ai", installed: false, free: false),
        ]
        write(OnboardingView(checker: checker, client: client), "onboarding.png", dir: dir)
    }

    private static func write(_ view: some View, _ name: String, dir: String) {
        let renderer = ImageRenderer(content: view.background(Color(nsColor: .windowBackgroundColor)))
        renderer.scale = 2
        guard let image = renderer.nsImage,
              let tiff = image.tiffRepresentation,
              let rep = NSBitmapImageRep(data: tiff),
              let png = rep.representation(using: .png, properties: [:]) else { return }
        try? png.write(to: URL(fileURLWithPath: dir).appendingPathComponent(name))
    }
}
