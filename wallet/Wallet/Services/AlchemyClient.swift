import Foundation

struct DebugRawTransaction: Identifiable, Codable, Equatable {
    let hash: String
    let from: String
    let to: String?
    let valueHex: String
    let input: String
    let nonceHex: String
    let gasHex: String
    let gasPriceHex: String?
    let maxFeePerGasHex: String?
    let maxPriorityFeePerGasHex: String?
    let blockNumberHex: String?
    let blockHash: String?
    let transactionIndexHex: String?
    let chainIdHex: String?
    let typeHex: String

    var id: String { hash }

    var valueDisplay: String {
        Self.ethValueString(fromWeiHex: valueHex) ?? valueHex
    }

    var nonceDisplay: String {
        Self.decimalString(fromHexQuantity: nonceHex) ?? nonceHex
    }

    var gasDisplay: String {
        Self.decimalString(fromHexQuantity: gasHex) ?? gasHex
    }

    var blockNumberDisplay: String? {
        guard let blockNumberHex else { return nil }
        return Self.decimalString(fromHexQuantity: blockNumberHex) ?? blockNumberHex
    }

    var typeDisplay: String {
        switch typeHex.lowercased() {
        case "0x0":
            return "legacy"
        case "0x1":
            return "accessList"
        case "0x2":
            return "eip1559"
        case "0x3":
            return "blob"
        default:
            return typeHex
        }
    }

    var rawJSONString: String? {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        guard let data = try? encoder.encode(self) else {
            return nil
        }
        return String(data: data, encoding: .utf8)
    }

    static func fromRPCResult(_ result: [String: Any]) -> DebugRawTransaction? {
        guard let hash = string(result["hash"]),
              let from = string(result["from"]),
              let valueHex = string(result["value"]),
              let input = string(result["input"]) ?? string(result["data"]),
              let nonceHex = string(result["nonce"]),
              let gasHex = string(result["gas"]) else {
            return nil
        }

        return DebugRawTransaction(
            hash: hash,
            from: from,
            to: optionalString(result["to"]),
            valueHex: valueHex,
            input: input,
            nonceHex: nonceHex,
            gasHex: gasHex,
            gasPriceHex: optionalString(result["gasPrice"]),
            maxFeePerGasHex: optionalString(result["maxFeePerGas"]),
            maxPriorityFeePerGasHex: optionalString(result["maxPriorityFeePerGas"]),
            blockNumberHex: optionalString(result["blockNumber"]),
            blockHash: optionalString(result["blockHash"]),
            transactionIndexHex: optionalString(result["transactionIndex"]),
            chainIdHex: optionalString(result["chainId"]),
            typeHex: string(result["type"]) ?? "0x0"
        )
    }

    static func isValidTransactionHash(_ value: String) -> Bool {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.count == 66, trimmed.hasPrefix("0x") else {
            return false
        }

        return trimmed.dropFirst(2).allSatisfy { character in
            character.isASCII && character.isHexDigit
        }
    }

    static func decimalString(fromHexQuantity hex: String) -> String? {
        let trimmed = hex.trimmingCharacters(in: .whitespacesAndNewlines)
        let body = trimmed.hasPrefix("0x") ? String(trimmed.dropFirst(2)) : trimmed
        guard !body.isEmpty, body.allSatisfy({ $0.isHexDigit }) else {
            return nil
        }

        var digits = [0]
        for scalar in body.lowercased().unicodeScalars {
            guard let value = Int(String(scalar), radix: 16) else {
                return nil
            }
            multiplyDecimalDigits(&digits, by: 16)
            addDecimalDigits(&digits, value)
        }

        return digits.reversed().map(String.init).joined()
    }

    static func ethValueString(fromWeiHex hex: String) -> String? {
        guard let decimal = decimalString(fromHexQuantity: hex) else {
            return nil
        }

        let padded = String(repeating: "0", count: max(0, 19 - decimal.count)) + decimal
        let whole = String(padded.dropLast(18))
        var fraction = String(padded.suffix(18))
        while fraction.last == "0" {
            fraction.removeLast()
        }

        if fraction.isEmpty {
            return "\(whole) ETH"
        }
        return "\(whole).\(fraction) ETH"
    }

    private static func string(_ value: Any?) -> String? {
        guard let string = value as? String else {
            return nil
        }

        let trimmed = string.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    private static func optionalString(_ value: Any?) -> String? {
        if value == nil || value is NSNull {
            return nil
        }
        return string(value)
    }

    private static func multiplyDecimalDigits(_ digits: inout [Int], by multiplier: Int) {
        var carry = 0
        for index in digits.indices {
            let product = digits[index] * multiplier + carry
            digits[index] = product % 10
            carry = product / 10
        }

        while carry > 0 {
            digits.append(carry % 10)
            carry /= 10
        }
    }

    private static func addDecimalDigits(_ digits: inout [Int], _ addend: Int) {
        var carry = addend
        var index = 0
        while carry > 0 {
            if index == digits.count {
                digits.append(0)
            }

            let sum = digits[index] + carry
            digits[index] = sum % 10
            carry = sum / 10
            index += 1
        }
    }
}

final class AlchemyClient {
    private enum NetworkError: Error {
        case timedOut
        case invalidResponse
        case httpStatus(Int)
    }

    private let apiKey: String
    private let session: URLSession
    private let timeout: TimeInterval

    init(apiKey: String, session: URLSession? = nil, timeout: TimeInterval = 1.5) {
        self.apiKey = apiKey
        self.timeout = timeout
        self.session = session ?? Self.makeSession(timeout: timeout)
    }

    static func makeSession(timeout: TimeInterval = 1.5) -> URLSession {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = timeout
        configuration.timeoutIntervalForResource = timeout
        return URLSession(configuration: configuration)
    }

    func fetchTokenMetadata(chainId: UInt64, address: String) -> RemoteLookup<TokenMetadata> {
        guard let url = rpcURL(for: chainId) else {
            return .unavailable
        }

        let payload: [String: Any] = [
            "jsonrpc": "2.0",
            "id": 1,
            "method": "alchemy_getTokenMetadata",
            "params": [address]
        ]

        guard let json = performJSONRequest(url: url, method: "POST", jsonBody: payload) else {
            return .unavailable
        }

        if json["error"] != nil {
            return .unavailable
        }

        guard let result = json["result"] as? [String: Any] else {
            return .notFound
        }

        let decimals = (result["decimals"] as? NSNumber)?.intValue
        guard let metadata = TokenMetadata(
            name: result["name"] as? String,
            symbol: result["symbol"] as? String,
            decimals: decimals
        ) else {
            return .notFound
        }

        return .value(metadata)
    }

    func fetchNFTCollectionName(chainId: UInt64, address: String) -> RemoteLookup<String> {
        guard let baseURL = nftBaseURL(for: chainId) else {
            return .unavailable
        }

        var components = URLComponents(url: baseURL.appendingPathComponent("getContractMetadata"), resolvingAgainstBaseURL: false)
        components?.queryItems = [
            URLQueryItem(name: "contractAddress", value: address)
        ]

        guard let url = components?.url,
              let json = performJSONRequest(url: url, method: "GET", jsonBody: nil) else {
            return .unavailable
        }

        if json["error"] != nil {
            return .unavailable
        }

        let tokenType = (json["tokenType"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines)
        if tokenType == "NOT_A_CONTRACT" || tokenType == "NO_SUPPORTED_NFT_STANDARD" {
            return .notFound
        }

        if let openSea = json["openseaMetadata"] as? [String: Any],
           let collectionName = normalizedString(openSea["collectionName"] as? String) {
            return .value(collectionName)
        }

        if let name = normalizedString(json["name"] as? String) {
            return .value(name)
        }

        return .notFound
    }

    func fetchENSName(address: String, coinType: UInt64) -> RemoteLookup<String> {
        guard let url = rpcURL(for: 1),
              let data = UniversalResolverCall.reverseCallData(address: address, coinType: coinType) else {
            return .unavailable
        }

        let payload: [String: Any] = [
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_call",
            "params": [
                [
                    "to": UniversalResolverCall.contractAddress,
                    "data": data,
                ],
                "latest",
            ],
        ]

        guard let json = performJSONRequest(url: url, method: "POST", jsonBody: payload) else {
            return .unavailable
        }

        if let error = json["error"] as? [String: Any] {
            let message = [
                error["message"] as? String,
                error["data"] as? String,
            ]
            .compactMap { $0 }
            .joined(separator: " ")
            .lowercased()

            if message.contains("execution reverted")
                || message.contains("reversemismatch")
                || message.contains("reverseaddressmismatch")
                || message.contains("resolvernotfound")
                || message.contains("resolvererror") {
                return .notFound
            }

            return .unavailable
        }

        guard let result = json["result"] as? String else {
            return .notFound
        }

        guard let name = UniversalResolverCall.decodePrimaryName(fromHex: result) else {
            return .notFound
        }

        return .value(name)
    }

    func fetchBlockTimestamp(chainId: UInt64, blockNumber: UInt64) -> RemoteLookup<UInt64> {
        guard let url = rpcURL(for: chainId) else {
            return .unavailable
        }

        let payload: [String: Any] = [
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_getBlockByNumber",
            "params": [Self.hexQuantity(blockNumber), false]
        ]

        guard let json = performJSONRequest(url: url, method: "POST", jsonBody: payload) else {
            return .unavailable
        }

        if json["error"] != nil {
            return .unavailable
        }

        guard let result = json["result"] as? [String: Any],
              let timestampHex = result["timestamp"] as? String,
              let timestamp = Self.parseHexQuantity(timestampHex) else {
            return .notFound
        }

        return .value(timestamp)
    }

    func fetchTransactionByHash(chainId: UInt64, hash: String) -> RemoteLookup<DebugRawTransaction> {
        let trimmedHash = hash.trimmingCharacters(in: .whitespacesAndNewlines)
        guard DebugRawTransaction.isValidTransactionHash(trimmedHash) else {
            return .notFound
        }

        guard let url = rpcURL(for: chainId) else {
            return .unavailable
        }

        let payload: [String: Any] = [
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_getTransactionByHash",
            "params": [trimmedHash]
        ]

        guard let json = performJSONRequest(url: url, method: "POST", jsonBody: payload) else {
            return .unavailable
        }

        if json["error"] != nil {
            return .unavailable
        }

        guard let result = json["result"], !(result is NSNull) else {
            return .notFound
        }

        guard let object = result as? [String: Any],
              let transaction = DebugRawTransaction.fromRPCResult(object) else {
            return .unavailable
        }

        return .value(transaction)
    }

    private func performJSONRequest(url: URL, method: String, jsonBody: [String: Any]?) -> [String: Any]? {
        var request = URLRequest(url: url)
        request.httpMethod = method
        request.timeoutInterval = timeout

        if let jsonBody {
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
            request.httpBody = try? JSONSerialization.data(withJSONObject: jsonBody)
        }

        let result = perform(request)
        guard case .success(let data) = result,
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return nil
        }

        return object
    }

    private func perform(_ request: URLRequest) -> Result<Data, Error> {
        let semaphore = DispatchSemaphore(value: 0)
        let lock = NSLock()
        var result: Result<Data, Error> = .failure(NetworkError.invalidResponse)

        let task = session.dataTask(with: request) { data, response, error in
            defer { semaphore.signal() }

            lock.lock()
            defer { lock.unlock() }

            if let error {
                result = .failure(error)
                return
            }

            guard let httpResponse = response as? HTTPURLResponse,
                  let data else {
                result = .failure(NetworkError.invalidResponse)
                return
            }

            guard (200..<300).contains(httpResponse.statusCode) else {
                result = .failure(NetworkError.httpStatus(httpResponse.statusCode))
                return
            }

            result = .success(data)
        }

        task.resume()

        if semaphore.wait(timeout: .now() + timeout) == .timedOut {
            task.cancel()
            return .failure(NetworkError.timedOut)
        }

        lock.lock()
        defer { lock.unlock() }
        return result
    }

    private func rpcURL(for chainId: UInt64) -> URL? {
        guard let host = Self.chainHostnames[chainId] else {
            return nil
        }

        return URL(string: "https://\(host).g.alchemy.com/v2/\(apiKey)")
    }

    private func nftBaseURL(for chainId: UInt64) -> URL? {
        guard let host = Self.chainHostnames[chainId] else {
            return nil
        }

        return URL(string: "https://\(host).g.alchemy.com/nft/v3/\(apiKey)")
    }

    func fetchStorageAt(chainId: UInt64, address: String, slot: String) -> RemoteLookup<String> {
        guard let url = rpcURL(for: chainId) else {
            return .unavailable
        }

        let payload: [String: Any] = [
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_getStorageAt",
            "params": [address, slot, "latest"]
        ]

        guard let json = performJSONRequest(url: url, method: "POST", jsonBody: payload) else {
            return .unavailable
        }

        if json["error"] != nil {
            return .unavailable
        }

        guard let result = json["result"] as? String,
              let addr = addressFromStorageWord(result) else {
            return .notFound
        }

        return .value(addr)
    }

    private func addressFromStorageWord(_ hex: String) -> String? {
        let clean = hex.hasPrefix("0x") ? String(hex.dropFirst(2)) : hex
        let padded = String(repeating: "0", count: max(0, 64 - clean.count)) + clean
        let addressHex = String(padded.suffix(40))
        guard addressHex != String(repeating: "0", count: 40) else { return nil }
        return normalizedAddress("0x" + addressHex)
    }

    private static let chainHostnames: [UInt64: String] = [
        1: "eth-mainnet",
        10: "opt-mainnet",
        137: "polygon-mainnet",
        8453: "base-mainnet",
        42161: "arb-mainnet",
    ]

    private static func hexQuantity(_ value: UInt64) -> String {
        "0x" + String(value, radix: 16)
    }

    private static func parseHexQuantity(_ value: String) -> UInt64? {
        let trimmed = value.hasPrefix("0x") ? String(value.dropFirst(2)) : value
        return UInt64(trimmed, radix: 16)
    }
}
