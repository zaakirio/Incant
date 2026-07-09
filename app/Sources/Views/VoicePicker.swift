import SwiftUI

/// Voice chooser that decodes Kokoro ids into readable names, grouped by
/// language. The button shows the friendly current voice; the menu lists
/// "Name · Gender" under each language heading.
struct VoicePicker: View {
    let voices: [String]
    let selection: String
    var onPick: (String) -> Void

    private var current: VoiceInfo? {
        voices.contains(selection) ? VoiceCatalog.parse(selection) : nil
    }

    var body: some View {
        Menu {
            ForEach(VoiceCatalog.grouped(voices), id: \.language) { group in
                Section("\(group.flag)  \(group.language)") {
                    ForEach(group.voices) { voice in
                        Button {
                            onPick(voice.id)
                        } label: {
                            if voice.id == selection {
                                Label("\(voice.name) · \(voice.gender)", systemImage: "checkmark")
                            } else {
                                Text("\(voice.name) · \(voice.gender)")
                            }
                        }
                    }
                }
            }
        } label: {
            if let current {
                HStack(spacing: 5) {
                    Text(current.flag)
                    Text(current.name).fontWeight(.medium)
                    Text(current.gender).foregroundStyle(.secondary)
                }
                .font(.system(size: 12))
            } else {
                Text("Select a voice").font(.system(size: 12)).foregroundStyle(.secondary)
            }
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
    }
}
