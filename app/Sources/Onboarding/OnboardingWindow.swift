import AppKit
import SwiftUI
import Combine

/// Opens the onboarding window and keeps it refreshing live while open,
/// so dependency status updates as the user installs things.
@MainActor
final class OnboardingWindow {
    private let checker: DependencyChecker
    private let client: DaemonClient
    private var window: NSWindow?
    private var poll: Timer?

    init(client: DaemonClient) {
        self.client = client
        self.checker = DependencyChecker(client: client)
    }

    func show() {
        if window == nil {
            let host = NSHostingController(rootView: OnboardingView(checker: checker, client: client))
            let w = NSWindow(contentViewController: host)
            w.title = "Set up Incant"
            w.styleMask = [.titled, .closable]
            w.isReleasedWhenClosed = false
            w.center()
            window = w
        }
        NSApp.activate(ignoringOtherApps: true)
        window?.makeKeyAndOrderFront(nil)
        startPolling()
    }

    /// Show automatically on first run, or whenever a required piece is
    /// missing, so people aren't dropped into a silent menu bar icon.
    func showIfSetupIncomplete() {
        Task {
            await checker.recheck()
            if !checker.allRequiredOk { show() }
        }
    }

    private func startPolling() {
        poll?.invalidate()
        poll = Timer.scheduledTimer(withTimeInterval: 4.0, repeats: true) { [weak self] _ in
            guard let self, self.window?.isVisible == true else { self?.poll?.invalidate(); return }
            Task { await self.checker.recheck() }
        }
    }
}
