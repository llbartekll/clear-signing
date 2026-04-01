# Kotlin Integration

This guide covers the checked-in Android/Kotlin SDK surface exposed by the `android/clear-signing` module.

The primary integration API is the handwritten [android/clear-signing/src/main/kotlin/com/clearsigning/ClearSigningClient.kt](../android/clear-signing/src/main/kotlin/com/clearsigning/ClearSigningClient.kt), which wraps descriptor resolution and formatting around the generated UniFFI layer.

## What You Integrate

App-facing types:
- `com.clearsigning.ClearSigningClient`
- `com.clearsigning.DataProviderFfi`
- `com.clearsigning.DisplayModel`
- `com.clearsigning.TokenMetaFfi`
- `com.clearsigning.FfiException`

All client methods are `suspend` functions and should be called from a coroutine.

## Install The SDK

### Published Consumption

Current published consumption uses JitPack:

```groovy
dependencyResolutionManagement {
    repositories {
        maven { url 'https://jitpack.io' }
    }
}

dependencies {
    implementation 'com.github.llbartekll:clear-signing:0.1.0'
}
```

The release asset uploaded by the Kotlin workflow contains:
- Generated Kotlin bindings
- Prebuilt Android `libclear_signing.so` binaries

JitPack consumes that release output when building the Android artifact.

### Local Repo Development

The local flow matches the CI pipeline.

1. Build Android native libraries:

```sh
cargo ndk -t aarch64-linux-android build --release --features uniffi,github-registry -p clear-signing
cargo ndk -t armv7-linux-androideabi build --release --features uniffi,github-registry -p clear-signing
cargo ndk -t x86_64-linux-android build --release --features uniffi,github-registry -p clear-signing
```

2. Generate Kotlin bindings and stage native libraries:

```sh
GENERATED_DIR="android/build/generated/clear-signing"
mkdir -p "$GENERATED_DIR/kotlin" "$GENERATED_DIR/jniLibs/arm64-v8a" "$GENERATED_DIR/jniLibs/armeabi-v7a" "$GENERATED_DIR/jniLibs/x86_64"

cargo run -p clear-signing --features uniffi,github-registry --bin uniffi-bindgen -- generate \
  --library target/aarch64-linux-android/release/libclear_signing.so \
  --language kotlin --out-dir "$GENERATED_DIR/kotlin"

cp target/aarch64-linux-android/release/libclear_signing.so "$GENERATED_DIR/jniLibs/arm64-v8a/"
cp target/armv7-linux-androideabi/release/libclear_signing.so "$GENERATED_DIR/jniLibs/armeabi-v7a/"
cp target/x86_64-linux-android/release/libclear_signing.so "$GENERATED_DIR/jniLibs/x86_64/"
```

3. Assemble or publish the Android library locally:

```sh
cd android
./gradlew :clear-signing:assembleRelease :clear-signing:publishReleasePublicationToMavenLocal
```

The repo also contains a smoke consumer at [android-consumer-smoke/app/src/main/java/com/clearsigning/smoke/Smoke.kt](../android-consumer-smoke/app/src/main/java/com/clearsigning/smoke/Smoke.kt) that references the client and provider types.

## Integration Flow

Typical app flow:
1. Implement `DataProviderFfi`.
2. Construct `ClearSigningClient(dataProvider)`.
3. Call `formatCalldata(...)` or `formatTypedData(...)` from a coroutine.
4. Render the returned `DisplayModel`.

The client performs descriptor resolution before formatting:
- `formatCalldata(...)` resolves transaction descriptors, including nested calldata descriptors when needed.
- `formatTypedData(...)` resolves typed-data descriptors, including nested calldata descriptors when needed.
- Proxy detection is delegated to your `DataProviderFfi.getImplementationAddress(...)`.

## Implement DataProviderFfi

`DataProviderFfi` is the wallet-owned callback surface used for token metadata, naming, NFT metadata, block timestamps, and proxy resolution.

Skeleton only:

This sketch is intentionally illustrative and omits concrete return statements.

```kotlin
import com.clearsigning.DataProviderFfi
import com.clearsigning.TokenMetaFfi

class WalletMetadataProvider : DataProviderFfi {
    override fun resolveToken(chainId: ULong, address: String): TokenMetaFfi? {
        // Return token symbol, decimals, and name for this contract address.
        // Use wallet caches or RPC-backed metadata if you have it.
    }

    override fun resolveEnsName(address: String, chainId: ULong, types: List<String>?): String? {
        // Return an ENS or other remote name for this address when available.
        // Return null when the wallet cannot resolve a name.
    }

    override fun resolveLocalName(address: String, chainId: ULong, types: List<String>?): String? {
        // Return a wallet-local contact or account label for this address.
        // Return null when no local label exists.
    }

    override fun resolveNftCollectionName(collectionAddress: String, chainId: ULong): String? {
        // Return the NFT collection name for this contract address.
        // Return null when unknown.
    }

    override fun resolveBlockTimestamp(chainId: ULong, blockNumber: ULong): ULong? {
        // Return the block timestamp for date-format rendering that depends on block numbers.
        // Return null when the wallet cannot look it up.
    }

    override fun getImplementationAddress(chainId: ULong, address: String): String? {
        // Return the proxy implementation address when this contract is a supported proxy.
        // Return null for non-proxies or when proxy detection is unavailable.
    }
}
```

Callback contract:
- `resolveToken`: used for token amount formatting and symbol/decimals/name display.
- `resolveEnsName`: used for remote address naming, such as ENS-style labels.
- `resolveLocalName`: used for wallet-local labels or address book entries.
- `resolveNftCollectionName`: used when the descriptor wants a collection label for an NFT contract.
- `resolveBlockTimestamp`: used when descriptor rendering needs a block number converted to time.
- `getImplementationAddress`: used for proxy-aware descriptor resolution when `tx.to` does not directly match a descriptor.

## Use ClearSigningClient

### Initialize The Client

```kotlin
val provider = WalletMetadataProvider()
val client = ClearSigningClient(provider)
```

### Format Calldata

```kotlin
val model = client.formatCalldata(
    chainId = 1uL,
    to = "0xdAC17F958D2ee523a2206206994597C13D831ec7",
    calldataHex = "0xa9059cbb000000000000000000000000...",
    valueHex = null,
    fromAddress = "0x1234..."
)
```

Method behavior:
- Builds a `TransactionInput`
- Resolves descriptors for the transaction
- Formats the transaction into `DisplayModel`

Parameters:
- `chainId`: target EVM chain ID
- `to`: destination contract address
- `calldataHex`: calldata as `0x`-prefixed hex
- `valueHex`: optional `0x`-prefixed native token value
- `fromAddress`: optional sender address for sender-aware rendering

### Format Typed Data

```kotlin
val model = client.formatTypedData(
    typedDataJson = typedDataJson
)
```

Method behavior:
- Resolves descriptors for the typed data payload
- Formats the typed data into `DisplayModel`

### Resolve Descriptors For A Transaction

```kotlin
val descriptors = client.resolveDescriptorsForTx(
    chainId = 1uL,
    to = "0xdAC17F958D2ee523a2206206994597C13D831ec7",
    calldataHex = "0xa9059cbb000000000000000000000000...",
    valueHex = null,
    fromAddress = null
)
```

Use this when your app wants visibility into the resolved descriptor set before formatting.

### Resolve Descriptors For Typed Data

```kotlin
val descriptors = client.resolveDescriptorsForTypedData(
    typedDataJson = typedDataJson
)
```

Use this when your app wants descriptor diagnostics or staged formatting flows for typed data.

## Returned Types

### DisplayModel

`DisplayModel` is the SDK output you render in wallet UI.

Important fields:
- `intent`
- `interpolatedIntent`
- `entries`
- `warnings`
- `owner`

### TokenMetaFfi

`TokenMetaFfi` is the token metadata record returned from `resolveToken(...)`.

Fields:
- `symbol`
- `decimals`
- `name`

## Error Shape

Kotlin client methods can throw `FfiException`.

Current error families include:
- `InvalidDescriptorJson`
- `InvalidTypedDataJson`
- `InvalidCalldataHex`
- `InvalidValueHex`
- `Decode`
- `Descriptor`
- `Resolve`
- `TokenRegistry`
- `Render`

Treat these as SDK-level failures from descriptor parsing, resolution, decoding, or rendering.

## Related Paths

- [android/clear-signing/src/main/kotlin/com/clearsigning/ClearSigningClient.kt](../android/clear-signing/src/main/kotlin/com/clearsigning/ClearSigningClient.kt)
- [android/clear-signing/src/main/kotlin/com/clearsigning/ClearSigningTypes.kt](../android/clear-signing/src/main/kotlin/com/clearsigning/ClearSigningTypes.kt)
- [android/clear-signing/build.gradle](../android/clear-signing/build.gradle)
- [android-consumer-smoke/app/src/main/java/com/clearsigning/smoke/Smoke.kt](../android-consumer-smoke/app/src/main/java/com/clearsigning/smoke/Smoke.kt)
