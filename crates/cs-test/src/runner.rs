use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use clear_signing::eip712::TypedData;
use clear_signing::merge::merge_descriptors;
use clear_signing::resolver::{ResolvedDescriptor, StaticSource};
use clear_signing::types::context::DescriptorContext;
use clear_signing::types::descriptor::Descriptor;
use clear_signing::TransactionContext;
use clear_signing::{format_calldata, format_typed_data, resolve_descriptors_for_tx};

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
        let provider_stub =
            DataProviderStub::merged(file.data_provider.as_ref(), case.case_provider());
        let provider = StubDataProvider::new(provider_stub);
        let result = match case {
            TestCase::Calldata(c) => {
                run_calldata(c, &descriptor, &descriptor_path, path, &provider).await
            }
            TestCase::Eip712(c) => run_eip712(c, &descriptor, &provider).await,
        };
        results.push(result);
    }
    Ok(results)
}

async fn run_calldata(
    case: &crate::schema::CalldataCase,
    descriptor: &Descriptor,
    descriptor_path: &Path,
    test_path: &Path,
    provider: &StubDataProvider,
) -> CaseResult {
    let decoded = match decode_signed(&case.raw_tx) {
        Ok(d) => d,
        Err(e) => return case_error(&case.description, format!("decode rawTx: {e:#}")),
    };

    let source = build_calldata_source_from_dir(
        test_path,
        descriptor,
        descriptor_path,
        decoded.chain_id,
        &decoded.to,
    );

    let tx = TransactionContext {
        chain_id: decoded.chain_id,
        to: &decoded.to,
        calldata: &decoded.data,
        value: Some(&decoded.value),
        from: case.from.as_deref(),
        implementation_address: None,
    };

    // Resolve against the vendored descriptors only — passing the data provider
    // would trigger the engine's known-token ERC-20 synth and bypass a curated
    // descriptor (e.g. lido wstETH). Rendering still uses the provider below.
    let resolution = match resolve_descriptors_for_tx(&tx, &source, None).await {
        Ok(r) => r,
        Err(e) => return case_error(&case.description, format!("resolve descriptors: {e}")),
    };

    match format_calldata(resolution.as_slice(), &tx, provider).await {
        Ok(outcome) => compare(&case.description, &case.expected, &outcome),
        Err(e) => case_error(&case.description, format!("format_calldata: {e:?}")),
    }
}

/// Build an in-memory calldata descriptor source from the test directory: the
/// outer descriptor plus every sibling *contract* descriptor, indexed by their
/// declared deployments, so the engine can resolve nested `calldata` sub-calls
/// to other contracts (Safe/4337, kiln splitter). The outer is indexed first
/// and never overwritten; siblings are visited in sorted order, so resolution
/// is deterministic regardless of `read_dir` order. EIP-712 siblings are
/// excluded — only contract descriptors belong in a calldata source.
fn build_calldata_source_from_dir(
    test_path: &Path,
    outer: &Descriptor,
    outer_descriptor_path: &Path,
    outer_chain: u64,
    outer_addr: &str,
) -> StaticSource {
    let mut source = StaticSource::new();
    let mut seen: HashSet<(u64, String)> = HashSet::new();

    index_descriptor(&mut source, &mut seen, outer_chain, outer_addr, outer);
    for dep in outer.context.deployments() {
        index_descriptor(&mut source, &mut seen, dep.chain_id, &dep.address, outer);
    }

    let Some(dir) = test_path.parent() else {
        return source;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return source;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if !path.is_file()
            || name.ends_with(".tests.json")
            || path.extension().and_then(|e| e.to_str()) != Some("json")
            || same_file(&path, outer_descriptor_path)
        {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(resolved) = resolve_includes(&raw, &path, 0) else {
            continue;
        };
        let Ok(desc) = Descriptor::from_json(&resolved) else {
            continue;
        };
        if !matches!(desc.context, DescriptorContext::Contract(_)) {
            continue;
        }
        for dep in desc.context.deployments() {
            index_descriptor(&mut source, &mut seen, dep.chain_id, &dep.address, &desc);
        }
    }
    source
}

/// Insert a descriptor under `(chain_id, address)` unless that key is already
/// claimed — first writer wins, keeping the outer descriptor authoritative.
fn index_descriptor(
    source: &mut StaticSource,
    seen: &mut HashSet<(u64, String)>,
    chain_id: u64,
    address: &str,
    descriptor: &Descriptor,
) {
    if seen.insert((chain_id, address.to_lowercase())) {
        source.add_calldata(chain_id, address, descriptor.clone());
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
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
    let include_ref = value
        .get("includes")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
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
