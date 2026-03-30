import Foundation
import ClearSigning

final class WalletMetadataProvider: DataProviderFfi, @unchecked Sendable {

    static let supportedChainIds: Set<UInt64> = [1, 10, 137, 8453, 42161]
    static let localWalletName = "My Wallet"

    private enum TTL {
        static let token: TimeInterval = 30 * 24 * 60 * 60
        static let ens: TimeInterval = 7 * 24 * 60 * 60
        static let nft: TimeInterval = 30 * 24 * 60 * 60
        static let blockTimestamp: TimeInterval = 24 * 60 * 60
        static let negative: TimeInterval = 12 * 60 * 60
        static let implementation: TimeInterval = 90 * 24 * 60 * 60
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

    // MARK: - DataProviderFfi

    func resolveToken(chainId: UInt64, address: String) -> TokenMetaFfi? {
        lookupToken(chainId: chainId, address: address)?.ffiValue
    }

    func resolveEnsName(address: String, chainId: UInt64, types: [String]?) -> String? {
        lookupENSName(address: address, chainId: chainId)
    }

    func resolveLocalName(address: String, chainId: UInt64, types: [String]?) -> String? {
        guard let resolved = normalizedAddress(address),
              let wallet = normalizedAddress(walletAddressProvider()) else { return nil }
        return resolved == wallet ? Self.localWalletName : nil
    }

    func resolveNftCollectionName(collectionAddress: String, chainId: UInt64) -> String? {
        lookupNFTCollectionName(chainId: chainId, address: collectionAddress)
    }

    func resolveBlockTimestamp(chainId: UInt64, blockNumber: UInt64) -> UInt64? {
        let cacheKey = LookupKey.blockTimestamp(chainId: chainId, blockNumber: blockNumber)
        let date = now()

        switch cachedValue(for: cacheKey, as: UInt64.self, now: date, positiveTTL: TTL.blockTimestamp) {
        case .value(let timestamp):
            return timestamp
        case .negative:
            return nil
        case .missing:
            break
        }

        guard canPerformLiveLookup(on: chainId),
              let alchemyClient else {
            return nil
        }

        switch alchemyClient.fetchBlockTimestamp(chainId: chainId, blockNumber: blockNumber) {
        case .value(let timestamp):
            store(timestamp, key: cacheKey, ttl: TTL.blockTimestamp, now: date)
            return timestamp
        case .notFound:
            store(nil as UInt64?, key: cacheKey, ttl: TTL.negative, now: date)
            return nil
        case .unavailable:
            return nil
        }
    }

    private enum ProxySlot {
        /// EIP-1967 implementation slot: keccak256("eip1967.proxy.implementation") - 1
        static let eip1967 = "0x360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc"
        /// Safe proxy: singleton stored at slot 0
        static let safe = "0x0000000000000000000000000000000000000000000000000000000000000000"
    }

    func getImplementationAddress(chainId: UInt64, address: String) -> String? {
        guard let resolvedAddress = normalizedAddress(address) else {
            return nil
        }

        let cacheKey = LookupKey.implementation(chainId: chainId, address: resolvedAddress)
        let date = now()

        switch cachedValue(for: cacheKey, as: String.self, now: date, positiveTTL: TTL.implementation) {
        case .value(let impl):
            return impl
        case .negative:
            return nil
        case .missing:
            break
        }

        guard canPerformLiveLookup(on: chainId),
              let alchemyClient else {
            return nil
        }

        // Try EIP-1967 implementation slot first
        switch alchemyClient.fetchStorageAt(chainId: chainId, address: resolvedAddress, slot: ProxySlot.eip1967) {
        case .value(let impl) where impl != resolvedAddress:
            store(impl, key: cacheKey, ttl: TTL.implementation, now: date)
            return impl
        case .value:
            break // Self-reference, not a proxy — try Safe slot
        case .notFound:
            break
        case .unavailable:
            return nil
        }

        // Try Safe singleton slot (slot 0)
        switch alchemyClient.fetchStorageAt(chainId: chainId, address: resolvedAddress, slot: ProxySlot.safe) {
        case .value(let impl) where impl != resolvedAddress:
            store(impl, key: cacheKey, ttl: TTL.implementation, now: date)
            return impl
        case .value, .notFound:
            store(nil as String?, key: cacheKey, ttl: TTL.negative, now: date)
            return nil
        case .unavailable:
            return nil
        }
    }

    // MARK: - Native Gas Token

    private static let nativeTokenSentinel = "0xeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"

    private static let nativeGasTokens: [UInt64: TokenMetadata] = [
        1:     TokenMetadata(symbol: "ETH",   decimals: 18, name: "Ether"),
        10:    TokenMetadata(symbol: "ETH",   decimals: 18, name: "Ether"),
        56:    TokenMetadata(symbol: "BNB",   decimals: 18, name: "BNB"),
        137:   TokenMetadata(symbol: "POL",   decimals: 18, name: "POL"),
        8453:  TokenMetadata(symbol: "ETH",   decimals: 18, name: "Ether"),
        42161: TokenMetadata(symbol: "ETH",   decimals: 18, name: "Ether"),
    ]

    // MARK: - Token Resolution

    private func lookupToken(chainId: UInt64, address: String) -> TokenMetadata? {
        guard let resolvedAddress = normalizedAddress(address) else {
            print("[metadata] lookupToken: invalid address \(address)")
            return nil
        }

        if resolvedAddress == Self.nativeTokenSentinel {
            print("[metadata] lookupToken: native gas token for chain \(chainId)")
            return Self.nativeGasTokens[chainId]
        }

        let cacheKey = LookupKey.token(chainId: chainId, address: resolvedAddress)
        let date = now()

        switch memoryCache.lookup(cacheKey, as: TokenMetadata.self, now: date) {
        case .value(let token):
            print("[metadata] lookupToken: memory cache HIT for \(chainId):\(resolvedAddress) → \(token.symbol)")
            return token
        case .negative:
            print("[metadata] lookupToken: memory cache NEGATIVE for \(chainId):\(resolvedAddress)")
            return nil
        case .missing:
            break
        }

        if let seedToken = seedTokenStore.token(chainId: chainId, address: resolvedAddress) {
            print("[metadata] lookupToken: seed store HIT for \(chainId):\(resolvedAddress) → \(seedToken.symbol)")
            memoryCache.store(seedToken, key: cacheKey, ttl: TTL.token, now: date)
            return seedToken
        }

        switch persistentCache.lookup(cacheKey, as: TokenMetadata.self, now: date) {
        case .value(let token):
            print("[metadata] lookupToken: persistent cache HIT for \(chainId):\(resolvedAddress) → \(token.symbol)")
            memoryCache.store(token, key: cacheKey, ttl: TTL.token, now: date)
            return token
        case .negative:
            print("[metadata] lookupToken: persistent cache NEGATIVE for \(chainId):\(resolvedAddress)")
            memoryCache.store(nil as TokenMetadata?, key: cacheKey, ttl: TTL.negative, now: date)
            return nil
        case .missing:
            break
        }

        if !canPerformLiveLookup(on: chainId) {
            print("[metadata] lookupToken: canPerformLiveLookup=false for chain \(chainId) (isMainThread=\(isMainThread()), supported=\(Self.supportedChainIds.contains(chainId)), alchemy=\(alchemyClient != nil))")
            return nil
        }

        guard let alchemyClient else {
            print("[metadata] lookupToken: no alchemy client")
            return nil
        }

        print("[metadata] lookupToken: calling Alchemy for \(chainId):\(resolvedAddress)")
        switch alchemyClient.fetchTokenMetadata(chainId: chainId, address: resolvedAddress) {
        case .value(let token):
            print("[metadata] lookupToken: Alchemy OK → \(token.symbol) (\(token.decimals) decimals)")
            store(token, key: cacheKey, ttl: TTL.token, now: date)
            return token
        case .notFound:
            print("[metadata] lookupToken: Alchemy NOT FOUND for \(chainId):\(resolvedAddress)")
            store(nil as TokenMetadata?, key: cacheKey, ttl: TTL.negative, now: date)
            return nil
        case .unavailable:
            print("[metadata] lookupToken: Alchemy UNAVAILABLE for \(chainId):\(resolvedAddress)")
            return nil
        }
    }

    // MARK: - ENS Resolution

    private func lookupENSName(address: String, chainId: UInt64) -> String? {
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

    // MARK: - NFT Resolution

    private func lookupNFTCollectionName(chainId: UInt64, address: String) -> String? {
        guard let resolvedAddress = normalizedAddress(address) else {
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

    // MARK: - Caching

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
