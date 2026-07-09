import AppKit
import Combine

/// Keeps one bubble window per active session, mirroring the daemon's
/// session list. Hidden bubbles (right-click "Hide bubble") and the
/// global overlay toggle are respected.
@MainActor
final class BubbleController {
    static let overlayDefaultsKey = "bubbles.overlayEnabled"

    private let client: DaemonClient
    private var windows: [String: BubbleWindow] = [:]
    private var sink: AnyCancellable?

    var enabled: Bool {
        didSet {
            UserDefaults.standard.set(enabled, forKey: Self.overlayDefaultsKey)
            resync()
        }
    }

    init(client: DaemonClient) {
        self.client = client
        let defaults = UserDefaults.standard
        if defaults.object(forKey: Self.overlayDefaultsKey) == nil {
            defaults.set(true, forKey: Self.overlayDefaultsKey)
        }
        self.enabled = defaults.bool(forKey: Self.overlayDefaultsKey)
    }

    func start() {
        // Re-sync on session changes and on rename, so bubble labels update.
        sink = client.$sessions
            .combineLatest(client.$names)
            .receive(on: RunLoop.main)
            .sink { [weak self] sessions, _ in self?.sync(sessions) }
    }

    private func resync() { sync(client.sessions) }

    private func sync(_ sessions: [Session]) {
        guard enabled else {
            windows.values.forEach { $0.orderOut(nil) }
            windows.removeAll()
            return
        }
        let live = Set(sessions.map(\.key))
        for (key, window) in windows where !live.contains(key) {
            window.orderOut(nil)
            windows[key] = nil
        }
        for session in sessions {
            if let window = windows[session.key] {
                window.update(session)
            } else {
                // Next free slot below the existing bubbles.
                let window = BubbleWindow(session: session, client: client, index: windows.count)
                windows[session.key] = window
                window.orderFrontRegardless()
            }
        }
    }
}
