import Foundation
import os
import ReownWalletKit
import YttriumUtilsWrapper

private let log = Logger(subsystem: "com.lucidumbrella.wallet", category: "EvmSigningService")

final class EvmSigningService {
    static let shared = EvmSigningService()

    enum SigningError: LocalizedError {
        case missingProjectId
        case invalidParams(String)
        case addressMismatch(expected: String, got: String)

        var errorDescription: String? {
            switch self {
            case .missingProjectId:
                return "WalletConnectProjectID is missing"
            case .invalidParams(let reason):
                return "Invalid params: \(reason)"
            case .addressMismatch(let expected, let got):
                return "Requested address \(got) does not match imported key \(expected)"
            }
        }
    }

    private var cachedClient: EvmSigningClient?

    private func client() throws -> EvmSigningClient {
        if let cachedClient {
            return cachedClient
        }
        let projectId = Bundle.main.infoDictionary?["WalletConnectProjectID"] as? String ?? ""
        guard !projectId.isEmpty, projectId != "YOUR_PROJECT_ID_HERE" else {
            throw SigningError.missingProjectId
        }
        let metadata = PulseMetadata(
            url: nil,
            bundleId: Bundle.main.bundleIdentifier ?? "",
            sdkVersion: "lucid-umbrella-wallet-1.0",
            sdkPlatform: "mobile"
        )
        let client = EvmSigningClient(projectId: projectId, pulseMetadata: metadata)
        cachedClient = client
        return client
    }

    func extractTypedDataPayload(from params: AnyCodable) throws -> (address: String?, json: String) {
        let rawDump: String
        if let data = try? JSONEncoder().encode(params),
           let str = String(data: data, encoding: .utf8) {
            rawDump = str
        } else {
            rawDump = String(describing: params.value)
        }
        log.info("typed-data raw params: \(rawDump.prefix(800))")

        if let array = try? params.get([AnyCodable].self), array.count >= 2 {
            let address = array[0].value as? String
            let payload = array[1].value
            if let payloadString = payload as? String {
                log.info("typed-data payload extracted from [AnyCodable] as string, address=\(address ?? "nil")")
                return (address, payloadString)
            }
            let data = try JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys])
            guard let json = String(data: data, encoding: .utf8) else {
                throw SigningError.invalidParams("typed data payload not valid utf8")
            }
            log.info("typed-data payload extracted from [AnyCodable] object, address=\(address ?? "nil")")
            return (address, json)
        }

        if let array = try? params.get([String].self), array.count >= 2 {
            log.info("typed-data payload extracted from [String], address=\(array[0])")
            return (array[0], array[1])
        }

        let data = try params.getDataRepresentation()
        guard let json = String(data: data, encoding: .utf8) else {
            throw SigningError.invalidParams("typed data params not valid utf8")
        }
        log.info("typed-data payload extracted from raw data representation")
        return (nil, json)
    }

    func extractPersonalSignPayload(from params: AnyCodable) throws -> (message: Data, address: String?) {
        // Dump raw params for debugging
        let rawDump: String
        if let data = try? JSONEncoder().encode(params),
           let str = String(data: data, encoding: .utf8) {
            rawDump = str
        } else {
            rawDump = String(describing: params.value)
        }
        log.info("personal_sign raw params: \(rawDump.prefix(500))")

        var param0: String?
        var param1: String?

        // Strategy 1: decode as [AnyCodable]
        if let array = try? params.get([AnyCodable].self) {
            log.info("personal_sign decoded as [AnyCodable], count=\(array.count)")
            if array.count >= 1 {
                param0 = array[0].value as? String ?? (try? array[0].get(String.self))
            }
            if array.count >= 2 {
                param1 = array[1].value as? String ?? (try? array[1].get(String.self))
            }
        }

        // Strategy 2: decode as [String]
        if param0 == nil, let array = try? params.get([String].self) {
            log.info("personal_sign decoded as [String], count=\(array.count)")
            if array.count >= 1 { param0 = array[0] }
            if array.count >= 2 { param1 = array[1] }
        }

        // Strategy 3: params.value is already an array
        if param0 == nil, let array = params.value as? [Any] {
            log.info("personal_sign params.value is [Any], count=\(array.count)")
            if array.count >= 1 { param0 = array[0] as? String }
            if array.count >= 2 { param1 = array[1] as? String }
        }

        guard let first = param0 else {
            throw SigningError.invalidParams("personal_sign: could not extract params from: \(rawDump.prefix(200))")
        }

        log.info("personal_sign param0=\(first.prefix(40))... param1=\(param1?.prefix(40) ?? "nil")")

        // Auto-detect param order: standard is [message, address], some send [address, message]
        let firstIsAddress = first.count == 42 && first.hasPrefix("0x") &&
                             first.dropFirst(2).allSatisfy(\.isHexDigit)
        let secondIsAddress = param1.map { $0.count == 42 && $0.hasPrefix("0x") &&
                              $0.dropFirst(2).allSatisfy(\.isHexDigit) } ?? false

        let messageHex: String
        let address: String?

        if firstIsAddress && !secondIsAddress, let second = param1 {
            address = first
            messageHex = second
        } else {
            messageHex = first
            address = param1
        }

        let messageData: Data
        if messageHex.hasPrefix("0x") || messageHex.hasPrefix("0X") {
            let hex = String(messageHex.dropFirst(2))
            messageData = Data(hexString: hex) ?? Data(messageHex.utf8)
        } else {
            messageData = Data(messageHex.utf8)
        }

        return (messageData, address)
    }

    func signPersonalMessage(request: Request, privateKeyHex: String, expectedAddress: String) throws -> String {
        let payload = try extractPersonalSignPayload(from: request.params)
        if let requestedAddress = payload.address,
           requestedAddress.lowercased() != expectedAddress.lowercased() {
            throw SigningError.addressMismatch(expected: expectedAddress, got: requestedAddress)
        }
        return try Eip712Signer.signPersonalMessage(payload.message, privateKeyHex: privateKeyHex)
    }

    func signTypedData(request: Request, privateKeyHex: String, expectedAddress: String) throws -> String {
        let payload = try extractTypedDataPayload(from: request.params)
        log.info("signTypedData requested address=\(payload.address ?? "nil") expected address=\(expectedAddress)")
        if let requestedAddress = payload.address,
           requestedAddress.lowercased() != expectedAddress.lowercased() {
            log.error("typed-data address mismatch expected=\(expectedAddress) got=\(requestedAddress)")
            throw SigningError.addressMismatch(expected: expectedAddress, got: requestedAddress)
        }

        do {
            let signature = try Eip712Signer.sign(
                typedDataJson: payload.json,
                privateKeyHex: privateKeyHex
            )
            log.info("typed-data signing succeeded signature=\(signature.prefix(20))...")
            return signature
        } catch {
            log.error("typed-data signing failed: \(error)")
            throw error
        }
    }

    func signAndSend(request: Request, privateKeyHex: String, expectedAddress: String) async throws -> String {
        let tx = try extractTransactionParams(from: request.params)
        if let requestedFrom = tx.from,
           requestedFrom.lowercased() != expectedAddress.lowercased() {
            throw SigningError.addressMismatch(expected: expectedAddress, got: requestedFrom)
        }

        let from = tx.from ?? expectedAddress
        let dataValue = tx.data ?? tx.input
        let gasLimitValue = tx.gas ?? tx.gasLimit

        let has1559 = tx.maxFeePerGas != nil || tx.maxPriorityFeePerGas != nil
        let maxFee = tx.maxFeePerGas ?? (has1559 ? nil : tx.gasPrice)
        let maxPriority = tx.maxPriorityFeePerGas ?? (has1559 ? nil : tx.gasPrice)

        let params = SignAndSendParams(
            chainId: request.chainId.absoluteString,
            from: from,
            to: tx.to,
            value: tx.value,
            data: dataValue,
            gasLimit: gasLimitValue,
            maxFeePerGas: maxFee,
            maxPriorityFeePerGas: maxPriority,
            nonce: tx.nonce
        )

        let result = try await client().signAndSend(params: params, signer: privateKeyHex)
        return result.transactionHash
    }

    private func extractTransactionParams(from params: AnyCodable) throws -> TransactionParams {
        if let array = try? params.get([TransactionParams].self), let tx = array.first {
            return tx
        }
        if let single = try? params.get(TransactionParams.self) {
            return single
        }
        throw SigningError.invalidParams("transaction params")
    }

}

struct TransactionParams: Codable {
    let from: String?
    let to: String?
    let data: String?
    let input: String?
    let value: String?
    let gas: String?
    let gasLimit: String?
    let gasPrice: String?
    let maxFeePerGas: String?
    let maxPriorityFeePerGas: String?
    let nonce: String?
}
