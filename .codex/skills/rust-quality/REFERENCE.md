# Reference Patterns

Advanced patterns specific to the ERC-7730 v2 clear signing library.

## Signature-Based Decoding

Parse function signatures directly from descriptor format keys. No ABI JSON is
required.

```rust
let sig = parse_signature("transfer(address,uint256)")?;
let decoded = decode_calldata(&sig, calldata)?;
```

Descriptor format keys are the function signatures:
`"transfer(address,uint256)": { ... }`.

## Trait Pattern: DescriptorSource / TokenSource

Define the trait, then provide a static implementation for tests:

```rust
pub trait DescriptorSource {
    fn resolve_calldata(&self, chain_id: u64, address: &str)
        -> Result<ResolvedDescriptor, ResolveError>;
    fn resolve_typed(&self, chain_id: u64, address: &str)
        -> Result<ResolvedDescriptor, ResolveError>;
}

pub struct StaticSource {
    calldata: HashMap<String, Descriptor>,
    typed: HashMap<String, Descriptor>,
}

impl StaticSource {
    pub fn new() -> Self { ... }

    fn make_key(chain_id: u64, address: &str) -> String {
        format!("{}:{}", chain_id, address.to_lowercase())
    }
}
```

Follow this pattern when adding new source traits.

## RenderContext Pipeline

Mutable context carries shared state through rendering:

```rust
struct RenderContext<'a> {
    descriptor: &'a Descriptor,
    decoded: &'a DecodedArguments,
    chain_id: u64,
    token_source: &'a dyn TokenSource,
    address_book: &'a AddressBook,
    warnings: Vec<String>,
}
```

Return `Err(...)` for fatal issues. Push warnings for non-fatal degradation.

## ArgumentValue Recursive Enum

Decoded calldata values are recursive:

```rust
pub enum ArgumentValue {
    Address([u8; 20]),
    Uint(Vec<u8>),
    Int(Vec<u8>),
    Bool(bool),
    Bytes(Vec<u8>),
    FixedBytes(Vec<u8>),
    String(std::string::String),
    Array(Vec<ArgumentValue>),
    Tuple(Vec<ArgumentValue>),
}
```

Use JSON conversion when visibility rules need structured evaluation.

## Format Renderer Dispatch

Dispatch on `FieldFormat` in `format_value()`:

```rust
match fmt {
    FieldFormat::TokenAmount => format_token_amount(ctx, val, params),
    FieldFormat::Amount => format_amount(val),
    FieldFormat::Date => format_date(val),
    FieldFormat::Address => Ok(format_address(val)),
    FieldFormat::Calldata | FieldFormat::NftName => {
        ctx.warnings.push(format!("format {:?} not yet implemented", fmt));
        Ok(format_raw(val))
    }
}
```

When adding a new format:

1. Add the enum variant in `types/display.rs`
2. Add the serde rename
3. Add the match arm in `engine.rs`
4. Implement the formatter
5. Add tests

## BigUint Decimal Formatting

Format raw integer bytes using token decimals:

```rust
fn format_with_decimals(amount: &BigUint, decimals: u8) -> String {
    let s = amount.to_string();
    let decimals = decimals as usize;
    if s.len() <= decimals {
        // "0.000123" style
    } else {
        // Split at decimal point, trim trailing zeros
    }
}
```

## EIP-55 Checksum

Use mixed-case checksum formatting for Ethereum addresses:

```rust
fn eip55_checksum(addr: &[u8; 20]) -> String {
    let hex_addr = hex::encode(addr);
    format!("0x{result}")
}
```

## AddressBook Merge

Merge address labels from descriptor context and metadata:

```rust
impl AddressBook {
    pub fn from_descriptor(context: &Context, metadata: &Metadata) -> Self {
        // 1. deployment addresses -> contractName
        // 2. metadata.addressBook entries
    }
}
```

## Serde Patterns

Use untagged enums for flexible JSON:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DisplayField {
    Reference { reference: String },
    Group { #[serde(rename = "fieldGroup")] field_group: FieldGroup },
    Simple { path: String, label: String },
}
```

Use mixed-type visibility rules:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum VisibleRule {
    Bool(bool),
    Named(String),
    Condition(VisibilityCondition),
}
```

## UniFFI Guardrail

Keep UniFFI-specific attributes and wrapper logic in `uniffi_compat/`. Do not
spread UniFFI annotations across the rest of the library without a deliberate
API boundary decision.
