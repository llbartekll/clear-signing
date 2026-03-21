import Foundation

final class AlchemyClient {
    private enum NetworkError: Error {
        case timedOut
        case invalidResponse
        case httpStatus(Int)
    }

    private let apiKey: String
    private let session: URLSession
    private let timeout: TimeInterval

    init(apiKey: String, session: URLSession = AlchemyClient.makeSession(), timeout: TimeInterval = 1.5) {
        self.apiKey = apiKey
        self.session = session
        self.timeout = timeout
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

    private static let chainHostnames: [UInt64: String] = [
        1: "eth-mainnet",
        10: "opt-mainnet",
        137: "polygon-mainnet",
        8453: "base-mainnet",
        42161: "arb-mainnet",
    ]
}
