import XCTest

final class WalletMetadataProviderTests: XCTestCase {
    private let tokenAddress = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
    private let walletAddress = "0xbf01daf454dce008d3e2bfd47d5e186f71477253"
    private let ensAddress = "0xd8da6bf26964af9d7eed9e03e53415d37aa96045"
    private let transactionHash = "0x675a2e96e48b77e5d8edc16bfc4dc2ea7547f950edb76fdeff40e8af250d897e"

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
            seedEntries: [LookupKey.token(chainId: 1, address: tokenAddress): TokenMetadata(symbol: "USDC", decimals: 6, name: "USD Coin")],
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

    func testBlockTimestampUsesRemoteLookupAndCachesResult() {
        MockURLProtocol.handler = { request in
            let json = try XCTUnwrap(Self.jsonObject(from: request))
            XCTAssertEqual(json["method"] as? String, "eth_getBlockByNumber")
            let params = try XCTUnwrap(json["params"] as? [Any])
            XCTAssertEqual(params.first as? String, "0x1298be0")

            return Self.jsonResponse(
                url: request.url!,
                body: [
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": [
                        "timestamp": "0x65ec8780",
                    ],
                ]
            )
        }

        let provider = makeProvider(seedEntries: [:], session: makeSession())

        XCTAssertEqual(provider.resolveBlockTimestamp(chainId: 1, blockNumber: 19_500_000), 1_710_000_000)
        XCTAssertEqual(provider.resolveBlockTimestamp(chainId: 1, blockNumber: 19_500_000), 1_710_000_000)
        XCTAssertEqual(MockURLProtocol.requests.count, 1)
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

    // MARK: - Known-contract resolution

    /// Optimism Aave V3 Pool address from the bug report. Locks the exact
    /// rendering we promised in the plan ("Aave V3 Pool" on chain 10).
    private let optimismAavePool = "0x794a61358d6845594f94dc1db02a252b5b4814ad"
    private let mainnetAavePool = "0x87870bca3f3fd6335c3f4ce8392d69350b4fa4e2"
    private let oneInchV6Router = "0x111111125421ca6dc452d289314280a0f8842a65"

    private func contractStore(
        entries: [String: ContractMetadata]
    ) -> SeedContractStore {
        let data = try! JSONEncoder().encode(entries)
        return SeedContractStore(data: data)
    }

    func testResolveLocalNameReturnsKnownContractName() {
        let provider = makeProviderWithContractStore(
            contractStore(
                entries: [
                    LookupKey.contract(chainId: 10, address: optimismAavePool):
                        ContractMetadata(name: "Aave V3 Pool"),
                ]
            )
        )
        XCTAssertEqual(
            provider.resolveLocalName(address: optimismAavePool, chainId: 10),
            "Aave V3 Pool"
        )
    }

    func testResolveLocalNameKeyedPerChain() {
        // Same universal address on two chains — both should resolve via their own key.
        let provider = makeProviderWithContractStore(
            contractStore(
                entries: [
                    LookupKey.contract(chainId: 1, address: oneInchV6Router):
                        ContractMetadata(name: "1inch Aggregation Router V6"),
                    LookupKey.contract(chainId: 42161, address: oneInchV6Router):
                        ContractMetadata(name: "1inch Aggregation Router V6"),
                ]
            )
        )
        XCTAssertEqual(
            provider.resolveLocalName(address: oneInchV6Router, chainId: 1),
            "1inch Aggregation Router V6"
        )
        XCTAssertEqual(
            provider.resolveLocalName(address: oneInchV6Router, chainId: 42161),
            "1inch Aggregation Router V6"
        )
    }

    func testResolveLocalNameWalletWinsOverKnownContract() {
        // Pathological-but-instructive setup: the user's wallet address is also
        // bundled as a "known contract". The wallet-self check must win, so the
        // user never sees their own wallet labeled as a protocol contract.
        let conflictAddress = walletAddress
        let provider = makeProviderWithContractStore(
            contractStore(
                entries: [
                    LookupKey.contract(chainId: 1, address: conflictAddress):
                        ContractMetadata(name: "Some Protocol"),
                ]
            )
        )
        XCTAssertEqual(
            provider.resolveLocalName(address: conflictAddress, chainId: 1),
            WalletMetadataProvider.localWalletName
        )
    }

    func testResolveLocalNameReturnsNilForUnknownContract() {
        let provider = makeProviderWithContractStore(
            contractStore(
                entries: [
                    LookupKey.contract(chainId: 10, address: optimismAavePool):
                        ContractMetadata(name: "Aave V3 Pool"),
                ]
            )
        )
        // Different address, supported chain.
        XCTAssertNil(
            provider.resolveLocalName(
                address: "0x0000000000000000000000000000000000000042",
                chainId: 10
            )
        )
        // Mainnet Aave Pool address, but on the wrong chain.
        XCTAssertNil(
            provider.resolveLocalName(address: mainnetAavePool, chainId: 10)
        )
    }

    func testResolveLocalNameIsCaseInsensitive() {
        let provider = makeProviderWithContractStore(
            contractStore(
                entries: [
                    LookupKey.contract(chainId: 10, address: optimismAavePool):
                        ContractMetadata(name: "Aave V3 Pool"),
                ]
            )
        )
        // Pass the uppercase / EIP-55 checksummed form; lookup normalizes to
        // lowercase before keying.
        XCTAssertEqual(
            provider.resolveLocalName(
                address: "0x794A61358D6845594F94DC1DB02A252B5B4814AD",
                chainId: 10
            ),
            "Aave V3 Pool"
        )
    }

    private func makeProviderWithContractStore(_ store: SeedContractStore) -> WalletMetadataProvider {
        WalletMetadataProvider(
            seedTokenStore: SeedTokenStore(data: Data("{}".utf8)),
            seedContractStore: store,
            memoryCache: InMemoryResolutionCache(),
            persistentCache: makePersistentCache(name: UUID().uuidString),
            alchemyClient: nil,
            walletAddressProvider: { self.walletAddress },
            isMainThread: { false },
            now: { Date(timeIntervalSince1970: 1_700_000_000) }
        )
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

    func testFetchTransactionByHashDecodesEip1559Transaction() throws {
        MockURLProtocol.handler = { request in
            let json = try XCTUnwrap(Self.jsonObject(from: request))
            XCTAssertEqual(json["method"] as? String, "eth_getTransactionByHash")
            let params = try XCTUnwrap(json["params"] as? [Any])
            XCTAssertEqual(params.first as? String, self.transactionHash)

            return Self.jsonResponse(
                url: request.url!,
                body: [
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": Self.transactionResult(
                        hash: self.transactionHash,
                        type: "0x2",
                        feeFields: [
                            "maxFeePerGas": "0x59682f00",
                            "maxPriorityFeePerGas": "0x3b9aca00",
                        ]
                    ),
                ]
            )
        }

        let transaction = try requireValue(
            AlchemyClient(apiKey: "demo", session: makeSession())
                .fetchTransactionByHash(chainId: 1, hash: transactionHash)
        )

        XCTAssertEqual(transaction.hash, transactionHash)
        XCTAssertEqual(transaction.to, "0xae7ab96520de3a18e5e111b5eaab095312d7fe84")
        XCTAssertEqual(transaction.valueHex, "0xde0b6b3a7640000")
        XCTAssertEqual(transaction.valueDisplay, "1 ETH")
        XCTAssertEqual(transaction.typeDisplay, "eip1559")
        XCTAssertEqual(transaction.maxFeePerGasHex, "0x59682f00")
        XCTAssertEqual(transaction.maxPriorityFeePerGasHex, "0x3b9aca00")
        XCTAssertEqual(MockURLProtocol.requests.count, 1)
    }

    func testFetchTransactionByHashDecodesLegacyGasPrice() throws {
        let hash = "0x450c5259de51e99ad030963694108287f28d6114e3c74d2bebb8b2c4a5e962ff"
        MockURLProtocol.handler = { request in
            Self.jsonResponse(
                url: request.url!,
                body: [
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": Self.transactionResult(
                        hash: hash,
                        type: "0x0",
                        feeFields: ["gasPrice": "0x3b9aca00"]
                    ),
                ]
            )
        }

        let transaction = try requireValue(
            AlchemyClient(apiKey: "demo", session: makeSession())
                .fetchTransactionByHash(chainId: 1, hash: hash)
        )

        XCTAssertEqual(transaction.typeDisplay, "legacy")
        XCTAssertEqual(transaction.gasPriceHex, "0x3b9aca00")
        XCTAssertNil(transaction.maxFeePerGasHex)
        XCTAssertNil(transaction.maxPriorityFeePerGasHex)
    }

    func testFetchTransactionByHashAllowsContractCreationRecipient() throws {
        let hash = "0x7fd3cca7ea85567a7741fed3d6ca181d1ffd6e8002e6771d15c8911ebfde872d"
        MockURLProtocol.handler = { request in
            var result = Self.transactionResult(hash: hash, type: "0x2", feeFields: [:])
            result["to"] = NSNull()
            return Self.jsonResponse(
                url: request.url!,
                body: [
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": result,
                ]
            )
        }

        let transaction = try requireValue(
            AlchemyClient(apiKey: "demo", session: makeSession())
                .fetchTransactionByHash(chainId: 1, hash: hash)
        )

        XCTAssertNil(transaction.to)
        XCTAssertEqual(transaction.input, "0xa1903eab")
    }

    func testFetchTransactionByHashReturnsNotFoundForNullResult() {
        MockURLProtocol.handler = { request in
            Self.jsonResponse(
                url: request.url!,
                body: [
                    "jsonrpc": "2.0",
                    "id": 1,
                    "result": NSNull(),
                ]
            )
        }

        assertNotFound(
            AlchemyClient(apiKey: "demo", session: makeSession())
                .fetchTransactionByHash(chainId: 1, hash: transactionHash)
        )
        XCTAssertEqual(MockURLProtocol.requests.count, 1)
    }

    func testFetchTransactionByHashRejectsInvalidHashBeforeRPC() {
        assertNotFound(
            AlchemyClient(apiKey: "demo", session: makeSession())
                .fetchTransactionByHash(chainId: 1, hash: "0x1234")
        )
        XCTAssertEqual(MockURLProtocol.requests.count, 0)
    }

    func testFetchTransactionByHashRejectsUnicodeHexLikeHashBeforeRPC() {
        let fullwidthHash = "0x" + String(repeating: "１", count: 64)

        assertNotFound(
            AlchemyClient(apiKey: "demo", session: makeSession())
                .fetchTransactionByHash(chainId: 1, hash: fullwidthHash)
        )
        XCTAssertEqual(MockURLProtocol.requests.count, 0)
    }

    private func requireValue(
        _ lookup: RemoteLookup<DebugRawTransaction>,
        file: StaticString = #filePath,
        line: UInt = #line
    ) throws -> DebugRawTransaction {
        switch lookup {
        case .value(let transaction):
            return transaction
        case .notFound:
            XCTFail("expected transaction, got notFound", file: file, line: line)
        case .unavailable:
            XCTFail("expected transaction, got unavailable", file: file, line: line)
        }
        throw NSError(domain: "WalletMetadataProviderTests", code: 1)
    }

    private func assertNotFound(
        _ lookup: RemoteLookup<DebugRawTransaction>,
        file: StaticString = #filePath,
        line: UInt = #line
    ) {
        if case .notFound = lookup {
            return
        }
        XCTFail("expected notFound", file: file, line: line)
    }

    private static func transactionResult(
        hash: String,
        type: String,
        feeFields: [String: String]
    ) -> [String: Any] {
        var result: [String: Any] = [
            "hash": hash,
            "from": "0x84aac1001bac1ef90ee65b94de14397412845c1c",
            "to": "0xae7ab96520de3a18e5e111b5eaab095312d7fe84",
            "value": "0xde0b6b3a7640000",
            "input": "0xa1903eab",
            "nonce": "0x1",
            "gas": "0x1adb0",
            "blockNumber": "0x17954eb",
            "blockHash": "0x1111111111111111111111111111111111111111111111111111111111111111",
            "transactionIndex": "0x5",
            "chainId": "0x1",
            "type": type,
        ]

        for (key, value) in feeFields {
            result[key] = value
        }

        return result
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
        let body: Data?
        if let httpBody = request.httpBody {
            body = httpBody
        } else if let stream = request.httpBodyStream {
            body = Data(reading: stream)
        } else {
            body = nil
        }

        guard let body else {
            return nil
        }

        return try? JSONSerialization.jsonObject(with: body) as? [String: Any]
    }
}

final class DiagnosticCaptureTests: XCTestCase {
    func testCalldataCaptureExportIncludesFailureStageDescriptorsAndSelector() throws {
        var capture = CalldataCapture(
            method: "eth_sendTransaction",
            topic: "topic",
            requestId: "1",
            chainId: "eip155:1",
            rawParamsJson: "{}"
        )
        capture.outcome = .clearSigningFailed
        capture.to = "0x1111111111111111111111111111111111111111"
        capture.calldata = "0xa9059cbb0000000000000000000000001111111111111111111111111111111111111111"
        capture.selector = CalldataCapture.selectorHex(from: capture.calldata)
        capture.failedStage = .format
        capture.selectedDescriptorAddress = "0x2222222222222222222222222222222222222222"
        capture.resolvedDescriptorsJson = [#"{ "metadata": { "owner": "Example" } }"#]
        capture.clearSigningError = "format failed"
        capture.errorDescription = "format failed"

        let object = try XCTUnwrap(exportedObject(from: capture.exportJSONString))
        XCTAssertEqual(object["failedStage"] as? String, "format")
        XCTAssertEqual(object["selector"] as? String, "0xa9059cbb")
        XCTAssertEqual(
            object["selectedDescriptorAddress"] as? String,
            "0x2222222222222222222222222222222222222222"
        )
        XCTAssertEqual(object["errorDescription"] as? String, "format failed")
        XCTAssertEqual((object["resolvedDescriptorsJson"] as? [String])?.count, 1)
    }

    func testTypedDataCaptureExportIncludesFailureStageAndEmbeddedDescriptors() throws {
        var capture = TypedDataCapture(
            method: "eth_signTypedData_v4",
            topic: "topic",
            requestId: "2",
            chainId: "eip155:1",
            rawParamsJson: "{}"
        )
        capture.outcome = .clearSigningFailed
        capture.typedDataJson = #"{"primaryType":"PermitSingle"}"#
        capture.failedStage = .resolve
        capture.resolvedDescriptorsJson = []
        capture.clearSigningError = "resolve failed"
        capture.errorDescription = "resolve failed"

        let object = try XCTUnwrap(exportedObject(from: capture.exportJSONString))
        XCTAssertEqual(object["failedStage"] as? String, "resolve")
        XCTAssertEqual(object["errorDescription"] as? String, "resolve failed")
        XCTAssertEqual(object["resolvedDescriptorsJson"] as? [String], [])
    }

    func testCalldataCaptureSuccessExportRetainsEmbeddedDescriptors() throws {
        var capture = CalldataCapture(
            method: "eth_sendTransaction",
            topic: "topic",
            requestId: "3",
            chainId: "eip155:10",
            rawParamsJson: nil
        )
        capture.outcome = .clearSigningSucceeded
        capture.resolvedDescriptorsJson = [#"{ "metadata": { "owner": "Example" } }"#]
        capture.selector = "0x12345678"

        let object = try XCTUnwrap(exportedObject(from: capture.exportJSONString))
        XCTAssertEqual(object["selector"] as? String, "0x12345678")
        XCTAssertEqual((object["resolvedDescriptorsJson"] as? [String])?.count, 1)
    }

    func testTypedDataCaptureSuccessExportAllowsEmptyDescriptorList() throws {
        var capture = TypedDataCapture(
            method: "eth_signTypedData_v4",
            topic: "topic",
            requestId: "4",
            chainId: "eip155:8453",
            rawParamsJson: nil
        )
        capture.outcome = .clearSigningSucceeded
        capture.resolvedDescriptorsJson = []

        let object = try XCTUnwrap(exportedObject(from: capture.exportJSONString))
        XCTAssertEqual(object["resolvedDescriptorsJson"] as? [String], [])
    }

    private func exportedObject(from json: String) -> [String: Any]? {
        guard let data = json.data(using: .utf8) else {
            return nil
        }
        return try? JSONSerialization.jsonObject(with: data) as? [String: Any]
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

private extension Data {
    init(reading stream: InputStream) {
        self.init()

        stream.open()
        defer { stream.close() }

        let bufferSize = 1_024
        let buffer = UnsafeMutablePointer<UInt8>.allocate(capacity: bufferSize)
        defer { buffer.deallocate() }

        while stream.hasBytesAvailable {
            let count = stream.read(buffer, maxLength: bufferSize)
            if count <= 0 {
                break
            }
            append(buffer, count: count)
        }
    }
}
