import Foundation
import Erc7730
import os

private let log = Logger(subsystem: "com.lucidumbrella.wallet", category: "ClearSigningService")

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
        let outcome = await formatTypedDataDetailed(typedDataJson: typedDataJson)
        if let model = outcome.model {
            return .success(model)
        }
        return .failure(
            outcome.error
                ?? NSError(domain: "ClearSigningService", code: -1, userInfo: nil)
        )
    }

    func formatTypedDataDetailed(typedDataJson: String) async -> TypedDataFormattingOutcome {
        do {
            log.info("Resolving typed-data descriptors")
            // Single call resolves outer EIP-712 descriptor + any nested calldata descriptors.
            // Automatically detects proxies via dataProvider.getImplementationAddress().
            let descriptors = try await erc7730ResolveDescriptorsForTypedData(
                typedDataJson: typedDataJson,
                dataProvider: dataProvider
            )
            let descriptorOwners = descriptors.compactMap(Self.descriptorOwner)
            log.info("Resolved \(descriptors.count) typed-data descriptors")

            do {
                log.info("Formatting typed data with resolved descriptors")
                let model = try await erc7730FormatTypedData(
                    descriptorsJson: descriptors,
                    typedDataJson: typedDataJson,
                    dataProvider: dataProvider
                )
                log.info("Typed-data formatting OK: intent=\(model.intent) warnings=\(model.warnings.count)")
                return TypedDataFormattingOutcome(
                    descriptorOwners: descriptorOwners,
                    model: model,
                    error: nil,
                    failedStage: nil
                )
            } catch {
                log.error("Typed-data formatting failed: \(error)")
                return TypedDataFormattingOutcome(
                    descriptorOwners: descriptorOwners,
                    model: nil,
                    error: error,
                    failedStage: .format
                )
            }
        } catch {
            log.error("Typed-data descriptor resolution failed: \(error)")
            return TypedDataFormattingOutcome(
                descriptorOwners: [],
                model: nil,
                error: error,
                failedStage: .resolve
            )
        }
    }

    private static func descriptorOwner(from descriptorJson: String) -> String? {
        guard let data = descriptorJson.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let metadata = object["metadata"] as? [String: Any] else {
            return nil
        }
        return metadata["owner"] as? String
    }
}
