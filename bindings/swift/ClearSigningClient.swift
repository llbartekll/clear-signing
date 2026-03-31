import Foundation

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
    ) async throws -> DisplayModel {
        let transaction = TransactionInput(
            chainId: chainId,
            to: to,
            calldataHex: calldataHex,
            valueHex: valueHex,
            fromAddress: fromAddress
        )
        let descriptors = try await resolveDescriptorsForTx(transaction: transaction)
        return try await clearSigningFormatCalldata(
            descriptorsJson: descriptors,
            transaction: transaction,
            dataProvider: dataProvider
        )
    }

    public func formatTypedData(
        typedDataJson: String
    ) async throws -> DisplayModel {
        let descriptors = try await resolveDescriptorsForTypedData(typedDataJson: typedDataJson)
        return try await clearSigningFormatTypedData(
            descriptorsJson: descriptors,
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
    ) async throws -> [String] {
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
    ) async throws -> [String] {
        try await clearSigningResolveDescriptorsForTypedData(
            typedDataJson: typedDataJson,
            dataProvider: dataProvider
        )
    }

    private func resolveDescriptorsForTx(
        transaction: TransactionInput
    ) async throws -> [String] {
        try await clearSigningResolveDescriptorsForTx(
            transaction: transaction,
            dataProvider: dataProvider
        )
    }
}
