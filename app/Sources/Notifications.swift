import Foundation
import UserNotifications

/// Posts macOS notifications for agent lifecycle moments: a finished
/// turn, and an agent blocked waiting on the user (approval or input).
/// Each kind can be toggled off in the popover; attention notifications
/// are withdrawn automatically once the agent resumes working.
@MainActor
final class NotificationManager: NSObject, UNUserNotificationCenterDelegate {
    static let shared = NotificationManager()

    static let turnCompleteKey = "notify.turnComplete"
    static let attentionKey = "notify.attention"

    private var authorized = false

    func setup() {
        // Snapshot tests and other unbundled runs have no notification
        // center; requesting one would crash.
        guard Bundle.main.bundleIdentifier != nil else { return }
        let center = UNUserNotificationCenter.current()
        center.delegate = self
        center.requestAuthorization(options: [.alert, .sound]) { granted, _ in
            Task { @MainActor in self.authorized = granted }
        }
        UserDefaults.standard.register(defaults: [
            Self.turnCompleteKey: true,
            Self.attentionKey: true,
        ])
    }

    var turnCompleteEnabled: Bool { UserDefaults.standard.bool(forKey: Self.turnCompleteKey) }
    var attentionEnabled: Bool { UserDefaults.standard.bool(forKey: Self.attentionKey) }

    func turnCompleted(sessionKey: String, agent: String, project: String, text: String?) {
        guard authorized, turnCompleteEnabled else { return }
        let content = UNMutableNotificationContent()
        content.title = "\(agent) finished in \(project)"
        if let text, !text.isEmpty {
            content.body = String(text.prefix(180))
        }
        // Silent: the narration itself is the sound.
        deliver(content, id: "turn-\(sessionKey)")
        // A finished turn supersedes any stale "needs attention" banner.
        withdrawAttention(sessionKey: sessionKey)
    }

    func needsAttention(sessionKey: String, agent: String, project: String,
                        status: SessionStatus, detail: String?) {
        guard authorized, attentionEnabled else { return }
        let content = UNMutableNotificationContent()
        content.title = "\(agent) \(status == .awaitingApproval ? "needs approval" : "needs input") — \(project)"
        if let detail, !detail.isEmpty {
            content.body = String(detail.prefix(180))
        }
        content.sound = .default
        deliver(content, id: "attention-\(sessionKey)")
    }

    func withdrawAttention(sessionKey: String) {
        guard Bundle.main.bundleIdentifier != nil else { return }
        let center = UNUserNotificationCenter.current()
        center.removeDeliveredNotifications(withIdentifiers: ["attention-\(sessionKey)"])
        center.removePendingNotificationRequests(withIdentifiers: ["attention-\(sessionKey)"])
    }

    private func deliver(_ content: UNMutableNotificationContent, id: String) {
        // A stable id per session+kind replaces rather than stacks banners.
        let request = UNNotificationRequest(identifier: id, content: content, trigger: nil)
        UNUserNotificationCenter.current().add(request)
    }

    // Menu-bar apps count as "foreground", which by default swallows
    // banners; show them anyway.
    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        completionHandler([.banner, .sound])
    }
}
