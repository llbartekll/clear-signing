import SwiftUI
import UIKit
import ClearSigning

struct DisplayModelView: View {
    let outcome: FormatOutcome

    private var model: DisplayModel {
        outcome.model
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            statusBanner

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

            if !outcome.diagnostics.isEmpty {
                VStack(alignment: .leading, spacing: 8) {
                    Text("Diagnostics")
                        .font(.subheadline.bold())

                    ForEach(Array(outcome.diagnostics.enumerated()), id: \.offset) { _, diagnostic in
                        Button {
                            copyToClipboard(diagnostic.message)
                        } label: {
                            Label(diagnostic.message, systemImage: diagnostic.severity == .warning ? "exclamationmark.triangle.fill" : "info.circle.fill")
                                .font(.footnote)
                                .foregroundStyle(diagnostic.severity == .warning ? .orange : .secondary)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                        .buttonStyle(.plain)
                        .contentShape(Rectangle())
                    }
                }
            }
        }
    }

    private var statusBanner: some View {
        HStack(spacing: 8) {
            Image(systemName: outcome.isClearSigned ? "checkmark.shield.fill" : "exclamationmark.shield.fill")
                .foregroundStyle(outcome.isClearSigned ? .green : .orange)
            Text(outcome.isClearSigned ? "Clear Signed" : outcome.fallbackReason?.displayLabel ?? "Fallback")
                .font(.caption.weight(.semibold))
                .foregroundStyle(outcome.isClearSigned ? .green : .orange)
            Spacer()
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 8)
        .background(
            RoundedRectangle(cornerRadius: 10)
                .fill(outcome.isClearSigned ? Color.green.opacity(0.12) : Color.orange.opacity(0.14))
        )
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
        case .nested(let label, let intent, let entries):
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
