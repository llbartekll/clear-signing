import Foundation
import ClearSigning
import ReownWalletKit

struct CalldataCapture: Identifiable, Codable {
    enum Outcome: String, Codable {
        case received
        case paramsExtracted
        case paramsExtractionFailed
        case clearSigningSucceeded
        case clearSigningFallback
        case clearSigningFailed
        case signingSucceeded
        case signingFailed
        case rejected
    }

    let id: UUID
    let timestamp: Date
    let method: String
    let topic: String
    let requestId: String
    let chainId: String

    var outcome: Outcome
    var failedStage: CalldataFormattingOutcome.Stage?
    var rawParamsJson: String?
    var to: String?
    var from: String?
    var value: String?
    var calldata: String?
    var selector: String?
    var implementationAddress: String?
    var matchedAddress: String?
    var selectedDescriptorAddress: String?
    var usedImplementationAddress: Bool?
    var expectedAddress: String?
    var descriptorCount: Int?
    var descriptorOwners: [String]?
    var resolvedDescriptorsJson: [String]?
    var selectedDescriptorOwner: String?
    var clearSigningOutcomeKind: ClearSigningOutcomeKind?
    var clearSigningFallbackReason: String?
    var clearSigningDiagnostics: [CapturedFormatDiagnostic]?
    var clearSigningIntent: String?
    var clearSigningInterpolatedIntent: String?
    var clearSigningEntryPreview: [String]?
    var clearSigningFailureType: String?
    var clearSigningFailureRetryable: Bool?
    var clearSigningError: String?
    var errorDescription: String?
    var signingError: String?
    var notes: [String]

    init(request: Request, rawParamsJson: String?) {
        self.id = UUID()
        self.timestamp = Date()
        self.method = request.method
        self.topic = request.topic
        self.requestId = String(describing: request.id)
        self.chainId = request.chainId.absoluteString
        self.outcome = .received
        self.failedStage = nil
        self.rawParamsJson = rawParamsJson
        self.to = nil
        self.from = nil
        self.value = nil
        self.calldata = nil
        self.selector = nil
        self.implementationAddress = nil
        self.matchedAddress = nil
        self.selectedDescriptorAddress = nil
        self.usedImplementationAddress = nil
        self.expectedAddress = nil
        self.descriptorCount = nil
        self.descriptorOwners = nil
        self.resolvedDescriptorsJson = nil
        self.selectedDescriptorOwner = nil
        self.clearSigningOutcomeKind = nil
        self.clearSigningFallbackReason = nil
        self.clearSigningDiagnostics = nil
        self.clearSigningIntent = nil
        self.clearSigningInterpolatedIntent = nil
        self.clearSigningEntryPreview = nil
        self.clearSigningFailureType = nil
        self.clearSigningFailureRetryable = nil
        self.clearSigningError = nil
        self.errorDescription = nil
        self.signingError = nil
        self.notes = []
    }

    init(
        method: String,
        topic: String,
        requestId: String,
        chainId: String,
        rawParamsJson: String?
    ) {
        self.id = UUID()
        self.timestamp = Date()
        self.method = method
        self.topic = topic
        self.requestId = requestId
        self.chainId = chainId
        self.outcome = .received
        self.failedStage = nil
        self.rawParamsJson = rawParamsJson
        self.to = nil
        self.from = nil
        self.value = nil
        self.calldata = nil
        self.selector = nil
        self.implementationAddress = nil
        self.matchedAddress = nil
        self.selectedDescriptorAddress = nil
        self.usedImplementationAddress = nil
        self.expectedAddress = nil
        self.descriptorCount = nil
        self.descriptorOwners = nil
        self.resolvedDescriptorsJson = nil
        self.selectedDescriptorOwner = nil
        self.clearSigningOutcomeKind = nil
        self.clearSigningFallbackReason = nil
        self.clearSigningDiagnostics = nil
        self.clearSigningIntent = nil
        self.clearSigningInterpolatedIntent = nil
        self.clearSigningEntryPreview = nil
        self.clearSigningFailureType = nil
        self.clearSigningFailureRetryable = nil
        self.clearSigningError = nil
        self.errorDescription = nil
        self.signingError = nil
        self.notes = []
    }

    var exportJSONString: String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        encoder.dateEncodingStrategy = .iso8601
        guard let data = try? encoder.encode(self),
              let string = String(data: data, encoding: .utf8) else {
            return "{ \"error\": \"failed to encode calldata capture\" }"
        }
        return string
    }

    mutating func applyClearSigningSuccess(_ outcome: CalldataFormattingOutcome) {
        guard let formatOutcome = outcome.formatOutcome else {
            return
        }

        self.outcome = formatOutcome.isClearSigned ? .clearSigningSucceeded : .clearSigningFallback
        failedStage = nil
        descriptorCount = outcome.descriptorOwners.count
        descriptorOwners = outcome.descriptorOwners
        resolvedDescriptorsJson = outcome.resolvedDescriptorsJson
        implementationAddress = outcome.implementationAddress
        matchedAddress = outcome.matchedAddress
        selectedDescriptorAddress = outcome.selectedDescriptorAddress
        usedImplementationAddress = outcome.usedImplementationAddress
        clearSigningOutcomeKind = formatOutcome.outcomeKind
        clearSigningFallbackReason = formatOutcome.fallbackReason?.captureValue
        clearSigningDiagnostics = formatOutcome.diagnostics.map(CapturedFormatDiagnostic.init)
        selectedDescriptorOwner = formatOutcome.model.owner
        clearSigningIntent = formatOutcome.model.intent
        clearSigningInterpolatedIntent = formatOutcome.model.interpolatedIntent
        clearSigningEntryPreview = previewEntries(from: formatOutcome.model)
        clearSigningFailureType = nil
        clearSigningFailureRetryable = nil
        clearSigningError = nil
        errorDescription = nil
    }

    mutating func applyClearSigningFailure(_ outcome: CalldataFormattingOutcome) {
        self.outcome = .clearSigningFailed
        failedStage = outcome.failedStage
        descriptorCount = outcome.descriptorOwners.count
        descriptorOwners = outcome.descriptorOwners
        resolvedDescriptorsJson = outcome.resolvedDescriptorsJson
        implementationAddress = outcome.implementationAddress
        matchedAddress = outcome.matchedAddress
        selectedDescriptorAddress = outcome.selectedDescriptorAddress
        usedImplementationAddress = outcome.usedImplementationAddress
        selectedDescriptorOwner = nil
        clearSigningOutcomeKind = .failure
        clearSigningFallbackReason = nil
        clearSigningDiagnostics = nil
        clearSigningIntent = nil
        clearSigningInterpolatedIntent = nil
        clearSigningEntryPreview = nil
        clearSigningFailureType = outcome.formatFailure?.captureValue
        clearSigningFailureRetryable = outcome.formatFailure?.retryable
        clearSigningError = outcome.formatFailure?.message
        errorDescription = outcome.formatFailure?.message
    }

    static func selectorHex(from calldata: String?) -> String? {
        guard let calldata else {
            return nil
        }

        let normalized = calldata
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()
        let hexBody = normalized.hasPrefix("0x") ? String(normalized.dropFirst(2)) : normalized

        guard hexBody.count >= 8 else {
            return nil
        }

        return "0x" + String(hexBody.prefix(8))
    }

    private func previewEntries(from model: DisplayModel) -> [String] {
        model.entries.prefix(8).compactMap { entry in
            switch entry {
            case .item(let item):
                return "\(item.label): \(item.value)"
            case .group(let label, _, let items):
                return "\(label): \(items.count) item(s)"
            case .nested(let label, _, _):
                return "\(label): nested"
            }
        }
    }
}

struct CalldataFormattingOutcome {
    enum Stage: String, Codable {
        case resolve
        case format
    }

    let descriptorOwners: [String]
    let resolvedDescriptorsJson: [String]
    let resolutionOutcome: DescriptorResolutionOutcome?
    let formatOutcome: FormatOutcome?
    let formatFailure: FormatFailure?
    let failedStage: Stage?
    let implementationAddress: String?
    let matchedAddress: String?
    let selectedDescriptorAddress: String?
    let usedImplementationAddress: Bool
}
