import Foundation

/// Bundled lookup of well-known DeFi contract addresses → display names.
/// Mirrors `SeedTokenStore`'s pattern; keys follow `LookupKey.contract`.
struct SeedContractStore {
    private let contracts: [String: ContractMetadata]

    init(bundle: Bundle, resourceName: String = "known-contracts", resourceExtension: String = "json") {
        let data = bundle.url(forResource: resourceName, withExtension: resourceExtension)
            .flatMap { try? Data(contentsOf: $0) }
        contracts = Self.decode(from: data)
    }

    init(data: Data) {
        contracts = Self.decode(from: data)
    }

    func contract(chainId: UInt64, address: String) -> ContractMetadata? {
        contracts[LookupKey.contract(chainId: chainId, address: address)]
    }

    private static func decode(from data: Data?) -> [String: ContractMetadata] {
        guard let data else { return [:] }
        let decoder = JSONDecoder()
        return (try? decoder.decode([String: ContractMetadata].self, from: data)) ?? [:]
    }
}
