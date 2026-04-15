import Foundation

public extension FormatOutcome {
    var model: DisplayModel {
        switch self {
        case .clearSigned(let model, _), .fallback(let model, _, _):
            return model
        }
    }

    var diagnostics: [FormatDiagnostic] {
        switch self {
        case .clearSigned(_, let diagnostics), .fallback(_, _, let diagnostics):
            return diagnostics
        }
    }

    var fallbackReason: FallbackReason? {
        switch self {
        case .clearSigned:
            return nil
        case .fallback(_, let reason, _):
            return reason
        }
    }

    var isClearSigned: Bool {
        if case .clearSigned = self {
            return true
        }
        return false
    }
}

public extension DescriptorResolutionOutcome {
    var descriptors: [String] {
        switch self {
        case .found(let descriptors):
            return descriptors
        case .notFound:
            return []
        }
    }
}

public extension FormatFailure {
    var message: String {
        switch self {
        case .InvalidInput(let message, _),
                .InvalidDescriptor(let message, _),
                .ResolutionFailed(let message, _),
                .Internal(let message, _):
            return message
        }
    }

    var retryable: Bool {
        switch self {
        case .InvalidInput(_, let retryable),
                .InvalidDescriptor(_, let retryable),
                .ResolutionFailed(_, let retryable),
                .Internal(_, let retryable):
            return retryable
        }
    }
}

public final class ClearSigningClient {
    private let dataProvider: DataProviderFfi

    public init(dataProvider: DataProviderFfi) {
        self.dataProvider = dataProvider
    }

    public func formatCalldata(
        chainId: UInt64,
        to: String,
        calldataHex: String,
        valueHex: String? = nil,
        fromAddress: String? = nil
    ) async throws -> FormatOutcome {
        let transaction = TransactionInput(
            chainId: chainId,
            to: to,
            calldataHex: calldataHex,
            valueHex: valueHex,
            fromAddress: fromAddress
        )
        let descriptors = try await resolveDescriptorsForTx(transaction: transaction)
        return try await clearSigningFormatCalldata(
            descriptorsJson: descriptors.descriptors,
            transaction: transaction,
            dataProvider: dataProvider
        )
    }

    public func formatTypedData(
        typedDataJson: String
    ) async throws -> FormatOutcome {
        let descriptors = try await resolveDescriptorsForTypedData(typedDataJson: typedDataJson)
        return try await clearSigningFormatTypedData(
            descriptorsJson: descriptors.descriptors,
            typedDataJson: typedDataJson,
            dataProvider: dataProvider
        )
    }

    public func resolveDescriptorsForTx(
        chainId: UInt64,
        to: String,
        calldataHex: String,
        valueHex: String? = nil,
        fromAddress: String? = nil
    ) async throws -> DescriptorResolutionOutcome {
        let transaction = TransactionInput(
            chainId: chainId,
            to: to,
            calldataHex: calldataHex,
            valueHex: valueHex,
            fromAddress: fromAddress
        )
        return try await resolveDescriptorsForTx(transaction: transaction)
    }

    public func resolveDescriptorsForTypedData(
        typedDataJson: String
    ) async throws -> DescriptorResolutionOutcome {
        try await clearSigningResolveDescriptorsForTypedData(
            typedDataJson: typedDataJson,
            dataProvider: dataProvider
        )
    }

    private func resolveDescriptorsForTx(
        transaction: TransactionInput
    ) async throws -> DescriptorResolutionOutcome {
        try await clearSigningResolveDescriptorsForTx(
            transaction: transaction,
            dataProvider: dataProvider
        )
    }
}
