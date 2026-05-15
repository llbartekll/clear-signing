# clear-signing RN debug app

Bare React Native (0.81.5, Hermes + New Arch) consumer of the local `react-native-clear-signing` package. Lets you pick from a handful of bundled real-world Ethereum transactions and see them rendered through the clear-signing engine — the RN equivalent of the iOS `Wallet.xcodeproj` Debug tab.

The package itself is consumed via `"react-native-clear-signing": "file:.."`, so any change to the binding sources or Rust crate flows through after a regenerate.

## Run it

From this directory:

```sh
npm install
bundle install                  # iOS only, first time
bundle exec pod install --project-directory=ios  # iOS only
npm run ios       # iOS sim
npm run android   # Android emulator
```

Metro starts automatically. The first descriptor lookup hits the EF GitHub registry over HTTPS — no API keys required.

## What's in the app

- `src/fixtures.ts` — five hand-picked tx records (Uniswap V3 swap, Lido stETH submit, Aave V3 supply, WETH deposit, 1inch swap). Raw `chainId` + `to` + `calldata` + `value`, no on-chain fetch at runtime.
- `src/DemoDataProvider.ts` — `DataProviderFfi` implementation backed by a static seed map of token metadata for the fixtures' tokens. Proxy detection, ENS, NFT, block-time lookups all return undefined.
- `src/components/DebugScreen.tsx` — top-level screen. Picker → raw tx card → clear-signing card.
- `src/components/{FixturePicker,RawTransactionCard,ClearTransactionCard,EntryRow}.tsx` — the four UI building blocks.

Format pipeline: `clearSigningResolveDescriptorsForTx` → `clearSigningFormatCalldata` → render `DisplayModel.entries`.

## Limits

- Calldata only — no EIP-712 typed data screen.
- Bundled fixtures only — no tx-hash input field, no Alchemy fetch.
- Mainnet only — no chain switcher.
- No live RPC for proxy detection (`getImplementationAddress` always returns undefined). For demos that exercise the proxy path, see the spike harness history in git or the iOS Wallet app's `WalletMetadataProvider`.
