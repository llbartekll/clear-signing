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
    /// Resolves descriptors from the GitHub registry, then formats.
    /// Automatically detects proxies via dataProvider.getImplementationAddress().
    func formatTypedData(typedDataJson: String) async -> Result<DisplayModel, Error> {
        do {
            // Parse domain + primaryType for descriptor resolution
            let info = parseTypedDataInfo(from: typedDataJson)
            let descriptor: String? = if let chainId = info.chainId,
                                         let address = info.verifyingContract,
                                         let primaryType = info.primaryType {
                try await erc7730ResolveDescriptorForTypedData(
                    chainId: chainId,
                    verifyingContract: address,
                    primaryType: primaryType,
                    dataProvider: dataProvider
                )
            } else {
                nil
            }
            let descriptors = descriptor.map { [$0] } ?? []

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

    // MARK: - Private

    private struct TypedDataInfo {
        let chainId: UInt64?
        let verifyingContract: String?
        let primaryType: String?
    }

    private func parseTypedDataInfo(from json: String) -> TypedDataInfo {
        guard let data = json.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return TypedDataInfo(chainId: nil, verifyingContract: nil, primaryType: nil)
        }
        let domain = obj["domain"] as? [String: Any]
        let chainId = (domain?["chainId"] as? NSNumber)?.uint64Value
        let contract = domain?["verifyingContract"] as? String
        let primaryType = obj["primaryType"] as? String
        return TypedDataInfo(chainId: chainId, verifyingContract: contract, primaryType: primaryType)
    }
}
