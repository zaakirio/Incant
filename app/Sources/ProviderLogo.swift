import SwiftUI

/// A provider mark that activates to its brand color. Muted (or grayscale)
/// at rest, full brand color when `active` (hovered / emphasized). Marks
/// without a brand color activate to the foreground instead.
struct ProviderLogo: View {
    let source: String
    var active: Bool = false
    /// When true, the resting state is desaturated grayscale (used on
    /// bubbles); when false, the brand color shows at rest but muted
    /// (used in the popover, so colors are visible on open).
    var muteWhenInactive: Bool = false
    var size: CGFloat = 20

    var body: some View {
        let brand = AgentStyle.brandColor(source) ?? .primary
        image
            .frame(width: size, height: size)
            .foregroundStyle(tint(brand))
            .saturation(active ? 1 : (muteWhenInactive ? 0 : 0.95))
            .scaleEffect(active ? 1.08 : 1.0)
            .animation(.easeOut(duration: 0.15), value: active)
    }

    private func tint(_ brand: Color) -> Color {
        if active { return brand }
        return muteWhenInactive ? .secondary : brand.opacity(0.9)
    }

    @ViewBuilder private var image: some View {
        if let name = AgentStyle.logo(source) {
            Image(name).renderingMode(.template).resizable().scaledToFit()
        } else {
            Image(systemName: AgentStyle.fallbackGlyph(source)).resizable().scaledToFit()
        }
    }
}
