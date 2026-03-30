import Foundation
import ClearSigning
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
        let outcome = await formatCalldataDetailed(
            chainId: chainId,
            to: to,
            calldata: calldata,
            value: value,
            from: from
        )
        if let model = outcome.model {
            return .success(model)
        }
        return .failure(
            outcome.error
                ?? NSError(domain: "ClearSigningService", code: -1, userInfo: nil)
        )
    }

    func formatCalldataDetailed(
        chainId: UInt64,
        to: String,
        calldata: String,
        value: String?,
        from: String?
    ) async -> CalldataFormattingOutcome {
        let implementationAddress = dataProvider.getImplementationAddress(chainId: chainId, address: to)
        let matchedAddress = implementationAddress ?? to
        let usedImplementationAddress = implementationAddress.map {
            $0.caseInsensitiveCompare(to) != .orderedSame
        } ?? false

        do {
            let tx = TransactionInput(
                chainId: chainId,
                to: to,
                calldataHex: calldata,
                valueHex: value,
                fromAddress: from
            )

            log.info(
                "Resolving calldata descriptors to=\(to) matched=\(matchedAddress) chainId=\(chainId)"
            )
            let descriptors = try await clearSigningResolveDescriptorsForTx(
                transaction: tx,
                dataProvider: dataProvider
            )
            let descriptorOwners = descriptors.compactMap(Self.descriptorOwner)
            log.info("Resolved \(descriptors.count) calldata descriptors")

            do {
                let model = try await clearSigningFormatCalldata(
                    descriptorsJson: descriptors,
                    transaction: tx,
                    dataProvider: dataProvider
                )
                log.info("Calldata formatting OK: intent=\(model.intent) warnings=\(model.warnings.count)")
                return CalldataFormattingOutcome(
                    descriptorOwners: descriptorOwners,
                    resolvedDescriptorsJson: descriptors,
                    model: model,
                    error: nil,
                    failedStage: nil,
                    implementationAddress: implementationAddress,
                    matchedAddress: matchedAddress,
                    selectedDescriptorAddress: matchedAddress,
                    usedImplementationAddress: usedImplementationAddress
                )
            } catch {
                log.error("Calldata formatting failed: \(error)")
                return CalldataFormattingOutcome(
                    descriptorOwners: descriptorOwners,
                    resolvedDescriptorsJson: descriptors,
                    model: nil,
                    error: error,
                    failedStage: .format,
                    implementationAddress: implementationAddress,
                    matchedAddress: matchedAddress,
                    selectedDescriptorAddress: matchedAddress,
                    usedImplementationAddress: usedImplementationAddress
                )
            }
        } catch {
            log.error("Calldata descriptor resolution failed: \(error)")
            return CalldataFormattingOutcome(
                descriptorOwners: [],
                resolvedDescriptorsJson: [],
                model: nil,
                error: error,
                failedStage: .resolve,
                implementationAddress: implementationAddress,
                matchedAddress: matchedAddress,
                selectedDescriptorAddress: matchedAddress,
                usedImplementationAddress: usedImplementationAddress
            )
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
            let descriptors = try await clearSigningResolveDescriptorsForTypedData(
                typedDataJson: typedDataJson,
                dataProvider: dataProvider
            )
            let descriptorOwners = descriptors.compactMap(Self.descriptorOwner)
            log.info("Resolved \(descriptors.count) typed-data descriptors")

            do {
                log.info("Formatting typed data with resolved descriptors")
                let model = try await clearSigningFormatTypedData(
                    descriptorsJson: descriptors,
                    typedDataJson: typedDataJson,
                    dataProvider: dataProvider
                )
                log.info("Typed-data formatting OK: intent=\(model.intent) warnings=\(model.warnings.count)")
                return TypedDataFormattingOutcome(
                    descriptorOwners: descriptorOwners,
                    resolvedDescriptorsJson: descriptors,
                    model: model,
                    error: nil,
                    failedStage: nil
                )
            } catch {
                log.error("Typed-data formatting failed: \(error)")
                return TypedDataFormattingOutcome(
                    descriptorOwners: descriptorOwners,
                    resolvedDescriptorsJson: descriptors,
                    model: nil,
                    error: error,
                    failedStage: .format
                )
            }
        } catch {
            log.error("Typed-data descriptor resolution failed: \(error)")
            return TypedDataFormattingOutcome(
                descriptorOwners: [],
                resolvedDescriptorsJson: [],
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
