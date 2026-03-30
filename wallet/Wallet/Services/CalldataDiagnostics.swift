import Foundation
import ClearSigning
import ReownWalletKit

struct CalldataCapture: Identifiable, Codable {
    enum Outcome: String, Codable {
        case received
        case paramsExtracted
        case paramsExtractionFailed
        case clearSigningSucceeded
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
    var rawParamsJson: String?
    var to: String?
    var from: String?
    var value: String?
    var calldata: String?
    var implementationAddress: String?
    var matchedAddress: String?
    var usedImplementationAddress: Bool?
    var expectedAddress: String?
    var descriptorCount: Int?
    var descriptorOwners: [String]?
    var selectedDescriptorOwner: String?
    var clearSigningIntent: String?
    var clearSigningInterpolatedIntent: String?
    var clearSigningWarnings: [String]?
    var clearSigningEntryPreview: [String]?
    var clearSigningError: String?
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
        self.rawParamsJson = rawParamsJson
        self.to = nil
        self.from = nil
        self.value = nil
        self.calldata = nil
        self.implementationAddress = nil
        self.matchedAddress = nil
        self.usedImplementationAddress = nil
        self.expectedAddress = nil
        self.descriptorCount = nil
        self.descriptorOwners = nil
        self.selectedDescriptorOwner = nil
        self.clearSigningIntent = nil
        self.clearSigningInterpolatedIntent = nil
        self.clearSigningWarnings = nil
        self.clearSigningEntryPreview = nil
        self.clearSigningError = nil
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
        self.outcome = .clearSigningSucceeded
        descriptorCount = outcome.descriptorOwners.count
        descriptorOwners = outcome.descriptorOwners
        implementationAddress = outcome.implementationAddress
        matchedAddress = outcome.matchedAddress
        usedImplementationAddress = outcome.usedImplementationAddress

        if let model = outcome.model {
            selectedDescriptorOwner = model.owner
            clearSigningIntent = model.intent
            clearSigningInterpolatedIntent = model.interpolatedIntent
            clearSigningWarnings = model.warnings
            clearSigningEntryPreview = previewEntries(from: model)
        }
    }

    mutating func applyClearSigningFailure(_ outcome: CalldataFormattingOutcome) {
        self.outcome = .clearSigningFailed
        descriptorCount = outcome.descriptorOwners.count
        descriptorOwners = outcome.descriptorOwners
        implementationAddress = outcome.implementationAddress
        matchedAddress = outcome.matchedAddress
        usedImplementationAddress = outcome.usedImplementationAddress
        clearSigningError = outcome.error?.localizedDescription
    }

    private func previewEntries(from model: DisplayModel) -> [String] {
        model.entries.prefix(8).compactMap { entry in
            switch entry {
            case .item(let item):
                return "\(item.label): \(item.value)"
            case .group(let label, _, let items):
                return "\(label): \(items.count) item(s)"
            case .nested(let label, _, _, _):
                return "\(label): nested"
            }
        }
    }
}

struct CalldataFormattingOutcome {
    enum Stage: String {
        case resolve
        case format
    }

    let descriptorOwners: [String]
    let model: DisplayModel?
    let error: Error?
    let failedStage: Stage?
    let implementationAddress: String?
    let matchedAddress: String?
    let usedImplementationAddress: Bool
}
