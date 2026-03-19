import XCTest

final class WalletMetadataProviderTests: XCTestCase {
    private let tokenAddress = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
    private let walletAddress = "0xbf01daf454dce008d3e2bfd47d5e186f71477253"
    private let ensAddress = "0xd8da6bf26964af9d7eed9e03e53415d37aa96045"

    override func tearDown() {
        MockURLProtocol.reset()
        super.tearDown()
    }

    func testTokenResolutionUsesSeedBeforePersistentOrRemote() {
        let now = Date(timeIntervalSince1970: 1_700_000_000)
        let persistentCache = makePersistentCache(name: #function)
        persistentCache.store(
            TokenMetadata(symbol: "PERSIST", decimals: 18, name: "Persistent Token"),
            key: LookupKey.token(chainId: 1, address: tokenAddress),
            ttl: 30,
            now: now
        )

        MockURLProtocol.handler = { request in
            XCTFail("seed lookup should not hit the network: \(String(describing: request.url))")
            return Self.jsonResponse(
                url: request.url!,
                body: [
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": [
                        "symbol": "REMOTE",
                        "name": "Remote Token",
                        "decimals": 18,
                    ],
                ]
            )
        }

        let provider = makeProvider(
            seedEntries: [LookupKey.tokenKey(chainId: 1, address: tokenAddress): TokenMetadata(symbol: "USDC", decimals: 6, name: "USD Coin")],
            persistentCache: persistentCache,
            session: makeSession()
        )

        let token = provider.resolveToken(chainId: 1, address: tokenAddress)
        XCTAssertEqual(token?.symbol, "USDC")
        XCTAssertEqual(MockURLProtocol.requests.count, 0)
    }

    func testTokenResolutionFallsBackToPersistentCacheBeforeRemote() {
        let now = Date(timeIntervalSince1970: 1_700_000_000)
        let persistentCache = makePersistentCache(name: #function)
        persistentCache.store(
            TokenMetadata(symbol: "PERSIST", decimals: 18, name: "Persistent Token"),
            key: LookupKey.token(chainId: 1, address: tokenAddress),
            ttl: 30,
            now: now
        )

        MockURLProtocol.handler = { request in
            XCTFail("persistent cache should avoid network lookup: \(String(describing: request.url))")
            return Self.jsonResponse(url: request.url!, body: [:])
        }

        let provider = makeProvider(
            seedEntries: [:],
            persistentCache: persistentCache,
            session: makeSession()
        )

        let token = provider.resolveToken(chainId: 1, address: tokenAddress)
        XCTAssertEqual(token?.symbol, "PERSIST")
        XCTAssertEqual(MockURLProtocol.requests.count, 0)
    }

    func testTokenResolutionCachesNegativeMisses() {
        MockURLProtocol.handler = { request in
            Self.jsonResponse(
                url: request.url!,
                body: [
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": [:],
                ]
            )
        }

        let provider = makeProvider(seedEntries: [:], session: makeSession())

        XCTAssertNil(provider.resolveToken(chainId: 1, address: tokenAddress))

        MockURLProtocol.handler = { request in
            XCTFail("negative cache should avoid follow-up network lookup: \(String(describing: request.url))")
            return Self.jsonResponse(url: request.url!, body: [:])
        }

        XCTAssertNil(provider.resolveToken(chainId: 1, address: tokenAddress))
        XCTAssertEqual(MockURLProtocol.requests.count, 1)
    }

    func testENSCoinTypeUsesENSIP11Mapping() {
        XCTAssertEqual(ENSCoinType.value(for: 1), 60)
        XCTAssertEqual(ENSCoinType.value(for: 10), 2_147_483_658)
        XCTAssertEqual(ENSCoinType.value(for: 137), 2_147_483_785)
        XCTAssertEqual(ENSCoinType.value(for: 8453), 2_147_492_101)
        XCTAssertEqual(ENSCoinType.value(for: 42161), 2_147_525_809)
    }

    func testENSResolutionUsesUniversalResolverResponseAndCachesResult() {
        let expectedData = UniversalResolverCall.reverseCallData(
            address: ensAddress,
            coinType: ENSCoinType.value(for: 8453)
        )
        let encodedResponse = encodeUniversalResolverReverseResponse(name: "vitalik.eth")

        MockURLProtocol.handler = { request in
            let json = try XCTUnwrap(Self.jsonObject(from: request))
            let params = try XCTUnwrap(json["params"] as? [Any])
            let call = try XCTUnwrap(params.first as? [String: Any])
            XCTAssertEqual(call["to"] as? String, UniversalResolverCall.contractAddress)
            XCTAssertEqual(call["data"] as? String, expectedData)

            return Self.jsonResponse(
                url: request.url!,
                body: [
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": encodedResponse,
                ]
            )
        }

        let provider = makeProvider(seedEntries: [:], session: makeSession())

        XCTAssertEqual(provider.resolveEnsName(address: ensAddress, chainId: 8453), "vitalik.eth")
        XCTAssertEqual(provider.resolveEnsName(address: ensAddress, chainId: 8453), "vitalik.eth")
        XCTAssertEqual(MockURLProtocol.requests.count, 1)
    }

    func testNFTCollectionPrefersOpenSeaCollectionName() {
        MockURLProtocol.handler = { request in
            Self.jsonResponse(
                url: request.url!,
                body: [
                    "name": "Contract Name",
                    "tokenType": "ERC721",
                    "openseaMetadata": [
                        "collectionName": "Preferred Collection",
                    ],
                ]
            )
        }

        let provider = makeProvider(seedEntries: [:], session: makeSession())

        let name = provider.resolveNftCollectionName(collectionAddress: tokenAddress, chainId: 1)
        XCTAssertEqual(name, "Preferred Collection")
    }

    func testNFTCollectionFallsBackToContractName() {
        MockURLProtocol.handler = { request in
            Self.jsonResponse(
                url: request.url!,
                body: [
                    "name": "Fallback Contract Name",
                    "tokenType": "ERC1155",
                ]
            )
        }

        let provider = makeProvider(seedEntries: [:], session: makeSession())

        let name = provider.resolveNftCollectionName(collectionAddress: tokenAddress, chainId: 1)
        XCTAssertEqual(name, "Fallback Contract Name")
    }

    func testPersistentCacheExpiresEntries() {
        let cache = makePersistentCache(name: #function)
        let createdAt = Date(timeIntervalSince1970: 1_700_000_000)
        cache.store(
            "cached.eth",
            key: LookupKey.ens(chainId: 1, address: ensAddress),
            ttl: 5,
            now: createdAt
        )

        XCTAssertEqual(
            cache.lookup(LookupKey.ens(chainId: 1, address: ensAddress), as: String.self, now: createdAt.addingTimeInterval(4)),
            .value("cached.eth")
        )
        XCTAssertEqual(
            cache.lookup(LookupKey.ens(chainId: 1, address: ensAddress), as: String.self, now: createdAt.addingTimeInterval(6)),
            .missing
        )
    }

    func testNoKeyStillUsesPersistentAndLocalData() {
        let persistentCache = makePersistentCache(name: #function)
        let now = Date(timeIntervalSince1970: 1_700_000_000)
        persistentCache.store(
            "cached.eth",
            key: LookupKey.ens(chainId: 1, address: ensAddress),
            ttl: 30,
            now: now
        )

        let provider = WalletMetadataProvider(
            seedTokenStore: SeedTokenStore(data: Data("{}".utf8)),
            memoryCache: InMemoryResolutionCache(),
            persistentCache: persistentCache,
            alchemyClient: nil,
            walletAddressProvider: { self.walletAddress },
            isMainThread: { false },
            now: { now }
        )

        XCTAssertEqual(provider.resolveEnsName(address: ensAddress, chainId: 1), "cached.eth")
        XCTAssertEqual(provider.resolveLocalName(address: walletAddress, chainId: 1), WalletMetadataProvider.localWalletName)
        XCTAssertNil(provider.resolveToken(chainId: 1, address: tokenAddress))
    }

    func testMainThreadSkipsLiveLookup() {
        MockURLProtocol.handler = { request in
            XCTFail("main-thread guard should prevent network lookup: \(String(describing: request.url))")
            return Self.jsonResponse(url: request.url!, body: [:])
        }

        let provider = WalletMetadataProvider(
            seedTokenStore: SeedTokenStore(data: Data("{}".utf8)),
            memoryCache: InMemoryResolutionCache(),
            persistentCache: makePersistentCache(name: #function),
            alchemyClient: AlchemyClient(apiKey: "demo", session: makeSession()),
            walletAddressProvider: { nil },
            isMainThread: { true },
            now: Date.init
        )

        XCTAssertNil(provider.resolveToken(chainId: 1, address: tokenAddress))
        XCTAssertEqual(MockURLProtocol.requests.count, 0)
    }

    private func makeProvider(
        seedEntries: [String: TokenMetadata],
        persistentCache: PersistentResolutionCache? = nil,
        session: URLSession
    ) -> WalletMetadataProvider {
        let seedData = try! JSONEncoder().encode(seedEntries)
        return WalletMetadataProvider(
            seedTokenStore: SeedTokenStore(data: seedData),
            memoryCache: InMemoryResolutionCache(),
            persistentCache: persistentCache ?? makePersistentCache(name: UUID().uuidString),
            alchemyClient: AlchemyClient(apiKey: "demo", session: session),
            walletAddressProvider: { self.walletAddress },
            isMainThread: { false },
            now: { Date(timeIntervalSince1970: 1_700_000_000) }
        )
    }

    private func makePersistentCache(name: String) -> PersistentResolutionCache {
        let suiteName = "WalletMetadataProviderTests.\(name)"
        let defaults = UserDefaults(suiteName: suiteName)!
        defaults.removePersistentDomain(forName: suiteName)
        return PersistentResolutionCache(userDefaults: defaults, namespace: suiteName)
    }

    private func makeSession() -> URLSession {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [MockURLProtocol.self]
        return URLSession(configuration: configuration)
    }

    private func encodeUniversalResolverReverseResponse(name: String) -> String {
        let nameData = Data(name.utf8)
        let offset = abiWord(96)
        let resolver = Data(repeating: 0, count: 32)
        let reverseResolver = Data(repeating: 0, count: 32)
        var payload = Data()
        payload.append(offset)
        payload.append(resolver)
        payload.append(reverseResolver)
        payload.append(abiWord(UInt64(nameData.count)))
        payload.append(nameData)

        let remainder = nameData.count % 32
        if remainder != 0 {
            payload.append(Data(repeating: 0, count: 32 - remainder))
        }

        return "0x" + payload.hexString
    }

    private func abiWord(_ value: UInt64) -> Data {
        var data = Data(repeating: 0, count: 32)
        withUnsafeBytes(of: value.bigEndian) { rawBuffer in
            data.replaceSubrange(24..<32, with: rawBuffer)
        }
        return data
    }

    private static func jsonResponse(url: URL, body: [String: Any]) -> (HTTPURLResponse, Data) {
        let data = try! JSONSerialization.data(withJSONObject: body)
        let response = HTTPURLResponse(url: url, statusCode: 200, httpVersion: nil, headerFields: nil)!
        return (response, data)
    }

    private static func jsonObject(from request: URLRequest) -> [String: Any]? {
        guard let body = request.httpBody else {
            return nil
        }

        return try? JSONSerialization.jsonObject(with: body) as? [String: Any]
    }
}

final class MockURLProtocol: URLProtocol {
    static var handler: ((URLRequest) throws -> (HTTPURLResponse, Data))?
    static private(set) var requests: [URLRequest] = []

    override class func canInit(with request: URLRequest) -> Bool {
        true
    }

    override class func canonicalRequest(for request: URLRequest) -> URLRequest {
        request
    }

    override func startLoading() {
        Self.requests.append(request)

        guard let handler = Self.handler else {
            client?.urlProtocol(self, didFailWithError: URLError(.badServerResponse))
            return
        }

        do {
            let (response, data) = try handler(request)
            client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
            client?.urlProtocol(self, didLoad: data)
            client?.urlProtocolDidFinishLoading(self)
        } catch {
            client?.urlProtocol(self, didFailWithError: error)
        }
    }

    override func stopLoading() {}

    static func reset() {
        handler = nil
        requests = []
    }
}
