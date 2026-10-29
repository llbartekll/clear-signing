//! Display configuration types: field formats, visibility rules, and layout groups.

use num_bigint::{BigInt, BigUint, Sign};
use serde::{de, ser, Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, HashMap};

/// Top-level display section of a descriptor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DescriptorDisplay {
    /// Reusable field definitions that can be referenced via `$ref`.
    #[serde(default)]
    pub definitions: HashMap<String, DisplayField>,

    /// Map of format key → display format.
    /// For calldata: key is function signature like `"transfer(address,uint256)"`.
    /// For EIP-712: key is primary type name.
    pub formats: HashMap<String, DisplayFormat>,
}

/// Extract an intent string from a validated intent value.
pub fn intent_as_string(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(obj) => obj
            .iter()
            .map(|(label, value)| format!("{label}: {}", value.as_str().unwrap_or_default()))
            .collect::<Vec<_>>()
            .join(", "),
        _ => val.to_string(),
    }
}

fn deserialize_intent<'de, D>(deserializer: D) -> Result<Option<serde_json::Value>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    let Some(value) = value else {
        return Ok(None);
    };

    match &value {
        serde_json::Value::String(_) => Ok(Some(value)),
        serde_json::Value::Object(obj) => {
            for (key, entry) in obj {
                if !entry.is_string() {
                    return Err(de::Error::custom(format!(
                        "intent object value for key '{}' must be a string",
                        key
                    )));
                }
            }
            Ok(Some(value))
        }
        _ => Err(de::Error::custom(
            "intent must be a string or a flat object of string values",
        )),
    }
}

/// A single display format for a function or message type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayFormat {
    /// Optional format identifier (v2).
    #[serde(rename = "$id")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Human-readable intent label (string or object per spec). Optional: a
    /// format may omit it (e.g. minimal formats), so default to `None` when
    /// absent rather than rejecting the descriptor.
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_intent")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<serde_json::Value>,

    /// Intent with `${path}` or `{name}` template variables for interpolation.
    #[serde(rename = "interpolatedIntent")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interpolated_intent: Option<String>,

    /// Ordered list of fields to display.
    #[serde(default)]
    pub fields: Vec<DisplayField>,

    /// Deprecated in v2 — list of excluded paths.
    #[serde(default)]
    pub excluded: Vec<String>,
}

/// A display field — can be a simple field, a field group, or a reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)]
pub enum DisplayField {
    /// A reference to a definition: `{ "$ref": "$.display.definitions.foo", "path": "...", ... }`.
    ///
    /// Per ERC-7730 spec, a reference object carries `$ref` plus the field's own
    /// `path`, optional `label` and `params` (which override the definition's),
    /// and `visible`.
    Reference {
        #[serde(rename = "$ref")]
        reference: String,

        /// Path to resolve in decoded arguments (from the referencing field).
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,

        /// Label that overrides the definition's label, if set.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,

        /// Params that override/extend the definition's params.
        #[serde(skip_serializing_if = "Option::is_none")]
        params: Option<FormatParams>,

        /// Encryption override for the referenced definition (field-level, per spec).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encryption: Option<EncryptionParams>,

        /// Visibility rule (from the referencing field).
        #[serde(default = "default_visible")]
        visible: VisibleRule,
    },

    /// A grouped set of fields (v2): `{ "fieldGroup": { ... } }`.
    Group {
        #[serde(rename = "fieldGroup")]
        field_group: FieldGroup,
    },

    /// Direct spec group object: `{ "path": "...", "label": "...", "fields": [...] }`.
    Scope {
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,

        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,

        #[serde(default)]
        iteration: Iteration,

        fields: Vec<DisplayField>,
    },

    /// A simple field with path, label, format, etc.
    Simple {
        /// Path to resolve in decoded arguments. Optional when `value` is provided.
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,

        /// Optional in the schema: a definition can omit it, and the
        /// referencing field then gives it. Empty when absent.
        #[serde(default)]
        label: String,

        /// Literal constant value (alternative to `path`).
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<String>,

        #[serde(skip_serializing_if = "Option::is_none")]
        format: Option<FieldFormat>,

        #[serde(skip_serializing_if = "Option::is_none")]
        params: Option<FormatParams>,

        /// Encryption metadata (field-level, sibling of `params`, per ERC-7730 spec).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encryption: Option<EncryptionParams>,

        /// Separator string for array-typed values.
        #[serde(skip_serializing_if = "Option::is_none")]
        separator: Option<String>,

        #[serde(default = "default_visible")]
        visible: VisibleRule,
    },
}

fn default_visible() -> VisibleRule {
    VisibleRule::Always
}

/// A field group — replaces v1's `nestedFields`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldGroup {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    #[serde(default)]
    pub iteration: Iteration,

    pub fields: Vec<DisplayField>,
}

/// How grouped fields should be iterated for display.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Iteration {
    #[default]
    Sequential,
    Bundled,
}

/// Visibility rule for a field.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(untagged)]
pub enum VisibleRule {
    /// Boolean shorthand: true = Always, false = Never.
    Bool(bool),

    /// String shorthand: "always", "never", or "optional".
    Named(VisibleLiteral),

    /// Conditional visibility.
    Condition(VisibleCondition),

    /// Default: always visible.
    #[default]
    Always,
}

/// Named visibility literals accepted by the current spec.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VisibleLiteral {
    Always,
    Never,
    Optional,
}

/// Conditional visibility: `ifNotIn` or `mustMatch`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisibleCondition {
    #[serde(rename = "ifNotIn")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub if_not_in: Option<Vec<serde_json::Value>>,

    #[serde(rename = "mustMatch", alias = "mustBe")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub must_match: Option<Vec<serde_json::Value>>,
}

impl VisibleCondition {
    pub fn hides_for_if_not_in(&self, value: &serde_json::Value) -> bool {
        self.if_not_in.as_ref().is_some_and(|excluded| {
            excluded
                .iter()
                .any(|expected| visibility_values_equal(expected, value))
        })
    }

    pub fn matches_must_match(&self, value: &serde_json::Value) -> bool {
        self.must_match.as_ref().is_none_or(|required| {
            required
                .iter()
                .any(|expected| visibility_values_equal(expected, value))
        })
    }
}

/// Integer parsed from a visibility value.
enum VisibilityInt {
    /// Decimal literal, possibly negative.
    Decimal(BigInt),
    /// Hex literal with its bit width.
    Hex(BigUint, u64),
}

fn parse_visibility_int(text: &str) -> Option<VisibilityInt> {
    if let Some(hex_digits) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        let bytes = hex::decode(hex_digits).ok()?;
        return Some(VisibilityInt::Hex(
            BigUint::from_bytes_be(&bytes),
            hex_digits.len() as u64 * 4,
        ));
    }
    if !text
        .strip_prefix('-')
        .unwrap_or(text)
        .bytes()
        .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    text.parse::<BigInt>().ok().map(VisibilityInt::Decimal)
}

fn visibility_scalar_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn is_hex_literal(text: &str) -> bool {
    text.starts_with("0x") || text.starts_with("0X")
}

/// Interpret a hex literal as two's complement of its own bit width.
fn hex_as_signed(magnitude: &BigUint, bits: u64) -> BigInt {
    let signed = BigInt::from(magnitude.clone());
    if bits > 0 && magnitude.bit(bits - 1) {
        signed - (BigInt::from(1) << bits)
    } else {
        signed
    }
}

/// Compare a descriptor `mustMatch` / `ifNotIn` value with a decoded value.
///
/// Hex literals compare case-insensitively, so checksummed addresses match
/// lowercase decoded addresses. A decimal literal (string or JSON number)
/// compares as an integer against a hex word or another decimal. A negative
/// decimal reads a hex word as two's complement.
fn visibility_values_equal(expected: &serde_json::Value, actual: &serde_json::Value) -> bool {
    if expected == actual {
        return true;
    }
    let (Some(expected), Some(actual)) = (
        visibility_scalar_text(expected),
        visibility_scalar_text(actual),
    ) else {
        return false;
    };
    if is_hex_literal(&expected) && is_hex_literal(&actual) {
        return expected.eq_ignore_ascii_case(&actual);
    }
    let (Some(expected), Some(actual)) = (
        parse_visibility_int(&expected),
        parse_visibility_int(&actual),
    ) else {
        return false;
    };
    match (expected, actual) {
        (VisibilityInt::Decimal(a), VisibilityInt::Decimal(b)) => a == b,
        (VisibilityInt::Decimal(dec), VisibilityInt::Hex(mag, bits))
        | (VisibilityInt::Hex(mag, bits), VisibilityInt::Decimal(dec)) => {
            if dec.sign() == Sign::Minus {
                hex_as_signed(&mag, bits) == dec
            } else {
                BigInt::from(mag) == dec
            }
        }
        (VisibilityInt::Hex(..), VisibilityInt::Hex(..)) => false,
    }
}

/// Field format types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FieldFormat {
    TokenAmount,
    Amount,
    Date,
    #[serde(rename = "enum")]
    Enum,
    Address,
    AddressName,
    Number,
    Raw,
    TokenTicker,
    ChainId,
    Calldata,
    NftName,
    Duration,
    Unit,
    /// ERC-7930 interoperable address format.
    InteroperableAddressName,
}

/// Format parameters — varies by format type.
///
/// Serde goes through the manual impls below (`remote = "Self"`), which move
/// spec map references out of the constant fields into [`Self::map_refs`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(remote = "Self")]
pub struct FormatParams {
    /// Token address path for tokenAmount/tokenTicker (resolved from calldata).
    #[serde(rename = "tokenPath")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_path: Option<String>,

    /// Static token address or `$.metadata.constants.*` ref for tokenAmount/tokenTicker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    /// Native currency address — single address or array of addresses/constant refs.
    /// Per spec: "Either a string or an array of strings."
    #[serde(rename = "nativeCurrencyAddress")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_currency_address: Option<NativeCurrencyAddress>,

    /// Static chain ID for cross-chain token resolution.
    #[serde(rename = "chainId")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<u64>,

    /// Dynamic chain ID path from calldata.
    #[serde(rename = "chainIdPath")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_id_path: Option<String>,

    /// Enum lookup key in metadata.enums.
    #[serde(rename = "enumPath")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enum_path: Option<String>,

    /// `$ref` enum reference path (v2): e.g., `"$.metadata.enums.interestRateMode"`.
    #[serde(rename = "$ref")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_path: Option<String>,

    /// Map reference key in metadata.maps.
    #[serde(rename = "mapReference")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub map_reference: Option<String>,

    /// Threshold for max-amount display (v2).
    /// Value or `"$.metadata.constants.max"` reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<String>,

    /// Message to display when amount >= threshold (e.g., "All", "Max").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// Unit base symbol (e.g., "%", "bps", "h") for the `unit` format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,

    /// Decimal places for the `unit` format (default 0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decimals: Option<u8>,

    /// Whether to use SI prefix notation for the `unit` format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<bool>,

    /// Encryption parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption: Option<EncryptionParams>,

    /// Date encoding: `"timestamp"` (default) or `"blockheight"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,

    /// Path to resolve which selector to use for nested calldata decoding.
    #[serde(rename = "selectorPath")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector_path: Option<String>,

    /// Constant selector override for nested calldata decoding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,

    /// Path to the callee address for nested calldata (e.g., "to").
    #[serde(rename = "calleePath")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callee_path: Option<String>,

    /// Constant callee address for nested calldata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callee: Option<String>,

    /// Path to the value amount for nested calldata (injected as `@.value` in inner context).
    #[serde(rename = "amountPath")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount_path: Option<String>,

    /// Constant native amount for nested calldata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<UintLiteral>,

    /// Path to the spender/from address for nested calldata (injected as `@.from` in inner context).
    #[serde(rename = "spenderPath")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spender_path: Option<String>,

    /// Constant spender/from address for nested calldata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spender: Option<String>,

    /// Address types for addressName format (spec: "eoa", "contract", etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub types: Option<Vec<String>>,

    /// Trusted name sources for addressName format (spec: "ens", "local").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<String>>,

    /// Sender address check for addressName format.
    #[serde(rename = "senderAddress")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender_address: Option<SenderAddress>,

    /// Path to the collection address for nftName format.
    #[serde(rename = "collectionPath")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection_path: Option<String>,

    /// Constant collection address for nftName format.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection: Option<String>,

    /// Spec map references (`metadata.maps`) given in place of a constant
    /// parameter, keyed by the parameter name (e.g. `"token"`). Rendering
    /// replaces each one with the map value for the transaction.
    #[serde(skip)]
    pub map_refs: BTreeMap<String, MapReference>,
}

/// Parameters that take a constant, so they also accept a map reference.
pub(crate) const MAP_REFERENCE_PARAMS: [&str; 7] = [
    "token",
    "chainId",
    "callee",
    "selector",
    "amount",
    "spender",
    "collection",
];

/// Path prefix of a map in the descriptor.
pub(crate) const MAP_PATH_PREFIX: &str = "$.metadata.maps.";

/// Spec map reference: `{"map": "$.metadata.maps.<name>", "keyPath": "<path>"}`.
///
/// The ERC's example files write `key` in place of `keyPath`; both are accepted.
/// Without a key path, the map's own `keyPath` applies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapReference {
    pub map: String,
    #[serde(rename = "keyPath", alias = "key")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_path: Option<String>,
}

impl<'de> Deserialize<'de> for FormatParams {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut object = serde_json::Map::<String, serde_json::Value>::deserialize(deserializer)?;
        let mut map_refs = BTreeMap::new();
        for name in MAP_REFERENCE_PARAMS {
            let reference = match object.get(name) {
                Some(serde_json::Value::Object(_)) => object
                    .remove(name)
                    .map(serde_json::from_value::<MapReference>)
                    .transpose()
                    .map_err(de::Error::custom)?,
                // The ERC's inline example writes the map path as a plain string.
                Some(serde_json::Value::String(path)) if path.starts_with(MAP_PATH_PREFIX) => {
                    let map = path.clone();
                    object.remove(name);
                    Some(MapReference {
                        map,
                        key_path: None,
                    })
                }
                _ => None,
            };
            if let Some(reference) = reference {
                map_refs.insert(name.to_string(), reference);
            }
        }
        let mut params = FormatParams::deserialize(serde_json::Value::Object(object))
            .map_err(de::Error::custom)?;
        params.map_refs = map_refs;
        Ok(params)
    }
}

impl Serialize for FormatParams {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.map_refs.is_empty() {
            return FormatParams::serialize(self, serializer);
        }
        let mut value = FormatParams::serialize(self, serde_json::value::Serializer)
            .map_err(ser::Error::custom)?;
        if let serde_json::Value::Object(object) = &mut value {
            for (name, reference) in &self.map_refs {
                let reference = serde_json::to_value(reference).map_err(ser::Error::custom)?;
                object.insert(name.clone(), reference);
            }
        }
        value.serialize(serializer)
    }
}

/// Native currency address — single address or array of addresses/constant refs.
/// Per ERC-7730 spec: "Either a string or an array of strings."
/// Values may be `$.metadata.constants.xxx` references resolved at comparison time.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NativeCurrencyAddress {
    Single(String),
    Multiple(Vec<String>),
}

impl NativeCurrencyAddress {
    /// Check if `addr` matches any native currency address, resolving `$.metadata.constants.*` refs.
    pub fn matches(&self, addr: &str, constants: &HashMap<String, serde_json::Value>) -> bool {
        let items: Vec<&str> = match self {
            NativeCurrencyAddress::Single(s) => vec![s.as_str()],
            NativeCurrencyAddress::Multiple(v) => v.iter().map(|s| s.as_str()).collect(),
        };
        items.iter().any(|item| {
            let resolved = if let Some(key) = item.strip_prefix("$.metadata.constants.") {
                constants.get(key).and_then(|v| v.as_str()).unwrap_or(item)
            } else {
                item
            };
            resolved.eq_ignore_ascii_case(addr)
        })
    }
}

/// Sender address — can be a single address or an array of addresses/paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SenderAddress {
    Single(String),
    Multiple(Vec<String>),
}

/// Unsigned integer literal for descriptor params.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum UintLiteral {
    Number(u64),
    String(String),
}

impl UintLiteral {
    pub fn to_biguint(&self) -> Option<BigUint> {
        match self {
            UintLiteral::Number(value) => Some(BigUint::from(*value)),
            UintLiteral::String(value) => {
                let trimmed = value.trim();
                if let Some(hex) = trimmed
                    .strip_prefix("0x")
                    .or_else(|| trimmed.strip_prefix("0X"))
                {
                    let bytes = hex::decode(hex).ok()?;
                    Some(BigUint::from_bytes_be(&bytes))
                } else {
                    trimmed.parse::<BigUint>().ok()
                }
            }
        }
    }
}

/// Encryption parameters for encrypted fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionParams {
    /// Encryption scheme identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheme: Option<String>,

    /// Type of the plaintext content.
    #[serde(rename = "plaintextType")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plaintext_type: Option<String>,

    #[serde(rename = "fallbackLabel")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_label: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn format_params_parse_and_write_map_references() {
        let raw = json!({
            "tokenPath": "token",
            "token": { "map": "$.metadata.maps.underlying", "key": "@.chainId" },
            "chainId": { "map": "$.metadata.maps.chains", "keyPath": "pool" },
            "callee": "$.metadata.maps.router",
            "collection": "0x0000000000000000000000000000000000000001"
        });
        let params: FormatParams = serde_json::from_value(raw).unwrap();

        assert_eq!(params.token, None);
        assert_eq!(params.chain_id, None);
        assert_eq!(params.callee, None);
        assert_eq!(params.token_path.as_deref(), Some("token"));
        assert_eq!(
            params.collection.as_deref(),
            Some("0x0000000000000000000000000000000000000001")
        );
        assert_eq!(
            params.map_refs["token"],
            MapReference {
                map: "$.metadata.maps.underlying".to_string(),
                key_path: Some("@.chainId".to_string()),
            }
        );
        assert_eq!(params.map_refs["chainId"].key_path.as_deref(), Some("pool"));
        assert_eq!(params.map_refs["callee"].key_path, None);

        let written = serde_json::to_value(&params).unwrap();
        assert_eq!(
            written["token"],
            json!({ "map": "$.metadata.maps.underlying", "keyPath": "@.chainId" })
        );
        assert_eq!(
            written["callee"],
            json!({ "map": "$.metadata.maps.router" })
        );
        assert_eq!(written["tokenPath"], json!("token"));
    }

    #[test]
    fn format_params_without_map_references_are_unchanged() {
        let raw = json!({ "token": "0xabc", "chainId": 10 });
        let params: FormatParams = serde_json::from_value(raw.clone()).unwrap();
        assert!(params.map_refs.is_empty());
        assert_eq!(params.token.as_deref(), Some("0xabc"));
        assert_eq!(params.chain_id, Some(10));
        assert_eq!(serde_json::to_value(&params).unwrap(), raw);
    }
}
