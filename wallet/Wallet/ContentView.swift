import SwiftUI
import UIKit
import ReownWalletKit

struct ContentView: View {
    @ObservedObject var viewModel: WalletViewModel

    var body: some View {
        NavigationStack {
            Form {
                KeyImportSection(viewModel: viewModel)

                if viewModel.ethereumAddress != nil, viewModel.wcConfigured {
                    walletConnectSection
                    sessionsSection
                    calldataDiagnosticsSection
                    typedDataDiagnosticsSection
                } else if viewModel.ethereumAddress != nil, !viewModel.wcConfigured {
                    Section("WalletConnect") {
                        Text("WalletConnect not configured. Set WALLETCONNECT_PROJECT_ID in Config.xcconfig.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .navigationTitle("Wallet")
            .sheet(isPresented: $viewModel.showScanner) {
                QRScannerSheet { code in
                    viewModel.pairFromQR(code)
                }
            }
            .sheet(isPresented: $viewModel.showProposal) {
                if let proposal = viewModel.pendingProposal {
                    SessionProposalSheet(
                        proposal: proposal,
                        onApprove: { viewModel.approveProposal() },
                        onReject: { viewModel.rejectProposal() }
                    )
                }
            }
            .sheet(isPresented: $viewModel.showRequest) {
                SessionRequestSheet(
                    method: viewModel.pendingRequest?.method ?? "unknown",
                    displayModel: viewModel.displayModel,
                    error: viewModel.requestError,
                    rawJSON: viewModel.rawRequestJSON,
                    diagnosticCaptureJSON: viewModel.currentTypedDataCapture?.exportJSONString
                        ?? viewModel.currentCalldataCapture?.exportJSONString,
                    onApprove: { viewModel.approveRequest() },
                    onReject: { viewModel.rejectRequest() }
                )
            }
            .onAppear {
                viewModel.configureWalletConnect()
            }
        }
    }

    // MARK: - Sections

    private var walletConnectSection: some View {
        Section("WalletConnect") {
            TextField("Paste WC URI", text: $viewModel.pairingURI)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .font(.caption.monospaced())

            Button("Pair") { viewModel.pair() }
                .disabled(viewModel.pairingURI.isEmpty)

            Button {
                viewModel.showScanner = true
            } label: {
                Label("Scan QR", systemImage: "qrcode.viewfinder")
            }

            if viewModel.isPaired {
                Label("Paired", systemImage: "checkmark.circle.fill")
                    .foregroundStyle(.green)
                    .font(.caption)
            }

            if let error = viewModel.pairingError {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
            }
        }
    }

    private var sessionsSection: some View {
        Section("Active Sessions") {
            if viewModel.activeSessions.isEmpty {
                Text("No active sessions")
                    .foregroundStyle(.secondary)
                    .font(.footnote)
            } else {
                ForEach(viewModel.activeSessions, id: \.topic) { session in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(session.peer.name)
                            .font(.subheadline)
                        Text(session.peer.url)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    .swipeActions(edge: .trailing, allowsFullSwipe: true) {
                        Button(role: .destructive) {
                            viewModel.disconnectSession(session)
                        } label: {
                            Label("Disconnect", systemImage: "xmark.circle")
                        }
                    }
                }
            }
        }
    }

    private var typedDataDiagnosticsSection: some View {
        Section {
            if viewModel.recentTypedDataCaptures.isEmpty {
                Text("No typed-data captures yet")
                    .foregroundStyle(.secondary)
                    .font(.footnote)
            } else {
                ForEach(Array(viewModel.recentTypedDataCaptures.prefix(5))) { capture in
                    Button {
                        UIPasteboard.general.string = capture.exportJSONString
                    } label: {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(capture.summary?.primaryType ?? capture.method)
                                .font(.subheadline)
                            Text(capture.outcome.rawValue)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                            if let contract = capture.summary?.verifyingContract {
                                Text(contract)
                                    .font(.caption2.monospaced())
                                    .foregroundStyle(.secondary)
                                    .lineLimit(1)
                            }
                        }
                    }
                    .buttonStyle(.plain)
                }
            }
        } header: {
            Text("Typed Data Diagnostics")
        } footer: {
            Text("Tap a capture to copy the structured diagnostic JSON.")
        }
    }

    private var calldataDiagnosticsSection: some View {
        Section {
            if viewModel.recentCalldataCaptures.isEmpty {
                Text("No calldata captures yet")
                    .foregroundStyle(.secondary)
                    .font(.footnote)
            } else {
                ForEach(Array(viewModel.recentCalldataCaptures.prefix(5))) { capture in
                    Button {
                        UIPasteboard.general.string = capture.exportJSONString
                    } label: {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(capture.clearSigningIntent ?? capture.method)
                                .font(.subheadline)
                            Text(capture.outcome.rawValue)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                            if let address = capture.matchedAddress ?? capture.to {
                                Text(address)
                                    .font(.caption2.monospaced())
                                    .foregroundStyle(.secondary)
                                    .lineLimit(1)
                            }
                        }
                    }
                    .buttonStyle(.plain)
                }
            }
        } header: {
            Text("Calldata Diagnostics")
        } footer: {
            Text("Tap a capture to copy the structured diagnostic JSON.")
        }
    }
}
