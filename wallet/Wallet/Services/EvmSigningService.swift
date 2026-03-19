import Foundation
import ReownWalletKit
import YttriumUtilsWrapper

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

        return try Eip712Signer.sign(typedDataJson: payload.json, privateKeyHex: privateKeyHex)
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
