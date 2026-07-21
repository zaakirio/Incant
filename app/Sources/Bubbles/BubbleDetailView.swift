import SwiftUI

/// Popover shown when a bubble is clicked: recent narrations plus the
/// per-session controls.
struct BubbleDetailView: View {
    @ObservedObject var client: DaemonClient
    let key: String

    private var session: Session? { client.sessions.first { $0.key == key } }

    var body: some View {
        if let session {
            VStack(alignment: .leading, spacing: 0) {
                header(session)
                Divider()
                history(session)
                Divider()
                controls(session)
            }
            .frame(width: 300)
        } else {
            Text("Session ended.").padding()
        }
    }

    private func header(_ s: Session) -> some View {
        HStack(spacing: 8) {
            if let name = AgentStyle.logo(s.source) {
                Image(name).renderingMode(.template).resizable().scaledToFit().frame(width: 18, height: 18)
            }
            VStack(alignment: .leading, spacing: 1) {
                Text(client.displayName(s)).font(.system(size: 13, weight: .semibold)).lineLimit(1)
                HStack(spacing: 4) {
                    Text(AgentStyle.label(s.source)).font(.system(size: 11)).foregroundStyle(.secondary)
                    if let status = statusLine(s) {
                        Text("· \(status)")
                            .font(.system(size: 11))
                            .foregroundStyle(s.sessionStatus.needsAttention ? Color.orange : Color.secondary)
                            .lineLimit(1)
                    }
                }
            }
            Spacer()
        }
        .padding(.horizontal, 12).padding(.vertical, 10)
    }

    private func statusLine(_ s: Session) -> String? {
        var parts: [String] = []
        if s.sessionStatus != .idle {
            parts.append(s.statusDetail ?? s.sessionStatus.label.lowercased())
        }
        if s.subagentCount > 0 { parts.append("\(s.subagentCount) agents") }
        return parts.isEmpty ? nil : parts.joined(separator: ", ")
    }

    private func history(_ s: Session) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            if s.unread {
                Button {
                    client.markRead(s.key)
                } label: {
                    Label("Hear latest", systemImage: "speaker.wave.2.fill")
                }
                .buttonStyle(.borderless)
            }
            if s.history.isEmpty {
                Text("No narrations yet.").font(.system(size: 12)).foregroundStyle(.secondary)
            } else {
                ForEach(Array(s.history.suffix(4).enumerated()), id: \.offset) { _, entry in
                    Text(entry.text)
                        .font(.system(size: 12))
                        .lineLimit(3)
                        .foregroundStyle(.primary)
                }
                Button {
                    client.replay(s.key)
                } label: {
                    Label("Replay last", systemImage: "arrow.counterclockwise")
                }
                .buttonStyle(.borderless)
                .font(.system(size: 12))
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 12).padding(.vertical, 10)
    }

    private func controls(_ s: Session) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("Narration").font(.system(size: 12)).foregroundStyle(.secondary)
                Spacer()
                Picker("", selection: behaviorBinding(s)) {
                    Text("Auto").tag("auto")
                    Text("Notify").tag("notify")
                    Text("Off").tag("off")
                }
                .pickerStyle(.segmented).labelsHidden().frame(width: 170)
            }
            if s.canKill {
                Button(role: .destructive) {
                    client.kill(s.key)
                } label: {
                    Label("End session", systemImage: "xmark.circle")
                }
                .buttonStyle(.borderless)
                .font(.system(size: 12))
            }
        }
        .padding(.horizontal, 12).padding(.vertical, 10)
    }

    private func behaviorBinding(_ s: Session) -> Binding<String> {
        Binding(
            get: { s.behavior },
            set: { client.setSessionBehavior(s.key, $0) }
        )
    }
}
