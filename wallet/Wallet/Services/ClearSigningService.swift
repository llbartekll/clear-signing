import Foundation
import ClearSigning
import os

private let log = Logger(subsystem: "com.lucidumbrella.wallet", category: "ClearSigningService")

struct ClearSigningService {

    private let dataProvider: DataProviderFfi
    private let client: ClearSigningClient

    init(dataProvider: DataProviderFfi) {
        self.dataProvider = dataProvider
        self.client = ClearSigningClient(dataProvider: dataProvider)
    }

    /// Format a contract call using the ERC-7730 library.
    /// Resolves all descriptors (outer + nested calldata) from the GitHub registry, then formats.
    func formatCalldata(
        chainId: UInt64,
        to: String,
        calldata: String,
        value: String?,
        from: String?
    ) async -> Result<FormatOutcome, FormatFailure> {
        do {
            let outcome = try await client.formatCalldata(
                chainId: chainId,
                to: to,
                calldataHex: calldata,
                valueHex: value,
                fromAddress: from
            )
            return .success(outcome)
        } catch {
            return .failure(Self.coerceFailure(error))
        }
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
            let resolution = try await client.resolveDescriptorsForTx(
                chainId: chainId,
                to: to,
                calldataHex: calldata,
                valueHex: value,
                fromAddress: from
            )
            let descriptors = resolution.descriptors
            let descriptorOwners = descriptors.compactMap(Self.descriptorOwner)
            switch resolution {
            case .found(let resolved):
                log.info("Resolved \(resolved.count) calldata descriptors")
            case .notFound:
                log.info("No calldata descriptors resolved; formatting will fall back")
            }

            do {
                let formatOutcome = try await clearSigningFormatCalldata(
                    descriptorsJson: descriptors,
                    transaction: tx,
                    dataProvider: dataProvider
                )
                log.info("Calldata formatting OK: \(Self.describe(formatOutcome))")
                return CalldataFormattingOutcome(
                    descriptorOwners: descriptorOwners,
                    resolvedDescriptorsJson: descriptors,
                    resolutionOutcome: resolution,
                    formatOutcome: formatOutcome,
                    formatFailure: nil,
                    failedStage: nil,
                    implementationAddress: implementationAddress,
                    matchedAddress: matchedAddress,
                    selectedDescriptorAddress: matchedAddress,
                    usedImplementationAddress: usedImplementationAddress
                )
            } catch {
                let failure = Self.coerceFailure(error)
                log.error("Calldata formatting failed: \(failure.message)")
                return CalldataFormattingOutcome(
                    descriptorOwners: descriptorOwners,
                    resolvedDescriptorsJson: descriptors,
                    resolutionOutcome: resolution,
                    formatOutcome: nil,
                    formatFailure: failure,
                    failedStage: .format,
                    implementationAddress: implementationAddress,
                    matchedAddress: matchedAddress,
                    selectedDescriptorAddress: matchedAddress,
                    usedImplementationAddress: usedImplementationAddress
                )
            }
        } catch {
            let failure = Self.coerceFailure(error)
            log.error("Calldata descriptor resolution failed: \(failure.message)")
            return CalldataFormattingOutcome(
                descriptorOwners: [],
                resolvedDescriptorsJson: [],
                resolutionOutcome: nil,
                formatOutcome: nil,
                formatFailure: failure,
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
    func formatTypedData(typedDataJson: String) async -> Result<FormatOutcome, FormatFailure> {
        do {
            let outcome = try await client.formatTypedData(typedDataJson: typedDataJson)
            return .success(outcome)
        } catch {
            return .failure(Self.coerceFailure(error))
        }
    }

    func formatTypedDataDetailed(typedDataJson: String) async -> TypedDataFormattingOutcome {
        do {
            log.info("Resolving typed-data descriptors")
            // Single call resolves outer EIP-712 descriptor + any nested calldata descriptors.
            // Automatically detects proxies via dataProvider.getImplementationAddress().
            let resolution = try await client.resolveDescriptorsForTypedData(
                typedDataJson: typedDataJson
            )
            let descriptors = resolution.descriptors
            let descriptorOwners = descriptors.compactMap(Self.descriptorOwner)
            switch resolution {
            case .found(let resolved):
                log.info("Resolved \(resolved.count) typed-data descriptors")
            case .notFound:
                log.info("No typed-data descriptors resolved; formatting will fall back")
            }

            do {
                log.info("Formatting typed data with resolved descriptors")
                let formatOutcome = try await clearSigningFormatTypedData(
                    descriptorsJson: descriptors,
                    typedDataJson: typedDataJson,
                    dataProvider: dataProvider
                )
                log.info("Typed-data formatting OK: \(Self.describe(formatOutcome))")
                return TypedDataFormattingOutcome(
                    descriptorOwners: descriptorOwners,
                    resolvedDescriptorsJson: descriptors,
                    resolutionOutcome: resolution,
                    formatOutcome: formatOutcome,
                    formatFailure: nil,
                    failedStage: nil
                )
            } catch {
                let failure = Self.coerceFailure(error)
                log.error("Typed-data formatting failed: \(failure.message)")
                return TypedDataFormattingOutcome(
                    descriptorOwners: descriptorOwners,
                    resolvedDescriptorsJson: descriptors,
                    resolutionOutcome: resolution,
                    formatOutcome: nil,
                    formatFailure: failure,
                    failedStage: .format
                )
            }
        } catch {
            let failure = Self.coerceFailure(error)
            log.error("Typed-data descriptor resolution failed: \(failure.message)")
            return TypedDataFormattingOutcome(
                descriptorOwners: [],
                resolvedDescriptorsJson: [],
                resolutionOutcome: nil,
                formatOutcome: nil,
                formatFailure: failure,
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

    private static func coerceFailure(_ error: Error) -> FormatFailure {
        if let failure = error as? FormatFailure {
            return failure
        }
        return .Internal(message: error.localizedDescription, retryable: false)
    }

    private static func describe(_ outcome: FormatOutcome) -> String {
        switch outcome {
        case .clearSigned(let model, let diagnostics):
            return "clearSigned intent=\(model.intent) diagnostics=\(diagnostics.count)"
        case .fallback(let model, let reason, let diagnostics):
            return "fallback reason=\(reason.displayLabel) intent=\(model.intent) diagnostics=\(diagnostics.count)"
        }
    }
}

enum ClearSigningOutcomeKind: String, Codable {
    case clearSigned
    case fallback
    case failure
}

struct CapturedFormatDiagnostic: Codable {
    let code: String
    let severity: String
    let message: String

    init(_ diagnostic: FormatDiagnostic) {
        code = diagnostic.code
        severity = diagnostic.severity.captureValue
        message = diagnostic.message
    }
}

extension DiagnosticSeverity {
    var captureValue: String {
        switch self {
        case .info:
            return "info"
        case .warning:
            return "warning"
        }
    }

    var displayLabel: String {
        switch self {
        case .info:
            return "Info"
        case .warning:
            return "Warning"
        }
    }
}

extension FallbackReason {
    var captureValue: String {
        switch self {
        case .descriptorNotFound:
            return "descriptor_not_found"
        case .formatNotFound:
            return "format_not_found"
        case .nestedCallNotClearSigned:
            return "nested_call_not_clear_signed"
        case .insufficientContext:
            return "insufficient_context"
        }
    }

    var displayLabel: String {
        switch self {
        case .descriptorNotFound:
            return "Descriptor not found"
        case .formatNotFound:
            return "Format not found"
        case .nestedCallNotClearSigned:
            return "Nested call not clear-signed"
        case .insufficientContext:
            return "Insufficient context"
        }
    }
}

extension FormatOutcome {
    var outcomeKind: ClearSigningOutcomeKind {
        fallbackReason == nil ? .clearSigned : .fallback
    }
}

extension FormatFailure {
    var captureValue: String {
        switch self {
        case .InvalidInput:
            return "invalid_input"
        case .InvalidDescriptor:
            return "invalid_descriptor"
        case .ResolutionFailed:
            return "resolution_failed"
        case .Internal:
            return "internal"
        }
    }

    var displayLabel: String {
        switch self {
        case .InvalidInput:
            return "Invalid input"
        case .InvalidDescriptor:
            return "Invalid descriptor"
        case .ResolutionFailed:
            return "Resolution failed"
        case .Internal:
            return "Internal error"
        }
    }
}
