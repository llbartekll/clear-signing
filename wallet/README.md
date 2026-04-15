# Wallet Demo

WalletConnect v2 wallet demo that uses the ERC-7730 clear signing library to display human-readable transaction details.

The demo now models clear-signing results explicitly:
- `clearSigned`: trusted clear-signing card
- `fallback`: degraded / unverified card with fallback reason and diagnostics
- `failure`: blocking error state

Diagnostic captures include outcome kind, fallback reason, diagnostics, and failure metadata.

## Setup

1. Copy the WalletConnect config template:

```sh
cp Config.xcconfig.template Config.xcconfig
```

2. Edit `Config.xcconfig`:

```xcconfig
WALLETCONNECT_PROJECT_ID = your_walletconnect_project_id
ALCHEMY_API_KEY = your_alchemy_api_key
```

`ALCHEMY_API_KEY` is optional. Without it, the wallet still clear-signs using bundled well-known token metadata, local wallet naming, and any previously cached lookups.

3. Build the XCFramework (if not already built):

```sh
./scripts/generate_uniffi_bindings.sh
./scripts/build-xcframework.sh
xcodebuild -project Wallet.xcodeproj -scheme Wallet -destination 'generic/platform=iOS Simulator' build
```

4. Open `Wallet.xcodeproj` and run on a simulator or device.

> The app works without a project ID — WalletConnect features are disabled and a message is shown instead. Key import and clear signing remain functional.
