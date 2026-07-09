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

    private let diameter: CGFloat = 46

    private var session: Session { model.session }

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
                }

                ProviderLogo(source: session.source,
                             active: model.hovering || session.speaking,
                             muteWhenInactive: true,
                             size: 24)

                if session.unread {
                    Circle()
                        .fill(Color.red)
                        .frame(width: 13, height: 13)
                        .overlay(Circle().strokeBorder(Color(nsColor: .windowBackgroundColor), lineWidth: 2))
                        .offset(x: diameter / 2 - 5, y: -diameter / 2 + 5)
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
        .onAppear { pulse = session.speaking }
        .onChange(of: session.speaking) { _, now in pulse = now }
    }
}
