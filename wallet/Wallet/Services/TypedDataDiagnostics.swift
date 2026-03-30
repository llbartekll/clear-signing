import Foundation
import ClearSigning
import ReownWalletKit

struct TypedDataSummary: Codable {
    let primaryType: String?
    let domainName: String?
    let domainVersion: String?
    let domainChainId: String?
    let verifyingContract: String?
    let typeNames: [String]

    static func from(json: String) -> TypedDataSummary? {
        guard let data = json.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return nil
        }

        let domain = object["domain"] as? [String: Any]
        let types = object["types"] as? [String: Any]

        return TypedDataSummary(
            primaryType: object["primaryType"] as? String,
            domainName: domain?["name"] as? String,
            domainVersion: domain?["version"] as? String,
            domainChainId: stringify(domain?["chainId"]),
            verifyingContract: domain?["verifyingContract"] as? String,
            typeNames: types.map { Array($0.keys).sorted() } ?? []
        )
    }

    private static func stringify(_ value: Any?) -> String? {
        switch value {
        case let string as String:
            return string
        case let number as NSNumber:
            return number.stringValue
        default:
            return nil
        }
    }
}

struct TypedDataCapture: Identifiable, Codable {
    enum Outcome: String, Codable {
        case received
        case payloadExtracted
        case payloadExtractionFailed
        case clearSigningSucceeded
        case clearSigningFailed
        case signingSucceeded
        case signingFailed
        case rejected
        case unsupportedMethod
    }

    let id: UUID
    let timestamp: Date
    let method: String
    let topic: String
    let requestId: String
    let chainId: String

    var outcome: Outcome
    var failedStage: TypedDataFormattingOutcome.Stage?
    var rawParamsJson: String?
    var typedDataJson: String?
    var requestedAddress: String?
    var expectedAddress: String?
    var summary: TypedDataSummary?
    var descriptorCount: Int?
    var descriptorOwners: [String]?
    var resolvedDescriptorsJson: [String]?
    var selectedDescriptorOwner: String?
    var clearSigningIntent: String?
    var clearSigningInterpolatedIntent: String?
    var clearSigningWarnings: [String]?
    var clearSigningEntryPreview: [String]?
    var clearSigningError: String?
    var errorDescription: String?
    var signerError: String?
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
        self.typedDataJson = nil
        self.requestedAddress = nil
        self.expectedAddress = nil
        self.summary = nil
        self.descriptorCount = nil
        self.descriptorOwners = nil
        self.resolvedDescriptorsJson = nil
        self.selectedDescriptorOwner = nil
        self.clearSigningIntent = nil
        self.clearSigningInterpolatedIntent = nil
        self.clearSigningWarnings = nil
        self.clearSigningEntryPreview = nil
        self.clearSigningError = nil
        self.errorDescription = nil
        self.signerError = nil
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
        self.typedDataJson = nil
        self.requestedAddress = nil
        self.expectedAddress = nil
        self.summary = nil
        self.descriptorCount = nil
        self.descriptorOwners = nil
        self.resolvedDescriptorsJson = nil
        self.selectedDescriptorOwner = nil
        self.clearSigningIntent = nil
        self.clearSigningInterpolatedIntent = nil
        self.clearSigningWarnings = nil
        self.clearSigningEntryPreview = nil
        self.clearSigningError = nil
        self.errorDescription = nil
        self.signerError = nil
        self.notes = []
    }

    var exportJSONString: String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        encoder.dateEncodingStrategy = .iso8601
        guard let data = try? encoder.encode(self),
              let string = String(data: data, encoding: .utf8) else {
            return "{ \"error\": \"failed to encode typed data capture\" }"
        }
        return string
    }

    mutating func applyClearSigningSuccess(_ formattingOutcome: TypedDataFormattingOutcome) {
        outcome = .clearSigningSucceeded
        failedStage = nil
        descriptorCount = formattingOutcome.descriptorOwners.count
        descriptorOwners = formattingOutcome.descriptorOwners
        resolvedDescriptorsJson = formattingOutcome.resolvedDescriptorsJson
        selectedDescriptorOwner = nil
        clearSigningIntent = nil
        clearSigningInterpolatedIntent = nil
        clearSigningWarnings = nil
        clearSigningEntryPreview = nil
        clearSigningError = nil
        errorDescription = nil

        guard let model = formattingOutcome.model else {
            return
        }

        selectedDescriptorOwner = model.owner
        clearSigningIntent = model.intent
        clearSigningInterpolatedIntent = model.interpolatedIntent
        clearSigningWarnings = model.warnings
        clearSigningEntryPreview = previewEntries(from: model)
    }

    mutating func applyClearSigningFailure(_ formattingOutcome: TypedDataFormattingOutcome) {
        outcome = .clearSigningFailed
        failedStage = formattingOutcome.failedStage
        descriptorCount = formattingOutcome.descriptorOwners.count
        descriptorOwners = formattingOutcome.descriptorOwners
        resolvedDescriptorsJson = formattingOutcome.resolvedDescriptorsJson
        selectedDescriptorOwner = nil
        clearSigningIntent = nil
        clearSigningInterpolatedIntent = nil
        clearSigningWarnings = nil
        clearSigningEntryPreview = nil
        clearSigningError = formattingOutcome.error?.localizedDescription
        errorDescription = formattingOutcome.error?.localizedDescription
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

struct TypedDataFormattingOutcome {
    enum Stage: String, Codable {
        case resolve
        case format
    }

    let descriptorOwners: [String]
    let resolvedDescriptorsJson: [String]
    let model: DisplayModel?
    let error: Error?
    let failedStage: Stage?
}
