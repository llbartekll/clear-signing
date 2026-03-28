---
name: rust-quality
description: Write idiomatic Rust following this project's patterns and conventions
---

# Rust Quality

Write idiomatic Rust code that follows this project's established patterns. This
library (ERC-7730 v2 clear signing) is the standalone version of yttrium's
clear signing module, adapted for Rust 2021 edition with a single-crate
architecture. Async (`tokio`/`reqwest`) is used behind the `github-registry`
feature for HTTP descriptor resolution and UniFFI async exports.

## Use This Skill For

- Writing or modifying `.rs` files
- Adding new modules, error types, traits, or tests
- Implementing format renderers, decoders, or resolvers

## Code Style

### Imports

Standard `use` per item, not block `use { }` style. Group by:

1. External crates
2. Crate-internal imports
3. Standard library, only when needed explicitly

```rust
use tiny_keccak::{Hasher, Keccak};

use crate::error::DecodeError;
```

### Error Handling

- Use `thiserror` with `#[derive(Debug, Error)]`
- Use descriptive `#[error("...")]` messages with context
- Prefer structured variants with named fields for multi-value errors
- Use `#[from]` for automatic conversion from sub-errors
- Never use `.unwrap()` in library code; use `Result` and `?`
- Convert external errors with `.map_err(|e| ...)`

```rust
#[derive(Debug, Error)]
pub enum MyError {
    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("not found: key={key}, context={context}")]
    NotFound { key: String, context: String },

    #[error("decode error: {0}")]
    Decode(#[from] DecodeError),
}
```

### Traits And Pluggability

- Define traits for extensible behavior like `DescriptorSource` and `TokenSource`
- Provide static or in-memory test implementations like `StaticSource`
- Implement `Default` for test sources via `new()` delegation
- Use `&dyn Trait` for trait object parameters; add `Send + Sync` only when async requires it

```rust
pub trait MySource {
    fn lookup(&self, key: &str) -> Option<MyResult>;
}

pub struct StaticMySource {
    data: HashMap<String, MyResult>,
}

impl Default for StaticMySource {
    fn default() -> Self { Self::new() }
}
```

### Types And Data Structures

- Add `#[derive(Debug, Clone)]` on all public types
- Add `#[derive(Debug, Clone, Serialize, Deserialize)]` for JSON roundtrips
- Use `#[serde(untagged)]` for flexible enums like `DisplayField` and `VisibleRule`
- Use `#[serde(rename_all = "camelCase")]` for JSON field mapping
- Use `Box` in recursive enums where needed
- Use lifetime-bound context structs for pipeline state

### Pipeline Pattern

Pass a mutable `RenderContext<'a>` through rendering pipelines to accumulate
warnings and carry shared state:

```rust
struct RenderContext<'a> {
    descriptor: &'a Descriptor,
    decoded: &'a DecodedArguments,
    chain_id: u64,
    token_source: &'a dyn TokenSource,
    warnings: Vec<String>,
}
```

### Module Structure

- One module per concern like `decoder.rs`, `engine.rs`, `resolver.rs`
- Types live under `types/` with submodules
- Public API is re-exported from `lib.rs`
- Use `pub(crate)` for internal helpers shared between modules

### Tests

- Keep inline `#[cfg(test)] mod tests { ... }` at the bottom of each module
- Use `use super::*;` in test modules
- Add helper functions for building test data
- Use explicit pattern matching in assertions
- Test both success and error paths

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn test_descriptor_json() -> &'static str {
        r#"{ ... }"#
    }

    #[test]
    fn test_my_feature() {
        let descriptor = Descriptor::from_json(test_descriptor_json()).unwrap();
        let result = my_function(&descriptor).unwrap();
        assert_eq!(result.field, "expected");
    }
}
```

## Formatting And Linting

```sh
cargo fmt
cargo clippy -- -D warnings
cargo test
```

Use stable defaults. Do not introduce `rustfmt.toml` overrides.

## Validation Checklist

- `cargo fmt --check` passes
- `cargo clippy -- -D warnings` passes
- `cargo test` passes
- No `.unwrap()` in library code
- Error types use `thiserror`
- Public types have `Debug` and `Clone`
- New public modules are re-exported from `lib.rs`
- Tests cover both success and error paths

## Anti-Patterns

Do not:

- Use `.unwrap()` or `.expect()` in library code
- Use `eyre`, `anyhow`, or `Box<dyn Error>` instead of structured errors
- Add `async` or `tokio` outside the `github-registry` feature gate
- Use block import style `use { foo, bar }`
- Add `Send + Sync` bounds unless async requires it
- Create `rustfmt.toml`
- Use nightly-only formatting workflows
- Skip `#[derive(Debug, Clone)]` on public types
- Add UniFFI attributes outside `uniffi_compat/`

## References

Read [REFERENCE.md](./REFERENCE.md) for project-specific Rust patterns.
