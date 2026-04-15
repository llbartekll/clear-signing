import SwiftUI
import UIKit
import ClearSigning

struct SessionRequestSheet: View {
    let method: String
    let formatOutcome: FormatOutcome?
    let formatFailure: FormatFailure?
    let error: String?
    let rawJSON: String?
    let diagnosticCaptureJSON: String?
    let onApprove: () -> Void
    let onReject: () -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var showRaw = true
    @State private var showDiagnostics = true

    private var approvalBlocked: Bool {
        formatFailure != nil || error != nil
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    Label(method, systemImage: "doc.text")
                        .font(.headline)

                    if let formatOutcome {
                        DisplayModelView(outcome: formatOutcome)
                            .padding()
                            .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 12))
                    }

                    if let formatFailure {
                        VStack(alignment: .leading, spacing: 8) {
                            Label(formatFailure.displayLabel, systemImage: "xmark.shield.fill")
                                .font(.headline)
                                .foregroundStyle(.red)

                            Button {
                                copyToClipboard(formatFailure.message)
                            } label: {
                                Text(formatFailure.message)
                                    .font(.footnote)
                                    .foregroundStyle(.red)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                            }
                            .buttonStyle(.plain)
                            .contentShape(Rectangle())

                            Text(formatFailure.retryable ? "Retryable failure" : "Blocking failure")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                        .padding()
                        .background(Color.red.opacity(0.08), in: RoundedRectangle(cornerRadius: 12))
                    } else if let error {
                        Button {
                            copyToClipboard(error)
                        } label: {
                            Label(error, systemImage: "xmark.circle")
                                .font(.footnote)
                                .foregroundStyle(.red)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                        .buttonStyle(.plain)
                        .contentShape(Rectangle())
                    }

                    if let raw = rawJSON {
                        DisclosureGroup("Raw Data", isExpanded: $showRaw) {
                            Button {
                                copyToClipboard(raw)
                            } label: {
                                Text(raw)
                                    .font(.caption2.monospaced())
                                    .textSelection(.enabled)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                            }
                            .buttonStyle(.plain)
                            .contentShape(Rectangle())
                        }
                    }

                    if let diagnosticCaptureJSON {
                        DisclosureGroup("Diagnostic Capture", isExpanded: $showDiagnostics) {
                            Button {
                                copyToClipboard(diagnosticCaptureJSON)
                            } label: {
                                Text(diagnosticCaptureJSON)
                                    .font(.caption2.monospaced())
                                    .textSelection(.enabled)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                            }
                            .buttonStyle(.plain)
                            .contentShape(Rectangle())
                        }
                    }
                }
                .padding()
            }
            .navigationTitle("Session Request")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Reject") {
                        onReject()
                        dismiss()
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Approve") {
                        onApprove()
                    }
                    .disabled(approvalBlocked)
                }
            }
        }
    }

    private func copyToClipboard(_ value: String) {
        UIPasteboard.general.string = value
    }
}
