#!/usr/bin/env python3

import argparse
import json
import os
import secrets
import subprocess
import sys
import tempfile
import textwrap
from pathlib import Path
from typing import Optional


DEFAULT_REGISTRY_URL = "https://raw.githubusercontent.com/llbartekll/clear-signing-erc7730-registry/v3"
REPO_ROOT = Path(__file__).resolve().parents[4]
TESTS_DIR = REPO_ROOT / "crates" / "clear-signing" / "tests"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Replay a wallet clear-signing diagnostic capture through the Rust library."
    )
    parser.add_argument("capture", help="Path to a capture JSON file, or '-' to read from stdin")
    parser.add_argument(
        "--keep-artifacts",
        action="store_true",
        help="Keep the temporary capture copy and generated Rust test file",
    )
    parser.add_argument(
        "--registry-url",
        default=DEFAULT_REGISTRY_URL,
        help="Registry URL to use when live descriptor resolution is required",
    )
    return parser.parse_args()


def read_capture(path: str) -> dict:
    if path == "-":
        return json.load(sys.stdin)
    with open(path, "r", encoding="utf-8") as handle:
        return json.load(handle)


def detect_capture_type(capture: dict) -> str:
    if capture.get("typedDataJson"):
        return "typed-data"
    if capture.get("calldata") and capture.get("to"):
        return "calldata"
    raise ValueError("capture is missing calldata or typed-data fields")


def parse_chain_id(value) -> int:
    if isinstance(value, int):
        return value
    if not isinstance(value, str):
        raise ValueError(f"unsupported chainId value: {value!r}")
    if ":" in value:
        value = value.rsplit(":", 1)[-1]
    return int(value, 10)


def selector_from_capture(capture: dict) -> Optional[str]:
    selector = capture.get("selector")
    if selector:
        return selector
    calldata = capture.get("calldata")
    if not isinstance(calldata, str):
        return None
    normalized = calldata.strip().lower()
    if normalized.startswith("0x"):
        normalized = normalized[2:]
    if len(normalized) < 8:
        return None
    return "0x" + normalized[:8]


def summarize(capture_type: str, capture: dict) -> None:
    failed_stage = capture.get("failedStage") or "unknown"
    error_text = capture.get("errorDescription") or capture.get("clearSigningError") or capture.get("signerError")
    descriptors = capture.get("resolvedDescriptorsJson") or []

    print(f"capture_type={capture_type}")
    print(f"failed_stage={failed_stage}")
    print(f"descriptor_count={len(descriptors)}")
    if capture_type == "calldata":
        print(f"chain_id={parse_chain_id(capture.get('chainId'))}")
        print(f"to={capture.get('to')}")
        print(f"selector={selector_from_capture(capture) or 'unknown'}")
        print(f"selected_descriptor_address={capture.get('selectedDescriptorAddress') or capture.get('matchedAddress') or capture.get('to')}")
    else:
        summary = capture.get("summary") or {}
        print(f"chain_id={parse_chain_id(capture.get('chainId'))}")
        print(f"primary_type={summary.get('primaryType') or 'unknown'}")
        print(f"verifying_contract={summary.get('verifyingContract') or 'unknown'}")
    if error_text:
        print(f"error_text={error_text}")
    for path in classify_files(capture_type, failed_stage):
        print(f"inspect_file={path}")


def classify_files(capture_type: str, failed_stage: str) -> list[str]:
    if failed_stage == "resolve":
        if capture_type == "typed-data":
            return [
                "wallet/Wallet/Services/ClearSigningService.swift",
                "crates/clear-signing/src/resolver/typed_selection.rs",
                "crates/clear-signing/src/resolver/nested_resolution.rs",
            ]
        return [
            "wallet/Wallet/Services/ClearSigningService.swift",
            "crates/clear-signing/src/resolver/nested_resolution.rs",
            "crates/clear-signing/src/uniffi_compat/mod.rs",
        ]

    if capture_type == "typed-data":
        return [
            "crates/clear-signing/src/eip712.rs",
            "crates/clear-signing/src/eip712_domain.rs",
            "crates/clear-signing/src/engine.rs",
        ]

    return [
        "crates/clear-signing/src/engine.rs",
        "crates/clear-signing/src/decoder.rs",
        "wallet/Wallet/Services/ClearSigningService.swift",
    ]


def write_temp_capture(capture: dict) -> Path:
    handle = tempfile.NamedTemporaryFile(
        mode="w",
        prefix="clear-signing-capture-",
        suffix=".json",
        delete=False,
        encoding="utf-8",
    )
    with handle:
        json.dump(capture, handle, indent=2, sort_keys=True)
        handle.write("\n")
    return Path(handle.name)


def generate_rust_test(capture_path: Path, capture_type: str, use_live_resolution: bool, registry_url: str) -> str:
    live_helpers = ""
    live_calldata = ""
    live_typed = ""

    if use_live_resolution:
        live_helpers = textwrap.dedent(
            """
            use clear_signing::resolver::GitHubRegistrySource;
            use clear_signing::{resolve_descriptors_for_typed_data, resolve_descriptors_for_tx};
            """
        ).strip()
        live_calldata = textwrap.dedent(
            f"""
                    if descriptors_json.is_empty() {{
                        eprintln!("REPRO_WARNING=embedded descriptors missing; using live registry resolution");
                        let source = GitHubRegistrySource::from_registry({json.dumps(registry_url)})
                            .await
                            .map_err(|e| format!("build registry source: {{e}}"))?;
                        return resolve_descriptors_for_tx(_tx, &source)
                            .await
                            .map_err(|e| format!("live resolve calldata: {{e}}"));
                    }}
            """
        ).rstrip()
        live_typed = textwrap.dedent(
            f"""
                    if descriptors_json.is_empty() {{
                        eprintln!("REPRO_WARNING=embedded descriptors missing; using live registry resolution");
                        let source = GitHubRegistrySource::from_registry({json.dumps(registry_url)})
                            .await
                            .map_err(|e| format!("build registry source: {{e}}"))?;
                        return resolve_descriptors_for_typed_data(&typed_data, &source)
                            .await
                            .map_err(|e| format!("live resolve typed data: {{e}}"));
                    }}
            """
        ).rstrip()

    live_calldata_block = textwrap.indent(live_calldata, " " * 12) if live_calldata else ""
    live_typed_block = textwrap.indent(live_typed, " " * 12) if live_typed else ""

    return textwrap.dedent(
        f"""
        use clear_signing::eip712::TypedData;
        use clear_signing::types::descriptor::Descriptor;
        use clear_signing::{{EmptyDataProvider, ResolvedDescriptor, TransactionContext, format_calldata, format_typed_data}};
        {live_helpers}

        fn parse_chain_id(value: &serde_json::Value) -> Result<u64, String> {{
            match value {{
                serde_json::Value::Number(number) => number
                    .as_u64()
                    .ok_or_else(|| "chainId number is not u64".to_string()),
                serde_json::Value::String(text) => {{
                    let chain = text.rsplit(':').next().unwrap_or(text);
                    chain
                        .parse::<u64>()
                        .map_err(|e| format!("parse chainId '{{chain}}': {{e}}"))
                }}
                _ => Err("unsupported chainId shape".to_string()),
            }}
        }}

        fn decode_hex(input: &str) -> Result<Vec<u8>, String> {{
            let trimmed = input
                .strip_prefix("0x")
                .or_else(|| input.strip_prefix("0X"))
                .unwrap_or(input);
            if trimmed.is_empty() {{
                return Ok(Vec::new());
            }}
            hex::decode(trimmed).map_err(|e| format!("decode hex '{{input}}': {{e}}"))
        }}

        fn string_field<'a>(value: &'a serde_json::Value, key: &str) -> Result<&'a str, String> {{
            value
                .get(key)
                .and_then(|field| field.as_str())
                .ok_or_else(|| format!("missing string field '{{key}}'"))
        }}

        fn string_vec_field(value: &serde_json::Value, key: &str) -> Result<Vec<String>, String> {{
            match value.get(key) {{
                Some(serde_json::Value::Array(items)) => items
                    .iter()
                    .map(|item| {{
                        item.as_str()
                            .map(|s| s.to_string())
                            .ok_or_else(|| format!("field '{{key}}' contains non-string item"))
                    }})
                    .collect(),
                Some(_) => Err(format!("field '{{key}}' is not an array")),
                None => Ok(Vec::new()),
            }}
        }}

        fn build_resolved_descriptors(
            descriptors_json: &[String],
            chain_id: u64,
            address: &str,
        ) -> Result<Vec<ResolvedDescriptor>, String> {{
            descriptors_json
                .iter()
                .map(|json| {{
                    let descriptor =
                        Descriptor::from_json(json).map_err(|e| format!("parse descriptor json: {{e}}"))?;
                    Ok(ResolvedDescriptor {{
                        descriptor,
                        chain_id,
                        address: address.to_lowercase(),
                    }})
                }})
                .collect()
        }}

        async fn resolve_calldata_descriptors(
            capture: &serde_json::Value,
            _tx: &TransactionContext<'_>,
        ) -> Result<Vec<ResolvedDescriptor>, String> {{
            let chain_id = parse_chain_id(capture.get("chainId").ok_or_else(|| "missing chainId".to_string())?)?;
            let address = capture
                .get("selectedDescriptorAddress")
                .and_then(|value| value.as_str())
                .or_else(|| capture.get("matchedAddress").and_then(|value| value.as_str()))
                .or_else(|| capture.get("to").and_then(|value| value.as_str()))
                .ok_or_else(|| "missing calldata match address".to_string())?;
            let descriptors_json = string_vec_field(capture, "resolvedDescriptorsJson")?;
{live_calldata_block}
            build_resolved_descriptors(&descriptors_json, chain_id, address)
        }}

        async fn resolve_typed_descriptors(
            capture: &serde_json::Value,
            typed_data: &TypedData,
        ) -> Result<Vec<ResolvedDescriptor>, String> {{
            let chain_id = match typed_data.domain.chain_id {{
                Some(chain_id) => chain_id,
                None => parse_chain_id(
                    capture
                        .get("chainId")
                        .ok_or_else(|| "missing typed-data chainId".to_string())?,
                )?,
            }};
            let address = typed_data
                .domain
                .verifying_contract
                .as_deref()
                .or_else(|| capture.get("summary").and_then(|summary| summary.get("verifyingContract")).and_then(|value| value.as_str()))
                .ok_or_else(|| "missing typed-data verifying contract".to_string())?;
            let descriptors_json = string_vec_field(capture, "resolvedDescriptorsJson")?;
{live_typed_block}
            build_resolved_descriptors(&descriptors_json, chain_id, address)
        }}

        fn print_model(model: &clear_signing::DisplayModel) {{
            eprintln!("REPRO_MODEL_INTENT={{}}", model.intent);
            if let Some(interpolated) = model.interpolated_intent.as_deref() {{
                eprintln!("REPRO_MODEL_INTERPOLATED_INTENT={{}}", interpolated);
            }}
            if let Some(owner) = model.owner.as_deref() {{
                eprintln!("REPRO_MODEL_OWNER={{}}", owner);
            }}
            for warning in &model.warnings {{
                eprintln!("REPRO_WARNING={{}}", warning);
            }}
            for entry in &model.entries {{
                match entry {{
                    clear_signing::DisplayEntry::Item(item) => {{
                        eprintln!("REPRO_ENTRY=item|{{}}|{{}}", item.label, item.value);
                    }}
                    clear_signing::DisplayEntry::Group {{ label, items, .. }} => {{
                        eprintln!("REPRO_ENTRY=group|{{}}|{{}} items", label, items.len());
                    }}
                    clear_signing::DisplayEntry::Nested {{ label, .. }} => {{
                        eprintln!("REPRO_ENTRY=nested|{{}}", label);
                    }}
                }}
            }}
        }}

        #[tokio::test]
        async fn replay_wallet_capture() {{
            let capture_json = std::fs::read_to_string({json.dumps(str(capture_path))})
                .unwrap_or_else(|e| panic!("read capture: {{e}}"));
            let capture: serde_json::Value = serde_json::from_str(&capture_json)
                .unwrap_or_else(|e| panic!("parse capture json: {{e}}"));

            match {json.dumps(capture_type)} {{
                "calldata" => {{
                    let chain_id = parse_chain_id(capture.get("chainId").expect("chainId")) .expect("chainId parse");
                    let to = string_field(&capture, "to").expect("to");
                    let calldata = decode_hex(string_field(&capture, "calldata").expect("calldata"))
                        .expect("calldata hex");
                    let value = capture
                        .get("value")
                        .and_then(|value| value.as_str())
                        .map(|value| decode_hex(value).expect("value hex"));
                    let from = capture.get("from").and_then(|value| value.as_str());
                    let implementation_address = capture
                        .get("implementationAddress")
                        .and_then(|value| value.as_str());
                    let tx = TransactionContext {{
                        chain_id,
                        to,
                        calldata: &calldata,
                        value: value.as_deref(),
                        from,
                        implementation_address,
                    }};
                    let descriptors = resolve_calldata_descriptors(&capture, &tx)
                        .await
                        .expect("resolve calldata descriptors");
                    match format_calldata(&descriptors, &tx, &EmptyDataProvider).await {{
                        Ok(model) => {{
                            eprintln!("REPRO_STATUS=success");
                            print_model(&model);
                        }}
                        Err(error) => {{
                            eprintln!("REPRO_STATUS=error");
                            eprintln!("REPRO_ERROR={{}}", error);
                        }}
                    }}
                }}
                "typed-data" => {{
                    let typed_data_json = string_field(&capture, "typedDataJson").expect("typedDataJson");
                    let typed_data: TypedData = serde_json::from_str(typed_data_json)
                        .unwrap_or_else(|e| panic!("parse typed data json: {{e}}"));
                    let descriptors = resolve_typed_descriptors(&capture, &typed_data)
                        .await
                        .expect("resolve typed-data descriptors");
                    match format_typed_data(&descriptors, &typed_data, &EmptyDataProvider).await {{
                        Ok(model) => {{
                            eprintln!("REPRO_STATUS=success");
                            print_model(&model);
                        }}
                        Err(error) => {{
                            eprintln!("REPRO_STATUS=error");
                            eprintln!("REPRO_ERROR={{}}", error);
                        }}
                    }}
                }}
                other => panic!("unsupported capture type {{other}}"),
            }}
        }}
        """
    ).strip() + "\n"


def run_repro(test_path: Path, use_live_resolution: bool) -> int:
    cmd = ["cargo", "test", "-p", "clear-signing"]
    if use_live_resolution:
        cmd.extend(["--features", "github-registry"])
    cmd.extend(["--test", test_path.stem, "--", "--nocapture"])

    print(f"repro_command={' '.join(cmd)}")
    result = subprocess.run(cmd, cwd=REPO_ROOT, check=False)
    return result.returncode


def main() -> int:
    args = parse_args()
    capture = read_capture(args.capture)
    capture_type = detect_capture_type(capture)
    summarize(capture_type, capture)

    use_live_resolution = not bool(capture.get("resolvedDescriptorsJson"))
    capture_copy = write_temp_capture(capture)
    test_name = f"debug_clear_signing_replay_{os.getpid()}_{secrets.token_hex(4)}.rs"
    test_path = TESTS_DIR / test_name

    try:
        rust_test = generate_rust_test(
            capture_path=capture_copy,
            capture_type=capture_type,
            use_live_resolution=use_live_resolution,
            registry_url=args.registry_url,
        )
        test_path.write_text(rust_test, encoding="utf-8")
        return run_repro(test_path, use_live_resolution)
    finally:
        if args.keep_artifacts:
            print(f"kept_capture={capture_copy}")
            print(f"kept_test={test_path}")
        else:
            if test_path.exists():
                test_path.unlink()
            if capture_copy.exists():
                capture_copy.unlink()


if __name__ == "__main__":
    sys.exit(main())
