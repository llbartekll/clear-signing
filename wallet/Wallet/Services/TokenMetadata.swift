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

    static func implementation(chainId: UInt64, address: String) -> String {
        "impl.eip155:\(chainId):\(address.lowercased())"
    }

    static func blockTimestamp(chainId: UInt64, blockNumber: UInt64) -> String {
        "block.eip155:\(chainId):\(blockNumber)"
    }

    static func tokenKey(chainId: UInt64, address: String) -> String {
        "eip155:\(chainId)/erc20:\(address.lowercased())"
    }
}
