//! ERC-7730 `encryption`: wallet-delegated decryption of encrypted field values.
//!
//! The library never decrypts anything itself — decryption needs a live
//! connection, a user signature and an access-control check, so it lives in the
//! wallet behind [`DataProvider::resolve_decrypted_value`]. Everything around
//! that call belongs here: parsing the declared `plaintextType`, fitting the
//! returned bytes to it, the fallback text and diagnostics, and the per-render
//! memoization that guarantees the wallet is asked at most once per encrypted
//! value.
//!
//! Both renderers share this path — `engine` turns the resolved plaintext into
//! an [`ArgumentValue`](crate::decoder::ArgumentValue), `eip712` into a
//! `serde_json::Value` — so calldata and typed data decrypt, validate and
//! degrade identically.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::error::Error;
use crate::outcome::{render_warning, FormatDiagnostic, RenderDiagnosticKind};
use crate::provider::DataProvider;
use crate::types::display::EncryptionParams;

/// Placeholder shown for an undecryptable field whose descriptor declares no
/// `fallbackLabel`. Deliberately generic — an encrypted field is not always an
/// amount ("Amount: [Encrypted]" reads naturally against the field's own label).
const DEFAULT_ENCRYPTED_PLACEHOLDER: &str = "[Encrypted]";

/// Width every integer plaintext is normalized to, matching the 32-byte words
/// the calldata decoder produces for `uintN`/`intN` arguments.
const WORD_BYTES: usize = 32;

/// Coarse plaintext category derived from a canonical Solidity `plaintextType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlainKind {
    Uint,
    Int,
    Address,
    Bool,
    Bytes,
    String,
}

/// A descriptor's declared `plaintextType`, parsed.
#[derive(Debug, Clone, Copy)]
struct PlaintextType {
    /// How the decrypted bytes are interpreted.
    kind: PlainKind,
    /// Widest plaintext the type can hold, in bytes. `None` for the unbounded
    /// types (`bytes`, `string`).
    max_bytes: Option<usize>,
}

/// Outcome of resolving one encrypted field value.
///
/// Only two, because the third possibility — a malformed `encryption` annotation
/// — is not an outcome of decrypting: it is a descriptor bug, reported as
/// [`Error::Descriptor`] by [`validate_annotation`] before any wallet is asked.
#[derive(Debug, Clone)]
pub(crate) enum Decryption {
    /// The wallet returned a plaintext that fits the declared `plaintextType`.
    Plaintext { kind: PlainKind, bytes: Vec<u8> },
    /// The value cannot be shown: render `text` and record `diagnostic`.
    Fallback {
        text: String,
        diagnostic: FormatDiagnostic,
    },
}

/// The text an undecryptable field renders: the descriptor's `fallbackLabel`,
/// or a generic placeholder when it declares none. Never the ciphertext.
pub(crate) fn fallback_text(enc: &EncryptionParams) -> String {
    enc.fallback_label
        .clone()
        .unwrap_or_else(|| DEFAULT_ENCRYPTED_PLACEHOLDER.to_string())
}

/// The value is real but unavailable — no decryptor, the wallet declined, or the
/// plaintext cannot be read faithfully. Recoverable: the field shows its fallback
/// text and the rest of the transaction formats as usual.
fn fallback(enc: &EncryptionParams, message: impl Into<String>) -> Decryption {
    Decryption::Fallback {
        text: fallback_text(enc),
        diagnostic: render_warning(RenderDiagnosticKind::DecryptionFailed, message),
    }
}

/// Parse a canonical Solidity `plaintextType` into its value category and fixed
/// byte width (`None` for dynamic `bytes`/`string`). Returns `None` for
/// non-canonical types (bare `uint`/`int`, `euint64`, `uint7`, `bytes33`), so
/// the caller can flag a descriptor error rather than silently guessing.
fn parse_plaintext_type(t: &str) -> Option<PlaintextType> {
    let bounded = |kind, max_bytes| {
        Some(PlaintextType {
            kind,
            max_bytes: Some(max_bytes),
        })
    };
    let unbounded = |kind| {
        Some(PlaintextType {
            kind,
            max_bytes: None,
        })
    };

    match t {
        "bool" => return bounded(PlainKind::Bool, 1),
        "address" => return bounded(PlainKind::Address, 20),
        "string" => return unbounded(PlainKind::String),
        "bytes" => return unbounded(PlainKind::Bytes),
        _ => {}
    }
    if let Some(rest) = t.strip_prefix("uint").or_else(|| t.strip_prefix("int")) {
        let signed = t.starts_with("int");
        let bits: usize = rest.parse().ok()?;
        if !(8..=256).contains(&bits) || !bits.is_multiple_of(8) {
            return None;
        }
        let kind = if signed {
            PlainKind::Int
        } else {
            PlainKind::Uint
        };
        return bounded(kind, bits / 8);
    }
    if let Some(n) = t.strip_prefix("bytes") {
        let size: usize = n.parse().ok()?;
        return (1..=32).contains(&size).then_some(PlaintextType {
            kind: PlainKind::Bytes,
            max_bytes: Some(size),
        });
    }
    None
}

fn trim_leading_zeros(bytes: &[u8]) -> &[u8] {
    let first = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
    &bytes[first..]
}

fn left_pad(bytes: &[u8], width: usize, pad: u8) -> Vec<u8> {
    let mut out = vec![pad; width.saturating_sub(bytes.len())];
    out.extend_from_slice(bytes);
    out
}

/// Reduce big-endian `bytes` to exactly `width` bytes, or `None` when the value
/// does not fit.
///
/// Integers, addresses and bools are big-endian quantities, so leading zeros
/// carry no value — a wallet returning a zero-padded 32-byte ABI word is
/// accepted as long as the value itself fits.
///
/// Only `0x00` is stripped, so a sign-extended negative (`-1` as `0xff…ff`) is
/// rejected where its zero-padded positive counterpart would pass. That is
/// intentional, matching the TypeScript library: a signed value is returned at
/// exactly its declared width, so padding it out to a wider word is already
/// outside the contract, and rejecting shows the fallback rather than a wrong
/// number.
fn declared_width_value(bytes: &[u8], width: usize) -> Option<Vec<u8>> {
    let significant = trim_leading_zeros(bytes);
    (significant.len() <= width).then(|| left_pad(significant, width, 0x00))
}

/// Widen a declared-width integer to a 32-byte two's-complement word — the
/// representation the calldata decoder produces for `uintN`/`intN`.
fn to_word(value: &[u8], kind: PlainKind) -> Vec<u8> {
    let negative = kind == PlainKind::Int && value.first().is_some_and(|b| b & 0x80 != 0);
    left_pad(value, WORD_BYTES, if negative { 0xff } else { 0x00 })
}

/// Fit decrypted bytes to the declared plaintext type, or `None` when the value
/// cannot be represented faithfully — a 32-byte value read as a `uint64` amount
/// would display a wildly wrong number.
///
/// For `bytesN` every byte is part of the value, so the raw length is what must
/// fit; the quantity types accept redundant leading zeros (see
/// [`declared_width_value`]).
///
/// Integers are then normalized to a 32-byte two's-complement word. Two's
/// complement is only meaningful at a known width, so this is what preserves the
/// declared one: `0xc8` under `int64` is +200, and reading those bytes as a
/// one-byte quantity would render -56. A negative is therefore only recognized
/// when the wallet returns it at its full declared width, as the callback
/// contract requires.
fn fit_plaintext_bytes(bytes: &[u8], ty: &PlaintextType) -> Option<Vec<u8>> {
    let Some(max) = ty.max_bytes else {
        return Some(bytes.to_vec());
    };
    if ty.kind == PlainKind::Bytes {
        return (bytes.len() <= max).then(|| bytes.to_vec());
    }

    let value = declared_width_value(bytes, max)?;
    Some(match ty.kind {
        PlainKind::Uint | PlainKind::Int => to_word(&value, ty.kind),
        _ => value,
    })
}

/// The scheme and parsed plaintext type an `encryption` annotation declares.
///
/// `Err` is a descriptor bug, not a decryption outcome: the annotation names no
/// scheme, no `plaintextType`, or a type that is not a canonical Solidity value
/// type. Fatal, like every other descriptor error here and like the TypeScript
/// library's `INVALID_DESCRIPTOR` — a wallet cannot know what a field holds, so
/// it must not present the transaction as understood.
fn parse_annotation(enc: &EncryptionParams) -> Result<(&str, &str, PlaintextType), Error> {
    let (Some(scheme), Some(plaintext_type)) =
        (enc.scheme.as_deref(), enc.plaintext_type.as_deref())
    else {
        return Err(Error::Descriptor(
            "field encryption requires both 'scheme' and 'plaintextType'".to_string(),
        ));
    };
    let parsed_type = parse_plaintext_type(plaintext_type).ok_or_else(|| {
        Error::Descriptor(format!(
            "unsupported encryption plaintextType '{plaintext_type}'"
        ))
    })?;
    Ok((scheme, plaintext_type, parsed_type))
}

/// Reject a malformed `encryption` annotation.
///
/// Pure — no wallet involved — so renderers apply it to every encrypted field,
/// including ones whose value is never read (a field hidden by its `visible`
/// rule). A descriptor bug is then caught wherever it appears, without asking
/// the wallet to decrypt anything that is not displayed.
pub(crate) fn validate_annotation(enc: &EncryptionParams) -> Result<(), Error> {
    parse_annotation(enc).map(|_| ())
}

/// Ask the wallet to decrypt `ciphertext` and re-read the answer as the
/// descriptor's declared `plaintextType`.
///
/// Recoverable failures — no decryptor, the wallet declined, malformed hex, a
/// plaintext too wide for its type — degrade to the field's fallback text with a
/// `DecryptionFailed` diagnostic. A malformed annotation is `Err`: fatal.
async fn decrypt(
    data_provider: &dyn DataProvider,
    chain_id: Option<u64>,
    contract_address: Option<&str>,
    ciphertext: &[u8],
    enc: &EncryptionParams,
) -> Result<Decryption, Error> {
    let (scheme, plaintext_type, parsed_type) = parse_annotation(enc)?;

    // Access control is per chain, so a decryption request without one cannot be
    // made — an EIP-712 domain may legitimately omit `chainId`.
    let Some(chain_id) = chain_id else {
        return Ok(fallback(
            enc,
            format!("Cannot decrypt '{scheme}' value without a chain id"),
        ));
    };

    let encrypted_hex = format!("0x{}", hex::encode(ciphertext));
    let Some(hex_value) = data_provider
        .resolve_decrypted_value(chain_id, &encrypted_hex, scheme, contract_address)
        .await
    else {
        // One message for both causes: unlike the TypeScript library, whose
        // callback is an optional property, a Rust `DataProvider` always has the
        // method — its default returns `None` — so "no decryptor" and "the wallet
        // declined" are indistinguishable here.
        return Ok(fallback(
            enc,
            format!("Could not decrypt '{scheme}' value (no decryptor, or the wallet declined)"),
        ));
    };

    let stripped = hex_value.strip_prefix("0x").unwrap_or(&hex_value);
    let Ok(bytes) = hex::decode(stripped) else {
        return Ok(fallback(
            enc,
            format!("Decrypted '{scheme}' value '{hex_value}' is not valid hex"),
        ));
    };

    let Some(bytes) = fit_plaintext_bytes(&bytes, &parsed_type) else {
        return Ok(fallback(
            enc,
            format!(
                "Decrypted '{scheme}' value is {} bytes, too wide for '{plaintext_type}'",
                bytes.len()
            ),
        ));
    };

    Ok(Decryption::Plaintext {
        kind: parsed_type.kind,
        bytes,
    })
}

/// One ciphertext under one annotation. The declared `plaintextType` is part of
/// the key because it decides how the returned bytes are read.
type DecryptionKey = (Vec<u8>, String, String);

/// Decryptions already resolved in this render frame.
#[derive(Default)]
pub(crate) struct DecryptionCache {
    entries: Mutex<HashMap<DecryptionKey, Decryption>>,
}

impl DecryptionCache {
    /// Decrypt `ciphertext`, reusing an earlier answer for the same
    /// (ciphertext, scheme, plaintextType).
    ///
    /// Decryption is wallet-side and typically interactive (a signature prompt
    /// plus an access-control check), so it must happen at most once per
    /// encrypted value. A field is read up to three times per render —
    /// conditional visibility, its display entry, and `interpolatedIntent` — and
    /// without memoization the wallet would be asked once per read: the user
    /// gets repeated prompts and, if any answer differs, the entry and the
    /// intent disagree about what is being signed. The diagnostic is recorded on
    /// the first resolution only, for the same reason.
    pub(crate) async fn resolve(
        &self,
        data_provider: &dyn DataProvider,
        chain_id: Option<u64>,
        contract_address: Option<&str>,
        ciphertext: &[u8],
        enc: &EncryptionParams,
        warnings: &mut Vec<FormatDiagnostic>,
    ) -> Result<Decryption, Error> {
        let key = (
            ciphertext.to_vec(),
            enc.scheme.clone().unwrap_or_default(),
            enc.plaintext_type.clone().unwrap_or_default(),
        );
        if let Some(cached) = self.get(&key) {
            return Ok(cached);
        }

        let outcome = decrypt(data_provider, chain_id, contract_address, ciphertext, enc).await?;
        if let Decryption::Fallback { diagnostic, .. } = &outcome {
            warnings.push(diagnostic.clone());
        }
        self.store(key, &outcome);
        Ok(outcome)
    }

    fn get(&self, key: &DecryptionKey) -> Option<Decryption> {
        self.entries.lock().ok()?.get(key).cloned()
    }

    fn store(&self, key: DecryptionKey, outcome: &Decryption) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(key, outcome.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ty(t: &str) -> PlaintextType {
        parse_plaintext_type(t).expect("canonical plaintextType")
    }

    fn fit(hex_bytes: &[u8], t: &str) -> Option<Vec<u8>> {
        fit_plaintext_bytes(hex_bytes, &ty(t))
    }

    #[test]
    fn rejects_non_canonical_plaintext_types() {
        for t in ["uint", "int", "euint64", "uint7", "uint264", "bytes33", ""] {
            assert!(parse_plaintext_type(t).is_none(), "{t} should be rejected");
        }
    }

    #[test]
    fn minimal_positive_encoding_keeps_the_declared_width() {
        // 0xc8 under int64 is +200, not the one-byte -56.
        let fitted = fit(&[0xc8], "int64").expect("fits int64");
        assert_eq!(fitted, left_pad(&[0xc8], WORD_BYTES, 0x00));
    }

    #[test]
    fn full_width_negative_int_is_sign_extended_to_a_word() {
        // int64 -56, two's complement at the declared width.
        let fitted =
            fit(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xc8], "int64").expect("fits int64");
        assert_eq!(fitted, left_pad(&[0xc8], WORD_BYTES, 0xff));
    }

    #[test]
    fn a_shortened_negative_reads_as_the_positive_it_encodes() {
        // `0xff` is +255 under int64, not -1: only the declared width decides
        // the sign, so a negative has to arrive at that full width.
        assert_eq!(
            fit(&[0xff], "int64"),
            Some(left_pad(&[0xff], WORD_BYTES, 0))
        );
    }

    #[test]
    fn zero_padded_value_is_accepted_when_it_fits() {
        let word = left_pad(&[0x0f, 0x42, 0x40], WORD_BYTES, 0x00);
        assert_eq!(fit(&word, "uint64"), Some(word));
    }

    #[test]
    fn over_wide_values_are_rejected() {
        // 9 significant bytes for a declared uint64.
        assert!(fit(&[1, 0, 0, 0, 0, 0, 0x0f, 0x42, 0x40], "uint64").is_none());
        // Only 0x00 is stripped, so a value padded out with 0xff is over-wide
        // whether or not the declared type is signed — a wallet returns a
        // negative at exactly its declared width.
        for t in ["uint64", "int64"] {
            assert!(fit(&left_pad(&[0xc8], WORD_BYTES, 0xff), t).is_none());
        }
        // For bytesN every byte counts, so padding does not help.
        assert!(fit(&left_pad(&[1], WORD_BYTES, 0x00), "bytes8").is_none());
    }

    #[test]
    fn address_and_bool_are_reduced_to_their_own_width() {
        let addr = left_pad(&[0xab; 20], WORD_BYTES, 0x00);
        assert_eq!(fit(&addr, "address"), Some(vec![0xab; 20]));
        assert_eq!(fit(&[0, 0, 1], "bool"), Some(vec![1]));
    }

    #[test]
    fn dynamic_types_are_taken_as_returned() {
        assert_eq!(fit(b"hello", "string"), Some(b"hello".to_vec()));
        assert_eq!(fit(&[1, 2, 3], "bytes"), Some(vec![1, 2, 3]));
    }
}
