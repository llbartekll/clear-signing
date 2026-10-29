//! Spec map references (`metadata.maps`) with the ERC's example files
//! `assets/erc-7730/example-maps.json` and `example-maps-pools.json` (vendored
//! verbatim). A parameter `{"map": "$.metadata.maps.<name>", "keyPath": ...}`
//! takes the map value for the key read at the key path. The examples write
//! `key` in place of the spec's `keyPath`; both are accepted.

use clear_signing::resolver::ResolvedDescriptor;
use clear_signing::token::{StaticTokenSource, TokenMeta};
use clear_signing::types::descriptor::Descriptor;
use clear_signing::{
    format_calldata, DisplayEntry, FormatFailure, FormatOutcome, TransactionContext,
};

const CONTRACT: &str = "0x00112233445566778899AABBCCDDEEFF00112233";

/// Load a fixture, deployed on `chain_id` in addition to its own deployments.
fn load_descriptor(fixture: &str, chain_id: u64) -> Descriptor {
    let path = format!("{}/tests/fixtures/{fixture}", env!("CARGO_MANIFEST_DIR"));
    let json = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let mut value: serde_json::Value =
        serde_json::from_str(&json).unwrap_or_else(|e| panic!("parse {path}: {e}"));
    value["context"]["contract"]["deployments"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({ "chainId": chain_id, "address": CONTRACT }));
    Descriptor::from_json(&value.to_string()).unwrap_or_else(|e| panic!("parse {path}: {e}"))
}

fn wrap_rd(descriptor: Descriptor, chain_id: u64) -> Vec<ResolvedDescriptor> {
    vec![ResolvedDescriptor {
        descriptor,
        chain_id,
        address: CONTRACT.to_lowercase(),
    }]
}

fn deposit_calldata(amount: u64, min_shares: u64) -> Vec<u8> {
    let sig = clear_signing::decoder::parse_signature("deposit(uint256,uint256)").unwrap();
    let mut calldata = sig.selector.to_vec();
    for value in [amount, min_shares] {
        let mut word = [0u8; 32];
        word[24..].copy_from_slice(&value.to_be_bytes());
        calldata.extend_from_slice(&word);
    }
    calldata
}

fn token(symbol: &str, decimals: u8) -> TokenMeta {
    TokenMeta {
        symbol: symbol.to_string(),
        decimals,
        name: symbol.to_string(),
    }
}

/// Underlying and share tokens per chain, at the addresses of the example maps.
fn example_tokens() -> StaticTokenSource {
    let mut tokens = StaticTokenSource::new();
    for (chain_id, underlying, share, suffix) in [
        (
            1,
            "0x0000000000000000000000000000000000deadbeef",
            "0x00000000000000000000000000000000abcdef01",
            "ETH",
        ),
        (
            137,
            "0x0000000000000000000000000000000000feedface",
            "0x00000000000000000000000000000000abcdef02",
            "POL",
        ),
        (
            42161,
            "0x0000000000000000000000000000000000cafebabe",
            "0x00000000000000000000000000000000abcdef03",
            "ARB",
        ),
    ] {
        tokens.insert(chain_id, underlying, token(&format!("UND{suffix}"), 6));
        tokens.insert(chain_id, share, token(&format!("SHR{suffix}"), 18));
    }
    tokens
}

async fn format_deposit(fixture: &str, chain_id: u64) -> Result<FormatOutcome, FormatFailure> {
    let calldata = deposit_calldata(2_500_000, 1_000_000_000_000_000_000);
    let tx = TransactionContext {
        chain_id,
        to: CONTRACT,
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };
    format_calldata(
        &wrap_rd(load_descriptor(fixture, chain_id), chain_id),
        &tx,
        &example_tokens(),
    )
    .await
}

fn items(result: &FormatOutcome) -> Vec<(String, String)> {
    result
        .entries
        .iter()
        .map(|entry| match entry {
            DisplayEntry::Item(item) => (item.label.clone(), item.value.clone()),
            other => panic!("expected item, got {other:?}"),
        })
        .collect()
}

#[tokio::test]
async fn example_maps_resolves_tokens_per_chain() {
    for (chain_id, suffix) in [(1, "ETH"), (137, "POL"), (42161, "ARB")] {
        let result = format_deposit("erc7730-example-maps.json", chain_id)
            .await
            .unwrap_or_else(|e| panic!("chain {chain_id}: {e}"));
        assert_eq!(
            items(&result),
            vec![
                ("Deposit Amount".to_string(), format!("2.5 UND{suffix}")),
                ("Min Received Shares".to_string(), format!("1 SHR{suffix}")),
            ],
            "chain {chain_id}"
        );
        assert_eq!(
            result.interpolated_intent.as_deref(),
            Some(format!("Deposit 2.5 UND{suffix} to get at least 1 SHR{suffix}").as_str())
        );
        assert!(
            result.diagnostics().is_empty(),
            "{:?}",
            result.diagnostics()
        );
    }
}

#[tokio::test]
async fn example_maps_pools_resolves_tokens() {
    let result = format_deposit("erc7730-example-maps-pools.json", 1)
        .await
        .unwrap();
    assert_eq!(
        items(&result),
        vec![
            ("Deposit Amount".to_string(), "2.5 UNDETH".to_string()),
            ("Min Received Shares".to_string(), "1 SHRETH".to_string()),
        ]
    );
}

/// "In case no values matches the wallet MUST consider the 7730 file invalid
/// for the transaction." Chain 10 is not a key of the maps (the test adds a
/// chain 10 deployment so that the descriptor applies).
#[tokio::test]
async fn example_maps_key_not_in_map_is_invalid() {
    for fixture in [
        "erc7730-example-maps.json",
        "erc7730-example-maps-pools.json",
    ] {
        match format_deposit(fixture, 10).await {
            Err(FormatFailure::InvalidDescriptor { detail, .. }) => {
                assert!(detail.contains("no value for key '10'"), "{detail}")
            }
            other => panic!("{fixture}: expected a descriptor error, got {other:?}"),
        }
    }
}
