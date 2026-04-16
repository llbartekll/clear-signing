# Swift Integration

This guide covers the checked-in Swift SDK surface exposed by the `ClearSigning` package.

The primary integration API is the handwritten [bindings/swift/ClearSigningClient.swift](../bindings/swift/ClearSigningClient.swift), which wraps descriptor resolution and formatting around the generated UniFFI layer.

## What You Integrate

Import the package product:

```swift
import ClearSigning
```

App-facing types:
- `ClearSigningClient`
- `DataProviderFfi`
- `FormatOutcome`
- `FormatFailure`
- `DescriptorResolutionOutcome`
- `FormatDiagnostic`
- `FallbackReason`
- `DisplayModel`
- `TokenMetaFfi`

## Install The SDK

### Published Consumption

Tagged releases are consumed as a Swift package from the repository URL:

```swift
dependencies: [
    .package(url: "https://github.com/llbartekll/clear-signing", from: "0.1.0")
]
```

Then add the `ClearSigning` product to your target dependencies.

Current repo caveat:
- The checked-in [Package.swift](../Package.swift) defaults to the release XCFramework URL.
- Set `USE_LOCAL_RUST_XCFRAMEWORK=1` to make SwiftPM resolve the local XCFramework instead.
- The checked-in `Package.swift` currently declares `.iOS(.v14)`.
- The release workflow updates the manifest URL and checksum when cutting a Swift release tag.

### Local Repo Development

Build the local XCFramework first:

```sh
./scripts/build-xcframework.sh
```

That script:
- Builds `target/ios/libclear_signing.xcframework`
- Regenerates `bindings/swift/clear_signing.swift`

The local package target then resolves against the XCFramework at:
- `target/ios/libclear_signing.xcframework`

Enable local package resolution explicitly when using SwiftPM from the CLI:

```sh
USE_LOCAL_RUST_XCFRAMEWORK=1 swift package describe
```

Any SwiftPM command can use the same environment variable, for example `swift build`, `swift test`, or `swift package describe`.

## Integration Flow

Typical app flow:
1. Implement `DataProviderFfi`.
2. Create `ClearSigningClient(dataProvider:)`.
3. Call `formatCalldata(...)` or `formatTypedData(...)`.
4. Switch on `FormatOutcome` and render either clear-signed or degraded UI.

The client performs descriptor resolution before formatting:
- `formatCalldata(...)` resolves transaction descriptors, including nested calldata descriptors when needed.
- `formatTypedData(...)` resolves typed-data descriptors, including nested calldata descriptors when needed.
- Proxy detection is delegated to your `DataProviderFfi.getImplementationAddress(...)`.
- Missing token/name/NFT metadata stays best-effort and surfaces as diagnostics, not hard failures.

Wallet policy should branch on `FormatDiagnostic.code`, not parse `FormatDiagnostic.message`.

## Implement DataProviderFfi

`DataProviderFfi` is the wallet-owned callback surface. The SDK calls it synchronously across the FFI boundary whenever it needs metadata that only the host app can provide.

Skeleton only:

This sketch is intentionally illustrative and omits concrete return statements.

```swift
import ClearSigning

final class WalletMetadataProvider: DataProviderFfi, @unchecked Sendable {
    func resolveToken(chainId: UInt64, address: String) -> TokenMetaFfi? {
        // Return token symbol, decimals, and name for this contract address.
        // Use wallet caches or RPC-backed metadata if you have it.
    }

    func resolveEnsName(address: String, chainId: UInt64, types: [String]?) -> String? {
        // Return an ENS or other remote name for this address when available.
        // Return nil when the wallet cannot resolve a name.
    }

    func resolveLocalName(address: String, chainId: UInt64, types: [String]?) -> String? {
        // Return a wallet-local contact or account label for this address.
        // Return nil when no local label exists.
    }

    func resolveNftCollectionName(collectionAddress: String, chainId: UInt64) -> String? {
        // Return the NFT collection name for this contract address.
        // Return nil when unknown.
    }

    func resolveBlockTimestamp(chainId: UInt64, blockNumber: UInt64) -> UInt64? {
        // Return the block timestamp for date-format rendering that depends on block numbers.
        // Return nil when the wallet cannot look it up.
    }

    func getImplementationAddress(chainId: UInt64, address: String) -> String? {
        // Return the proxy implementation address when this contract is a supported proxy.
        // Return nil for non-proxies or when proxy detection is unavailable.
    }
}
```

Callback contract:
- `resolveToken`: used for token amount formatting and symbol/decimals/name display.
- `resolveEnsName`: used for remote address naming, such as ENS-style labels.
- `resolveLocalName`: used for wallet-local labels, address book entries, or “My Wallet”-style naming.
- `resolveNftCollectionName`: used when the descriptor wants a collection label for an NFT contract.
- `resolveBlockTimestamp`: used when descriptor rendering needs a block number converted to time.
- `getImplementationAddress`: used for proxy-aware descriptor resolution when `tx.to` does not directly match a descriptor.

## Use ClearSigningClient

### Initialize The Client

```swift
let provider = WalletMetadataProvider()
let client = ClearSigningClient(dataProvider: provider)
```

### Format Calldata

```swift
let outcome = try await client.formatCalldata(
    chainId: 1,
    to: "0xdAC17F958D2ee523a2206206994597C13D831ec7",
    calldataHex: "0xa9059cbb000000000000000000000000...",
    valueHex: nil,
    fromAddress: "0x1234..."
)

switch outcome {
case .clearSigned(let model, let diagnostics):
    renderTrusted(model: model, diagnostics: diagnostics)

case .fallback(let model, let reason, let diagnostics):
    renderGeneric(model: model, reason: reason, diagnostics: diagnostics)
}
```

Method behavior:
- Builds a `TransactionInput`
- Resolves descriptors for the transaction
- Formats the transaction into `FormatOutcome`

Parameters:
- `chainId`: target EVM chain ID
- `to`: destination contract address
- `calldataHex`: calldata as `0x`-prefixed hex
- `valueHex`: optional `0x`-prefixed native token value
- `fromAddress`: optional sender address for sender-aware rendering

### Format Typed Data

```swift
let outcome = try await client.formatTypedData(
    typedDataJson: typedDataJson
)
```

Method behavior:
- Resolves descriptors for the typed data payload
- Formats the typed data into `FormatOutcome`

### Resolve Descriptors For A Transaction

```swift
let resolution = try await client.resolveDescriptorsForTx(
    chainId: 1,
    to: "0xdAC17F958D2ee523a2206206994597C13D831ec7",
    calldataHex: "0xa9059cbb000000000000000000000000...",
    valueHex: nil,
    fromAddress: nil
)

switch resolution {
case .found(let descriptors):
    print("resolved \(descriptors.count) descriptors")
case .notFound:
    print("no descriptors resolved")
}
```

Use this when your app wants visibility into the resolved descriptor set before formatting.

### Resolve Descriptors For Typed Data

```swift
let resolution = try await client.resolveDescriptorsForTypedData(
    typedDataJson: typedDataJson
)
```

Use this when your app wants descriptor diagnostics or staged formatting flows for typed data.

## Returned Types

### FormatOutcome

`FormatOutcome` is the primary SDK result.

Cases:
- `clearSigned(model:diagnostics:)`
- `fallback(model:reason:diagnostics:)`

Recommended wallet policy:
- `clearSigned`: show trusted clear-signing UI
- `fallback`: show generic / degraded UI and keep the reason visible
- thrown `FormatFailure`: fail closed

Concrete fallback cases:
- descriptor not found for the contract
- known contract but no selector / encodeType format matched
- typed data missing `domain.chainId` or `domain.verifyingContract`
- nested calldata could not be clear-signed

### DisplayModel

`DisplayModel` is the render payload inside `FormatOutcome`.

Important fields:
- `intent`: descriptor-defined intent label
- `interpolatedIntent`: optional resolved intent string with interpolated values
- `entries`: structured display entries for list/group/nested rendering
- `owner`: descriptor owner metadata when present

### FormatDiagnostic

`FormatDiagnostic` replaces legacy warning strings.

Fields:
- `code`
- `severity` (`info` or `warning`)
- `message`

Contract:
- `code` is machine-readable and intended for wallet policy and telemetry
- `message` is human-readable and may evolve independently

Example:

```swift
if diagnostics.contains(where: { $0.code == "nested_descriptor_not_found" }) {
    showGenericNestedCallBadge()
}
```

### DescriptorResolutionOutcome

Descriptor resolution no longer uses empty arrays to signal misses.

Cases:
- `found([String])`
- `notFound`

### TokenMetaFfi

`TokenMetaFfi` is the token metadata record returned from `resolveToken(...)`.

Fields:
- `symbol`
- `decimals`
- `name`

## Error Shape

Swift client methods throw `FormatFailure`.

Cases:
- `InvalidInput(message:retryable:)`
- `InvalidDescriptor(message:retryable:)`
- `ResolutionFailed(message:retryable:)`
- `Internal(message:retryable:)`

Example:

```swift
do {
    let outcome = try await client.formatTypedData(typedDataJson: typedDataJson)
    // handle outcome
} catch let failure as FormatFailure {
    switch failure {
    case .InvalidInput(let message, _):
        showBlockingError(message)
    case .ResolutionFailed(let message, let retryable):
        showResolutionError(message: message, retryable: retryable)
    default:
        showBlockingError(failure.message)
    }
}
```

`retryable` is intended for wallet policy and retry UX.

## Related Paths

- [bindings/swift/ClearSigningClient.swift](../bindings/swift/ClearSigningClient.swift)
- [bindings/swift/clear_signing.swift](../bindings/swift/clear_signing.swift)
- [Package.swift](../Package.swift)
- [wallet/Wallet/Services/WalletMetadataProvider.swift](../wallet/Wallet/Services/WalletMetadataProvider.swift)
