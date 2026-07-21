import SwiftUI
import Combine

/// Reactive state for one bubble, owned by its window and updated as the
/// session, hover, and custom name change.
@MainActor
final class BubbleModel: ObservableObject {
    @Published var session: Session
    @Published var hovering = false
    @Published var displayName: String

    init(session: Session, displayName: String) {
        self.session = session
        self.displayName = displayName
    }
}

/// The chat-head face. Purely presentational; the hosting window handles
/// drag / click / right-click so SwiftUI never intercepts those.
struct BubbleView: View {
    @ObservedObject var model: BubbleModel
    @State private var pulse = false
    @State private var attentionPulse = false
    @State private var spin = false

    private let diameter: CGFloat = 46

    private var session: Session { model.session }
    private var status: SessionStatus { session.sessionStatus }

    var body: some View {
        VStack(spacing: 3) {
            ZStack {
                Circle()
                    .fill(.regularMaterial)
                    .overlay(Circle().strokeBorder(Color.primary.opacity(0.12), lineWidth: 1))
                    .shadow(color: .black.opacity(0.28), radius: 5, y: 1)

                if session.speaking {
                    Circle()
                        .strokeBorder(Color.accentColor, lineWidth: 2)
                        .scaleEffect(pulse ? 1.18 : 1.0)
                        .opacity(pulse ? 0 : 0.9)
                        .animation(.easeOut(duration: 1.0).repeatForever(autoreverses: false), value: pulse)
                } else if status == .working {
                    // In progress: a slowly orbiting arc.
                    Circle()
                        .trim(from: 0.1, to: 0.75)
                        .stroke(Color.accentColor.opacity(0.75),
                                style: StrokeStyle(lineWidth: 2, lineCap: .round))
                        .padding(1)
                        .rotationEffect(.degrees(spin ? 360 : 0))
                        .animation(.linear(duration: 1.8).repeatForever(autoreverses: false), value: spin)
                } else if status.needsAttention {
                    // Blocked on the user: an insistent orange pulse.
                    Circle()
                        .strokeBorder(Color.orange, lineWidth: 2)
                        .scaleEffect(attentionPulse ? 1.18 : 1.0)
                        .opacity(attentionPulse ? 0 : 0.9)
                        .animation(.easeOut(duration: 0.9).repeatForever(autoreverses: false), value: attentionPulse)
                }

                ProviderLogo(source: session.source,
                             active: model.hovering || session.speaking || status != .idle,
                             muteWhenInactive: true,
                             size: 24)

                if session.unread {
                    Circle()
                        .fill(Color.red)
                        .frame(width: 13, height: 13)
                        .overlay(Circle().strokeBorder(Color(nsColor: .windowBackgroundColor), lineWidth: 2))
                        .offset(x: diameter / 2 - 5, y: -diameter / 2 + 5)
                } else if status.needsAttention {
                    ZStack {
                        Circle()
                            .fill(Color.orange)
                            .overlay(Circle().strokeBorder(Color(nsColor: .windowBackgroundColor), lineWidth: 2))
                        Text(status == .awaitingApproval ? "!" : "?")
                            .font(.system(size: 9, weight: .heavy))
                            .foregroundStyle(.white)
                    }
                    .frame(width: 14, height: 14)
                    .offset(x: diameter / 2 - 5, y: -diameter / 2 + 5)
                }

                if session.subagentCount > 0 {
                    // Swarm badge: how many subagents are running right now.
                    Text("×\(session.subagentCount)")
                        .font(.system(size: 8, weight: .bold).monospacedDigit())
                        .padding(.horizontal, 4)
                        .padding(.vertical, 1)
                        .background(Capsule().fill(Color.accentColor))
                        .foregroundStyle(.white)
                        .offset(x: diameter / 2 - 8, y: diameter / 2 - 4)
                }
            }
            .frame(width: diameter, height: diameter)

            Text(model.displayName)
                .font(.system(size: 10, weight: .medium))
                .lineLimit(1)
                .truncationMode(.tail)
                .padding(.horizontal, 6)
                .padding(.vertical, 1)
                .frame(maxWidth: 96)
                .background(Capsule().fill(.regularMaterial))
        }
        .frame(width: 104)
        .onAppear { syncAnimations() }
        .onChange(of: session.speaking) { _, _ in syncAnimations() }
        .onChange(of: status) { _, _ in syncAnimations() }
    }

    private func syncAnimations() {
        pulse = session.speaking
        spin = !session.speaking && status == .working
        attentionPulse = !session.speaking && status.needsAttention
    }
}
