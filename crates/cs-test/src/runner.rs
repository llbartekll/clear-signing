use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use clear_signing::TransactionContext;
use clear_signing::{format_calldata, format_typed_data};
use clear_signing::eip712::TypedData;
use clear_signing::merge::merge_descriptors;
use clear_signing::resolver::ResolvedDescriptor;
use clear_signing::types::descriptor::Descriptor;

use crate::compare::{case_error, compare, CaseResult};
use crate::provider::StubDataProvider;
use crate::rlp::decode_signed;
use crate::schema::{DataProviderStub, TestCase, TestFile};

const MAX_INCLUDE_DEPTH: u8 = 3;

pub async fn run_file(path: &Path, case_filter: Option<&str>) -> Result<Vec<CaseResult>> {
    let file_str = std::fs::read_to_string(path)
        .with_context(|| format!("read test file {}", path.display()))?;
    let file: TestFile = serde_json::from_str(&file_str)
        .with_context(|| format!("parse test file {}", path.display()))?;

    let descriptor_path = resolve_descriptor_path(path, &file.descriptor);
    let descriptor_json = std::fs::read_to_string(&descriptor_path)
        .with_context(|| format!("read descriptor {}", descriptor_path.display()))?;
    let descriptor_json = resolve_includes(&descriptor_json, &descriptor_path, 0)?;
    let descriptor = Descriptor::from_json(&descriptor_json)
        .with_context(|| format!("parse descriptor {}", descriptor_path.display()))?;

    let matching: Vec<&TestCase> = file
        .tests
        .iter()
        .filter(|c| case_filter.is_none_or(|f| c.description() == f))
        .collect();

    if let Some(filter) = case_filter {
        if matching.is_empty() {
            return Err(anyhow!(
                "--case {filter:?} matched no test case in {}",
                path.display()
            ));
        }
    }

    let mut results = Vec::new();
    for case in matching {
        let provider_stub = DataProviderStub::merged(file.data_provider.as_ref(), case.case_provider());
        let provider = StubDataProvider::new(provider_stub);
        let result = match case {
            TestCase::Calldata(c) => run_calldata(c, &descriptor, &provider).await,
            TestCase::Eip712(c) => run_eip712(c, &descriptor, &provider).await,
        };
        results.push(result);
    }
    Ok(results)
}

async fn run_calldata(
    case: &crate::schema::CalldataCase,
    descriptor: &Descriptor,
    provider: &StubDataProvider,
) -> CaseResult {
    let decoded = match decode_signed(&case.raw_tx) {
        Ok(d) => d,
        Err(e) => return case_error(&case.description, format!("decode rawTx: {e:#}")),
    };

    let descriptors = vec![ResolvedDescriptor {
        descriptor: descriptor.clone(),
        chain_id: decoded.chain_id,
        address: decoded.to.clone(),
    }];

    let tx = TransactionContext {
        chain_id: decoded.chain_id,
        to: &decoded.to,
        calldata: &decoded.data,
        value: Some(&decoded.value),
        from: None,
        implementation_address: None,
    };

    match format_calldata(&descriptors, &tx, provider).await {
        Ok(outcome) => compare(&case.description, &case.expected, &outcome),
        Err(e) => case_error(&case.description, format!("format_calldata: {e:?}")),
    }
}

async fn run_eip712(
    case: &crate::schema::Eip712Case,
    descriptor: &Descriptor,
    provider: &StubDataProvider,
) -> CaseResult {
    let typed: TypedData = match serde_json::from_value(case.data.clone()) {
        Ok(t) => t,
        Err(e) => return case_error(&case.description, format!("parse typed data: {e}")),
    };

    let chain_id = typed.domain.chain_id.unwrap_or(1);
    let verifying = typed
        .domain
        .verifying_contract
        .clone()
        .unwrap_or_else(|| "0x0000000000000000000000000000000000000000".to_string());

    let descriptors = vec![ResolvedDescriptor {
        descriptor: descriptor.clone(),
        chain_id,
        address: verifying,
    }];

    match format_typed_data(&descriptors, &typed, provider).await {
        Ok(outcome) => compare(&case.description, &case.expected, &outcome),
        Err(e) => case_error(&case.description, format!("format_typed_data: {e:?}")),
    }
}

fn resolve_descriptor_path(test_path: &Path, descriptor_rel: &Path) -> std::path::PathBuf {
    if descriptor_rel.is_absolute() {
        return descriptor_rel.to_path_buf();
    }
    test_path
        .parent()
        .map(|p| p.join(descriptor_rel))
        .unwrap_or_else(|| descriptor_rel.to_path_buf())
}

fn resolve_includes(descriptor_json: &str, descriptor_path: &Path, depth: u8) -> Result<String> {
    if depth > MAX_INCLUDE_DEPTH {
        return Err(anyhow!(
            "include chain exceeds depth {MAX_INCLUDE_DEPTH} at {}",
            descriptor_path.display()
        ));
    }
    let value: serde_json::Value = serde_json::from_str(descriptor_json)?;
    let include_ref = value.get("includes").and_then(|v| v.as_str()).map(|s| s.to_string());
    let Some(rel) = include_ref else {
        return Ok(descriptor_json.to_string());
    };
    let included_path: PathBuf = descriptor_path
        .parent()
        .map(|p| p.join(&rel))
        .unwrap_or_else(|| PathBuf::from(&rel));
    let included_raw = std::fs::read_to_string(&included_path)
        .with_context(|| format!("read included descriptor {}", included_path.display()))?;
    let included_resolved = resolve_includes(&included_raw, &included_path, depth + 1)?;
    let merged = merge_descriptors(descriptor_json, &included_resolved)
        .map_err(|e| anyhow!("merge_descriptors at {}: {e}", descriptor_path.display()))?;
    Ok(merged)
}
