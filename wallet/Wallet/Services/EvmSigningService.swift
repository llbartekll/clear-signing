import Foundation
import ReownWalletKit
import YttriumUtilsWrapper

final class EvmSigningService {
    static let shared = EvmSigningService()

    enum SigningError: LocalizedError {
        case missingProjectId
        case invalidParams(String)
        case addressMismatch(expected: String, got: String)
        case invalidSignatureResponse

        var errorDescription: String? {
            switch self {
            case .missingProjectId:
                return "WalletConnectProjectID is missing"
            case .invalidParams(let reason):
                return "Invalid params: \(reason)"
            case .addressMismatch(let expected, let got):
                return "Requested address \(got) does not match imported key \(expected)"
            case .invalidSignatureResponse:
                return "Invalid signature response"
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
        if let array = try? params.get([AnyCodable].self), array.count >= 2 {
            let address = array[0].value as? String
            let payload = array[1].value
            if let payloadString = payload as? String {
                return (address, payloadString)
            }
            let data = try JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys])
            guard let json = String(data: data, encoding: .utf8) else {
                throw SigningError.invalidParams("typed data payload not valid utf8")
            }
            return (address, json)
        }

        if let array = try? params.get([String].self), array.count >= 2 {
            return (array[0], array[1])
        }

        let data = try params.getDataRepresentation()
        guard let json = String(data: data, encoding: .utf8) else {
            throw SigningError.invalidParams("typed data params not valid utf8")
        }
        return (nil, json)
    }

    func signTypedData(request: Request, privateKeyHex: String, expectedAddress: String) throws -> String {
        let payload = try extractTypedDataPayload(from: request.params)
        if let requestedAddress = payload.address,
           requestedAddress.lowercased() != expectedAddress.lowercased() {
            throw SigningError.addressMismatch(expected: expectedAddress, got: requestedAddress)
        }

        let response = try client().signTypedData(jsonData: payload.json, signer: privateKeyHex)
        return try normalizeSignature(response)
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

    private func normalizeSignature(_ response: String) throws -> String {
        let trimmed = response.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.hasPrefix("0x") {
            return trimmed
        }

        struct SignatureResponse: Decodable {
            let v: Int
            let r: String
            let s: String
        }

        if let data = trimmed.data(using: .utf8),
           let decoded = try? JSONDecoder().decode(SignatureResponse.self, from: data) {
            let rHex = decoded.r.hasPrefix("0x") ? String(decoded.r.dropFirst(2)) : decoded.r
            let sHex = decoded.s.hasPrefix("0x") ? String(decoded.s.dropFirst(2)) : decoded.s
            let vHex = String(format: "%02x", decoded.v)
            return "0x\(rHex)\(sHex)\(vHex)"
        }

        if let data = trimmed.data(using: .utf8),
           let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
            if let signature = object["signature"] as? String {
                return signature
            }
            if let r = object["r"] as? String,
               let s = object["s"] as? String,
               let v = object["v"] as? Int {
                let rHex = r.hasPrefix("0x") ? String(r.dropFirst(2)) : r
                let sHex = s.hasPrefix("0x") ? String(s.dropFirst(2)) : s
                let vHex = String(format: "%02x", v)
                return "0x\(rHex)\(sHex)\(vHex)"
            }
        }

        throw SigningError.invalidSignatureResponse
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
