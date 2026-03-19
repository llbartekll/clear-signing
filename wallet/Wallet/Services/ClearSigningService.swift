import Foundation
import Erc7730

struct ClearSigningService {

    private let dataProvider: DataProviderFfi

    init(dataProvider: DataProviderFfi) {
        self.dataProvider = dataProvider
    }

    /// Format a contract call using the ERC-7730 library.
    /// Resolves descriptors from the GitHub registry, then formats.
    func formatCalldata(
        chainId: UInt64,
        to: String,
        calldata: String,
        value: String?,
        from: String?,
        implementationAddress: String? = nil
    ) async -> Result<DisplayModel, Error> {
        do {
            let resolveAddr = implementationAddress ?? to
            let descriptor = try await erc7730ResolveDescriptor(
                chainId: chainId,
                address: resolveAddr
            )
            let descriptors = descriptor.map { [$0] } ?? []

            let tx = TransactionInput(
                chainId: chainId,
                to: to,
                calldataHex: calldata,
                valueHex: value,
                fromAddress: from,
                implementationAddress: implementationAddress
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
    func formatTypedData(typedDataJson: String) async -> Result<DisplayModel, Error> {
        do {
            // Parse domain to get chainId + verifyingContract for resolution
            let domainInfo = parseDomainInfo(from: typedDataJson)
            let descriptor: String? = if let chainId = domainInfo.chainId,
                                         let address = domainInfo.verifyingContract {
                try await erc7730ResolveDescriptor(
                    chainId: chainId,
                    address: address
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

    private struct DomainInfo {
        let chainId: UInt64?
        let verifyingContract: String?
    }

    private func parseDomainInfo(from json: String) -> DomainInfo {
        guard let data = json.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let domain = obj["domain"] as? [String: Any] else {
            return DomainInfo(chainId: nil, verifyingContract: nil)
        }
        let chainId = (domain["chainId"] as? NSNumber)?.uint64Value
        let contract = domain["verifyingContract"] as? String
        return DomainInfo(chainId: chainId, verifyingContract: contract)
    }
}
