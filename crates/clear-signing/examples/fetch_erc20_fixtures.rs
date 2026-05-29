//! Refresh the real-transaction fixture set under
//! `crates/clear-signing/tests/fixtures/standard_token/`.
//!
//! For each tuple in `CURATED`, fetch the transaction via Etherscan V2, run the
//! library's synthesis + format pipeline, and snapshot the rendered output into
//! a JSON fixture committed to the repo. The e2e test then asserts the library
//! still produces the same output on every CI run.
//!
//! Usage:
//!   [ -f .env ] && export $(grep -v '^#' .env | xargs 2>/dev/null)
//!   cargo run -p clear-signing --example fetch_erc20_fixtures \
//!       --features github-registry

use std::path::PathBuf;
use std::time::Duration;

use clear_signing::resolver::StaticSource;
use clear_signing::token::StaticTokenSource;
use clear_signing::{
    format_calldata, resolve_descriptors_for_tx, DisplayEntry, TokenMeta, TransactionContext,
};
use serde_json::json;

struct Tuple {
    label: &'static str,
    chain_id: u64,
    token_address: &'static str,
    token: TokenSpec,
    tx_hash: &'static str,
}

struct TokenSpec {
    symbol: &'static str,
    decimals: u8,
    name: &'static str,
}

const CURATED: &[Tuple] = &[
    // Add curated (chain, token, fn, tx) tuples here. Keep the file size small —
    // each fixture is committed. The label drives the output filename.
    //
    // Examples (commented out — fill in real tx hashes manually):
    //
    // Tuple {
    //     label: "mainnet-usdt-approve",
    //     chain_id: 1,
    //     token_address: "0xdac17f958d2ee523a2206206994597c13d831ec7",
    //     token: TokenSpec { symbol: "USDT", decimals: 6, name: "Tether USD" },
    //     tx_hash: "0x...",
    // },
];

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("ETHERSCAN_API_KEY")
        .map_err(|_| "ETHERSCAN_API_KEY is required (load via `.env`)")?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/standard_token");
    std::fs::create_dir_all(&out_dir)?;

    #[allow(clippy::const_is_empty)]
    if CURATED.is_empty() {
        eprintln!(
            "CURATED is empty — add (chain, token, tx_hash) tuples in examples/fetch_erc20_fixtures.rs"
        );
        return Ok(());
    }

    for tuple in CURATED {
        println!("Fetching {} (tx {})", tuple.label, tuple.tx_hash);

        let tx_data = fetch_transaction(&client, &api_key, tuple.chain_id, tuple.tx_hash).await?;

        let calldata_bytes = decode_hex(&tx_data.input)?;
        if calldata_bytes.len() < 4 {
            return Err(format!(
                "[{}] calldata too short (got {} bytes)",
                tuple.label,
                calldata_bytes.len()
            )
            .into());
        }
        let selector: [u8; 4] = calldata_bytes[..4].try_into().unwrap();
        if !is_standard_erc20_selector(selector) {
            return Err(format!(
                "[{}] selector 0x{} is not a standard ERC-20 selector",
                tuple.label,
                hex::encode(selector)
            )
            .into());
        }

        let token_meta = TokenMeta {
            symbol: tuple.token.symbol.to_string(),
            decimals: tuple.token.decimals,
            name: tuple.token.name.to_string(),
        };
        let mut tokens = StaticTokenSource::new();
        tokens.insert(tuple.chain_id, tuple.token_address, token_meta);

        let value_bytes = if tx_data.value == "0x" || tx_data.value == "0x0" {
            None
        } else {
            Some(decode_hex(&tx_data.value)?)
        };

        let source = StaticSource::new();
        let tx = TransactionContext {
            chain_id: tuple.chain_id,
            to: tuple.token_address,
            calldata: &calldata_bytes,
            value: value_bytes.as_deref(),
            from: Some(&tx_data.from),
            implementation_address: None,
        };

        let descriptors = resolve_descriptors_for_tx(&tx, &source, Some(&tokens)).await?;
        if descriptors.is_empty() {
            return Err(format!("[{}] synth did not fire", tuple.label).into());
        }
        let model = format_calldata(&descriptors, &tx, &tokens).await?;

        let mut fields = Vec::new();
        for entry in &model.entries {
            match entry {
                DisplayEntry::Item(item) => fields.push(json!({
                    "label": item.label,
                    "value": item.value,
                })),
                DisplayEntry::Group { items, .. } => {
                    for item in items {
                        fields.push(json!({
                            "label": item.label,
                            "value": item.value,
                        }));
                    }
                }
                DisplayEntry::Nested { .. } => {
                    return Err(format!(
                        "[{}] unexpected nested entry for direct ERC-20 call",
                        tuple.label
                    )
                    .into());
                }
            }
        }

        let fixture = json!({
            "tx_hash": tuple.tx_hash,
            "chain_id": tuple.chain_id,
            "to": tuple.token_address,
            "from": tx_data.from,
            "calldata_hex": tx_data.input,
            "value_hex": if tx_data.value == "0x" { "0x00".to_string() } else { tx_data.value.clone() },
            "token_meta": {
                "symbol": tuple.token.symbol,
                "decimals": tuple.token.decimals,
                "name": tuple.token.name,
            },
            "expected": {
                "intent": model.intent,
                "interpolated_intent": model.interpolated_intent.clone().unwrap_or_default(),
                "fields": fields,
            }
        });

        let out_path = out_dir.join(format!("{}.json", tuple.label));
        std::fs::write(&out_path, serde_json::to_string_pretty(&fixture)?)?;
        println!("  wrote {}", out_path.display());

        // Polite throttle — Etherscan's free tier rate-limits to 5 req/s.
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    Ok(())
}

fn is_standard_erc20_selector(s: [u8; 4]) -> bool {
    matches!(
        s,
        [0xa9, 0x05, 0x9c, 0xbb] | [0x09, 0x5e, 0xa7, 0xb3] | [0x23, 0xb8, 0x72, 0xdd]
    )
}

fn decode_hex(s: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let trimmed = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let padded;
    let h = if !trimmed.len().is_multiple_of(2) {
        padded = format!("0{trimmed}");
        &padded
    } else {
        trimmed
    };
    Ok(hex::decode(h)?)
}

#[derive(serde::Deserialize)]
struct EtherscanTx {
    from: String,
    #[allow(dead_code)]
    to: Option<String>,
    input: String,
    value: String,
}

#[derive(serde::Deserialize)]
struct EtherscanResponse {
    result: Option<EtherscanTx>,
    #[serde(default)]
    error: Option<EtherscanError>,
}

#[derive(serde::Deserialize)]
struct EtherscanError {
    #[allow(dead_code)]
    code: i64,
    message: String,
}

async fn fetch_transaction(
    client: &reqwest::Client,
    api_key: &str,
    chain_id: u64,
    tx_hash: &str,
) -> Result<EtherscanTx, Box<dyn std::error::Error>> {
    let url = format!(
        "https://api.etherscan.io/v2/api?chainid={chain_id}&module=proxy&action=eth_getTransactionByHash&txhash={tx_hash}&apikey={api_key}"
    );
    let resp: EtherscanResponse = client.get(&url).send().await?.json().await?;
    if let Some(err) = resp.error {
        return Err(format!("etherscan error: {}", err.message).into());
    }
    resp.result
        .ok_or_else(|| format!("tx {tx_hash} not found on chain {chain_id}").into())
}
