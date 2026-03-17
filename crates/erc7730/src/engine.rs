//! Formatting pipeline: resolves display fields, formats decoded values,
//! and produces a [`DisplayModel`] with labeled entries for wallet UIs.

use std::future::Future;
use std::pin::Pin;

use num_bigint::BigUint;

use crate::decoder::{ArgumentValue, DecodedArguments};
use crate::error::Error;
use crate::provider::DataProvider;
use crate::resolver::ResolvedDescriptor;
use crate::types::descriptor::Descriptor;
use crate::types::display::{
    DisplayField, DisplayFormat, FieldFormat, FieldGroup, FormatParams, Iteration, SenderAddress,
    VisibleRule,
};

/// Maximum recursion depth for nested calldata formatting.
const MAX_CALLDATA_DEPTH: u8 = 3;

/// Output model for clear signing display.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct DisplayModel {
    pub intent: String,
    pub interpolated_intent: Option<String>,
    pub entries: Vec<DisplayEntry>,
    pub warnings: Vec<String>,
}

/// A display entry — either a flat item, a group of items, or a nested calldata call.
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[derive(Debug, Clone, serde::Serialize)]
pub enum DisplayEntry {
    Item(DisplayItem),
    Group {
        label: String,
        iteration: GroupIteration,
        items: Vec<DisplayItem>,
    },
    Nested {
        label: String,
        intent: String,
        entries: Vec<DisplayEntry>,
        warnings: Vec<String>,
    },
}

#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[derive(Debug, Clone, serde::Serialize)]
pub enum GroupIteration {
    Sequential,
    Bundled,
}

/// A single label+value pair for display.
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[derive(Debug, Clone, serde::Serialize)]
pub struct DisplayItem {
    pub label: String,
    pub value: String,
}

/// Known chain IDs → human-readable names (public for eip712 module).
pub(crate) fn chain_name_public(chain_id: u64) -> String {
    chain_name(chain_id)
}

fn chain_name(chain_id: u64) -> String {
    match chain_id {
        1 => "Ethereum".to_string(),
        10 => "Optimism".to_string(),
        56 => "BNB Chain".to_string(),
        100 => "Gnosis".to_string(),
        137 => "Polygon".to_string(),
        250 => "Fantom".to_string(),
        324 => "zkSync Era".to_string(),
        8453 => "Base".to_string(),
        42161 => "Arbitrum One".to_string(),
        42170 => "Arbitrum Nova".to_string(),
        43114 => "Avalanche".to_string(),
        59144 => "Linea".to_string(),
        534352 => "Scroll".to_string(),
        7777777 => "Zora".to_string(),
        _ => format!("Chain {chain_id}"),
    }
}

/// Rendering context passed through the pipeline (immutable).
struct RenderContext<'a> {
    descriptor: &'a Descriptor,
    decoded: &'a DecodedArguments,
    chain_id: u64,
    data_provider: &'a dyn DataProvider,
    descriptors: &'a [ResolvedDescriptor],
    depth: u8,
}

/// Format calldata into a display model using a descriptor.
pub async fn format_calldata(
    descriptor: &Descriptor,
    chain_id: u64,
    _to: &str,
    decoded: &DecodedArguments,
    _value: Option<&[u8]>,
    data_provider: &dyn DataProvider,
) -> Result<DisplayModel, Error> {
    format_calldata_inner(
        descriptor,
        chain_id,
        _to,
        decoded,
        _value,
        data_provider,
        &[],
    )
    .await
}

/// Format calldata with pre-resolved inner descriptors for nested calldata support.
pub async fn format_calldata_multi(
    descriptor: &Descriptor,
    chain_id: u64,
    to: &str,
    decoded: &DecodedArguments,
    value: Option<&[u8]>,
    data_provider: &dyn DataProvider,
    descriptors: &[ResolvedDescriptor],
) -> Result<DisplayModel, Error> {
    format_calldata_inner(
        descriptor,
        chain_id,
        to,
        decoded,
        value,
        data_provider,
        descriptors,
    )
    .await
}

async fn format_calldata_inner(
    descriptor: &Descriptor,
    chain_id: u64,
    _to: &str,
    decoded: &DecodedArguments,
    _value: Option<&[u8]>,
    data_provider: &dyn DataProvider,
    descriptors: &[ResolvedDescriptor],
) -> Result<DisplayModel, Error> {
    // Find matching format by function name + signature
    let format = find_format(descriptor, &decoded.function_name, &decoded.selector)?;

    let ctx = RenderContext {
        descriptor,
        decoded,
        chain_id,
        data_provider,
        descriptors,
        depth: 0,
    };

    let mut warnings = Vec::new();
    let entries = render_fields(&ctx, &format.fields, &mut warnings).await?;

    let interpolated = match format.interpolated_intent.as_ref() {
        Some(template) => Some(interpolate_intent(template, &ctx, &format.fields).await),
        None => None,
    };

    Ok(DisplayModel {
        intent: format
            .intent
            .clone()
            .unwrap_or_else(|| decoded.function_name.clone()),
        interpolated_intent: interpolated,
        entries,
        warnings,
    })
}

/// Find the display format matching the decoded function.
fn find_format<'a>(
    descriptor: &'a Descriptor,
    function_name: &str,
    selector: &[u8; 4],
) -> Result<&'a DisplayFormat, Error> {
    let selector_hex = hex::encode(selector);

    // Try exact match on format keys
    for (key, format) in &descriptor.display.formats {
        // Match by full signature or by function name
        if key == function_name {
            return Ok(format);
        }
        // Match by computing selector from the key (handles named params)
        if key.contains('(') {
            if let Ok(parsed) = crate::decoder::parse_signature(key) {
                if hex::encode(parsed.selector) == selector_hex {
                    return Ok(format);
                }
            }
        }
    }

    Err(Error::Render(format!(
        "no display format found for function '{}' (selector 0x{})",
        function_name, selector_hex
    )))
}

/// Render a list of display fields into display entries.
///
/// Uses `Pin<Box<dyn Future>>` to support recursive calls (references, groups).
fn render_fields<'a>(
    ctx: &'a RenderContext<'a>,
    fields: &[DisplayField],
    warnings: &'a mut Vec<String>,
) -> Pin<Box<dyn Future<Output = Result<Vec<DisplayEntry>, Error>> + Send + 'a>> {
    let fields = fields.to_vec();
    Box::pin(async move {
        let mut entries = Vec::new();

        for field in &fields {
            match field {
                DisplayField::Reference { reference } => {
                    if let Some(resolved) = resolve_reference(ctx.descriptor, reference) {
                        let resolved_slice = vec![resolved];
                        let mut sub = render_fields(ctx, &resolved_slice, warnings).await?;
                        entries.append(&mut sub);
                    } else {
                        warnings.push(format!("unresolved reference: {reference}"));
                    }
                }
                DisplayField::Group { field_group } => {
                    if let Some(entry) = render_field_group(ctx, field_group, warnings).await? {
                        entries.push(entry);
                    }
                }
                DisplayField::Simple {
                    path,
                    label,
                    format,
                    params,
                    visible,
                } => {
                    // Resolve the value from decoded arguments
                    let value = resolve_path(ctx.decoded, path);

                    // Check visibility
                    if !check_visibility(visible, &value) {
                        continue;
                    }

                    // Intercept calldata format — produces a Nested entry instead of a flat value
                    if matches!(format.as_ref(), Some(FieldFormat::Calldata)) {
                        let entry =
                            render_calldata_field(ctx, &value, params.as_ref(), label).await?;
                        entries.push(entry);
                        continue;
                    }

                    let formatted = format_value(
                        ctx,
                        &value,
                        format.as_ref(),
                        params.as_ref(),
                        path,
                        label,
                        warnings,
                    )
                    .await?;

                    entries.push(DisplayEntry::Item(DisplayItem {
                        label: label.clone(),
                        value: formatted,
                    }));
                }
            }
        }

        Ok(entries)
    })
}

/// Render a field group recursively.
async fn render_field_group<'a>(
    ctx: &'a RenderContext<'a>,
    group: &FieldGroup,
    warnings: &'a mut Vec<String>,
) -> Result<Option<DisplayEntry>, Error> {
    let mut items = Vec::new();

    let sub_entries = render_fields(ctx, &group.fields, warnings).await?;
    for entry in sub_entries {
        match entry {
            DisplayEntry::Item(item) => items.push(item),
            DisplayEntry::Group {
                items: sub_items, ..
            } => {
                items.extend(sub_items);
            }
            DisplayEntry::Nested { intent, .. } => {
                items.push(DisplayItem {
                    label: "Nested call".to_string(),
                    value: intent,
                });
            }
        }
    }

    if items.is_empty() {
        return Ok(None);
    }

    let iteration = match group.iteration {
        Iteration::Sequential => GroupIteration::Sequential,
        Iteration::Bundled => GroupIteration::Bundled,
    };

    Ok(Some(DisplayEntry::Group {
        label: group.label.clone(),
        iteration,
        items,
    }))
}

/// Resolve a `$ref` to a definition.
fn resolve_reference(descriptor: &Descriptor, reference: &str) -> Option<DisplayField> {
    // Expected format: "#/definitions/foo"
    let key = reference.strip_prefix("#/definitions/")?;
    descriptor.display.definitions.get(key).cloned()
}

/// Resolve a path like `@.to` or `@.args[0]` to a decoded value.
///
/// When the path starts with `@.`, container values (appended last by
/// `inject_container_values`) take priority over function params with the
/// same name.  Without the prefix, function params are matched first.
fn resolve_path(decoded: &DecodedArguments, path: &str) -> Option<ArgumentValue> {
    let path = path.trim();

    // Detect `@.` prefix — means "prefer container value" for named lookup
    let (prefer_container, path) = if let Some(stripped) = path.strip_prefix("@.") {
        (true, stripped)
    } else {
        (false, path)
    };

    // Try numeric index first (positional: "0", "1", etc.)
    if let Ok(index) = path.parse::<usize>() {
        return decoded.args.get(index).map(|a| a.value.clone());
    }

    // Try named parameter matching by splitting dotted paths
    let segments: Vec<&str> = path.split('.').collect();

    // First segment indexes into top-level args
    if let Ok(index) = segments[0].parse::<usize>() {
        if let Some(arg) = decoded.args.get(index) {
            if segments.len() == 1 {
                return Some(arg.value.clone());
            }
            return navigate_value(&arg.value, &segments[1..]);
        }
    }

    // Handle array index notation: "args[0]"
    if let Some(rest) = segments[0].strip_prefix("args") {
        if let Some(idx_str) = rest.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            if let Ok(index) = idx_str.parse::<usize>() {
                if let Some(arg) = decoded.args.get(index) {
                    if segments.len() == 1 {
                        return Some(arg.value.clone());
                    }
                    return navigate_value(&arg.value, &segments[1..]);
                }
            }
        }
    }

    // Try named parameter matching
    // When `@.` prefix was present, search from the end (container values are appended last)
    let name = segments[0];
    let arg = if prefer_container {
        decoded
            .args
            .iter()
            .rfind(|a| a.name.as_deref() == Some(name))
    } else {
        decoded
            .args
            .iter()
            .find(|a| a.name.as_deref() == Some(name))
    };

    if let Some(arg) = arg {
        if segments.len() == 1 {
            return Some(arg.value.clone());
        }
        return navigate_value(&arg.value, &segments[1..]);
    }

    None
}

/// Navigate into a value using path segments.
fn navigate_value(value: &ArgumentValue, segments: &[&str]) -> Option<ArgumentValue> {
    if segments.is_empty() {
        return Some(value.clone());
    }

    match value {
        ArgumentValue::Tuple(members) | ArgumentValue::Array(members) => {
            let seg = segments[0];
            if let Ok(index) = seg.parse::<usize>() {
                members
                    .get(index)
                    .and_then(|v| navigate_value(v, &segments[1..]))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Check if a field should be visible based on the visibility rule and decoded value.
fn check_visibility(rule: &VisibleRule, value: &Option<ArgumentValue>) -> bool {
    match rule {
        VisibleRule::Always => true,
        VisibleRule::Bool(b) => *b,
        VisibleRule::Named(s) => s != "never",
        VisibleRule::Condition(cond) => {
            if let Some(val) = value {
                let json_val = val.to_json_value();
                cond.evaluate(&json_val)
            } else {
                true // Show if value is unresolvable
            }
        }
    }
}

/// Format a decoded value according to its format type.
async fn format_value(
    ctx: &RenderContext<'_>,
    value: &Option<ArgumentValue>,
    format: Option<&FieldFormat>,
    params: Option<&FormatParams>,
    path: &str,
    label: &str,
    warnings: &mut Vec<String>,
) -> Result<String, Error> {
    let Some(val) = value else {
        warnings.push(format!(
            "could not resolve path: {} for field '{}'",
            path, label
        ));
        return Ok("<unresolved>".to_string());
    };

    // Check for encryption — if present and we can't decrypt, use fallback
    if let Some(params) = params {
        if let Some(ref enc) = params.encryption {
            if let Some(ref fallback) = enc.fallback_label {
                return Ok(fallback.clone());
            }
        }
    }

    // Check for map reference
    if let Some(params) = params {
        if let Some(ref map_ref) = params.map_reference {
            if let Some(mapped) = resolve_map(ctx, map_ref, val) {
                return Ok(mapped);
            }
        }
    }

    let Some(fmt) = format else {
        return Ok(format_raw(val));
    };

    match fmt {
        FieldFormat::TokenAmount => {
            format_token_amount(ctx, val, params, label, path, warnings).await
        }
        FieldFormat::Amount => format_amount(ctx, val, path),
        FieldFormat::Date => format_date(val),
        FieldFormat::Enum => format_enum(ctx, val, params),
        FieldFormat::Address => Ok(format_address(val)),
        FieldFormat::AddressName => format_address_name(ctx, val, params).await,
        FieldFormat::Number => Ok(format_number(val)),
        FieldFormat::Raw => Ok(format_raw(val)),
        FieldFormat::TokenTicker => format_token_ticker(ctx, val, params, warnings).await,
        FieldFormat::ChainId => format_chain_id(val),
        FieldFormat::Duration => Ok(format_duration(val)),
        FieldFormat::Unit => Ok(format_unit(val, params)),
        FieldFormat::Calldata => {
            // Should not reach here — calldata format is intercepted in render_fields
            warnings.push(format!(
                "calldata format should be handled by render_calldata_field for field '{}' (path: {})",
                label, path
            ));
            Ok(format_raw(val))
        }
        FieldFormat::NftName => format_nft_name(ctx, val, params, label, path, warnings).await,
    }
}

/// Render a nested calldata field by decoding the inner call and recursively formatting it.
async fn render_calldata_field(
    ctx: &RenderContext<'_>,
    val: &Option<ArgumentValue>,
    params: Option<&FormatParams>,
    label: &str,
) -> Result<DisplayEntry, Error> {
    // Extract bytes from value
    let inner_calldata = match val {
        Some(ArgumentValue::Bytes(bytes)) => bytes,
        _ => {
            let raw = val
                .as_ref()
                .map(format_raw)
                .unwrap_or_else(|| "<unresolved>".to_string());
            return Ok(DisplayEntry::Nested {
                label: label.to_string(),
                intent: "Unknown".to_string(),
                entries: vec![DisplayEntry::Item(DisplayItem {
                    label: "Raw data".to_string(),
                    value: raw,
                })],
                warnings: vec!["calldata field is not bytes".to_string()],
            });
        }
    };

    // Check depth limit
    if ctx.depth >= MAX_CALLDATA_DEPTH {
        return Ok(DisplayEntry::Nested {
            label: label.to_string(),
            intent: "Unknown".to_string(),
            entries: vec![DisplayEntry::Item(DisplayItem {
                label: "Raw data".to_string(),
                value: format!("0x{}", hex::encode(inner_calldata)),
            })],
            warnings: vec![format!(
                "nested calldata depth limit ({}) reached",
                MAX_CALLDATA_DEPTH
            )],
        });
    }

    if inner_calldata.len() < 4 {
        return Ok(DisplayEntry::Nested {
            label: label.to_string(),
            intent: "Unknown".to_string(),
            entries: vec![DisplayEntry::Item(DisplayItem {
                label: "Raw data".to_string(),
                value: format!("0x{}", hex::encode(inner_calldata)),
            })],
            warnings: vec!["inner calldata too short".to_string()],
        });
    }

    // Resolve callee address from params
    let callee_addr = params
        .and_then(|p| p.callee_path.as_ref())
        .and_then(|path| resolve_path(ctx.decoded, path))
        .and_then(|v| match v {
            ArgumentValue::Address(addr) => Some(format!("0x{}", hex::encode(addr))),
            _ => None,
        });

    let callee = match callee_addr {
        Some(addr) => addr,
        None => {
            // No callee — return raw preview
            return Ok(build_raw_nested(label, inner_calldata));
        }
    };

    // Resolve amount (for @.value injection)
    let amount_bytes: Option<Vec<u8>> = params
        .and_then(|p| p.amount_path.as_ref())
        .and_then(|path| resolve_path(ctx.decoded, path))
        .and_then(|v| match v {
            ArgumentValue::Uint(bytes) | ArgumentValue::Int(bytes) => Some(bytes),
            _ => None,
        });

    // Resolve spender/from (for @.from injection)
    let spender_addr: Option<String> = params
        .and_then(|p| p.spender_path.as_ref())
        .and_then(|path| resolve_path(ctx.decoded, path))
        .and_then(|v| match v {
            ArgumentValue::Address(addr) => Some(format!("0x{}", hex::encode(addr))),
            _ => None,
        });

    // Find matching inner descriptor by chain_id + callee address
    let inner_descriptor = ctx.descriptors.iter().find(|rd| {
        rd.descriptor.context.deployments().iter().any(|dep| {
            dep.chain_id == ctx.chain_id && dep.address.to_lowercase() == callee.to_lowercase()
        })
    });

    let inner_descriptor = match inner_descriptor {
        Some(rd) => &rd.descriptor,
        None => {
            return Ok(build_raw_nested(label, inner_calldata));
        }
    };

    // Find matching signature
    let (sig, _format_key) =
        match crate::find_matching_signature(inner_descriptor, &inner_calldata[..4]) {
            Ok(result) => result,
            Err(_) => {
                return Ok(build_raw_nested(label, inner_calldata));
            }
        };

    // Decode inner calldata
    let mut decoded = match crate::decoder::decode_calldata(&sig, inner_calldata) {
        Ok(d) => d,
        Err(_) => {
            return Ok(build_raw_nested(label, inner_calldata));
        }
    };

    // Inject container values into inner context
    crate::inject_container_values(
        &mut decoded,
        ctx.chain_id,
        &callee,
        amount_bytes.as_deref(),
        spender_addr.as_deref(),
    );

    // Build inner render context
    let inner_format =
        match find_format(inner_descriptor, &decoded.function_name, &decoded.selector) {
            Ok(f) => f,
            Err(_) => {
                return Ok(build_raw_nested(label, inner_calldata));
            }
        };

    let inner_ctx = RenderContext {
        descriptor: inner_descriptor,
        decoded: &decoded,
        chain_id: ctx.chain_id,
        data_provider: ctx.data_provider,
        descriptors: ctx.descriptors,
        depth: ctx.depth + 1,
    };

    let mut inner_warnings = Vec::new();
    let inner_entries =
        render_fields(&inner_ctx, &inner_format.fields, &mut inner_warnings).await?;

    let intent = inner_format
        .intent
        .clone()
        .unwrap_or_else(|| decoded.function_name.clone());

    Ok(DisplayEntry::Nested {
        label: label.to_string(),
        intent,
        entries: inner_entries,
        warnings: inner_warnings,
    })
}

/// Build a raw-preview Nested entry for inner calldata when no descriptor matches.
fn build_raw_nested(label: &str, calldata: &[u8]) -> DisplayEntry {
    let selector = if calldata.len() >= 4 {
        format!("0x{}", hex::encode(&calldata[..4]))
    } else {
        format!("0x{}", hex::encode(calldata))
    };

    let data = if calldata.len() > 4 {
        &calldata[4..]
    } else {
        &[]
    };

    let mut entries = Vec::new();
    for (i, chunk) in data.chunks(32).enumerate() {
        entries.push(DisplayEntry::Item(DisplayItem {
            label: format!("Param {}", i),
            value: format!("0x{}", hex::encode(chunk)),
        }));
    }

    DisplayEntry::Nested {
        label: label.to_string(),
        intent: format!("Unknown function {}", selector),
        entries,
        warnings: vec!["No matching descriptor for inner call".to_string()],
    }
}

fn format_raw(val: &ArgumentValue) -> String {
    match val {
        ArgumentValue::Address(addr) => format!("0x{}", hex::encode(addr)),
        ArgumentValue::Uint(bytes) | ArgumentValue::Int(bytes) => {
            let n = BigUint::from_bytes_be(bytes);
            n.to_string()
        }
        ArgumentValue::Bool(b) => b.to_string(),
        ArgumentValue::Bytes(b) | ArgumentValue::FixedBytes(b) => {
            format!("0x{}", hex::encode(b))
        }
        ArgumentValue::String(s) => s.clone(),
        ArgumentValue::Array(items) => {
            let rendered: Vec<String> = items.iter().map(format_raw).collect();
            format!("[{}]", rendered.join(", "))
        }
        ArgumentValue::Tuple(items) => {
            let rendered: Vec<String> = items.iter().map(format_raw).collect();
            format!("({})", rendered.join(", "))
        }
    }
}

fn format_address(val: &ArgumentValue) -> String {
    match val {
        ArgumentValue::Address(addr) => eip55_checksum(addr),
        _ => format_raw(val),
    }
}

/// Format an address as a trusted name (spec: addressName).
///
/// 1. Check senderAddress match → "Sender"
/// 2. Try local name via provider
/// 3. Try ENS name via provider
/// 4. Fallback → EIP-55 checksum
async fn format_address_name(
    ctx: &RenderContext<'_>,
    val: &ArgumentValue,
    params: Option<&FormatParams>,
) -> Result<String, Error> {
    let ArgumentValue::Address(addr) = val else {
        return Ok(format_raw(val));
    };

    let hex_addr = format!("0x{}", hex::encode(addr));

    // 1. Check senderAddress
    if let Some(params) = params {
        if let Some(ref sender) = params.sender_address {
            let sender_addrs = match sender {
                SenderAddress::Single(s) => vec![s.as_str()],
                SenderAddress::Multiple(v) => v.iter().map(|s| s.as_str()).collect(),
            };
            for sender_ref in &sender_addrs {
                // Resolve path references like "@.from"
                let resolved_addr = if sender_ref.starts_with("@.") || sender_ref.starts_with('#') {
                    resolve_path(ctx.decoded, sender_ref).and_then(|v| match v {
                        ArgumentValue::Address(a) => Some(format!("0x{}", hex::encode(a))),
                        _ => None,
                    })
                } else {
                    Some(sender_ref.to_string())
                };
                if let Some(resolved) = resolved_addr {
                    if resolved.to_lowercase() == hex_addr.to_lowercase() {
                        return Ok("Sender".to_string());
                    }
                }
            }
        }
    }

    // 2. Determine allowed sources (default: both)
    let sources = params.and_then(|p| p.sources.as_ref());
    let local_allowed = sources
        .map(|s| s.iter().any(|src| src == "local"))
        .unwrap_or(true);
    let ens_allowed = sources
        .map(|s| s.iter().any(|src| src == "ens"))
        .unwrap_or(true);

    // 3. Try local name
    if local_allowed {
        if let Some(name) = ctx
            .data_provider
            .resolve_local_name(&hex_addr, ctx.chain_id)
            .await
        {
            return Ok(name);
        }
    }

    // 4. Try ENS name
    if ens_allowed {
        if let Some(name) = ctx
            .data_provider
            .resolve_ens_name(&hex_addr, ctx.chain_id)
            .await
        {
            return Ok(name);
        }
    }

    // 5. Fallback: EIP-55 checksum
    Ok(eip55_checksum(addr))
}

/// EIP-55 mixed-case checksum encoding.
fn eip55_checksum(addr: &[u8; 20]) -> String {
    use tiny_keccak::{Hasher, Keccak};

    let hex_addr = hex::encode(addr);
    let mut hasher = Keccak::v256();
    hasher.update(hex_addr.as_bytes());
    let mut hash = [0u8; 32];
    hasher.finalize(&mut hash);

    let mut result = String::with_capacity(42);
    result.push_str("0x");
    for (i, c) in hex_addr.chars().enumerate() {
        let hash_nibble = if i % 2 == 0 {
            (hash[i / 2] >> 4) & 0x0f
        } else {
            hash[i / 2] & 0x0f
        };
        if hash_nibble >= 8 {
            result.push(c.to_ascii_uppercase());
        } else {
            result.push(c);
        }
    }
    result
}

fn format_number(val: &ArgumentValue) -> String {
    match val {
        ArgumentValue::Uint(bytes) | ArgumentValue::Int(bytes) => {
            BigUint::from_bytes_be(bytes).to_string()
        }
        _ => format_raw(val),
    }
}

async fn format_token_amount(
    ctx: &RenderContext<'_>,
    val: &ArgumentValue,
    params: Option<&FormatParams>,
    label: &str,
    path: &str,
    warnings: &mut Vec<String>,
) -> Result<String, Error> {
    let raw_amount = match val {
        ArgumentValue::Uint(bytes) | ArgumentValue::Int(bytes) => BigUint::from_bytes_be(bytes),
        _ => return Ok(format_raw(val)),
    };

    // Determine chain ID for token lookup (cross-chain support)
    let lookup_chain_id = resolve_chain_id(ctx, params);

    // Try to resolve token metadata
    let token_meta = if let Some(params) = params {
        if let Some(ref token_path) = params.token_path {
            // Resolve token address from calldata
            let token_addr = resolve_path(ctx.decoded, token_path);
            if let Some(ArgumentValue::Address(addr)) = token_addr {
                let addr_hex = format!("0x{}", hex::encode(addr));

                // Check for native currency
                if let Some(ref native) = params.native_currency_address {
                    if addr_hex.to_lowercase() == native.to_lowercase() {
                        Some(native_token_meta(lookup_chain_id))
                    } else {
                        ctx.data_provider
                            .resolve_token(lookup_chain_id, &addr_hex)
                            .await
                    }
                } else {
                    ctx.data_provider
                        .resolve_token(lookup_chain_id, &addr_hex)
                        .await
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // Check threshold/message for max-amount display
    if let Some(params) = params {
        if let (Some(ref threshold_ref), Some(ref message)) = (&params.threshold, &params.message) {
            if let Some(threshold) = resolve_metadata_constant(ctx.descriptor, threshold_ref) {
                if raw_amount >= threshold {
                    if let Some(ref meta) = token_meta {
                        return Ok(format!("{} {}", message, meta.symbol));
                    }
                    return Ok(message.clone());
                }
            }
        }
    }

    if let Some(meta) = token_meta {
        let formatted = format_with_decimals(&raw_amount, meta.decimals);
        Ok(format!("{} {}", formatted, meta.symbol))
    } else {
        warnings.push(format!(
            "token metadata not found for field '{}' (path: {})",
            label, path
        ));
        Ok(raw_amount.to_string())
    }
}

/// Resolve a `$.metadata.constants.xxx` or literal hex reference to a BigUint.
fn resolve_metadata_constant(descriptor: &Descriptor, ref_path: &str) -> Option<BigUint> {
    if let Some(const_name) = ref_path.strip_prefix("$.metadata.constants.") {
        let val = descriptor.metadata.constants.get(const_name)?;
        parse_constant_to_biguint(val)
    } else {
        // Try parsing as literal hex
        let hex_str = ref_path.strip_prefix("0x").unwrap_or(ref_path);
        BigUint::parse_bytes(hex_str.as_bytes(), 16)
    }
}

fn parse_constant_to_biguint(val: &serde_json::Value) -> Option<BigUint> {
    match val {
        serde_json::Value::String(s) => {
            let hex_str = s
                .strip_prefix("0x")
                .or_else(|| s.strip_prefix("0X"))
                .unwrap_or(s);
            BigUint::parse_bytes(hex_str.as_bytes(), 16)
        }
        serde_json::Value::Number(n) => n.as_u64().map(BigUint::from),
        _ => None,
    }
}

async fn format_token_ticker(
    ctx: &RenderContext<'_>,
    val: &ArgumentValue,
    params: Option<&FormatParams>,
    warnings: &mut Vec<String>,
) -> Result<String, Error> {
    let lookup_chain_id = resolve_chain_id(ctx, params);

    if let ArgumentValue::Address(addr) = val {
        let addr_hex = format!("0x{}", hex::encode(addr));
        if let Some(meta) = ctx
            .data_provider
            .resolve_token(lookup_chain_id, &addr_hex)
            .await
        {
            return Ok(meta.symbol);
        }
    }

    warnings.push("token ticker not found".to_string());
    Ok(format_raw(val))
}

fn format_chain_id(val: &ArgumentValue) -> Result<String, Error> {
    if let ArgumentValue::Uint(bytes) = val {
        let n = BigUint::from_bytes_be(bytes);
        let chain_id: u64 = n.try_into().unwrap_or(0);
        Ok(chain_name(chain_id))
    } else {
        Ok(format_raw(val))
    }
}

/// Resolve the chain ID for cross-chain token lookups.
fn resolve_chain_id(ctx: &RenderContext<'_>, params: Option<&FormatParams>) -> u64 {
    if let Some(params) = params {
        // Static chain ID takes precedence
        if let Some(cid) = params.chain_id {
            return cid;
        }
        // Dynamic chain ID from calldata path
        if let Some(ref path) = params.chain_id_path {
            if let Some(ArgumentValue::Uint(bytes)) = resolve_path(ctx.decoded, path) {
                let n = BigUint::from_bytes_be(&bytes);
                if let Ok(cid) = u64::try_from(n) {
                    return cid;
                }
            }
        }
    }
    ctx.chain_id
}

/// Get native token metadata for a chain.
fn native_token_meta(chain_id: u64) -> crate::token::TokenMeta {
    let (symbol, name) = match chain_id {
        1 | 5 | 11155111 => ("ETH", "Ether"),
        137 | 80001 => ("MATIC", "Polygon"),
        56 | 97 => ("BNB", "BNB"),
        43114 | 43113 => ("AVAX", "Avalanche"),
        250 => ("FTM", "Fantom"),
        42161 | 421613 => ("ETH", "Ether"),
        10 | 420 => ("ETH", "Ether"),
        8453 | 84531 => ("ETH", "Ether"),
        _ => ("ETH", "Ether"),
    };
    crate::token::TokenMeta {
        symbol: symbol.to_string(),
        decimals: 18,
        name: name.to_string(),
    }
}

fn format_amount(
    ctx: &RenderContext<'_>,
    val: &ArgumentValue,
    path: &str,
) -> Result<String, Error> {
    match val {
        ArgumentValue::Uint(bytes) | ArgumentValue::Int(bytes) => {
            let n = BigUint::from_bytes_be(bytes);
            if path.starts_with("@.value") {
                let meta = native_token_meta(ctx.chain_id);
                let formatted = format_with_decimals(&n, meta.decimals);
                Ok(format!("{} {}", formatted, meta.symbol))
            } else {
                Ok(n.to_string())
            }
        }
        _ => Ok(format_raw(val)),
    }
}

fn format_date(val: &ArgumentValue) -> Result<String, Error> {
    match val {
        ArgumentValue::Uint(bytes) => {
            let n = BigUint::from_bytes_be(bytes);
            let timestamp: i64 = i64::try_from(n).unwrap_or(0);

            let dt = time::OffsetDateTime::from_unix_timestamp(timestamp)
                .map_err(|e| Error::Render(format!("invalid timestamp: {e}")))?;

            let format = time::format_description::parse(
                "[year]-[month]-[day] [hour]:[minute]:[second] UTC",
            )
            .map_err(|e| Error::Render(format!("format error: {e}")))?;

            Ok(dt
                .format(&format)
                .map_err(|e| Error::Render(format!("format error: {e}")))?)
        }
        _ => Ok(format_raw(val)),
    }
}

fn format_enum(
    ctx: &RenderContext<'_>,
    val: &ArgumentValue,
    params: Option<&FormatParams>,
) -> Result<String, Error> {
    let raw = format_raw(val);

    if let Some(params) = params {
        // Try direct enumPath first
        if let Some(ref enum_path) = params.enum_path {
            if let Some(enum_def) = ctx.descriptor.metadata.enums.get(enum_path) {
                if let Some(label) = enum_def.get(&raw) {
                    return Ok(label.clone());
                }
            }
        }
        // Try $ref path (v2): "$.metadata.enums.interestRateMode"
        if let Some(ref ref_path) = params.ref_path {
            if let Some(enum_name) = ref_path.strip_prefix("$.metadata.enums.") {
                if let Some(enum_def) = ctx.descriptor.metadata.enums.get(enum_name) {
                    if let Some(label) = enum_def.get(&raw) {
                        return Ok(label.clone());
                    }
                }
            }
        }
    }

    Ok(raw)
}

/// Resolve a map reference to a display value.
fn resolve_map(ctx: &RenderContext<'_>, map_ref: &str, val: &ArgumentValue) -> Option<String> {
    let raw = format_raw(val);
    let map_def = ctx.descriptor.metadata.maps.get(map_ref)?;
    map_def.entries.get(&raw).cloned()
}

/// Format a BigUint with decimal places (public for eip712 module).
pub(crate) fn format_with_decimals(amount: &BigUint, decimals: u8) -> String {
    let s = amount.to_string();
    let decimals = decimals as usize;

    if decimals == 0 {
        return s;
    }

    if s.len() <= decimals {
        let zeros = decimals - s.len();
        let mut result = String::from("0.");
        result.extend(std::iter::repeat_n('0', zeros));
        result.push_str(&s);
        // Trim trailing zeros after decimal point
        let trimmed = result.trim_end_matches('0');
        if trimmed.ends_with('.') {
            return format!("{trimmed}0");
        }
        return trimmed.to_string();
    }

    let (integer_part, decimal_part) = s.split_at(s.len() - decimals);
    let trimmed = decimal_part.trim_end_matches('0');
    if trimmed.is_empty() {
        integer_part.to_string()
    } else {
        format!("{integer_part}.{trimmed}")
    }
}

/// Format an NFT name: "{collection_name} #{token_id}" or "#{token_id}" fallback.
async fn format_nft_name(
    ctx: &RenderContext<'_>,
    val: &ArgumentValue,
    params: Option<&FormatParams>,
    label: &str,
    path: &str,
    warnings: &mut Vec<String>,
) -> Result<String, Error> {
    // Extract token_id from uint value
    let token_id = match val {
        ArgumentValue::Uint(bytes) | ArgumentValue::Int(bytes) => {
            BigUint::from_bytes_be(bytes).to_string()
        }
        _ => return Ok(format_raw(val)),
    };

    // Resolve collection address
    let collection_addr = params.and_then(|p| {
        // Try collectionPath first
        if let Some(ref cpath) = p.collection_path {
            let resolved = resolve_path(ctx.decoded, cpath);
            if let Some(ArgumentValue::Address(addr)) = resolved {
                return Some(format!("0x{}", hex::encode(addr)));
            }
        }
        // Fallback to constant collection address
        p.collection.clone()
    });

    let Some(collection_addr) = collection_addr else {
        warnings.push(format!(
            "no collection address for nftName field '{}' (path: {})",
            label, path
        ));
        return Ok(token_id);
    };

    // Ask the provider for the collection name
    if let Some(name) = ctx
        .data_provider
        .resolve_nft_collection_name(&collection_addr, ctx.chain_id)
        .await
    {
        Ok(format!("{} #{}", name, token_id))
    } else {
        warnings.push(format!(
            "NFT collection not found for '{}' (address: {})",
            label, collection_addr
        ));
        Ok(format!("#{}", token_id))
    }
}

/// Interpolate `${path}` and `{name}` templates in an intent string.
///
/// Supports both v1 `${path}` and v2 `{paramName}` interpolation patterns.
async fn interpolate_intent(
    template: &str,
    ctx: &RenderContext<'_>,
    fields: &[DisplayField],
) -> String {
    let mut result = template.to_string();

    // First pass: replace ${path} patterns (v1)
    while let Some(start) = result.find("${") {
        let end = match result[start..].find('}') {
            Some(e) => start + e,
            None => break,
        };
        let path = result[start + 2..end].to_string();
        let replacement = resolve_and_format_for_interpolation(ctx, fields, &path).await;
        result.replace_range(start..=end, &replacement);
    }

    // Second pass: replace {name} patterns (v2) — only single `{` not preceded by `$`
    let mut pos = 0;
    while pos < result.len() {
        if let Some(rel_start) = result[pos..].find('{') {
            let start = pos + rel_start;
            // Skip if preceded by '$' (already handled)
            if start > 0 && result.as_bytes()[start - 1] == b'$' {
                pos = start + 1;
                continue;
            }
            let end = match result[start..].find('}') {
                Some(e) => start + e,
                None => break,
            };
            let path = result[start + 1..end].to_string();
            let replacement = resolve_and_format_for_interpolation(ctx, fields, &path).await;
            result.replace_range(start..=end, &replacement);
            pos = start + replacement.len();
        } else {
            break;
        }
    }

    result
}

/// Format a duration value (seconds → human-readable).
fn format_duration(val: &ArgumentValue) -> String {
    let secs = match val {
        ArgumentValue::Uint(bytes) | ArgumentValue::Int(bytes) => {
            let n = BigUint::from_bytes_be(bytes);
            u64::try_from(n).unwrap_or(0)
        }
        _ => return format_raw(val),
    };

    if secs == 0 {
        return "0 seconds".to_string();
    }

    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;

    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!(
            "{} {}",
            days,
            if days == 1 { "day" } else { "days" }
        ));
    }
    if hours > 0 {
        parts.push(format!(
            "{} {}",
            hours,
            if hours == 1 { "hour" } else { "hours" }
        ));
    }
    if minutes > 0 {
        parts.push(format!(
            "{} {}",
            minutes,
            if minutes == 1 { "minute" } else { "minutes" }
        ));
    }
    if seconds > 0 {
        parts.push(format!(
            "{} {}",
            seconds,
            if seconds == 1 { "second" } else { "seconds" }
        ));
    }
    parts.join(" ")
}

/// Format a unit value (e.g., percentage, bps) with optional decimals and SI prefix.
fn format_unit(val: &ArgumentValue, params: Option<&FormatParams>) -> String {
    let raw_val = match val {
        ArgumentValue::Uint(bytes) | ArgumentValue::Int(bytes) => BigUint::from_bytes_be(bytes),
        _ => return format_raw(val),
    };

    let base = params.and_then(|p| p.base.as_deref()).unwrap_or("");
    let decimals = params.and_then(|p| p.decimals).unwrap_or(0);

    let formatted = if decimals > 0 {
        format_with_decimals(&raw_val, decimals)
    } else {
        raw_val.to_string()
    };

    if base.is_empty() {
        formatted
    } else {
        format!("{} {}", formatted, base)
    }
}

async fn resolve_and_format_for_interpolation(
    ctx: &RenderContext<'_>,
    fields: &[DisplayField],
    path: &str,
) -> String {
    let Some(v) = resolve_path(ctx.decoded, path) else {
        return "<?>".to_string();
    };

    let (field_format, field_params) = fields
        .iter()
        .find_map(|f| {
            if let DisplayField::Simple {
                path: fp,
                format,
                params,
                ..
            } = f
            {
                if fp == path {
                    Some((format.as_ref(), params.as_ref()))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .unwrap_or((None, None));

    match field_format {
        Some(FieldFormat::Date) => format_date(&v).unwrap_or_else(|_| format_raw(&v)),
        Some(FieldFormat::Number) => format_number(&v),
        Some(FieldFormat::Address) => format_address(&v),
        Some(FieldFormat::TokenAmount) => {
            format_token_amount_for_interpolation(ctx, &v, field_params).await
        }
        Some(FieldFormat::Amount) => {
            if path.starts_with("@.value") {
                let meta = native_token_meta(ctx.chain_id);
                match &v {
                    ArgumentValue::Uint(bytes) | ArgumentValue::Int(bytes) => {
                        let n = BigUint::from_bytes_be(bytes);
                        let formatted = format_with_decimals(&n, meta.decimals);
                        format!("{} {}", formatted, meta.symbol)
                    }
                    _ => format_raw(&v),
                }
            } else {
                format_raw(&v)
            }
        }
        Some(FieldFormat::Enum) => format_enum_for_interpolation(ctx, &v, field_params),
        _ => format_raw(&v),
    }
}

/// Resolve an enum value for interpolation using descriptor metadata.
fn format_enum_for_interpolation(
    ctx: &RenderContext<'_>,
    val: &ArgumentValue,
    params: Option<&FormatParams>,
) -> String {
    let raw = format_raw(val);
    if let Some(params) = params {
        if let Some(ref enum_path) = params.enum_path {
            if let Some(enum_def) = ctx.descriptor.metadata.enums.get(enum_path) {
                if let Some(label) = enum_def.get(&raw) {
                    return label.clone();
                }
            }
        }
        if let Some(ref ref_path) = params.ref_path {
            if let Some(enum_name) = ref_path.strip_prefix("$.metadata.enums.") {
                if let Some(enum_def) = ctx.descriptor.metadata.enums.get(enum_name) {
                    if let Some(label) = enum_def.get(&raw) {
                        return label.clone();
                    }
                }
            }
        }
    }
    raw
}

/// Format a token amount for interpolation (simplified version of format_token_amount).
async fn format_token_amount_for_interpolation(
    ctx: &RenderContext<'_>,
    val: &ArgumentValue,
    params: Option<&FormatParams>,
) -> String {
    let raw_amount = match val {
        ArgumentValue::Uint(bytes) | ArgumentValue::Int(bytes) => BigUint::from_bytes_be(bytes),
        _ => return format_raw(val),
    };

    let lookup_chain_id = resolve_chain_id(ctx, params);

    let token_meta = if let Some(p) = params {
        if let Some(ref token_path) = p.token_path {
            let token_addr = resolve_path(ctx.decoded, token_path);
            if let Some(ArgumentValue::Address(addr)) = token_addr {
                let addr_hex = format!("0x{}", hex::encode(addr));
                if let Some(ref native) = p.native_currency_address {
                    if addr_hex.to_lowercase() == native.to_lowercase() {
                        Some(native_token_meta(lookup_chain_id))
                    } else {
                        ctx.data_provider
                            .resolve_token(lookup_chain_id, &addr_hex)
                            .await
                    }
                } else {
                    ctx.data_provider
                        .resolve_token(lookup_chain_id, &addr_hex)
                        .await
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // Check threshold/message
    if let Some(p) = params {
        if let (Some(ref threshold_ref), Some(ref message)) = (&p.threshold, &p.message) {
            if let Some(threshold) = resolve_metadata_constant(ctx.descriptor, threshold_ref) {
                if raw_amount >= threshold {
                    if let Some(ref meta) = token_meta {
                        return format!("{} {}", message, meta.symbol);
                    }
                    return message.clone();
                }
            }
        }
    }

    if let Some(meta) = token_meta {
        let formatted = format_with_decimals(&raw_amount, meta.decimals);
        format!("{} {}", formatted, meta.symbol)
    } else {
        raw_amount.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_with_decimals() {
        let amount = BigUint::from(1_000_000u64);
        assert_eq!(format_with_decimals(&amount, 6), "1");

        let amount = BigUint::from(1_500_000u64);
        assert_eq!(format_with_decimals(&amount, 6), "1.5");

        let amount = BigUint::from(500_000u64);
        assert_eq!(format_with_decimals(&amount, 6), "0.5");

        let amount = BigUint::from(123u64);
        assert_eq!(format_with_decimals(&amount, 6), "0.000123");

        let amount = BigUint::from(0u64);
        assert_eq!(format_with_decimals(&amount, 18), "0.0");
    }

    #[test]
    fn test_chain_name() {
        assert_eq!(chain_name(1), "Ethereum");
        assert_eq!(chain_name(137), "Polygon");
        assert_eq!(chain_name(99999), "Chain 99999");
    }

    #[test]
    fn test_eip55_checksum() {
        // Known checksum: 0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed
        let addr_bytes = hex::decode("5aaeb6053f3e94c9b9a09f33669435e7ef1beaed").unwrap();
        let mut addr = [0u8; 20];
        addr.copy_from_slice(&addr_bytes);
        let checksummed = eip55_checksum(&addr);
        assert_eq!(checksummed, "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed");
    }

    #[tokio::test]
    async fn test_interpolate_intent() {
        use crate::decoder::{DecodedArgument, ParamType};
        use crate::provider::EmptyDataProvider;

        let decoded = DecodedArguments {
            function_name: "transfer".to_string(),
            selector: [0; 4],
            args: vec![
                DecodedArgument {
                    index: 0,
                    name: None,
                    param_type: ParamType::Address,
                    value: ArgumentValue::Address([0u8; 20]),
                },
                DecodedArgument {
                    index: 1,
                    name: None,
                    param_type: ParamType::Uint(256),
                    value: ArgumentValue::Uint(vec![
                        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                        0, 0, 0, 0, 0, 0x03, 0xe8,
                    ]),
                },
            ],
        };

        let descriptor: Descriptor = serde_json::from_str(
            r#"{"context":{"contract":{"deployments":[]}},"metadata":{"owner":"test","enums":{},"constants":{},"maps":{}},"display":{"definitions":{},"formats":{}}}"#
        ).unwrap();
        let data_provider = EmptyDataProvider;
        let ctx = RenderContext {
            descriptor: &descriptor,
            decoded: &decoded,
            chain_id: 1,
            data_provider: &data_provider,
            descriptors: &[],
            depth: 0,
        };

        let result = interpolate_intent("Send ${1} to ${0}", &ctx, &[]).await;
        assert_eq!(
            result,
            "Send 1000 to 0x0000000000000000000000000000000000000000"
        );
    }
}
