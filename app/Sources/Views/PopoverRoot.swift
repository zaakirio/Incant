import SwiftUI
import AppKit

private let POPOVER_WIDTH: CGFloat = 390

struct PopoverRoot: View {
    @EnvironmentObject var client: DaemonClient

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HeaderBar()
            HDivider()
            if client.connected {
                SessionsSection()
                HDivider()
                VoiceSection()
                HDivider()
                DefaultsSection()
            } else {
                DisconnectedView()
            }
            HDivider()
            FooterBar()
        }
        .frame(width: POPOVER_WIDTH)
    }
}

// MARK: - Header

private struct HeaderBar: View {
    @EnvironmentObject var client: DaemonClient

    var body: some View {
        HStack(spacing: 8) {
            Circle()
                .fill(statusColor)
                .frame(width: 8, height: 8)
            Text(statusText)
                .font(.system(size: 13, weight: .semibold))
            Spacer()
            if client.connected {
                Button(action: client.toggleMute) {
                    Image(systemName: client.muted ? "speaker.slash.fill" : "speaker.wave.2.fill")
                }
                .help(client.muted ? "Unmute" : "Mute")
                Button(action: client.skip) {
                    Image(systemName: "forward.fill")
                }
                .help("Skip current narration")
            }
        }
        .buttonStyle(.borderless)
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
    }

    private var statusColor: Color {
        if !client.connected { return .secondary }
        return client.muted ? .orange : .green
    }
    private var statusText: String {
        if !client.connected { return "Engine offline" }
        return client.muted ? "Muted" : "Narrating"
    }
}

// MARK: - Sessions

private struct SessionsSection: View {
    @EnvironmentObject var client: DaemonClient

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            SectionLabel("ACTIVE SESSIONS")
            if client.sessions.isEmpty {
                Text("No active sessions. Finish a turn in Claude Code, Codex, or OpenCode.")
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 14)
                    .padding(.bottom, 12)
            } else {
                ForEach(client.sessions) { session in
                    SessionRow(session: session)
                }
                .padding(.bottom, 6)
                .animation(.default, value: client.sessions)
            }
        }
    }
}

private struct SessionRow: View {
    @EnvironmentObject var client: DaemonClient
    let session: Session
    @State private var confirmingKill = false
    @State private var hovering = false

    var body: some View {
        HStack(spacing: 10) {
            ProviderLogo(source: session.source, active: hovering || session.speaking, size: 20)

            VStack(alignment: .leading, spacing: 1) {
                HStack(spacing: 5) {
                    Text(client.displayName(session))
                        .font(.system(size: 13, weight: .medium))
                        .lineLimit(1)
                        .truncationMode(.tail)
                    if session.unread {
                        Circle().fill(Color.red).frame(width: 7, height: 7)
                    }
                }
                Text(AgentStyle.label(session.source))
                    .font(.system(size: 11))
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: 150, alignment: .leading)

            Spacer(minLength: 6)

            if session.unread {
                Button("Hear") { client.markRead(session.key) }
                    .font(.system(size: 11))
                    .buttonStyle(.borderless)
            }

            BehaviorMenu(session: session)

            if session.canKill {
                Button(role: .destructive) {
                    confirmingKill = true
                } label: {
                    Image(systemName: "xmark.circle")
                }
                .buttonStyle(.borderless)
                .help("End this session")
                .confirmationDialog(
                    "End the \(AgentStyle.label(session.source)) session in \(client.displayName(session))?",
                    isPresented: $confirmingKill
                ) {
                    Button("End session", role: .destructive) { client.kill(session.key) }
                    Button("Cancel", role: .cancel) {}
                } message: {
                    Text("This terminates the agent process (pid \(session.pid.map(String.init) ?? "?")).")
                }
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 6)
        .background(hovering ? Color.primary.opacity(0.05) : .clear)
        .onHover { hovering = $0 }
    }
}

private struct BehaviorMenu: View {
    @EnvironmentObject var client: DaemonClient
    let session: Session

    var body: some View {
        Menu {
            Button("Auto") { client.setSessionBehavior(session.key, "auto") }
            Button("Notify only") { client.setSessionBehavior(session.key, "notify") }
            Button("Off") { client.setSessionBehavior(session.key, "off") }
            Divider()
            Button("Use default") { client.setSessionBehavior(session.key, nil) }
        } label: {
            Text(behaviorLabel)
                .font(.system(size: 11, weight: .medium))
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
    }

    private var behaviorLabel: String {
        switch session.behavior {
        case "notify": return "Notify"
        case "off": return "Off"
        default: return "Auto"
        }
    }
}

// MARK: - Voice

private struct VoiceSection: View {
    @EnvironmentObject var client: DaemonClient

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionLabel("VOICE")
            HStack {
                VoicePicker(voices: client.availableVoices, selection: client.config?.voice ?? "") { picked in
                    client.setConfig(["voice": picked])
                    client.audition(voice: picked)
                }
                Spacer()
            }
            .padding(.horizontal, 14)

            HStack(spacing: 8) {
                Text("Speed").font(.system(size: 12)).foregroundStyle(.secondary)
                Slider(value: speedBinding, in: 0.7...1.5, step: 0.05)
                Text(String(format: "%.2f", client.config?.speed ?? 1.0))
                    .font(.system(size: 11).monospacedDigit())
                    .foregroundStyle(.secondary)
                    .frame(width: 34, alignment: .trailing)
            }
            .padding(.horizontal, 14)
            .padding(.bottom, 12)
        }
    }

    private var speedBinding: Binding<Double> {
        Binding(
            get: { client.config?.speed ?? 1.0 },
            set: { client.setConfig(["speed": $0]) }
        )
    }
}

// MARK: - Defaults

private struct DefaultsSection: View {
    @EnvironmentObject var client: DaemonClient

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionLabel("DEFAULTS")

            HStack {
                Text("Behavior").font(.system(size: 12)).foregroundStyle(.secondary)
                Spacer()
                Picker("", selection: behaviorBinding) {
                    Text("Auto").tag("auto")
                    Text("Notify").tag("notify")
                    Text("Off").tag("off")
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(width: 200)
            }
            .padding(.horizontal, 14)

            HStack {
                Text("Digest").font(.system(size: 12)).foregroundStyle(.secondary)
                Spacer()
                Picker("", selection: modeBinding) {
                    Text("Full").tag("full")
                    Text("TL;DR").tag("tldr")
                    Text("Summary").tag("summary")
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(width: 200)
            }
            .padding(.horizontal, 14)
            .padding(.bottom, 12)
        }
    }

    private var behaviorBinding: Binding<String> {
        Binding(
            get: { client.config?.behavior ?? "auto" },
            set: { client.setConfig(["behavior": $0]) }
        )
    }
    private var modeBinding: Binding<String> {
        Binding(
            get: { client.config?.mode ?? "full" },
            set: { client.setConfig(["mode": $0]) }
        )
    }
}

// MARK: - Disconnected

private struct DisconnectedView: View {
    @EnvironmentObject var client: DaemonClient

    var body: some View {
        VStack(spacing: 10) {
            Text("The incant narration engine isn't running.")
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
            Button("Start engine") { client.startEngine() }
                .controlSize(.large)
        }
        .frame(maxWidth: .infinity)
        .padding(.horizontal, 14)
        .padding(.vertical, 18)
    }
}

// MARK: - Footer

private struct FooterBar: View {
    @EnvironmentObject var client: DaemonClient

    @AppStorage(BubbleController.overlayDefaultsKey) private var bubblesEnabled = true

    var body: some View {
        VStack(spacing: 6) {
            Toggle("Show session bubbles", isOn: $bubblesEnabled)
                .toggleStyle(.switch)
                .controlSize(.small)
                .font(.system(size: 12))
                .onChange(of: bubblesEnabled) { _, on in
                    AppDelegate.bubbles.enabled = on
                }
            HStack(spacing: 14) {
                Button("Set up…") { AppDelegate.onboarding.show() }
                    .buttonStyle(.borderless)
                    .font(.system(size: 12))
                Button("Open config") { openConfig() }
                    .buttonStyle(.borderless)
                    .font(.system(size: 12))
                Spacer()
                Button("Quit") { NSApp.terminate(nil) }
                    .buttonStyle(.borderless)
                    .font(.system(size: 12))
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 8)
    }

    private func openConfig() {
        let path = ("~/.config/incant/config.toml" as NSString).expandingTildeInPath
        NSWorkspace.shared.open(URL(fileURLWithPath: path))
    }
}

// MARK: - Shared

private struct SectionLabel: View {
    let text: String
    init(_ text: String) { self.text = text }
    var body: some View {
        Text(text)
            .font(.system(size: 10, weight: .semibold))
            .foregroundStyle(.secondary)
            .padding(.horizontal, 14)
            .padding(.top, 10)
            .padding(.bottom, 6)
    }
}
