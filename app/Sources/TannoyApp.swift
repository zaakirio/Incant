import SwiftUI
import AppKit
import Combine

@main
struct IncantApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate

    var body: some Scene {
        Settings { EmptyView() }
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    static let client = DaemonClient()
    static let bubbles = BubbleController(client: client)
    static let onboarding = OnboardingWindow(client: client)

    private var statusItem: NSStatusItem!
    private let popover = NSPopover()
    private var sinks: Set<AnyCancellable> = []

    func applicationDidFinishLaunching(_ notification: Notification) {
        statusItem = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        if let button = statusItem.button {
            button.target = self
            button.action = #selector(togglePopover)
        }
        updateIcon()

        // Icon reflects live state: muted, disconnected, or unread.
        Self.client.$muted.combineLatest(Self.client.$connected, Self.client.$sessions)
            .receive(on: RunLoop.main)
            .sink { [weak self] _, _, _ in self?.updateIcon() }
            .store(in: &sinks)

        let root = PopoverRoot().environmentObject(Self.client)
        let host = NSHostingController(rootView: root)
        host.sizingOptions = .preferredContentSize
        popover.contentViewController = host
        popover.behavior = .transient
        popover.animates = true

        if handleSnapshotIfNeeded() { return }
        Self.client.start()
        Self.bubbles.start()
        if CommandLine.arguments.contains("--show-onboarding") {
            Self.onboarding.show()
        } else {
            Self.onboarding.showIfSetupIncomplete()
        }
    }

    /// `Incant --snapshot <dir>`: render the popover with sample state to
    /// PNG and exit. Used for pixel-checking the layout headlessly.
    private func handleSnapshotIfNeeded() -> Bool {
        let args = CommandLine.arguments
        if let i = args.firstIndex(of: "--snapshot-onboarding"), args.count > i + 1 {
            Snapshot.renderOnboarding(to: args[i + 1])
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { NSApp.terminate(nil) }
            return true
        }
        guard let i = args.firstIndex(of: "--snapshot"), args.count > i + 1 else { return false }
        let dir = args[i + 1]
        Snapshot.render(to: dir)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { NSApp.terminate(nil) }
        return true
    }

    private func updateIcon() {
        guard let button = statusItem.button else { return }
        // Custom Incant wand glyph as a template image (adapts to light/dark
        // and the system's active tint); state is shown via opacity.
        let icon = NSImage(named: "IncantIcon") ?? NSImage(systemSymbolName: "waveform", accessibilityDescription: "Incant")
        icon?.isTemplate = true
        // Fill the menu-bar height, preserving the wand's tall aspect (snug, like Hex).
        if let natural = icon?.size, natural.height > 0 {
            let h: CGFloat = 19
            icon?.size = NSSize(width: (h * natural.width / natural.height).rounded(), height: h)
        }
        button.image = icon
        button.alphaValue = !Self.client.connected ? 0.35 : (Self.client.muted ? 0.45 : 1.0)
        button.toolTip = !Self.client.connected ? "Incant — engine offline"
            : (Self.client.muted ? "Incant — muted" : "Incant")
    }

    @objc private func togglePopover() {
        guard let button = statusItem.button else { return }
        if popover.isShown {
            popover.performClose(nil)
        } else {
            NSApp.activate(ignoringOtherApps: true)
            popover.show(relativeTo: button.bounds, of: button, preferredEdge: .minY)
        }
    }
}

// Explicit hairline: structural Divider() misplaces under NSHostingView
// preferred-size layout (learned from Datesy).
struct HDivider: View {
    var body: some View {
        Rectangle().fill(Color(nsColor: .separatorColor)).frame(height: 1)
    }
}
