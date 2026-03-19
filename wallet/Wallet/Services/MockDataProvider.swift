import Foundation
import Erc7730
import ReownWalletKit

/// Mock implementation of the `DataProviderFfi` protocol for development/testing.
///
/// Simulates async wallet-side resolution with a 1-second delay per call.
/// Uses WalletConnect's CAIP-10 `Account` for chain+address lookups.
/// Replace with real resolution (contacts DB, ENS lookup, etc.) in production.
final class MockDataProvider: DataProviderFfi {

    /// Simulate network/DB latency.
    private func simulateLatency() {
        Thread.sleep(forTimeInterval: 1.0)
    }

    private func account(chainId: UInt64, address: String) -> Account? {
        Account(chainIdentifier: "eip155:\(chainId)", address: address)
    }

    func resolveToken(chainId: UInt64, address: String) -> TokenMetaFfi? {
        guard let acct = account(chainId: chainId, address: address) else { return nil }

        switch acct.absoluteString.lowercased() {
        case "eip155:1:0xdac17f958d2ee523a2206206994597c13d831ec7":
            return TokenMetaFfi(symbol: "USDT", decimals: 6, name: "Tether USD")
        case "eip155:1:0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48":
            return TokenMetaFfi(symbol: "USDC", decimals: 6, name: "USD Coin")
        default:
            return nil
        }
    }

    func resolveEnsName(address: String, chainId: UInt64) -> String? {
        simulateLatency()
        guard let acct = account(chainId: chainId, address: address) else { return nil }

        switch acct.address.lowercased() {
        case "0xd8da6bf26964af9d7eed9e03e53415d37aa96045":
            return "vitalik.eth"
        default:
            return nil
        }
    }

    func resolveLocalName(address: String, chainId: UInt64) -> String? {
        simulateLatency()
        guard let acct = account(chainId: chainId, address: address) else { return nil }

        switch acct.address.lowercased() {
        case "0xbf01daf454dce008d3e2bfd47d5e186f71477253":
            return "My Wallet"
        default:
            return nil
        }
    }

    func resolveNftCollectionName(collectionAddress: String, chainId: UInt64) -> String? {
        simulateLatency()
        return nil
    }
}
