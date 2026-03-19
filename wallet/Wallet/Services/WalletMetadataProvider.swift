import Foundation
import Erc7730

enum AppConfig {
    static var alchemyAPIKey: String? {
        sanitizedValue(forInfoDictionaryKey: "AlchemyAPIKey")
    }

    private static func sanitizedValue(forInfoDictionaryKey key: String) -> String? {
        guard let rawValue = Bundle.main.object(forInfoDictionaryKey: key) as? String else {
            return nil
        }

        let trimmed = rawValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, !trimmed.hasPrefix("YOUR_"), !trimmed.hasPrefix("$(") else {
            return nil
        }

        return trimmed
    }
}

enum ENSCoinType {
    static func value(for chainId: UInt64) -> UInt64 {
        chainId == 1 ? 60 : (0x8000_0000 ^ chainId)
    }
}

enum CacheLookup<Value> {
    case value(Value)
    case negative
    case missing
}

extension CacheLookup: Equatable where Value: Equatable {}

struct CachedResolution<Value: Codable>: Codable {
    let value: Value?
    let storedAt: Date
    let expiresAt: Date

    func isExpired(at date: Date) -> Bool {
        expiresAt <= date
    }
}

struct TokenMetadata: Codable, Equatable {
    let symbol: String
    let decimals: UInt8
    let name: String

    init(symbol: String, decimals: UInt8, name: String) {
        self.symbol = symbol
        self.decimals = decimals
        self.name = name
    }

    init?(name: String?, symbol: String?, decimals: Int?) {
        let normalizedSymbol = symbol?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        guard !normalizedSymbol.isEmpty else { return nil }

        let normalizedName = name?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let finalName = normalizedName.isEmpty ? normalizedSymbol : normalizedName

        guard let decimals, (0...Int(UInt8.max)).contains(decimals) else {
            return nil
        }

        self.symbol = normalizedSymbol
        self.decimals = UInt8(decimals)
        self.name = finalName
    }

    var ffiValue: TokenMetaFfi {
        TokenMetaFfi(symbol: symbol, decimals: decimals, name: name)
    }
}

final class InMemoryResolutionCache {
    private let lock = NSLock()
    private let encoder = JSONEncoder()
    private let decoder = JSONDecoder()
    private var storage: [String: Data] = [:]

    func lookup<Value: Codable>(_ key: String, as type: Value.Type, now: Date) -> CacheLookup<Value> {
        lock.lock()
        defer { lock.unlock() }

        guard let data = storage[key] else {
            return .missing
        }

        guard let record = try? decoder.decode(CachedResolution<Value>.self, from: data) else {
            storage.removeValue(forKey: key)
            return .missing
        }

        if record.isExpired(at: now) {
            storage.removeValue(forKey: key)
            return .missing
        }

        if let value = record.value {
            return .value(value)
        }

        return .negative
    }

    func store<Value: Codable>(_ value: Value?, key: String, ttl: TimeInterval, now: Date) {
        let record = CachedResolution(
            value: value,
            storedAt: now,
            expiresAt: now.addingTimeInterval(ttl)
        )

        guard let data = try? encoder.encode(record) else {
            return
        }

        lock.lock()
        storage[key] = data
        lock.unlock()
    }
}

final class PersistentResolutionCache {
    private let userDefaults: UserDefaults
    private let namespace: String
    private let encoder = JSONEncoder()
    private let decoder = JSONDecoder()

    init(userDefaults: UserDefaults, namespace: String = "wallet.metadata") {
        self.userDefaults = userDefaults
        self.namespace = namespace
    }

    func lookup<Value: Codable>(_ key: String, as type: Value.Type, now: Date) -> CacheLookup<Value> {
        let namespacedKey = namespaced(key)
        guard let data = userDefaults.data(forKey: namespacedKey) else {
            return .missing
        }

        guard let record = try? decoder.decode(CachedResolution<Value>.self, from: data) else {
            userDefaults.removeObject(forKey: namespacedKey)
            return .missing
        }

        if record.isExpired(at: now) {
            userDefaults.removeObject(forKey: namespacedKey)
            return .missing
        }

        if let value = record.value {
            return .value(value)
        }

        return .negative
    }

    func store<Value: Codable>(_ value: Value?, key: String, ttl: TimeInterval, now: Date) {
        let record = CachedResolution(
            value: value,
            storedAt: now,
            expiresAt: now.addingTimeInterval(ttl)
        )

        guard let data = try? encoder.encode(record) else {
            return
        }

        userDefaults.set(data, forKey: namespaced(key))
    }

    private func namespaced(_ key: String) -> String {
        "\(namespace).\(key)"
    }
}

struct SeedTokenStore {
    private let tokens: [String: TokenMetadata]

    init(bundle: Bundle, resourceName: String = "tokens", resourceExtension: String = "json") {
        let data = bundle.url(forResource: resourceName, withExtension: resourceExtension)
            .flatMap { try? Data(contentsOf: $0) }
        tokens = Self.decodeTokens(from: data)
    }

    init(data: Data) {
        tokens = Self.decodeTokens(from: data)
    }

    func token(chainId: UInt64, address: String) -> TokenMetadata? {
        tokens[LookupKey.token(chainId: chainId, address: address)]
    }

    private static func decodeTokens(from data: Data?) -> [String: TokenMetadata] {
        guard let data else {
            return [:]
        }

        let decoder = JSONDecoder()
        return (try? decoder.decode([String: TokenMetadata].self, from: data)) ?? [:]
    }
}

enum RemoteLookup<Value> {
    case value(Value)
    case notFound
    case unavailable
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

final class WalletMetadataProvider: DataProviderFfi, @unchecked Sendable {
    static let supportedChainIds: Set<UInt64> = [1, 10, 137, 8453, 42161]
    static let localWalletName = "My Wallet"

    private enum TTL {
        static let token: TimeInterval = 30 * 24 * 60 * 60
        static let ens: TimeInterval = 7 * 24 * 60 * 60
        static let nft: TimeInterval = 30 * 24 * 60 * 60
        static let negative: TimeInterval = 12 * 60 * 60
    }

    private let seedTokenStore: SeedTokenStore
    private let memoryCache: InMemoryResolutionCache
    private let persistentCache: PersistentResolutionCache
    private let alchemyClient: AlchemyClient?
    private let walletAddressProvider: () -> String?
    private let isMainThread: () -> Bool
    private let now: () -> Date

    static func live(bundle: Bundle = .main) -> WalletMetadataProvider {
        WalletMetadataProvider(
            seedTokenStore: SeedTokenStore(bundle: bundle),
            memoryCache: InMemoryResolutionCache(),
            persistentCache: PersistentResolutionCache(userDefaults: .standard),
            alchemyClient: AppConfig.alchemyAPIKey.map { AlchemyClient(apiKey: $0) },
            walletAddressProvider: { KeyManager.restore()?.ethereumAddress },
            isMainThread: { Thread.isMainThread },
            now: Date.init
        )
    }

    init(
        seedTokenStore: SeedTokenStore,
        memoryCache: InMemoryResolutionCache,
        persistentCache: PersistentResolutionCache,
        alchemyClient: AlchemyClient?,
        walletAddressProvider: @escaping () -> String?,
        isMainThread: @escaping () -> Bool,
        now: @escaping () -> Date
    ) {
        self.seedTokenStore = seedTokenStore
        self.memoryCache = memoryCache
        self.persistentCache = persistentCache
        self.alchemyClient = alchemyClient
        self.walletAddressProvider = walletAddressProvider
        self.isMainThread = isMainThread
        self.now = now
    }

    func resolveToken(chainId: UInt64, address: String) -> TokenMetaFfi? {
        guard let resolvedAddress = normalizedAddress(address) else {
            return nil
        }

        let cacheKey = LookupKey.token(chainId: chainId, address: resolvedAddress)
        let date = now()

        switch memoryCache.lookup(cacheKey, as: TokenMetadata.self, now: date) {
        case .value(let token):
            return token.ffiValue
        case .negative:
            return nil
        case .missing:
            break
        }

        if let seedToken = seedTokenStore.token(chainId: chainId, address: resolvedAddress) {
            memoryCache.store(seedToken, key: cacheKey, ttl: TTL.token, now: date)
            return seedToken.ffiValue
        }

        switch persistentCache.lookup(cacheKey, as: TokenMetadata.self, now: date) {
        case .value(let token):
            memoryCache.store(token, key: cacheKey, ttl: TTL.token, now: date)
            return token.ffiValue
        case .negative:
            memoryCache.store(nil as TokenMetadata?, key: cacheKey, ttl: TTL.negative, now: date)
            return nil
        case .missing:
            break
        }

        if !canPerformLiveLookup(on: chainId) {
            return nil
        }

        guard let alchemyClient else {
            return nil
        }

        switch alchemyClient.fetchTokenMetadata(chainId: chainId, address: resolvedAddress) {
        case .value(let token):
            store(token, key: cacheKey, ttl: TTL.token, now: date)
            return token.ffiValue
        case .notFound:
            store(nil as TokenMetadata?, key: cacheKey, ttl: TTL.negative, now: date)
            return nil
        case .unavailable:
            return nil
        }
    }

    func resolveEnsName(address: String, chainId: UInt64) -> String? {
        guard let resolvedAddress = normalizedAddress(address) else {
            return nil
        }

        let cacheKey = LookupKey.ens(chainId: chainId, address: resolvedAddress)
        let date = now()

        switch cachedValue(for: cacheKey, as: String.self, now: date, positiveTTL: TTL.ens) {
        case .value(let name):
            return name
        case .negative:
            return nil
        case .missing:
            break
        }

        guard canPerformLiveLookup(on: chainId),
              let alchemyClient else {
            return nil
        }

        switch alchemyClient.fetchENSName(
            address: resolvedAddress,
            coinType: ENSCoinType.value(for: chainId)
        ) {
        case .value(let name):
            store(name, key: cacheKey, ttl: TTL.ens, now: date)
            return name
        case .notFound:
            store(nil as String?, key: cacheKey, ttl: TTL.negative, now: date)
            return nil
        case .unavailable:
            return nil
        }
    }

    func resolveLocalName(address: String, chainId: UInt64) -> String? {
        let _ = chainId

        guard let resolvedAddress = normalizedAddress(address),
              let walletAddress = normalizedAddress(walletAddressProvider()) else {
            return nil
        }

        return resolvedAddress == walletAddress ? Self.localWalletName : nil
    }

    func resolveNftCollectionName(collectionAddress: String, chainId: UInt64) -> String? {
        guard let resolvedAddress = normalizedAddress(collectionAddress) else {
            return nil
        }

        let cacheKey = LookupKey.nft(chainId: chainId, address: resolvedAddress)
        let date = now()

        switch cachedValue(for: cacheKey, as: String.self, now: date, positiveTTL: TTL.nft) {
        case .value(let name):
            return name
        case .negative:
            return nil
        case .missing:
            break
        }

        guard canPerformLiveLookup(on: chainId),
              let alchemyClient else {
            return nil
        }

        switch alchemyClient.fetchNFTCollectionName(chainId: chainId, address: resolvedAddress) {
        case .value(let name):
            store(name, key: cacheKey, ttl: TTL.nft, now: date)
            return name
        case .notFound:
            store(nil as String?, key: cacheKey, ttl: TTL.negative, now: date)
            return nil
        case .unavailable:
            return nil
        }
    }

    private func cachedValue<Value: Codable>(
        for key: String,
        as type: Value.Type,
        now: Date,
        positiveTTL: TimeInterval
    ) -> CacheLookup<Value> {
        switch memoryCache.lookup(key, as: type, now: now) {
        case .value(let value):
            return .value(value)
        case .negative:
            return .negative
        case .missing:
            break
        }

        switch persistentCache.lookup(key, as: type, now: now) {
        case .value(let value):
            memoryCache.store(value, key: key, ttl: positiveTTL, now: now)
            return .value(value)
        case .negative:
            memoryCache.store(nil as Value?, key: key, ttl: TTL.negative, now: now)
            return .negative
        case .missing:
            return .missing
        }
    }

    private func store<Value: Codable>(_ value: Value?, key: String, ttl: TimeInterval, now: Date) {
        memoryCache.store(value, key: key, ttl: ttl, now: now)
        persistentCache.store(value, key: key, ttl: ttl, now: now)
    }

    private func canPerformLiveLookup(on chainId: UInt64) -> Bool {
        guard Self.supportedChainIds.contains(chainId) else {
            return false
        }

        guard !isMainThread() else {
            return false
        }

        return alchemyClient != nil
    }
}

enum LookupKey {
    static func token(chainId: UInt64, address: String) -> String {
        "token.\(tokenKey(chainId: chainId, address: address))"
    }

    static func nft(chainId: UInt64, address: String) -> String {
        "nft.eip155:\(chainId):\(address.lowercased())"
    }

    static func ens(chainId: UInt64, address: String) -> String {
        "ens.eip155:\(chainId):\(address.lowercased())"
    }

    static func tokenKey(chainId: UInt64, address: String) -> String {
        "eip155:\(chainId)/erc20:\(address.lowercased())"
    }
}

enum UniversalResolverCall {
    static let contractAddress = "0xeEeEEEeE14D718C2B47D9923Deab1335E144EeEe"

    static func reverseCallData(address: String, coinType: UInt64) -> String? {
        guard let addressBytes = Data(hexString: address) else {
            return nil
        }

        var payload = Data()
        payload.append(functionSelector(for: "reverse(bytes,uint256)"))
        payload.append(abiEncodeUInt256(64))
        payload.append(abiEncodeUInt256(coinType))
        payload.append(abiEncodeBytes(addressBytes))
        return "0x" + payload.hexString
    }

    static func decodePrimaryName(fromHex hex: String) -> String? {
        guard let data = Data(hexString: hex), data.count >= 96 else {
            return nil
        }

        let offset = Int(readUInt64(from: data, at: 0))
        guard offset + 32 <= data.count else {
            return nil
        }

        let length = Int(readUInt64(from: data, at: offset))
        let valueStart = offset + 32
        let valueEnd = valueStart + length
        guard length > 0, valueEnd <= data.count else {
            return nil
        }

        let nameData = data.subdata(in: valueStart..<valueEnd)
        guard let name = String(data: nameData, encoding: .utf8) else {
            return nil
        }

        return normalizedString(name)
    }

    private static func functionSelector(for signature: String) -> Data {
        Data(keccak256(Data(signature.utf8)).prefix(4))
    }

    private static func abiEncodeBytes(_ value: Data) -> Data {
        var encoded = Data()
        encoded.append(abiEncodeUInt256(UInt64(value.count)))
        encoded.append(value)
        let remainder = value.count % 32
        if remainder != 0 {
            encoded.append(Data(repeating: 0, count: 32 - remainder))
        }
        return encoded
    }

    private static func abiEncodeUInt256(_ value: UInt64) -> Data {
        var encoded = Data(repeating: 0, count: 32)
        withUnsafeBytes(of: value.bigEndian) { rawBuffer in
            encoded.replaceSubrange(24..<32, with: rawBuffer)
        }
        return encoded
    }

    private static func readUInt64(from data: Data, at offset: Int) -> UInt64 {
        guard offset + 32 <= data.count else {
            return 0
        }

        let word = data.subdata(in: offset..<(offset + 32))
        return word.suffix(8).reduce(UInt64(0)) { partialResult, byte in
            (partialResult << 8) | UInt64(byte)
        }
    }
}

func normalizedAddress(_ value: String?) -> String? {
    guard let value else {
        return nil
    }

    let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
    guard trimmed.count == 42, trimmed.hasPrefix("0x") else {
        return nil
    }

    guard Data(hexString: trimmed) != nil else {
        return nil
    }

    return trimmed.lowercased()
}

private func normalizedString(_ value: String?) -> String? {
    guard let value else {
        return nil
    }

    let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
    return trimmed.isEmpty ? nil : trimmed
}
