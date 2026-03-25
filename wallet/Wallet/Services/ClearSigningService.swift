import Foundation
import Erc7730

struct ClearSigningService {

    private let dataProvider: DataProviderFfi

    init(dataProvider: DataProviderFfi) {
        self.dataProvider = dataProvider
    }

    /// Format a contract call using the ERC-7730 library.
    /// Resolves all descriptors (outer + nested calldata) from the GitHub registry, then formats.
    func formatCalldata(
        chainId: UInt64,
        to: String,
        calldata: String,
        value: String?,
        from: String?
    ) async -> Result<DisplayModel, Error> {
        do {
            let tx = TransactionInput(
                chainId: chainId,
                to: to,
                calldataHex: calldata,
                valueHex: value,
                fromAddress: from
            )

            // Single call resolves outer + any nested descriptors (e.g., Safe → inner contract).
            // Automatically detects proxies via dataProvider.getImplementationAddress().
            let descriptors = try await erc7730ResolveDescriptorsForTx(
                transaction: tx,
                dataProvider: dataProvider
            )

            let model = try await erc7730FormatCalldata(
                descriptorsJson: descriptors,
                transaction: tx,
                dataProvider: dataProvider
            )
            return .success(model)
        } catch {
            return .failure(error)
        }
    }

    /// Format EIP-712 typed data.
    /// Resolves all descriptors (outer + nested calldata) from the GitHub registry, then formats.
    /// Automatically detects proxies via dataProvider.getImplementationAddress().
    func formatTypedData(typedDataJson: String) async -> Result<DisplayModel, Error> {
        do {
            // Single call resolves outer EIP-712 descriptor + any nested calldata descriptors.
            // Automatically detects proxies via dataProvider.getImplementationAddress().
            let descriptors = try await erc7730ResolveDescriptorsForTypedData(
                typedDataJson: typedDataJson,
                dataProvider: dataProvider
            )

            let model = try await erc7730FormatTypedData(
                descriptorsJson: descriptors,
                typedDataJson: typedDataJson,
                dataProvider: dataProvider
            )
            return .success(model)
        } catch {
            return .failure(error)
        }
    }
}
