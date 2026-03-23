//! EIP-712 typed data formatting — parses structured typed data and produces
//! a [`DisplayModel`](crate::engine::DisplayModel) using the same descriptor format as calldata.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use crate::engine::{DisplayEntry, DisplayItem, DisplayModel, GroupIteration};
use crate::error::Error;
use crate::provider::DataProvider;
use crate::resolver::ResolvedDescriptor;
use crate::types::descriptor::Descriptor;
use crate::types::display::{
    DisplayField, FieldFormat, FieldGroup, FormatParams, Iteration, VisibleRule,
};

/// Maximum recursion depth for nested calldata in EIP-712 context.
const MAX_CALLDATA_DEPTH: u8 = 3;

/// EIP-712 typed data as received for signing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypedData {
    pub types: HashMap<String, Vec<TypedDataField>>,

    #[serde(rename = "primaryType")]
    pub primary_type: String,

    pub domain: TypedDataDomain,

    pub message: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypedDataField {
    pub name: String,

    #[serde(rename = "type")]
    pub field_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypedDataDomain {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    #[serde(rename = "chainId")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<u64>,

    #[serde(rename = "verifyingContract")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verifying_contract: Option<String>,
}

/// Format EIP-712 typed data into a display model.
///
/// `descriptors` provides pre-resolved inner descriptors for nested calldata support.
pub async fn format_typed_data(
    descriptor: &Descriptor,
    data: &TypedData,
    data_provider: &dyn DataProvider,
    descriptors: &[ResolvedDescriptor],
) -> Result<DisplayModel, Error> {
    let chain_id = data.domain.chain_id.unwrap_or(1);

    // Find format by primary type name (exact match first, then signature prefix match)
    let format = descriptor
        .display
        .formats
        .get(&data.primary_type)
        .or_else(|| {
            // Try matching by type name prefix: "Order(address owner,...)" matches primaryType "Order"
            let prefix = format!("{}(", data.primary_type);
            descriptor
                .display
                .formats
                .iter()
                .find(|(key, _)| key.starts_with(&prefix))
                .map(|(_, v)| v)
        });

    // Graceful fallback: if no format matches, show raw message fields
    let Some(format) = format else {
        return Ok(build_typed_raw_fallback(data));
    };

    let mut warnings = Vec::new();
    let entries = render_typed_fields(
        descriptor,
        &data.message,
        &format.fields,
        chain_id,
        data_provider,
        &mut warnings,
        descriptors,
        0,
    )
    .await?;

    Ok(DisplayModel {
        intent: format
            .intent
            .as_ref()
            .map(crate::types::display::intent_as_string)
            .unwrap_or_else(|| data.primary_type.clone()),
        interpolated_intent: format
            .interpolated_intent
            .as_ref()
            .map(|template| interpolate_typed_intent(template, &data.message, &format.fields)),
        entries,
        warnings,
        owner: descriptor.metadata.owner.clone(),
    })
}

/// Render typed data fields recursively.
///
/// Uses `Pin<Box<dyn Future>>` to support recursive calls.
#[allow(clippy::too_many_arguments)]
fn render_typed_fields<'a>(
    descriptor: &'a Descriptor,
    message: &'a serde_json::Value,
    fields: &[DisplayField],
    chain_id: u64,
    data_provider: &'a dyn DataProvider,
    warnings: &'a mut Vec<String>,
    descriptors: &'a [ResolvedDescriptor],
    depth: u8,
) -> Pin<Box<dyn Future<Output = Result<Vec<DisplayEntry>, Error>> + Send + 'a>> {
    let fields = fields.to_vec();
    Box::pin(async move {
        let mut entries = Vec::new();

        for field in &fields {
            match field {
                DisplayField::Reference {
                    reference,
                    path,
                    params: ref_params,
                    visible,
                } => {
                    let key = reference
                        .strip_prefix("$.display.definitions.")
                        .or_else(|| reference.strip_prefix("#/definitions/"))
                        .unwrap_or(reference);
                    if let Some(resolved) = descriptor.display.definitions.get(key) {
                        let merged = crate::engine::merge_ref_with_definition(
                            resolved.clone(),
                            path,
                            ref_params,
                            visible,
                        );
                        let merged_slice = vec![merged];
                        let mut sub = render_typed_fields(
                            descriptor,
                            message,
                            &merged_slice,
                            chain_id,
                            data_provider,
                            warnings,
                            descriptors,
                            depth,
                        )
                        .await?;
                        entries.append(&mut sub);
                    } else {
                        warnings.push(format!("unresolved reference: {reference}"));
                    }
                }
                DisplayField::Group { field_group } => {
                    if let Some(entry) = render_typed_field_group(
                        descriptor,
                        message,
                        field_group,
                        chain_id,
                        data_provider,
                        warnings,
                        descriptors,
                        depth,
                    )
                    .await?
                    {
                        entries.push(entry);
                    }
                }
                DisplayField::Simple {
                    path,
                    label,
                    value: literal_value,
                    format,
                    params,
                    separator: _,
                    visible,
                } => {
                    // If literal value is provided (no path), use it directly
                    if let Some(lit) = literal_value {
                        entries.push(DisplayEntry::Item(DisplayItem {
                            label: label.clone(),
                            value: lit.clone(),
                        }));
                        continue;
                    }

                    let path_str = path.as_deref().unwrap_or("");
                    let value = resolve_typed_path(message, path_str);

                    // Check visibility
                    if !check_typed_visibility(visible, &value) {
                        continue;
                    }

                    // Intercept calldata format
                    if matches!(format.as_ref(), Some(FieldFormat::Calldata)) {
                        let entry = render_typed_calldata_field(
                            descriptor,
                            message,
                            &value,
                            params.as_ref(),
                            label,
                            chain_id,
                            data_provider,
                            descriptors,
                            depth,
                        )
                        .await?;
                        entries.push(entry);
                        continue;
                    }

                    let formatted = format_typed_value(
                        descriptor,
                        &value,
                        format.as_ref(),
                        params.as_ref(),
                        chain_id,
                        message,
                        data_provider,
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

#[allow(clippy::too_many_arguments)]
async fn render_typed_field_group<'a>(
    descriptor: &'a Descriptor,
    message: &'a serde_json::Value,
    group: &FieldGroup,
    chain_id: u64,
    data_provider: &'a dyn DataProvider,
    warnings: &'a mut Vec<String>,
    descriptors: &'a [ResolvedDescriptor],
    depth: u8,
) -> Result<Option<DisplayEntry>, Error> {
    let sub = render_typed_fields(
        descriptor,
        message,
        &group.fields,
        chain_id,
        data_provider,
        warnings,
        descriptors,
        depth,
    )
    .await?;

    let items: Vec<DisplayItem> = sub
        .into_iter()
        .flat_map(|e| match e {
            DisplayEntry::Item(i) => vec![i],
            DisplayEntry::Group { items, .. } => items,
            DisplayEntry::Nested { intent, .. } => {
                vec![DisplayItem {
                    label: "Nested call".to_string(),
                    value: intent,
                }]
            }
        })
        .collect();

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

/// Render a nested calldata field within EIP-712 typed data.
///
/// The `#.` path prefix resolves from message fields (EIP-712 specific).
#[allow(clippy::too_many_arguments)]
async fn render_typed_calldata_field(
    _descriptor: &Descriptor,
    message: &serde_json::Value,
    val: &Option<serde_json::Value>,
    params: Option<&FormatParams>,
    label: &str,
    chain_id: u64,
    data_provider: &dyn DataProvider,
    descriptors: &[ResolvedDescriptor],
    depth: u8,
) -> Result<DisplayEntry, Error> {
    // Extract hex bytes from JSON value
    let inner_calldata = match val {
        Some(serde_json::Value::String(s)) => {
            let hex_str = s
                .strip_prefix("0x")
                .or_else(|| s.strip_prefix("0X"))
                .unwrap_or(s);
            match hex::decode(hex_str) {
                Ok(bytes) => bytes,
                Err(_) => {
                    return Ok(DisplayEntry::Nested {
                        label: label.to_string(),
                        intent: "Unknown".to_string(),
                        entries: vec![DisplayEntry::Item(DisplayItem {
                            label: "Raw data".to_string(),
                            value: s.clone(),
                        })],
                        warnings: vec!["could not decode calldata hex".to_string()],
                    });
                }
            }
        }
        _ => {
            let raw = val
                .as_ref()
                .map(json_value_to_string)
                .unwrap_or_else(|| "<unresolved>".to_string());
            return Ok(DisplayEntry::Nested {
                label: label.to_string(),
                intent: "Unknown".to_string(),
                entries: vec![DisplayEntry::Item(DisplayItem {
                    label: "Raw data".to_string(),
                    value: raw,
                })],
                warnings: vec!["calldata field is not a hex string".to_string()],
            });
        }
    };

    // Check depth limit
    if depth >= MAX_CALLDATA_DEPTH {
        return Ok(DisplayEntry::Nested {
            label: label.to_string(),
            intent: "Unknown".to_string(),
            entries: vec![DisplayEntry::Item(DisplayItem {
                label: "Raw data".to_string(),
                value: format!("0x{}", hex::encode(&inner_calldata)),
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
                value: format!("0x{}", hex::encode(&inner_calldata)),
            })],
            warnings: vec!["inner calldata too short".to_string()],
        });
    }

    // Resolve callee address — supports `#.` prefix for message field reference
    let callee_addr: Option<String> =
        params
            .and_then(|p| p.callee_path.as_ref())
            .and_then(|path| {
                let resolved = if let Some(rest) = path.strip_prefix("#.") {
                    resolve_typed_path(message, rest)
                } else {
                    resolve_typed_path(message, path)
                };
                resolved.and_then(|v| match v {
                    serde_json::Value::String(s) => Some(s),
                    _ => None,
                })
            });

    let callee = match callee_addr {
        Some(addr) => addr,
        None => {
            return Ok(crate::engine::build_raw_nested(label, &inner_calldata));
        }
    };

    // Resolve amount (for @.value injection)
    let amount_bytes: Option<Vec<u8>> =
        params
            .and_then(|p| p.amount_path.as_ref())
            .and_then(|path| {
                let resolved = if let Some(rest) = path.strip_prefix("#.") {
                    resolve_typed_path(message, rest)
                } else {
                    resolve_typed_path(message, path)
                };
                resolved.and_then(|v| {
                    let s = json_value_to_string(&v);
                    let n: num_bigint::BigUint = s.parse().ok()?;
                    let bytes = n.to_bytes_be();
                    let mut padded = vec![0u8; 32usize.saturating_sub(bytes.len())];
                    padded.extend_from_slice(&bytes);
                    Some(padded)
                })
            });

    // Resolve spender/from
    let spender_addr: Option<String> =
        params
            .and_then(|p| p.spender_path.as_ref())
            .and_then(|path| {
                let resolved = if let Some(rest) = path.strip_prefix("#.") {
                    resolve_typed_path(message, rest)
                } else {
                    resolve_typed_path(message, path)
                };
                resolved.and_then(|v| match v {
                    serde_json::Value::String(s) => Some(s),
                    _ => None,
                })
            });

    // Find matching inner descriptor
    let inner_descriptor = descriptors.iter().find(|rd| {
        rd.descriptor.context.deployments().iter().any(|dep| {
            dep.chain_id == chain_id && dep.address.to_lowercase() == callee.to_lowercase()
        })
    });

    let inner_descriptor = match inner_descriptor {
        Some(rd) => &rd.descriptor,
        None => {
            return Ok(crate::engine::build_raw_nested(label, &inner_calldata));
        }
    };

    // Find matching signature + decode
    let (sig, _) = match crate::find_matching_signature(inner_descriptor, &inner_calldata[..4]) {
        Ok(result) => result,
        Err(_) => {
            return Ok(crate::engine::build_raw_nested(label, &inner_calldata));
        }
    };

    let mut decoded = match crate::decoder::decode_calldata(&sig, &inner_calldata) {
        Ok(d) => d,
        Err(_) => {
            return Ok(crate::engine::build_raw_nested(label, &inner_calldata));
        }
    };

    crate::inject_container_values(
        &mut decoded,
        chain_id,
        &callee,
        amount_bytes.as_deref(),
        spender_addr.as_deref(),
    );

    // Use engine's format pipeline for the inner call
    let result = crate::engine::format_calldata(
        inner_descriptor,
        chain_id,
        &callee,
        &decoded,
        amount_bytes.as_deref(),
        data_provider,
        descriptors,
    )
    .await?;

    Ok(DisplayEntry::Nested {
        label: label.to_string(),
        intent: result.intent,
        entries: result.entries,
        warnings: result.warnings,
    })
}

/// Build a raw fallback DisplayModel for EIP-712 typed data when no format matches.
pub(crate) fn build_typed_raw_fallback(data: &TypedData) -> DisplayModel {
    let mut entries = Vec::new();

    // Use the primary type's field definitions to order entries if available
    if let Some(type_fields) = data.types.get(&data.primary_type) {
        for field in type_fields {
            let value = data
                .message
                .get(&field.name)
                .map(json_value_to_string)
                .unwrap_or_else(|| "<missing>".to_string());
            entries.push(DisplayEntry::Item(DisplayItem {
                label: field.name.clone(),
                value,
            }));
        }
    } else if let Some(obj) = data.message.as_object() {
        // Fallback: iterate message keys
        for (key, val) in obj {
            entries.push(DisplayEntry::Item(DisplayItem {
                label: key.clone(),
                value: json_value_to_string(val),
            }));
        }
    }

    DisplayModel {
        intent: data.primary_type.clone(),
        interpolated_intent: None,
        entries,
        warnings: vec!["No matching descriptor format found".to_string()],
        owner: None,
    }
}

/// Resolve a path in EIP-712 message JSON (e.g., "recipient" or "details.amount").
///
/// Supports `[index]` and `[start:end]` slice notation.
fn resolve_typed_path(message: &serde_json::Value, path: &str) -> Option<serde_json::Value> {
    let path = path.strip_prefix("@.").unwrap_or(path);
    let mut current = message.clone();

    for segment in path.split('.') {
        // Handle array index: "items[0]" or "items[0:3]"
        if let Some(bracket) = segment.find('[') {
            let key = &segment[..bracket];
            let idx_str = &segment[bracket + 1..segment.len() - 1];

            if !key.is_empty() {
                current = current.get(key)?.clone();
            }

            // Check for slice syntax
            if let Some(colon) = idx_str.find(':') {
                let start: usize = idx_str[..colon].parse().ok()?;
                let end: usize = idx_str[colon + 1..].parse().ok()?;
                let arr = current.as_array()?;
                let slice: Vec<serde_json::Value> = arr.get(start..end)?.to_vec();
                current = serde_json::Value::Array(slice);
            } else if let Ok(idx) = idx_str.parse::<usize>() {
                current = current.get(idx)?.clone();
            } else {
                return None;
            }
        } else {
            current = current.get(segment)?.clone();
        }
    }

    Some(current)
}

fn check_typed_visibility(rule: &VisibleRule, value: &Option<serde_json::Value>) -> bool {
    match rule {
        VisibleRule::Always => true,
        VisibleRule::Bool(b) => *b,
        VisibleRule::Named(s) => s != "never",
        VisibleRule::Condition(cond) => {
            if let Some(val) = value {
                cond.evaluate(val)
            } else {
                true
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn format_typed_value(
    descriptor: &Descriptor,
    value: &Option<serde_json::Value>,
    format: Option<&FieldFormat>,
    params: Option<&FormatParams>,
    chain_id: u64,
    message: &serde_json::Value,
    data_provider: &dyn DataProvider,
    warnings: &mut Vec<String>,
) -> Result<String, Error> {
    let Some(val) = value else {
        return Ok("<unresolved>".to_string());
    };

    // Check encryption fallback
    if let Some(params) = params {
        if let Some(ref enc) = params.encryption {
            if let Some(ref fallback) = enc.fallback_label {
                return Ok(fallback.clone());
            }
        }
    }

    // Map reference
    if let Some(params) = params {
        if let Some(ref map_ref) = params.map_reference {
            let raw = json_value_to_string(val);
            if let Some(map_def) = descriptor.metadata.maps.get(map_ref) {
                if let Some(mapped) = map_def.entries.get(&raw) {
                    return Ok(mapped.clone());
                }
            }
        }
    }

    let Some(fmt) = format else {
        return Ok(json_value_to_string(val));
    };

    match fmt {
        FieldFormat::Address => Ok(json_value_to_string(val)),
        FieldFormat::AddressName | FieldFormat::InteroperableAddressName => {
            let addr = json_value_to_string(val);

            // Check senderAddress
            if let Some(params) = params {
                if let Some(ref sender) = params.sender_address {
                    let sender_addrs = match sender {
                        crate::types::display::SenderAddress::Single(s) => vec![s.as_str()],
                        crate::types::display::SenderAddress::Multiple(v) => {
                            v.iter().map(|s| s.as_str()).collect()
                        }
                    };
                    for sender_ref in &sender_addrs {
                        let resolved =
                            if sender_ref.starts_with("@.") || sender_ref.starts_with('#') {
                                resolve_typed_path(message, sender_ref).and_then(|v| match v {
                                    serde_json::Value::String(s) => Some(s),
                                    _ => None,
                                })
                            } else {
                                Some(sender_ref.to_string())
                            };
                        if let Some(resolved_addr) = resolved {
                            if resolved_addr.to_lowercase() == addr.to_lowercase() {
                                return Ok("Sender".to_string());
                            }
                        }
                    }
                }
            }

            // Determine allowed sources
            let sources = params.and_then(|p| p.sources.as_ref());
            let local_allowed = sources
                .map(|s| s.iter().any(|src| src == "local"))
                .unwrap_or(true);
            let ens_allowed = sources
                .map(|s| s.iter().any(|src| src == "ens"))
                .unwrap_or(true);

            if local_allowed {
                if let Some(name) = data_provider
                    .resolve_local_name(&addr, chain_id, params.and_then(|p| p.types.as_deref()))
                    .await
                {
                    return Ok(name);
                }
            }
            if ens_allowed {
                if let Some(name) = data_provider
                    .resolve_ens_name(&addr, chain_id, params.and_then(|p| p.types.as_deref()))
                    .await
                {
                    return Ok(name);
                }
            }
            Ok(addr)
        }
        FieldFormat::TokenAmount => {
            let amount_str = json_value_to_string(val);
            let amount: num_bigint::BigUint = amount_str
                .parse()
                .unwrap_or_else(|_| num_bigint::BigUint::from(0u64));

            let lookup_chain = resolve_typed_chain_id(params, chain_id, message);

            let token_meta = if let Some(params) = params {
                if let Some(ref token_path) = params.token_path {
                    let token_addr = resolve_typed_path(message, token_path);
                    if let Some(serde_json::Value::String(addr)) = token_addr {
                        data_provider.resolve_token(lookup_chain, &addr).await
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(meta) = token_meta {
                let formatted = crate::engine::format_with_decimals(&amount, meta.decimals);
                Ok(format!("{formatted} {}", meta.symbol))
            } else {
                Ok(amount.to_string())
            }
        }
        FieldFormat::Date => {
            let ts: i64 = match val {
                serde_json::Value::Number(n) => n.as_i64().unwrap_or(0),
                serde_json::Value::String(s) => s.parse().unwrap_or(0),
                _ => 0,
            };
            let dt = time::OffsetDateTime::from_unix_timestamp(ts)
                .map_err(|e| Error::Render(format!("invalid timestamp: {e}")))?;
            let format = time::format_description::parse(
                "[year]-[month]-[day] [hour]:[minute]:[second] UTC",
            )
            .map_err(|e| Error::Render(format!("format error: {e}")))?;
            Ok(dt
                .format(&format)
                .map_err(|e| Error::Render(format!("format error: {e}")))?)
        }
        FieldFormat::Enum => {
            let raw = json_value_to_string(val);
            if let Some(params) = params {
                if let Some(ref enum_path) = params.enum_path {
                    if let Some(enum_def) = descriptor.metadata.enums.get(enum_path) {
                        if let Some(label) = enum_def.get(&raw) {
                            return Ok(label.clone());
                        }
                    }
                }
                // $ref path (v2): "$.metadata.enums.interestRateMode"
                if let Some(ref ref_path) = params.ref_path {
                    if let Some(enum_name) = ref_path.strip_prefix("$.metadata.enums.") {
                        if let Some(enum_def) = descriptor.metadata.enums.get(enum_name) {
                            if let Some(label) = enum_def.get(&raw) {
                                return Ok(label.clone());
                            }
                        }
                    }
                }
            }
            Ok(raw)
        }
        FieldFormat::Number => Ok(json_value_to_string(val)),
        FieldFormat::TokenTicker => {
            let lookup_chain = resolve_typed_chain_id(params, chain_id, message);
            let addr = json_value_to_string(val);
            if let Some(meta) = data_provider.resolve_token(lookup_chain, &addr).await {
                Ok(meta.symbol)
            } else {
                warnings.push("token ticker not found".to_string());
                Ok(addr)
            }
        }
        FieldFormat::ChainId => {
            let cid: u64 = match val {
                serde_json::Value::Number(n) => n.as_u64().unwrap_or(0),
                serde_json::Value::String(s) => s.parse().unwrap_or(0),
                _ => 0,
            };
            Ok(crate::engine::chain_name(cid))
        }
        FieldFormat::Raw => Ok(json_value_to_string(val)),
        FieldFormat::Amount => {
            // For EIP-712, amounts are plain numeric strings
            Ok(json_value_to_string(val))
        }
        FieldFormat::Duration => {
            let secs: u64 = match val {
                serde_json::Value::Number(n) => n.as_u64().unwrap_or(0),
                serde_json::Value::String(s) => s.parse().unwrap_or(0),
                _ => 0,
            };
            Ok(format_typed_duration(secs))
        }
        FieldFormat::Unit => {
            let raw = json_value_to_string(val);
            let base = params.and_then(|p| p.base.as_deref()).unwrap_or("");
            let decimals = params.and_then(|p| p.decimals).unwrap_or(0);
            let amount: num_bigint::BigUint = raw
                .parse()
                .unwrap_or_else(|_| num_bigint::BigUint::from(0u64));
            let formatted = if decimals > 0 {
                crate::engine::format_with_decimals(&amount, decimals)
            } else {
                amount.to_string()
            };
            if base.is_empty() {
                Ok(formatted)
            } else {
                Ok(format!("{} {}", formatted, base))
            }
        }
        FieldFormat::NftName => {
            let token_id = json_value_to_string(val);
            let collection_addr = params.and_then(|p| {
                if let Some(ref cpath) = p.collection_path {
                    let resolved = resolve_typed_path(message, cpath);
                    if let Some(serde_json::Value::String(addr)) = resolved {
                        return Some(addr);
                    }
                }
                p.collection.clone()
            });
            if let Some(ref addr) = collection_addr {
                if let Some(name) = data_provider
                    .resolve_nft_collection_name(addr, chain_id)
                    .await
                {
                    return Ok(format!("{} #{}", name, token_id));
                }
            }
            Ok(format!("#{}", token_id))
        }
        FieldFormat::Calldata => {
            // Should not reach here — calldata is intercepted in render_typed_fields
            warnings.push("calldata format should be handled separately".to_string());
            Ok(json_value_to_string(val))
        }
    }
}

fn resolve_typed_chain_id(
    params: Option<&FormatParams>,
    default_chain: u64,
    message: &serde_json::Value,
) -> u64 {
    if let Some(params) = params {
        if let Some(cid) = params.chain_id {
            return cid;
        }
        if let Some(ref path) = params.chain_id_path {
            if let Some(val) = resolve_typed_path(message, path) {
                if let Some(n) = val.as_u64() {
                    return n;
                }
            }
        }
    }
    default_chain
}

fn format_typed_duration(secs: u64) -> String {
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

fn json_value_to_string(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

fn interpolate_typed_intent(
    template: &str,
    message: &serde_json::Value,
    fields: &[DisplayField],
) -> String {
    // Pre-process: replace {{ and }} with sentinels
    const OPEN_SENTINEL: &str = "\x00OPEN_BRACE\x00";
    const CLOSE_SENTINEL: &str = "\x00CLOSE_BRACE\x00";
    let mut result = template
        .replace("{{", OPEN_SENTINEL)
        .replace("}}", CLOSE_SENTINEL);

    // First pass: replace ${path} patterns (v1)
    while let Some(start) = result.find("${") {
        let end = match result[start..].find('}') {
            Some(e) => start + e,
            None => break,
        };
        let path = &result[start + 2..end];
        let replacement = resolve_and_format_typed_interpolation(message, fields, path);
        result.replace_range(start..=end, &replacement);
    }

    // Second pass: replace {name} patterns (v2)
    let mut pos = 0;
    while pos < result.len() {
        if let Some(rel_start) = result[pos..].find('{') {
            let start = pos + rel_start;
            if start > 0 && result.as_bytes()[start - 1] == b'$' {
                pos = start + 1;
                continue;
            }
            let end = match result[start..].find('}') {
                Some(e) => start + e,
                None => break,
            };
            let path = result[start + 1..end].to_string();
            let replacement = resolve_and_format_typed_interpolation(message, fields, &path);
            result.replace_range(start..=end, &replacement);
            pos = start + replacement.len();
        } else {
            break;
        }
    }

    // Post-process: restore escaped braces
    result
        .replace(OPEN_SENTINEL, "{")
        .replace(CLOSE_SENTINEL, "}")
}

fn resolve_and_format_typed_interpolation(
    message: &serde_json::Value,
    fields: &[DisplayField],
    path: &str,
) -> String {
    resolve_typed_path(message, path)
        .map(|v| {
            let field_format = fields.iter().find_map(|f| {
                if let DisplayField::Simple {
                    path: fp, format, ..
                } = f
                {
                    if fp.as_deref() == Some(path) {
                        format.as_ref()
                    } else {
                        None
                    }
                } else {
                    None
                }
            });
            match field_format {
                Some(FieldFormat::Date) => {
                    let ts: i64 = match &v {
                        serde_json::Value::Number(n) => n.as_i64().unwrap_or(0),
                        serde_json::Value::String(s) => s.parse().unwrap_or(0),
                        _ => 0,
                    };
                    if let Ok(dt) = time::OffsetDateTime::from_unix_timestamp(ts) {
                        if let Ok(fmt) = time::format_description::parse(
                            "[year]-[month]-[day] [hour]:[minute]:[second] UTC",
                        ) {
                            if let Ok(s) = dt.format(&fmt) {
                                return s;
                            }
                        }
                    }
                    json_value_to_string(&v)
                }
                _ => json_value_to_string(&v),
            }
        })
        .unwrap_or_else(|| "<?>".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_typed_path() {
        let message = serde_json::json!({
            "recipient": "0xabc",
            "details": {
                "amount": "1000",
                "token": "0xdef"
            }
        });

        assert_eq!(
            resolve_typed_path(&message, "recipient"),
            Some(serde_json::json!("0xabc"))
        );
        assert_eq!(
            resolve_typed_path(&message, "details.amount"),
            Some(serde_json::json!("1000"))
        );
        assert_eq!(resolve_typed_path(&message, "nonexistent"), None);
    }

    #[test]
    fn test_json_value_to_string() {
        assert_eq!(json_value_to_string(&serde_json::json!("hello")), "hello");
        assert_eq!(json_value_to_string(&serde_json::json!(42)), "42");
        assert_eq!(json_value_to_string(&serde_json::json!(true)), "true");
    }

    #[tokio::test]
    async fn test_permit_graceful_fallback() {
        // Real USDC Permit typed data from wallet — no descriptor format for "Permit"
        let typed_data_json = r#"{
            "types": {
                "EIP712Domain": [
                    {"name":"name","type":"string"},
                    {"name":"version","type":"string"},
                    {"name":"chainId","type":"uint256"},
                    {"name":"verifyingContract","type":"address"}
                ],
                "Permit": [
                    {"name":"owner","type":"address"},
                    {"name":"spender","type":"address"},
                    {"name":"value","type":"uint256"},
                    {"name":"nonce","type":"uint256"},
                    {"name":"deadline","type":"uint256"}
                ]
            },
            "primaryType": "Permit",
            "domain": {
                "name": "USD Coin",
                "version": "2",
                "chainId": 1,
                "verifyingContract": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
            },
            "message": {
                "owner": "0xbf01daf454dce008d3e2bfd47d5e186f71477253",
                "spender": "0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2",
                "value": "100000",
                "nonce": 0,
                "deadline": "1773156895"
            }
        }"#;

        let typed_data: TypedData = serde_json::from_str(typed_data_json).unwrap();

        // Empty descriptor — no formats at all
        let descriptor_json = r#"{
            "context": {
                "eip712": {
                    "deployments": [
                        {"chainId": 1, "address": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"}
                    ]
                }
            },
            "metadata": {
                "owner": "test",
                "enums": {},
                "constants": {},
                "maps": {}
            },
            "display": {
                "definitions": {},
                "formats": {}
            }
        }"#;

        let descriptor = Descriptor::from_json(descriptor_json).unwrap();
        let provider = crate::provider::EmptyDataProvider;

        let result = format_typed_data(&descriptor, &typed_data, &provider, &[])
            .await
            .unwrap();

        assert_eq!(result.intent, "Permit");
        assert!(!result.warnings.is_empty());
        assert!(result.warnings[0].contains("No matching descriptor format found"));

        // Should have all 5 fields from the Permit type, in order
        assert_eq!(result.entries.len(), 5);

        if let DisplayEntry::Item(ref item) = result.entries[0] {
            assert_eq!(item.label, "owner");
            assert_eq!(item.value, "0xbf01daf454dce008d3e2bfd47d5e186f71477253");
        } else {
            panic!("expected Item");
        }

        if let DisplayEntry::Item(ref item) = result.entries[1] {
            assert_eq!(item.label, "spender");
            assert_eq!(item.value, "0x87870Bca3F3fD6335C3F4ce8392D69350B4fA4E2");
        } else {
            panic!("expected Item");
        }

        if let DisplayEntry::Item(ref item) = result.entries[2] {
            assert_eq!(item.label, "value");
            assert_eq!(item.value, "100000");
        } else {
            panic!("expected Item");
        }

        if let DisplayEntry::Item(ref item) = result.entries[4] {
            assert_eq!(item.label, "deadline");
            assert_eq!(item.value, "1773156895");
        } else {
            panic!("expected Item");
        }
    }
}
