//! Shared render helpers used by both calldata and EIP-712 formatting.

use num_bigint::BigUint;

use crate::chain_names::CHAIN_NAMES;
use crate::decoder::ArgumentValue;
use crate::error::Error;
use crate::native_currencies::NATIVE_CURRENCIES;
use crate::provider::DataProvider;
use crate::token::TokenMeta;
use crate::types::descriptor::Descriptor;
use crate::types::display::{DisplayField, FieldFormat, FormatParams};

/// Human-readable name for a chain ID.
///
/// The spec's `chainId` format converts the value "to a Blockchain name using
/// EIP-155 reference values", so the names are the `name` fields of the EIP-155
/// chain list at <https://chainid.network/chains.json> (e.g. 1 -> "Ethereum Mainnet").
/// An unknown chain ID renders as "Chain <id>".
pub(crate) fn chain_name(chain_id: u64) -> String {
    CHAIN_NAMES
        .binary_search_by_key(&chain_id, |&(id, _)| id)
        .map(|i| CHAIN_NAMES[i].1.to_string())
        .unwrap_or_else(|_| format!("Chain {chain_id}"))
}

/// Resolve a `$.metadata.constants.xxx` reference to its string value, or return the input as-is.
pub(crate) fn resolve_metadata_constant_str(descriptor: &Descriptor, ref_str: &str) -> String {
    if let Some(const_name) = ref_str.strip_prefix("$.metadata.constants.") {
        descriptor
            .metadata
            .constants
            .get(const_name)
            .and_then(|v| v.as_str())
            .unwrap_or(ref_str)
            .to_string()
    } else {
        ref_str.to_string()
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

/// Resolve a `$.metadata.constants.xxx` or literal hex reference to a BigUint.
pub(crate) fn resolve_metadata_constant_biguint(
    descriptor: &Descriptor,
    ref_path: &str,
) -> Option<BigUint> {
    if let Some(const_name) = ref_path.strip_prefix("$.metadata.constants.") {
        let val = descriptor.metadata.constants.get(const_name)?;
        parse_constant_to_biguint(val)
    } else {
        let hex_str = ref_path.strip_prefix("0x").unwrap_or(ref_path);
        BigUint::parse_bytes(hex_str.as_bytes(), 16)
    }
}

/// Get native token metadata for a chain from the EIP-155 chain list
/// (chainid.network). Unknown chains fall back to ETH with 18 decimals.
pub(crate) fn native_token_meta(chain_id: u64) -> TokenMeta {
    let (symbol, name, decimals) = NATIVE_CURRENCIES
        .binary_search_by_key(&chain_id, |&(id, ..)| id)
        .map(|i| {
            let (_, symbol, name, decimals) = NATIVE_CURRENCIES[i];
            (symbol, name, decimals)
        })
        .unwrap_or(("ETH", "Ether", 18));
    TokenMeta {
        symbol: symbol.to_string(),
        decimals,
        name: name.to_string(),
    }
}

/// Format a BigUint with decimal places.
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
        let trimmed = result.trim_end_matches('0').trim_end_matches('.');
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

pub(crate) fn format_duration_seconds(secs: u64) -> String {
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

pub(crate) fn coerce_unsigned_biguint_from_argument_value(val: &ArgumentValue) -> Option<BigUint> {
    match val {
        ArgumentValue::Uint(bytes)
        | ArgumentValue::Bytes(bytes)
        | ArgumentValue::FixedBytes(bytes) => Some(BigUint::from_bytes_be(bytes)),
        _ => None,
    }
}

pub(crate) fn coerce_unsigned_decimal_string_from_argument_value(
    val: &ArgumentValue,
) -> Option<String> {
    coerce_unsigned_biguint_from_argument_value(val).map(|n| n.to_string())
}

pub(crate) fn coerce_unsigned_biguint_from_typed_value(val: &serde_json::Value) -> Option<BigUint> {
    match val {
        serde_json::Value::Number(n) => n.as_u64().map(BigUint::from),
        serde_json::Value::String(s) => {
            let trimmed = s.trim();
            if let Some(hex_str) = trimmed
                .strip_prefix("0x")
                .or_else(|| trimmed.strip_prefix("0X"))
            {
                let bytes = hex::decode(hex_str).ok()?;
                Some(BigUint::from_bytes_be(&bytes))
            } else if trimmed.starts_with('-') {
                None
            } else {
                trimmed.parse::<BigUint>().ok()
            }
        }
        _ => None,
    }
}

pub(crate) fn coerce_unsigned_decimal_string_from_typed_value(
    val: &serde_json::Value,
) -> Option<String> {
    coerce_unsigned_biguint_from_typed_value(val).map(|n| n.to_string())
}

pub(crate) fn parse_unsigned_biguint_from_typed_value(
    val: &serde_json::Value,
    format_name: &str,
) -> Result<BigUint, Error> {
    coerce_unsigned_biguint_from_typed_value(val)
        .ok_or_else(|| Error::Render(format!("{format_name} field must be an unsigned integer")))
}

pub(crate) fn format_unit_biguint(
    raw_val: &BigUint,
    base: &str,
    params: Option<&FormatParams>,
) -> String {
    let decimals = params.and_then(|p| p.decimals).unwrap_or(0);
    let use_prefix = params.and_then(|p| p.prefix).unwrap_or(false);

    let formatted = if decimals > 0 {
        format_with_decimals(raw_val, decimals)
    } else {
        raw_val.to_string()
    };

    if use_prefix {
        let si_formatted = apply_si_prefix(&formatted);
        format!("{si_formatted}{base}")
    } else {
        format!("{formatted}{base}")
    }
}

fn apply_si_prefix(value_str: &str) -> String {
    let n: f64 = match value_str.parse() {
        Ok(v) => v,
        Err(_) => return value_str.to_string(),
    };
    let abs = n.abs();
    let (divisor, prefix) = if abs >= 1e18 {
        (1e18, "E")
    } else if abs >= 1e15 {
        (1e15, "P")
    } else if abs >= 1e12 {
        (1e12, "T")
    } else if abs >= 1e9 {
        (1e9, "G")
    } else if abs >= 1e6 {
        (1e6, "M")
    } else if abs >= 1e3 {
        (1e3, "k")
    } else {
        return value_str.to_string();
    };
    let scaled = n / divisor;
    let formatted = format!("{:.2}", scaled);
    let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
    format!("{trimmed}{prefix}")
}

pub(crate) fn format_timestamp(timestamp: i64) -> Result<String, Error> {
    let dt = time::OffsetDateTime::from_unix_timestamp(timestamp)
        .map_err(|e| Error::Render(format!("invalid timestamp: {e}")))?;

    let format = time::format_description::parse("[year]-[month]-[day] [hour]:[minute]:[second]Z")
        .map_err(|e| Error::Render(format!("format error: {e}")))?;

    dt.format(&format)
        .map_err(|e| Error::Render(format!("format error: {e}")))
}

pub(crate) async fn format_blockheight_timestamp(
    data_provider: &dyn DataProvider,
    chain_id: u64,
    block_number: u64,
) -> Result<String, Error> {
    let timestamp = data_provider
        .resolve_block_timestamp(chain_id, block_number)
        .await
        .ok_or_else(|| {
            Error::Render(format!(
                "could not resolve approximate timestamp for block {} on chain {}",
                block_number, chain_id
            ))
        })?;
    let timestamp = i64::try_from(timestamp)
        .map_err(|_| Error::Render(format!("timestamp {} does not fit into i64", timestamp)))?;
    format_timestamp(timestamp)
}

pub(crate) fn is_excluded_path(excluded: &[String], path: &str) -> bool {
    excluded.iter().any(|excluded_path| excluded_path == path)
}

#[derive(Debug)]
pub(crate) struct InterpolationFieldSpec<'a> {
    pub label: &'a str,
    pub path: &'a str,
    pub format: Option<&'a FieldFormat>,
    pub params: Option<&'a FormatParams>,
    pub separator: Option<&'a str>,
    /// Set when the placeholder names one element (`x.[2].y`) of an array
    /// field (`x.[].y`): the element index.
    pub element_index: Option<usize>,
}

/// Normalize a path for interpolation matching: a bare parameter name (`amount`)
/// and the `#.`-prefixed form (`#.amount`) refer to the same field.
fn normalize_interpolation_path(path: &str) -> &str {
    path.strip_prefix("#.").unwrap_or(path)
}

/// If `placeholder` names one element of the array field path `field_path`
/// (`x.[2].y` for `x.[].y`), return the element index.
fn array_element_index(field_path: &str, placeholder: &str) -> Option<usize> {
    let field_path = normalize_interpolation_path(field_path);
    let placeholder = normalize_interpolation_path(placeholder);
    let pos = field_path.find(".[]")?;
    let (base, rest) = (&field_path[..pos], &field_path[pos + 3..]);
    let after_base = placeholder.strip_prefix(base)?.strip_prefix(".[")?;
    let (index, after_index) = after_base.split_once(']')?;
    if after_index != rest || index.is_empty() || !index.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    index.parse().ok()
}

fn find_field_matching<'a>(
    fields: &'a [DisplayField],
    matcher: &dyn Fn(&str) -> Option<Option<usize>>,
) -> Option<InterpolationFieldSpec<'a>> {
    for field in fields {
        match field {
            DisplayField::Simple {
                path: Some(field_path),
                label,
                value: None,
                format,
                params,
                separator,
                ..
            } => {
                if let Some(element_index) = matcher(field_path) {
                    return Some(InterpolationFieldSpec {
                        label,
                        path: field_path,
                        format: format.as_ref(),
                        params: params.as_ref(),
                        separator: separator.as_deref(),
                        element_index,
                    });
                }
            }
            DisplayField::Group { field_group } => {
                if let Some(spec) = find_field_matching(&field_group.fields, matcher) {
                    return Some(spec);
                }
            }
            _ => {}
        }
    }

    None
}

/// Find the display field for an interpolation placeholder. An exact path
/// match wins. Otherwise a placeholder that names one element of an array
/// field (`x.[2].y` for the field `x.[].y`) matches that field.
pub(crate) fn find_interpolation_field<'a>(
    fields: &'a [DisplayField],
    path: &str,
) -> Option<InterpolationFieldSpec<'a>> {
    let wanted = normalize_interpolation_path(path);
    find_field_matching(fields, &|field_path| {
        (normalize_interpolation_path(field_path) == wanted).then_some(None)
    })
    .or_else(|| {
        find_field_matching(fields, &|field_path| {
            array_element_index(field_path, path).map(Some)
        })
    })
}

pub(crate) fn resolve_interpolation_field_spec<'a>(
    fields: &'a [DisplayField],
    excluded: &[String],
    path: &str,
) -> Result<InterpolationFieldSpec<'a>, Error> {
    if is_excluded_path(excluded, path) {
        return Err(Error::Descriptor(format!(
            "interpolatedIntent path '{}' refers to an excluded field",
            path
        )));
    }

    let field = find_interpolation_field(fields, path).ok_or_else(|| {
        Error::Descriptor(format!(
            "interpolatedIntent path '{}' does not match any display field",
            path
        ))
    })?;

    if matches!(field.format, Some(FieldFormat::Calldata)) {
        return Err(Error::Descriptor(format!(
            "interpolatedIntent path '{}' refers to non-stringable calldata field",
            path
        )));
    }

    Ok(field)
}

pub(crate) fn lookup_map_entry(
    descriptor: &Descriptor,
    map_ref: &str,
    key: &str,
) -> Option<String> {
    let map_def = descriptor.metadata.maps.get(map_ref)?;
    map_def.entries.get(key).cloned()
}

pub(crate) fn format_token_amount_output(
    descriptor: &Descriptor,
    raw_amount: &BigUint,
    params: Option<&FormatParams>,
    token_meta: Option<&TokenMeta>,
) -> String {
    if let Some(params) = params {
        if let (Some(threshold_ref), Some(message)) = (&params.threshold, &params.message) {
            if let Some(threshold) = resolve_metadata_constant_biguint(descriptor, threshold_ref) {
                if raw_amount >= &threshold {
                    if let Some(meta) = token_meta {
                        return format!("{} {}", message, meta.symbol);
                    }
                    return message.clone();
                }
            }
        }
    }

    if let Some(meta) = token_meta {
        let formatted = format_with_decimals(raw_amount, meta.decimals);
        format!("{formatted} {}", meta.symbol)
    } else {
        raw_amount.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::display::FormatParams;
    use serde_json::json;

    fn descriptor_with_metadata(metadata: serde_json::Value) -> Descriptor {
        Descriptor::from_json(
            &json!({
                "context": {
                    "contract": {
                        "deployments": []
                    }
                },
                "metadata": metadata,
                "display": {
                    "formats": {}
                }
            })
            .to_string(),
        )
        .unwrap()
    }

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
        assert_eq!(format_with_decimals(&amount, 18), "0");
    }

    #[test]
    fn test_format_timestamp_uses_rfc3339_z_suffix() {
        assert_eq!(format_timestamp(0).unwrap(), "1970-01-01 00:00:00Z");
        let s = format_timestamp(1_700_000_000).unwrap();
        assert!(
            s.ends_with('Z') && !s.contains("UTC"),
            "expected RFC-3339 Z suffix, got {s:?}"
        );
    }

    #[test]
    fn test_array_element_index() {
        assert_eq!(array_element_index("x.[].y", "x.[2].y"), Some(2));
        assert_eq!(array_element_index("#.x.[].y", "x.[0].y"), Some(0));
        assert_eq!(array_element_index("x.[]", "x.[10]"), Some(10));
        assert_eq!(array_element_index("x.[].y", "x.[2].z"), None);
        assert_eq!(array_element_index("x.[].y", "x.[-1].y"), None);
        assert_eq!(array_element_index("x.[].y", "x.[].y"), None);
        assert_eq!(array_element_index("x.y", "x.y"), None);
    }

    #[test]
    fn test_native_token_meta() {
        assert_eq!(native_token_meta(1).symbol, "ETH");
        assert_eq!(native_token_meta(137).symbol, "POL");
        assert_eq!(native_token_meta(80001).symbol, "MATIC");
        assert_eq!(native_token_meta(42220).symbol, "CELO");
        assert_eq!(native_token_meta(100).symbol, "XDAI");
        assert_eq!(native_token_meta(56).symbol, "BNB");
        assert_eq!(native_token_meta(14).symbol, "FLR");
        assert_eq!(native_token_meta(19).symbol, "SGB");
        assert_eq!(native_token_meta(u64::MAX).symbol, "ETH");
    }

    #[test]
    fn test_native_currencies_sorted_by_chain_id() {
        assert!(NATIVE_CURRENCIES.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn test_chain_name() {
        assert_eq!(chain_name(1), "Ethereum Mainnet");
        assert_eq!(chain_name(137), "Polygon Mainnet");
        assert_eq!(chain_name(42161), "Arbitrum One");
        assert_eq!(chain_name(14), "Flare Mainnet");
        assert_eq!(chain_name(19), "Songbird Canary-Network");
        assert_eq!(chain_name(999), "Wanchain Testnet");
        assert_eq!(chain_name(81457), "Blast");
        assert_eq!(chain_name(59144), "Linea");
        assert_eq!(chain_name(u64::MAX), format!("Chain {}", u64::MAX));
    }

    #[test]
    fn test_chain_names_sorted_by_chain_id() {
        assert!(CHAIN_NAMES.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn test_find_interpolation_field_recurses_into_groups() {
        let fields: Vec<DisplayField> = serde_json::from_value(json!([
            {
                "fieldGroup": {
                    "label": "Outer",
                    "fields": [
                        { "path": "amount", "label": "Amount", "format": "tokenAmount" }
                    ]
                }
            }
        ]))
        .unwrap();

        let field = find_interpolation_field(&fields, "amount").unwrap();
        assert_eq!(field.label, "Amount");
        assert_eq!(field.path, "amount");
        assert!(matches!(field.format, Some(FieldFormat::TokenAmount)));
    }

    #[test]
    fn test_format_token_amount_output_threshold_with_metadata() {
        let descriptor = descriptor_with_metadata(json!({
            "constants": {
                "max": "0x100"
            }
        }));
        let params: FormatParams = serde_json::from_value(json!({
            "threshold": "$.metadata.constants.max",
            "message": "All"
        }))
        .unwrap();
        let token_meta = TokenMeta {
            symbol: "USDC".to_string(),
            decimals: 6,
            name: "USD Coin".to_string(),
        };

        let rendered = format_token_amount_output(
            &descriptor,
            &BigUint::from(0x100u64),
            Some(&params),
            Some(&token_meta),
        );
        assert_eq!(rendered, "All USDC");
    }

    #[test]
    fn test_format_token_amount_output_without_metadata() {
        let descriptor = descriptor_with_metadata(json!({}));
        let params: FormatParams = serde_json::from_value(json!({
            "nativeCurrencyAddress": "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE"
        }))
        .unwrap();

        let rendered =
            format_token_amount_output(&descriptor, &BigUint::from(42u64), Some(&params), None);
        assert_eq!(rendered, "42");
    }

    #[test]
    fn test_unsigned_numeric_coercion_from_argument_value() {
        assert_eq!(
            coerce_unsigned_decimal_string_from_argument_value(&ArgumentValue::Bytes(vec![
                0x01, 0xf4
            ]))
            .as_deref(),
            Some("500")
        );
        assert_eq!(
            coerce_unsigned_decimal_string_from_argument_value(&ArgumentValue::Int(vec![0x01])),
            None
        );
    }

    #[test]
    fn test_unsigned_numeric_coercion_from_typed_value() {
        assert_eq!(
            coerce_unsigned_decimal_string_from_typed_value(&json!("0x01f4")).as_deref(),
            Some("500")
        );
        assert_eq!(
            coerce_unsigned_decimal_string_from_typed_value(&json!("500")).as_deref(),
            Some("500")
        );
        assert_eq!(
            coerce_unsigned_decimal_string_from_typed_value(&json!(500)).as_deref(),
            Some("500")
        );
        assert_eq!(
            parse_unsigned_biguint_from_typed_value(&json!("not-a-number"), "amount")
                .unwrap_err()
                .to_string(),
            "render error: amount field must be an unsigned integer"
        );
    }

    #[test]
    fn test_lookup_map_entry() {
        let descriptor = descriptor_with_metadata(json!({
            "maps": {
                "orderTypes": {
                    "entries": {
                        "1": "Market"
                    }
                }
            }
        }));

        assert_eq!(
            lookup_map_entry(&descriptor, "orderTypes", "1").as_deref(),
            Some("Market")
        );
        assert_eq!(lookup_map_entry(&descriptor, "orderTypes", "2"), None);
    }

    #[test]
    fn test_resolve_interpolation_field_spec_rejects_excluded_and_calldata() {
        let excluded = vec!["amount".to_string()];
        let fields: Vec<DisplayField> = serde_json::from_value(json!([
            { "path": "amount", "label": "Amount" },
            { "path": "data", "label": "Data", "format": "calldata" }
        ]))
        .unwrap();

        let excluded_err =
            resolve_interpolation_field_spec(&fields, &excluded, "amount").unwrap_err();
        assert!(excluded_err
            .to_string()
            .contains("interpolatedIntent path 'amount' refers to an excluded field"));

        let calldata_err =
            resolve_interpolation_field_spec(&fields, &Vec::new(), "data").unwrap_err();
        assert!(calldata_err
            .to_string()
            .contains("interpolatedIntent path 'data' refers to non-stringable calldata field"));
    }

    #[test]
    fn test_resolve_metadata_constant_biguint_supports_literal_and_constant() {
        let descriptor = descriptor_with_metadata(json!({
            "constants": {
                "max": "0x100"
            }
        }));

        assert_eq!(
            resolve_metadata_constant_biguint(&descriptor, "$.metadata.constants.max"),
            Some(BigUint::from(0x100u64))
        );
        assert_eq!(
            resolve_metadata_constant_biguint(&descriptor, "0x20"),
            Some(BigUint::from(0x20u64))
        );
    }
}
