import AppKit
import SwiftUI

/// The drop-to-end target shown at bottom center while dragging a bubble.
/// Release a bubble over it to terminate that session.
@MainActor
final class KillTarget {
    static let shared = KillTarget()

    private var window: NSPanel?
    private let hovered = HoverState()

    var frameOnScreen: NSRect { window?.frame ?? .zero }

    func show() {
        if window == nil { build() }
        hovered.hot = false
        window?.orderFrontRegardless()
    }

    func hide() { window?.orderOut(nil) }

    func highlight(contains point: NSPoint) {
        hovered.hot = frameOnScreen.contains(point)
    }

    private func build() {
        let size = NSSize(width: 68, height: 68)
        let vf = (NSScreen.main ?? NSScreen.screens.first)?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1440, height: 900)
        let origin = NSPoint(x: vf.midX - size.width / 2, y: vf.minY + 40)
        let panel = NSPanel(
            contentRect: NSRect(origin: origin, size: size),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = false
        panel.level = .floating
        panel.ignoresMouseEvents = true
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.contentView = NSHostingView(rootView: KillTargetView().environmentObject(hovered))
        window = panel
    }
}

final class HoverState: ObservableObject {
    @Published var hot = false
}

private struct KillTargetView: View {
    @EnvironmentObject var state: HoverState
    var body: some View {
        ZStack {
            Circle()
                .fill(state.hot ? Color.red : Color.black.opacity(0.55))
                .overlay(Circle().strokeBorder(Color.white.opacity(0.85), lineWidth: state.hot ? 3 : 1.5))
            Image(systemName: "xmark")
                .font(.system(size: 22, weight: .bold))
                .foregroundStyle(.white)
        }
        .frame(width: 68, height: 68)
        .scaleEffect(state.hot ? 1.12 : 1.0)
        .animation(.spring(response: 0.25, dampingFraction: 0.7), value: state.hot)
    }
}
