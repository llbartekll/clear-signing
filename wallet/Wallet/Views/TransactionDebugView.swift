import SwiftUI
import UIKit
import ClearSigning

struct TransactionDebugView: View {
    @StateObject private var viewModel: TransactionDebugViewModel

    init(metadataProvider: DataProviderFfi) {
        _viewModel = StateObject(wrappedValue: TransactionDebugViewModel(metadataProvider: metadataProvider))
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    HStack {
                        Label("Ethereum Mainnet", systemImage: "circle.fill")
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(.green)
                        Spacer()
                    }

                    TransactionHashInput(
                        value: $viewModel.txHash,
                        isLoading: viewModel.isFetching,
                        onSubmit: { viewModel.decode() }
                    )

                    DebugExampleTransactions(
                        examples: TransactionDebugViewModel.examples,
                        selectedHash: viewModel.txHash,
                        onSelect: { viewModel.selectExample($0) }
                    )

                    if let error = viewModel.error {
                        DebugErrorBanner(message: error)
                    }

                    if viewModel.isFetching {
                        DebugLoadingRow(message: "Fetching transaction...")
                    }

                    if viewModel.hasResult {
                        DebugResultsView(
                            rawTransaction: viewModel.rawTransaction,
                            formatOutcome: viewModel.formatOutcome,
                            formatFailure: viewModel.formatFailure,
                            isFormatting: viewModel.isFormatting,
                            diagnosticCaptureJSON: viewModel.debugCapture?.exportJSONString
                        )
                    }
                }
                .padding()
            }
            .navigationTitle("Debug")
            .navigationBarTitleDisplayMode(.inline)
        }
    }
}

@MainActor
final class TransactionDebugViewModel: ObservableObject {
    static let mainnetChainId: UInt64 = 1

    static let examples: [DebugExampleTransaction] = [
        DebugExampleTransaction(
            name: "Uniswap V3 Swap",
            description: "Token swap on the Uniswap V3 SwapRouter02",
            txHash: "0x675a2e96e48b77e5d8edc16bfc4dc2ea7547f950edb76fdeff40e8af250d897e",
            contractName: "UniswapV3Router02"
        ),
        DebugExampleTransaction(
            name: "Lido stETH Submit",
            description: "Stake ETH via Lido to receive stETH",
            txHash: "0x450c5259de51e99ad030963694108287f28d6114e3c74d2bebb8b2c4a5e962ff",
            contractName: "stETH"
        ),
        DebugExampleTransaction(
            name: "Aave V3 Supply",
            description: "Supply assets to the Aave V3 lending pool",
            txHash: "0xb0bd1c520f3b43405bb23f94f2a7f18e0fd0b671dc606516f3d0f9c9a199b608",
            contractName: "Aave Pool V3"
        ),
        DebugExampleTransaction(
            name: "WETH Deposit",
            description: "Wrap ETH into WETH (Wrapped Ether)",
            txHash: "0x7fd3cca7ea85567a7741fed3d6ca181d1ffd6e8002e6771d15c8911ebfde872d",
            contractName: "WETH"
        ),
        DebugExampleTransaction(
            name: "1inch Swap",
            description: "Token swap via 1inch Aggregation Router V6",
            txHash: "0xc8b5160898209df85207b18194b1e3a672b8e6c59548a434fa2de798b3a686f2",
            contractName: "AggregationRouterV6"
        ),
        DebugExampleTransaction(
            name: "Aave V3 Borrow",
            description: "Borrow assets from the Aave V3 lending pool",
            txHash: "0x78117e5a10483522f88edc342f9482b6c33da2f60afb27d4329a128acd6f5c6c",
            contractName: "Aave Pool V3"
        ),
        DebugExampleTransaction(
            name: "Safe Factory",
            description: "Deploy a new Safe multisig wallet via SafeProxyFactory",
            txHash: "0x1b26714409765f483cb4d455643510078c34f76cc850dc7b37f1b3176a5feea8",
            contractName: "SafeProxyFactory"
        ),
    ]

    @Published var txHash = ""
    @Published var rawTransaction: DebugRawTransaction?
    @Published var formatOutcome: FormatOutcome?
    @Published var formatFailure: FormatFailure?
    @Published var error: String?
    @Published var isFetching = false
    @Published var isFormatting = false
    @Published var debugCapture: CalldataCapture?

    var hasResult: Bool {
        rawTransaction != nil || formatOutcome != nil || formatFailure != nil || isFormatting
    }

    private let clearSigning: ClearSigningService
    private var task: Task<Void, Never>?
    private var activeDecodeID: UUID?

    init(metadataProvider: DataProviderFfi) {
        clearSigning = ClearSigningService(dataProvider: metadataProvider)
    }

    func selectExample(_ example: DebugExampleTransaction) {
        txHash = example.txHash
        decode(hashOverride: example.txHash)
    }

    func decode() {
        decode(hashOverride: nil)
    }

    private func decode(hashOverride: String?) {
        let hash = (hashOverride ?? txHash).trimmingCharacters(in: .whitespacesAndNewlines)
        task?.cancel()
        activeDecodeID = nil

        guard DebugRawTransaction.isValidTransactionHash(hash) else {
            resetForIdleError("Enter a valid 32-byte transaction hash.")
            return
        }

        guard let apiKey = AppConfig.alchemyAPIKey else {
            resetForIdleError("Set ALCHEMY_API_KEY in Config.xcconfig to use transaction debug.")
            return
        }

        rawTransaction = nil
        formatOutcome = nil
        formatFailure = nil
        error = nil
        debugCapture = nil
        isFetching = true
        isFormatting = false

        let decodeID = UUID()
        activeDecodeID = decodeID
        let clearSigning = self.clearSigning
        task = Task.detached(priority: .userInitiated) { [weak self] in
            let client = AlchemyClient(apiKey: apiKey, timeout: 8.0)
            let lookup = client.fetchTransactionByHash(chainId: Self.mainnetChainId, hash: hash)

            if Task.isCancelled {
                return
            }

            switch lookup {
            case .value(let raw):
                let appliedRawTransaction = await MainActor.run { () -> Bool in
                    guard let self = self, self.activeDecodeID == decodeID else {
                        return false
                    }
                    self.rawTransaction = raw
                    self.debugCapture = Self.makeCapture(from: raw)
                    self.isFetching = false
                    self.isFormatting = true
                    return true
                }

                guard appliedRawTransaction else {
                    return
                }

                guard let to = raw.to else {
                    await MainActor.run {
                        guard let self = self, self.activeDecodeID == decodeID else {
                            return
                        }
                        self.isFormatting = false
                        self.error = "Contract creation transactions do not have a recipient and cannot be clear-signed."
                        self.updateCapture { capture in
                            capture.outcome = .paramsExtractionFailed
                            capture.signingError = "Transaction has no recipient (contract creation)"
                            capture.notes.append("Contract creation transaction is unsupported for clear signing")
                        }
                        self.activeDecodeID = nil
                    }
                    return
                }

                let result = await clearSigning.formatCalldataDetailed(
                    chainId: Self.mainnetChainId,
                    to: to,
                    calldata: raw.input,
                    value: raw.valueHex,
                    from: raw.from
                )

                if Task.isCancelled {
                    return
                }

                await MainActor.run {
                    guard let self = self, self.activeDecodeID == decodeID else {
                        return
                    }
                    self.isFormatting = false
                    defer { self.activeDecodeID = nil }
                    if let outcome = result.formatOutcome {
                        self.formatOutcome = outcome
                        self.formatFailure = nil
                        self.error = nil
                        self.updateCapture { capture in
                            capture.applyClearSigningSuccess(result)
                            if result.usedImplementationAddress, let implementationAddress = result.implementationAddress {
                                capture.notes.append("Matched implementation address \(implementationAddress)")
                            }
                        }
                    } else if let failure = result.formatFailure {
                        self.formatOutcome = nil
                        self.formatFailure = failure
                        self.error = nil
                        self.updateCapture { capture in
                            capture.applyClearSigningFailure(result)
                            if let stage = result.failedStage {
                                capture.notes.append("Clear signing failed during \(stage.rawValue)")
                            }
                        }
                    }
                }

            case .notFound:
                await MainActor.run {
                    guard let self = self, self.activeDecodeID == decodeID else {
                        return
                    }
                    self.rawTransaction = nil
                    self.formatOutcome = nil
                    self.formatFailure = nil
                    self.debugCapture = nil
                    self.isFetching = false
                    self.isFormatting = false
                    self.error = "Transaction not found on Ethereum Mainnet."
                    self.activeDecodeID = nil
                }

            case .unavailable:
                await MainActor.run {
                    guard let self = self, self.activeDecodeID == decodeID else {
                        return
                    }
                    self.rawTransaction = nil
                    self.formatOutcome = nil
                    self.formatFailure = nil
                    self.debugCapture = nil
                    self.isFetching = false
                    self.isFormatting = false
                    self.error = "Transaction lookup is unavailable. Check ALCHEMY_API_KEY and network access."
                    self.activeDecodeID = nil
                }
            }
        }
    }

    private func resetForIdleError(_ message: String) {
        rawTransaction = nil
        formatOutcome = nil
        formatFailure = nil
        debugCapture = nil
        isFetching = false
        isFormatting = false
        error = message
        activeDecodeID = nil
    }

    private func updateCapture(_ update: (inout CalldataCapture) -> Void) {
        guard var capture = debugCapture else {
            return
        }
        update(&capture)
        debugCapture = capture
    }

    private static func makeCapture(from raw: DebugRawTransaction) -> CalldataCapture {
        var capture = CalldataCapture(
            method: "debug_transaction_hash",
            topic: "debug",
            requestId: raw.hash,
            chainId: "eip155:\(mainnetChainId)",
            rawParamsJson: raw.rawJSONString
        )
        capture.to = raw.to
        capture.from = raw.from
        capture.value = raw.valueHex
        capture.calldata = raw.input
        capture.selector = CalldataCapture.selectorHex(from: raw.input)
        capture.outcome = .paramsExtracted
        capture.notes.append("Fetched transaction \(raw.hash)")
        return capture
    }
}

struct DebugExampleTransaction: Identifiable, Equatable {
    let name: String
    let description: String
    let txHash: String
    let contractName: String

    var id: String { txHash }
}

private struct TransactionHashInput: View {
    @Binding var value: String
    let isLoading: Bool
    let onSubmit: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            TextField("Enter transaction hash (0x...)", text: $value)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .font(.caption.monospaced())
                .textFieldStyle(.roundedBorder)
                .onSubmit(onSubmit)

            Button(action: onSubmit) {
                if isLoading {
                    ProgressView()
                } else {
                    Label("Decode", systemImage: "play.fill")
                }
            }
            .buttonStyle(.borderedProminent)
            .disabled(isLoading || value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        }
    }
}

private struct DebugExampleTransactions: View {
    let examples: [DebugExampleTransaction]
    let selectedHash: String
    let onSelect: (DebugExampleTransaction) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Example Transactions")
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
                .textCase(.uppercase)

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(alignment: .top, spacing: 8) {
                    ForEach(examples) { example in
                        Button {
                            onSelect(example)
                        } label: {
                            VStack(alignment: .leading, spacing: 4) {
                                Text(example.name)
                                    .font(.subheadline.weight(.semibold))
                                Text(example.description)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(2)
                                Text(example.contractName)
                                    .font(.caption2.monospaced())
                                    .padding(.horizontal, 6)
                                    .padding(.vertical, 3)
                                    .background(Color.secondary.opacity(0.12), in: RoundedRectangle(cornerRadius: 4))
                            }
                            .frame(width: 210, alignment: .leading)
                            .padding(10)
                            .background(
                                RoundedRectangle(cornerRadius: 8)
                                    .fill(Color(uiColor: .secondarySystemGroupedBackground))
                            )
                            .overlay(
                                RoundedRectangle(cornerRadius: 8)
                                    .stroke(selectedHash == example.txHash ? Color.accentColor : Color.secondary.opacity(0.18), lineWidth: 1)
                            )
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.vertical, 2)
            }
        }
    }
}

private struct DebugResultsView: View {
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass

    let rawTransaction: DebugRawTransaction?
    let formatOutcome: FormatOutcome?
    let formatFailure: FormatFailure?
    let isFormatting: Bool
    let diagnosticCaptureJSON: String?

    var body: some View {
        Group {
            if horizontalSizeClass == .regular {
                HStack(alignment: .top, spacing: 16) {
                    rawColumn
                    clearColumn
                }
            } else {
                VStack(alignment: .leading, spacing: 16) {
                    rawColumn
                    clearColumn
                }
            }
        }
    }

    private var rawColumn: some View {
        Group {
            if let rawTransaction {
                RawTransactionCard(transaction: rawTransaction)
            }
        }
        .frame(maxWidth: .infinity, alignment: .topLeading)
    }

    private var clearColumn: some View {
        VStack(alignment: .leading, spacing: 12) {
            if isFormatting {
                DebugLoadingRow(message: "Formatting...")
                    .debugCard()
            }

            if let formatOutcome {
                DebugCard(title: "Clear Transaction") {
                    DisplayModelView(outcome: formatOutcome)
                }
            }

            if let formatFailure {
                ClearSigningFailureCard(failure: formatFailure)
            }

            if let diagnosticCaptureJSON {
                DiagnosticCaptureCard(json: diagnosticCaptureJSON)
            }
        }
        .frame(maxWidth: .infinity, alignment: .topLeading)
    }
}

private struct RawTransactionCard: View {
    let transaction: DebugRawTransaction
    @State private var showFullCalldata = false

    var body: some View {
        DebugCard(title: "Raw Transaction") {
            VStack(alignment: .leading, spacing: 0) {
                DebugFieldRow(label: "Hash", value: transaction.hash, monospaced: true)
                DebugFieldRow(label: "From", value: transaction.from, monospaced: true)
                DebugFieldRow(label: "To", value: transaction.to ?? "Contract creation", monospaced: transaction.to != nil)
                DebugFieldRow(label: "Value", value: transaction.valueDisplay)
                calldataRow
                DebugFieldRow(label: "Nonce", value: transaction.nonceDisplay)
                DebugFieldRow(label: "Gas", value: transaction.gasDisplay)
                if let blockNumber = transaction.blockNumberDisplay {
                    DebugFieldRow(label: "Block Number", value: blockNumber)
                }
                DebugFieldRow(label: "Type", value: transaction.typeDisplay)
            }
        }
    }

    private var calldataRow: some View {
        let shouldTruncate = transaction.input.count > 132
        let displayValue = showFullCalldata || !shouldTruncate
            ? transaction.input
            : String(transaction.input.prefix(132)) + "..."

        return VStack(alignment: .leading, spacing: 6) {
            DebugFieldRow(label: "Calldata", value: displayValue, monospaced: true)
            if shouldTruncate {
                Button(showFullCalldata ? "Show less" : "Show full") {
                    showFullCalldata.toggle()
                }
                .font(.caption)
                .padding(.leading, 116)
            }
        }
    }
}

private struct DebugCard<Content: View>: View {
    let title: String
    @ViewBuilder let content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(title)
                .font(.headline)
            content
        }
        .debugCard()
    }
}

private struct DebugFieldRow: View {
    let label: String
    let value: String
    var monospaced = false

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Text(label)
                .font(.footnote.weight(.semibold))
                .foregroundStyle(.secondary)
                .frame(width: 104, alignment: .leading)
            Text(value)
                .font(monospaced ? .caption.monospaced() : .footnote)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.vertical, 8)
        .overlay(alignment: .bottom) {
            Rectangle()
                .fill(Color.secondary.opacity(0.12))
                .frame(height: 0.5)
        }
    }
}

private struct ClearSigningFailureCard: View {
    let failure: FormatFailure

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Label(failure.displayLabel, systemImage: "xmark.shield.fill")
                .font(.headline)
                .foregroundStyle(.red)
            Text(failure.message)
                .font(.footnote)
                .foregroundStyle(.red)
                .textSelection(.enabled)
            Text(failure.retryable ? "Retryable failure" : "Blocking failure")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding()
        .background(Color.red.opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
    }
}

private struct DiagnosticCaptureCard: View {
    let json: String
    @State private var isExpanded = false

    var body: some View {
        DisclosureGroup("Diagnostic Capture", isExpanded: $isExpanded) {
            VStack(alignment: .leading, spacing: 8) {
                Button {
                    UIPasteboard.general.string = json
                } label: {
                    Label("Copy", systemImage: "doc.on.doc")
                }
                .buttonStyle(.bordered)

                Text(json)
                    .font(.caption2.monospaced())
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .padding(.top, 8)
        }
        .debugCard()
    }
}

private struct DebugErrorBanner: View {
    let message: String

    var body: some View {
        Label(message, systemImage: "exclamationmark.triangle.fill")
            .font(.footnote)
            .foregroundStyle(.red)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding()
            .background(Color.red.opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
    }
}

private struct DebugLoadingRow: View {
    let message: String

    var body: some View {
        HStack(spacing: 10) {
            ProgressView()
            Text(message)
                .font(.footnote)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

private extension View {
    func debugCard() -> some View {
        self
            .padding()
            .background(Color(uiColor: .secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 8))
            .overlay(
                RoundedRectangle(cornerRadius: 8)
                    .stroke(Color.secondary.opacity(0.16), lineWidth: 1)
            )
    }
}
