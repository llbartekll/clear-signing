import SwiftUI
import UIKit
import Erc7730

struct DisplayModelView: View {
    let model: DisplayModel

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(model.interpolatedIntent ?? model.intent)
                .font(.headline)
                .frame(maxWidth: .infinity, alignment: .leading)

            if let interpolated = model.interpolatedIntent, interpolated != model.intent {
                Text(model.intent)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }

            if let owner = model.owner {
                itemRow(DisplayItem(label: "Contract", value: owner))
            }

            ForEach(Array(model.entries.enumerated()), id: \.offset) { _, entry in
                entryView(entry)
            }

            ForEach(model.warnings, id: \.self) { warning in
                Button {
                    copyToClipboard(warning)
                } label: {
                    Label(warning, systemImage: "exclamationmark.triangle.fill")
                        .font(.footnote)
                        .foregroundStyle(.orange)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .buttonStyle(.plain)
                .contentShape(Rectangle())
            }
        }
    }

    @ViewBuilder
    private func entryView(_ entry: DisplayEntry) -> some View {
        switch entry {
        case .item(let item):
            itemRow(item)
        case .group(let label, _, let items):
            VStack(alignment: .leading, spacing: 6) {
                Text(label)
                    .font(.subheadline.bold())
                ForEach(Array(items.enumerated()), id: \.offset) { _, item in
                    itemRow(item)
                        .padding(.leading, 12)
                }
            }
        case .nested(let label, let intent, let entries, _):
            VStack(alignment: .leading, spacing: 6) {
                Text(label)
                    .font(.subheadline.bold())
                Text(intent)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                ForEach(Array(entries.enumerated()), id: \.offset) { _, nestedEntry in
                    AnyView(entryView(nestedEntry))
                        .padding(.leading, 12)
                }
            }
        }
    }

    private func itemRow(_ item: DisplayItem) -> some View {
        HStack(alignment: .top) {
            Text(item.label)
                .font(.footnote)
                .foregroundStyle(.secondary)
                .frame(width: 100, alignment: .trailing)
            Text(item.value)
                .font(.footnote.monospaced())
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func copyToClipboard(_ value: String) {
        UIPasteboard.general.string = value
    }
}
