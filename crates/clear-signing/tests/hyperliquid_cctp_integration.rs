//! Integration test for Hyperliquid CCTP descriptor byte-slice paths.

use clear_signing::resolver::ResolvedDescriptor;
use clear_signing::token::WellKnownTokenSource;
use clear_signing::types::descriptor::Descriptor;
use clear_signing::{format_calldata, DisplayEntry, TransactionContext};

fn load_descriptor() -> Descriptor {
    Descriptor::from_json(
        r#"{
  "$schema": "../../specs/erc7730-v1.schema.json",
  "context": {
    "$id": "Hyperliquid - CctpExtension",
    "contract": {
      "deployments": [{ "chainId": 42161, "address": "0xA95d9c1F655341597C94393fDdc30cf3c08E4fcE" }],
      "abi": [
        {
          "inputs": [
            {
              "components": [
                { "internalType": "uint256", "name": "amount", "type": "uint256" },
                { "internalType": "uint256", "name": "authValidAfter", "type": "uint256" },
                { "internalType": "uint256", "name": "authValidBefore", "type": "uint256" },
                { "internalType": "bytes32", "name": "authNonce", "type": "bytes32" },
                { "internalType": "uint8", "name": "v", "type": "uint8" },
                { "internalType": "bytes32", "name": "r", "type": "bytes32" },
                { "internalType": "bytes32", "name": "s", "type": "bytes32" }
              ],
              "internalType": "struct ICctpExtension.ReceiveWithAuthorizationData",
              "name": "_receiveWithAuthorizationData",
              "type": "tuple"
            },
            {
              "components": [
                { "internalType": "uint256", "name": "amount", "type": "uint256" },
                { "internalType": "uint32", "name": "destinationDomain", "type": "uint32" },
                { "internalType": "bytes32", "name": "mintRecipient", "type": "bytes32" },
                { "internalType": "bytes32", "name": "destinationCaller", "type": "bytes32" },
                { "internalType": "uint256", "name": "maxFee", "type": "uint256" },
                { "internalType": "uint32", "name": "minFinalityThreshold", "type": "uint32" },
                { "internalType": "bytes", "name": "hookData", "type": "bytes" }
              ],
              "internalType": "struct ICctpExtension.DepositForBurnWithHookData",
              "name": "_depositForBurnData",
              "type": "tuple"
            }
          ],
          "name": "batchDepositForBurnWithAuth",
          "outputs": [],
          "stateMutability": "nonpayable",
          "type": "function"
        }
      ]
    }
  },
  "metadata": {
    "owner": "Circle",
    "info": { "legalName": "Circle Internet Financial", "url": "https://www.circle.com/" },
    "constants": { "usdcToken": "0xaf88d065e77c8cC2239327C5EDb3A432268e5831" },
    "enums": { "cctpDomains": { "19": "HyperEVM" }, "destinationDex": { "0": "Perp", "255": "Spot" } }
  },
  "display": {
    "formats": {
      "batchDepositForBurnWithAuth((uint256 amount, uint256 authValidAfter, uint256 authValidBefore, bytes32 authNonce, uint8 v, bytes32 r, bytes32 s) _receiveWithAuthorizationData, (uint256 amount, uint32 destinationDomain, bytes32 mintRecipient, bytes32 destinationCaller, uint256 maxFee, uint32 minFinalityThreshold, bytes hookData) _depositForBurnData)": {
        "intent": "Bridge USDC via CCTP",
        "fields": [
          {
            "path": "_depositForBurnData.amount",
            "label": "Amount",
            "format": "tokenAmount",
            "params": { "token": "$.metadata.constants.usdcToken" }
          },
          {
            "path": "_depositForBurnData.destinationDomain",
            "label": "Destination chain",
            "format": "enum",
            "params": { "$ref": "$.metadata.enums.cctpDomains" }
          },
          {
            "path": "_depositForBurnData.mintRecipient.[-20:]",
            "label": "Mint recipient",
            "format": "addressName",
            "params": { "types": ["contract"], "sources": ["local"] }
          },
          {
            "path": "_depositForBurnData.destinationCaller.[-20:]",
            "label": "Destination caller",
            "format": "addressName",
            "params": { "types": ["contract"], "sources": ["local"] }
          },
          {
            "path": "_depositForBurnData.maxFee",
            "label": "Max fee",
            "format": "tokenAmount",
            "params": { "token": "$.metadata.constants.usdcToken" }
          },
          {
            "path": "_depositForBurnData.hookData.[32:52]",
            "label": "HyperEVM recipient",
            "format": "addressName",
            "params": { "types": ["eoa"], "sources": ["local", "ens"] }
          },
          {
            "path": "_depositForBurnData.hookData.[52:53]",
            "label": "Destination DEX",
            "format": "enum",
            "params": { "$ref": "$.metadata.enums.destinationDex" }
          }
        ],
        "required": [
          "_depositForBurnData.amount",
          "_depositForBurnData.destinationDomain",
          "_depositForBurnData.mintRecipient",
          "_depositForBurnData.destinationCaller",
          "_depositForBurnData.maxFee",
          "_depositForBurnData.minFinalityThreshold",
          "_depositForBurnData.hookData"
        ],
        "excluded": ["_receiveWithAuthorizationData", "_depositForBurnData.minFinalityThreshold"]
      }
    }
  }
}"#,
    )
    .unwrap()
}

fn wrap_rd(descriptor: Descriptor) -> Vec<ResolvedDescriptor> {
    vec![ResolvedDescriptor {
        descriptor,
        chain_id: 42161,
        address: "0xa95d9c1f655341597c94393fddc30cf3c08e4fce".to_string(),
    }]
}

fn entry_value<'a>(entries: &'a [DisplayEntry], label: &str) -> &'a str {
    for entry in entries {
        if let DisplayEntry::Item(item) = entry {
            if item.label == label {
                return &item.value;
            }
        }
    }
    panic!("missing entry '{label}'");
}

#[tokio::test]
async fn hyperliquid_cctp_byte_slice_fields_resolve() {
    let descriptor = load_descriptor();
    let descriptors = wrap_rd(descriptor);
    let calldata = hex::decode("95878db1000000000000000000000000000000000000000000000000000000001dbe22c00000000000000000000000000000000000000000000000000000000069c533620000000000000000000000000000000000000000000000000000000069c541ae47448c869ca8acc78b1d890609074411298e31fdef0e74e20143fd644570bb6a000000000000000000000000000000000000000000000000000000000000001c962f9325267a529131b6fe51f0b3abb2456b7eadd21229590ab86645814626465a4925770c83a6cd3d6e5d0f423e3049349c8f61de725220d2806030cde5fe020000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000001dbe22c00000000000000000000000000000000000000000000000000000000000000013000000000000000000000000b21d281dedb17ae5b501f6aa8256fe38c4e45757000000000000000000000000b21d281dedb17ae5b501f6aa8256fe38c4e457570000000000000000000000000000000000000000000000000000000000030d4000000000000000000000000000000000000000000000000000000000000003e800000000000000000000000000000000000000000000000000000000000000e00000000000000000000000000000000000000000000000000000000000000038636374702d666f72776172640000000000000000000000000000000000000018f0a063a21be62b709937ca2a808594b662fe41e6000000000000000000000000").unwrap();
    let tx = TransactionContext {
        chain_id: 42161,
        to: "0xA95d9c1F655341597C94393fDdc30cf3c08E4fcE",
        calldata: &calldata,
        value: None,
        from: Some("0xf0a063a21be62b709937ca2a808594b662fe41e6"),
        implementation_address: None,
    };
    let result = format_calldata(&descriptors, &tx, &WellKnownTokenSource::new())
        .await
        .unwrap();

    assert_eq!(result.intent, "Bridge USDC via CCTP");
    assert!(
        result.diagnostics().is_empty(),
        "unexpected diagnostics: {:?}",
        result.diagnostics()
    );
    assert_eq!(entry_value(&result.entries, "Amount"), "499 USDC");
    assert_eq!(
        entry_value(&result.entries, "Destination chain"),
        "HyperEVM"
    );
    assert_eq!(
        entry_value(&result.entries, "Mint recipient"),
        "0xb21D281DEdb17AE5B501F6AA8256fe38C4e45757"
    );
    assert_eq!(
        entry_value(&result.entries, "Destination caller"),
        "0xb21D281DEdb17AE5B501F6AA8256fe38C4e45757"
    );
    assert_eq!(entry_value(&result.entries, "Max fee"), "0.2 USDC");
    assert_eq!(
        entry_value(&result.entries, "HyperEVM recipient"),
        "0xF0A063A21Be62B709937Ca2a808594b662fE41E6"
    );
    assert_eq!(entry_value(&result.entries, "Destination DEX"), "Perp");
}
