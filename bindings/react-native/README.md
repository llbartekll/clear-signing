# @clear-signing/react-native

React Native (iOS + Android) bindings for the `clear-signing` Rust crate — ERC-7730 v2 clear-signing for Ethereum calldata and EIP-712 typed data.

Generated via [`uniffi-bindgen-react-native`](https://github.com/jhugman/uniffi-bindgen-react-native) on top of the existing UniFFI surface in `crates/clear-signing/src/uniffi_compat/mod.rs`.

## Status

Spike. Not yet published.

## Build

```sh
npm install
npm run ubrn:ios
npm run ubrn:android
```

Outputs:
- `cpp/` — JSI bridge
- `ios/` — TurboModule sources + podspec
- `android/` — TurboModule sources + build.gradle + JNI libs
- `src/generated/` — TypeScript bindings
