import SwiftUI
import AppKit

struct OnboardingView: View {
    @ObservedObject var checker: DependencyChecker
    let client: DaemonClient
    @State private var checking = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            header
            Divider()
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    setupSection
                    dictationSection
                }
                .padding(20)
            }
        }
        .frame(width: 520, height: 620)
        .task { await refresh() }
    }

    private var header: some View {
        HStack(spacing: 12) {
            Image(systemName: "waveform")
                .font(.system(size: 22, weight: .semibold))
                .foregroundStyle(Color.accentColor)
            VStack(alignment: .leading, spacing: 2) {
                Text("Set up Incant").font(.system(size: 17, weight: .bold))
                Text("Give your coding agents a voice.").font(.system(size: 12)).foregroundStyle(.secondary)
            }
            Spacer()
            Button {
                Task { await refresh() }
            } label: {
                Label("Recheck", systemImage: checking ? "arrow.triangle.2.circlepath" : "arrow.clockwise")
            }
            .disabled(checking)
        }
        .padding(16)
    }

    private var setupSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            sectionHeader("REQUIRED", trailing: checker.allRequiredOk ? "Ready" : nil)
            ForEach(checker.checks) { check in
                CheckRow(check: check) { checker.run(check) }
            }
            if checker.allRequiredOk {
                Text("You're all set. Finish a turn in any agent to hear it.")
                    .font(.system(size: 12)).foregroundStyle(.green)
                    .padding(.top, 2)
            }
        }
    }

    private var dictationSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            sectionHeader("DICTATION (OPTIONAL)", trailing: nil)
            Text("Incant only speaks; it has no microphone. Pair any speech-to-text tool to talk back to your agents. Bring your own — these are just popular options.")
                .font(.system(size: 12)).foregroundStyle(.secondary)
            ForEach(checker.dictation) { option in
                DictationRow(option: option)
            }
        }
    }

    private func sectionHeader(_ title: String, trailing: String?) -> some View {
        HStack {
            Text(title).font(.system(size: 11, weight: .semibold)).foregroundStyle(.secondary)
            Spacer()
            if let trailing {
                Text(trailing).font(.system(size: 11, weight: .semibold)).foregroundStyle(.green)
            }
        }
    }

    private func refresh() async {
        checking = true
        await checker.recheck()
        checking = false
    }
}

private struct CheckRow: View {
    let check: DependencyCheck
    let onRun: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 10) {
                statusIcon
                VStack(alignment: .leading, spacing: 1) {
                    Text(check.title).font(.system(size: 13, weight: .medium))
                    Text(check.detail).font(.system(size: 11)).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer()
                if check.runnable, check.state != .ok {
                    Button("Run", action: onRun).controlSize(.small)
                }
            }
            if let command = check.command, check.state != .ok {
                CommandStrip(command: command)
            }
        }
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 8).fill(Color.primary.opacity(0.04)))
    }

    @ViewBuilder private var statusIcon: some View {
        switch check.state {
        case .ok: Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
        case .missing: Image(systemName: "exclamationmark.circle.fill").foregroundStyle(.orange)
        case .unknown: Image(systemName: "questionmark.circle.fill").foregroundStyle(.secondary)
        case .checking: ProgressView().controlSize(.small)
        }
    }
}

private struct DictationRow: View {
    let option: DictationOption

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 10) {
                Image(systemName: option.installed ? "checkmark.circle.fill" : "circle")
                    .foregroundStyle(option.installed ? .green : .secondary)
                VStack(alignment: .leading, spacing: 1) {
                    HStack(spacing: 6) {
                        Text(option.name).font(.system(size: 13, weight: .medium))
                        Text(option.free ? "Free" : "Paid")
                            .font(.system(size: 9, weight: .semibold))
                            .padding(.horizontal, 5).padding(.vertical, 1)
                            .background(Capsule().fill(option.free ? Color.green.opacity(0.18) : Color.orange.opacity(0.18)))
                    }
                    Text(option.detail).font(.system(size: 11)).foregroundStyle(.secondary)
                }
                Spacer()
                Button {
                    if let url = URL(string: option.url) { NSWorkspace.shared.open(url) }
                } label: { Image(systemName: "arrow.up.right.square") }
                .buttonStyle(.borderless)
                .help("Learn more")
            }
            if let command = option.command, !option.installed {
                CommandStrip(command: command)
            }
        }
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 8).fill(Color.primary.opacity(0.04)))
    }
}

/// A monospaced command with a copy button.
private struct CommandStrip: View {
    let command: String
    @State private var copied = false

    var body: some View {
        HStack(spacing: 8) {
            Text(command)
                .font(.system(size: 11, design: .monospaced))
                .lineLimit(1)
                .truncationMode(.middle)
                .textSelection(.enabled)
            Spacer()
            Button {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(command, forType: .string)
                copied = true
                DispatchQueue.main.asyncAfter(deadline: .now() + 1.4) { copied = false }
            } label: {
                Image(systemName: copied ? "checkmark" : "doc.on.doc")
                    .foregroundStyle(copied ? .green : .secondary)
            }
            .buttonStyle(.borderless)
            .help("Copy to clipboard")
        }
        .padding(.horizontal, 10).padding(.vertical, 6)
        .background(RoundedRectangle(cornerRadius: 6).fill(Color.black.opacity(0.06)))
    }
}
