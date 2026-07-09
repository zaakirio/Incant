import AppKit
import SwiftUI

/// A floating chat-head window for one session. Borderless, non-
/// activating, always-on-top. The container view captures all mouse
/// events so we can distinguish drag (reposition), click (detail
/// popover), right-click (menu), and drop-on-X (end session), and tracks
/// hover so the logo activates its brand color.
@MainActor
final class BubbleWindow: NSPanel {
    let sessionKey: String
    private let source: String
    private let client: DaemonClient
    private let container = BubbleContainerView()
    private let model: BubbleModel
    private var hosting: NSHostingView<BubbleView>
    private let popover = NSPopover()
    private var session: Session

    init(session: Session, client: DaemonClient, index: Int) {
        self.session = session
        self.sessionKey = session.key
        self.source = session.source
        self.client = client
        self.model = BubbleModel(session: session, displayName: client.displayName(session))
        self.hosting = NSHostingView(rootView: BubbleView(model: model))

        super.init(
            contentRect: NSRect(x: 0, y: 0, width: 112, height: 84),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        isOpaque = false
        backgroundColor = .clear
        hasShadow = false
        level = .floating
        collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .ignoresCycle]
        isMovableByWindowBackground = false
        hidesOnDeactivate = false

        hosting.frame = container.bounds
        hosting.autoresizingMask = [.width, .height]
        container.autoresizingMask = [.width, .height]
        container.addSubview(hosting)
        contentView = container

        popover.behavior = .transient
        popover.animates = true
        popover.contentViewController = NSHostingController(
            rootView: BubbleDetailView(client: client, key: session.key)
        )

        container.onDragBegan = { [weak self] in self?.dragBegan() }
        container.onDragMoved = { [weak self] p in self?.dragMoved(to: p) }
        container.onDragEnded = { [weak self] moved in self?.dragEnded(moved: moved) }
        container.onClick = { [weak self] in self?.togglePopover() }
        container.onRightClick = { [weak self] in self?.showMenu() }
        container.onHover = { [weak self] over in self?.model.hovering = over }

        setFrameOrigin(BubblePositions.origin(for: source, index: index, size: frame.size))
    }

    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }

    func update(_ session: Session) {
        self.session = session
        model.session = session
        model.displayName = client.displayName(session)
    }

    // MARK: interaction

    private func dragBegan() {
        popover.performClose(nil)
        KillTarget.shared.show()
    }

    private func dragMoved(to screenPoint: NSPoint) {
        setFrameOrigin(NSPoint(x: screenPoint.x - frame.width / 2, y: screenPoint.y - frame.height / 2))
        KillTarget.shared.highlight(contains: screenPoint)
    }

    private func dragEnded(moved: Bool) {
        let overKill = KillTarget.shared.frameOnScreen.contains(NSEvent.mouseLocation)
        KillTarget.shared.hide()
        if !moved {
            togglePopover()
            return
        }
        if overKill { client.kill(sessionKey) }
    }

    private func togglePopover() {
        if popover.isShown {
            popover.performClose(nil)
        } else {
            if session.unread { client.markRead(sessionKey) }
            popover.show(relativeTo: container.bounds, of: container, preferredEdge: .minY)
        }
    }

    private func showMenu() {
        let menu = NSMenu()
        menu.addItem(withTitle: model.displayName, action: nil, keyEquivalent: "").isEnabled = false
        menu.addItem(.separator())
        let rename = NSMenuItem(title: "Rename…", action: #selector(renameBubble), keyEquivalent: "")
        rename.target = self
        menu.addItem(rename)
        menu.addItem(.separator())
        for (title, value) in [("Auto", "auto"), ("Notify only", "notify"), ("Off", "off")] {
            let item = NSMenuItem(title: title, action: #selector(setBehavior(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = value
            item.state = (session.behavior == value) ? .on : .off
            menu.addItem(item)
        }
        menu.addItem(.separator())
        let hide = NSMenuItem(title: "Hide bubble", action: #selector(hideBubble), keyEquivalent: "")
        hide.target = self
        menu.addItem(hide)
        if session.canKill {
            let end = NSMenuItem(title: "End session", action: #selector(endSession), keyEquivalent: "")
            end.target = self
            menu.addItem(end)
        }
        menu.popUp(positioning: nil, at: NSPoint(x: 0, y: container.bounds.height), in: container)
    }

    @objc private func renameBubble() {
        let alert = NSAlert()
        alert.messageText = "Rename this agent"
        alert.informativeText = "Shown on the bubble and in the menu. Leave blank to use the folder name."
        let field = NSTextField(frame: NSRect(x: 0, y: 0, width: 240, height: 24))
        field.stringValue = model.displayName
        field.placeholderString = session.project
        alert.accessoryView = field
        alert.addButton(withTitle: "Save")
        alert.addButton(withTitle: "Cancel")
        NSApp.activate(ignoringOtherApps: true)
        if alert.runModal() == .alertFirstButtonReturn {
            client.rename(session, to: field.stringValue)
            model.displayName = client.displayName(session)
        }
    }

    @objc private func setBehavior(_ sender: NSMenuItem) {
        client.setSessionBehavior(sessionKey, sender.representedObject as? String)
    }
    @objc private func hideBubble() { orderOut(nil) }
    @objc private func endSession() { client.kill(sessionKey) }
}

/// Captures mouse events for the whole bubble so SwiftUI doesn't eat the
/// drag. Distinguishes click from drag by a small movement threshold and
/// reports hover via a tracking area.
private final class BubbleContainerView: NSView {
    var onDragBegan: (() -> Void)?
    var onDragMoved: ((NSPoint) -> Void)?
    var onDragEnded: ((Bool) -> Void)?
    var onClick: (() -> Void)?
    var onRightClick: (() -> Void)?
    var onHover: ((Bool) -> Void)?

    private var down: NSPoint = .zero
    private var moved = false
    private var began = false
    private var tracking: NSTrackingArea?

    override func hitTest(_ point: NSPoint) -> NSView? { self }
    override var acceptsFirstResponder: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(rect: bounds, options: [.mouseEnteredAndExited, .activeAlways], owner: self, userInfo: nil)
        addTrackingArea(area)
        tracking = area
    }

    override func mouseEntered(with event: NSEvent) { onHover?(true) }
    override func mouseExited(with event: NSEvent) { onHover?(false) }

    override func mouseDown(with event: NSEvent) {
        down = NSEvent.mouseLocation
        moved = false
        began = false
    }

    override func mouseDragged(with event: NSEvent) {
        let now = NSEvent.mouseLocation
        if !moved && hypot(now.x - down.x, now.y - down.y) < 4 { return }
        moved = true
        if !began { began = true; onDragBegan?() }
        onDragMoved?(now)
    }

    override func mouseUp(with event: NSEvent) {
        if moved { onDragEnded?(true) } else { onClick?() }
    }

    override func rightMouseDown(with event: NSEvent) { onRightClick?() }
}

/// Default bubble placement: a vertical stack down the top-right of the
/// active screen, one distinct slot per bubble. Users drag from there.
enum BubblePositions {
    static func origin(for source: String, index: Int, size: NSSize) -> NSPoint {
        let vf = (NSScreen.main ?? NSScreen.screens.first)?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1440, height: 900)
        let x = vf.maxX - size.width - 18
        let y = vf.maxY - size.height - 8 - CGFloat(index) * (size.height + 8)
        return NSPoint(x: x, y: y)
    }
}
