//! Synthesize ERC-7730 descriptors on-the-fly for standard ERC-20 functions
//! when the wallet supplies token metadata.
//!
//! Edge cases handled here:
//! - `amount = 2^256 - 1` (DeFi infinite approval) → `tokenAmount` params carry
//!   `threshold` + `message`, engine renders "Unlimited {ticker}".
//! - Address fields equal to `tx.from` → `addressName` params carry
//!   `senderAddress: "@.from"`, engine renders "Sender" instead of the address.
//!
//! Considered but deferred to wallet UX: `approve(spender, 0)` renders as
//! "Approve {spender} to spend 0 {ticker}" which is accurate but doesn't
//! signal "you are revoking approval". A spec-compliant single descriptor
//! cannot switch its intent based on amount; the wallet's display layer is
//! the right place to override.

use std::collections::HashMap;

use crate::token::TokenMeta;
use crate::types::context::{ContractContext, ContractInfo, Deployment, DescriptorContext};
use crate::types::descriptor::Descriptor;
use crate::types::display::{
    DescriptorDisplay, DisplayField, DisplayFormat, FieldFormat, FormatParams, SenderAddress,
    VisibleRule,
};
use crate::types::metadata::{Metadata, TokenInfo};

use super::source::ResolvedDescriptor;

struct StandardFn {
    selector: [u8; 4],
    format_key: &'static str,
    intent: &'static str,
    interpolated_intent: &'static str,
    fields: &'static [SynthField],
}

struct SynthField {
    path: &'static str,
    label: &'static str,
    format: SynthFieldFormat,
}

#[derive(Clone, Copy)]
enum SynthFieldFormat {
    AddressName,
    TokenAmount,
}

const TRANSFER_FIELDS: &[SynthField] = &[
    SynthField {
        path: "to",
        label: "To",
        format: SynthFieldFormat::AddressName,
    },
    SynthField {
        path: "amount",
        label: "Amount",
        format: SynthFieldFormat::TokenAmount,
    },
];

const APPROVE_FIELDS: &[SynthField] = &[
    SynthField {
        path: "spender",
        label: "Spender",
        format: SynthFieldFormat::AddressName,
    },
    SynthField {
        path: "amount",
        label: "Amount",
        format: SynthFieldFormat::TokenAmount,
    },
];

const TRANSFER_FROM_FIELDS: &[SynthField] = &[
    SynthField {
        path: "from",
        label: "From",
        format: SynthFieldFormat::AddressName,
    },
    SynthField {
        path: "to",
        label: "To",
        format: SynthFieldFormat::AddressName,
    },
    SynthField {
        path: "amount",
        label: "Amount",
        format: SynthFieldFormat::TokenAmount,
    },
];

const STANDARD_ERC20_FNS: &[StandardFn] = &[
    StandardFn {
        selector: [0xa9, 0x05, 0x9c, 0xbb],
        format_key: "transfer(address to,uint256 amount)",
        intent: "Transfer tokens",
        interpolated_intent: "Transfer {amount} to {to}",
        fields: TRANSFER_FIELDS,
    },
    StandardFn {
        selector: [0x09, 0x5e, 0xa7, 0xb3],
        format_key: "approve(address spender,uint256 amount)",
        intent: "Approve token spending",
        interpolated_intent: "Approve {spender} to spend {amount}",
        fields: APPROVE_FIELDS,
    },
    StandardFn {
        selector: [0x23, 0xb8, 0x72, 0xdd],
        format_key: "transferFrom(address from,address to,uint256 amount)",
        intent: "Transfer tokens",
        interpolated_intent: "Transfer {amount} from {from} to {to}",
        fields: TRANSFER_FROM_FIELDS,
    },
];

/// True if the 4-byte selector is a standard ERC-20 selector handled by [`synthesize_erc20`].
pub(crate) fn is_erc20_selector(selector: [u8; 4]) -> bool {
    STANDARD_ERC20_FNS.iter().any(|f| f.selector == selector)
}

/// Build a synthetic ERC-7730 descriptor covering a single standard ERC-20 selector,
/// using on-chain metadata supplied by the wallet.
///
/// Returns `None` if the selector is not a recognized standard ERC-20 function.
pub(crate) fn synthesize_erc20(
    chain_id: u64,
    address: &str,
    selector: [u8; 4],
    meta: &TokenMeta,
) -> Option<ResolvedDescriptor> {
    let fn_def = STANDARD_ERC20_FNS.iter().find(|f| f.selector == selector)?;

    let display_format = DisplayFormat {
        id: None,
        intent: Some(serde_json::Value::String(fn_def.intent.to_string())),
        interpolated_intent: Some(fn_def.interpolated_intent.to_string()),
        fields: fn_def.fields.iter().map(build_field).collect(),
        excluded: Vec::new(),
    };

    let mut formats = HashMap::new();
    formats.insert(fn_def.format_key.to_string(), display_format);

    let descriptor = Descriptor {
        schema: None,
        includes: None,
        context: DescriptorContext::Contract(ContractContext {
            id: None,
            contract: ContractInfo {
                deployments: vec![Deployment {
                    chain_id,
                    address: address.to_string(),
                }],
                factory: None,
            },
        }),
        metadata: Metadata {
            owner: None,
            info: None,
            token: Some(TokenInfo {
                name: Some(meta.name.clone()),
                ticker: Some(meta.symbol.clone()),
                decimals: Some(meta.decimals),
            }),
            enums: HashMap::new(),
            constants: HashMap::new(),
            contract_name: Some(meta.name.clone()),
            maps: HashMap::new(),
        },
        display: DescriptorDisplay {
            definitions: HashMap::new(),
            formats,
        },
    };

    Some(ResolvedDescriptor {
        descriptor,
        chain_id,
        address: address.to_string(),
    })
}

fn build_field(spec: &SynthField) -> DisplayField {
    let (format, params) = match spec.format {
        SynthFieldFormat::AddressName => (
            FieldFormat::AddressName,
            Some(address_name_params_sender("@.from")),
        ),
        SynthFieldFormat::TokenAmount => {
            (FieldFormat::TokenAmount, Some(token_amount_params("@.to")))
        }
    };
    DisplayField::Simple {
        path: Some(spec.path.to_string()),
        label: spec.label.to_string(),
        value: None,
        format: Some(format),
        params,
        separator: None,
        visible: VisibleRule::Always,
    }
}

/// uint256 max — sentinel value used by every common DeFi "infinite approval" flow
/// (1inch, Uniswap, OpenSea, Permit2 aggregators). Engine comparison is `>=`, so
/// the exact-max case is included.
const UINT256_MAX_HEX: &str = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

fn empty_format_params() -> FormatParams {
    FormatParams {
        token_path: None,
        token: None,
        native_currency_address: None,
        chain_id: None,
        chain_id_path: None,
        enum_path: None,
        ref_path: None,
        map_reference: None,
        threshold: None,
        message: None,
        base: None,
        decimals: None,
        prefix: None,
        encryption: None,
        encoding: None,
        selector_path: None,
        selector: None,
        callee_path: None,
        callee: None,
        amount_path: None,
        amount: None,
        spender_path: None,
        spender: None,
        types: None,
        sources: None,
        sender_address: None,
        collection_path: None,
        collection: None,
    }
}

fn token_amount_params(token_path: &str) -> FormatParams {
    FormatParams {
        token_path: Some(token_path.to_string()),
        threshold: Some(UINT256_MAX_HEX.to_string()),
        message: Some("Unlimited".to_string()),
        ..empty_format_params()
    }
}

fn address_name_params_sender(sender_path: &str) -> FormatParams {
    FormatParams {
        sender_address: Some(SenderAddress::Single(sender_path.to_string())),
        ..empty_format_params()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::display::intent_as_string;

    fn usdc_meta() -> TokenMeta {
        TokenMeta {
            symbol: "USDC".to_string(),
            decimals: 6,
            name: "USD Coin".to_string(),
        }
    }

    const USDC_ADDR: &str = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";

    fn assert_intent_pair(
        resolved: &ResolvedDescriptor,
        format_key: &str,
        intent: &str,
        interpolated: &str,
    ) {
        let format = resolved
            .descriptor
            .display
            .formats
            .get(format_key)
            .expect("format key present");
        let intent_value = format.intent.as_ref().expect("intent present");
        assert_eq!(intent_as_string(intent_value), intent);
        assert_eq!(format.interpolated_intent.as_deref(), Some(interpolated));
    }

    #[test]
    fn synthesize_transfer_carries_both_intent_fields() {
        let resolved = synthesize_erc20(1, USDC_ADDR, [0xa9, 0x05, 0x9c, 0xbb], &usdc_meta())
            .expect("transfer synth");
        assert_intent_pair(
            &resolved,
            "transfer(address to,uint256 amount)",
            "Transfer tokens",
            "Transfer {amount} to {to}",
        );
    }

    #[test]
    fn synthesize_approve_carries_both_intent_fields() {
        let resolved = synthesize_erc20(1, USDC_ADDR, [0x09, 0x5e, 0xa7, 0xb3], &usdc_meta())
            .expect("approve synth");
        assert_intent_pair(
            &resolved,
            "approve(address spender,uint256 amount)",
            "Approve token spending",
            "Approve {spender} to spend {amount}",
        );
    }

    #[test]
    fn synthesize_transfer_from_carries_both_intent_fields() {
        let resolved = synthesize_erc20(1, USDC_ADDR, [0x23, 0xb8, 0x72, 0xdd], &usdc_meta())
            .expect("transferFrom synth");
        assert_intent_pair(
            &resolved,
            "transferFrom(address from,address to,uint256 amount)",
            "Transfer tokens",
            "Transfer {amount} from {from} to {to}",
        );
    }

    #[test]
    fn synthesize_populates_token_metadata() {
        let resolved = synthesize_erc20(1, USDC_ADDR, [0xa9, 0x05, 0x9c, 0xbb], &usdc_meta())
            .expect("transfer synth");
        let token = resolved.descriptor.metadata.token.expect("token info");
        assert_eq!(token.ticker.as_deref(), Some("USDC"));
        assert_eq!(token.decimals, Some(6));
        assert_eq!(token.name.as_deref(), Some("USD Coin"));
        assert_eq!(
            resolved.descriptor.metadata.contract_name.as_deref(),
            Some("USD Coin")
        );
    }

    #[test]
    fn synthesize_uses_token_amount_with_at_to_path() {
        let resolved = synthesize_erc20(1, USDC_ADDR, [0xa9, 0x05, 0x9c, 0xbb], &usdc_meta())
            .expect("transfer synth");
        let format = resolved
            .descriptor
            .display
            .formats
            .get("transfer(address to,uint256 amount)")
            .expect("format present");
        // last field is the amount
        let DisplayField::Simple {
            format: fmt,
            params,
            path,
            ..
        } = format.fields.last().unwrap()
        else {
            panic!("expected Simple field");
        };
        assert!(matches!(fmt, Some(FieldFormat::TokenAmount)));
        assert_eq!(path.as_deref(), Some("amount"));
        let params = params.as_ref().expect("params present");
        assert_eq!(params.token_path.as_deref(), Some("@.to"));
    }

    /// Every synth's amount field carries `threshold` + `message` so the
    /// engine renders the 2^256-1 "infinite approval" pattern as
    /// "Unlimited {ticker}" rather than the 70-digit decimal expansion.
    #[test]
    fn synthesize_amount_field_carries_threshold_and_message() {
        let cases = [
            (
                [0xa9, 0x05, 0x9c, 0xbb],
                "transfer(address to,uint256 amount)",
            ),
            (
                [0x09, 0x5e, 0xa7, 0xb3],
                "approve(address spender,uint256 amount)",
            ),
            (
                [0x23, 0xb8, 0x72, 0xdd],
                "transferFrom(address from,address to,uint256 amount)",
            ),
        ];

        for (selector, format_key) in cases {
            let resolved = synthesize_erc20(1, USDC_ADDR, selector, &usdc_meta()).expect("synth");
            let format = resolved
                .descriptor
                .display
                .formats
                .get(format_key)
                .expect("format present");
            let DisplayField::Simple { params, .. } = format.fields.last().expect("amount field")
            else {
                panic!("expected Simple field");
            };
            let params = params.as_ref().expect("params present");
            assert_eq!(
                params.threshold.as_deref(),
                Some("0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"),
                "format_key={format_key}"
            );
            assert_eq!(
                params.message.as_deref(),
                Some("Unlimited"),
                "format_key={format_key}"
            );
        }
    }

    /// Every `addressName` field in every synth carries `senderAddress: "@.from"`
    /// so the engine renders "Sender" when the field address equals `tx.from`.
    #[test]
    fn synthesize_address_fields_carry_sender_address_from() {
        let cases = [
            (
                [0xa9, 0x05, 0x9c, 0xbb],
                "transfer(address to,uint256 amount)",
            ),
            (
                [0x09, 0x5e, 0xa7, 0xb3],
                "approve(address spender,uint256 amount)",
            ),
            (
                [0x23, 0xb8, 0x72, 0xdd],
                "transferFrom(address from,address to,uint256 amount)",
            ),
        ];

        for (selector, format_key) in cases {
            let resolved = synthesize_erc20(1, USDC_ADDR, selector, &usdc_meta()).expect("synth");
            let format = resolved
                .descriptor
                .display
                .formats
                .get(format_key)
                .expect("format present");

            let mut address_fields_checked = 0;
            for field in &format.fields {
                let DisplayField::Simple {
                    format: fmt,
                    params,
                    ..
                } = field
                else {
                    continue;
                };
                if !matches!(fmt, Some(FieldFormat::AddressName)) {
                    continue;
                }
                let params = params.as_ref().expect("params present on address field");
                match params.sender_address.as_ref().expect("sender_address set") {
                    SenderAddress::Single(path) => {
                        assert_eq!(path.as_str(), "@.from", "format_key={format_key}")
                    }
                    SenderAddress::Multiple(_) => panic!("expected Single variant"),
                }
                address_fields_checked += 1;
            }
            assert!(
                address_fields_checked > 0,
                "format {format_key} should have at least one AddressName field"
            );
        }
    }

    #[test]
    fn synthesize_returns_none_for_unknown_selector() {
        let resolved = synthesize_erc20(1, USDC_ADDR, [0xff, 0xff, 0xff, 0xff], &usdc_meta());
        assert!(resolved.is_none());
    }

    #[test]
    fn is_erc20_selector_recognizes_standard_three() {
        assert!(is_erc20_selector([0xa9, 0x05, 0x9c, 0xbb]));
        assert!(is_erc20_selector([0x09, 0x5e, 0xa7, 0xb3]));
        assert!(is_erc20_selector([0x23, 0xb8, 0x72, 0xdd]));
        assert!(!is_erc20_selector([0x00, 0x00, 0x00, 0x00]));
        assert!(!is_erc20_selector([0xd0, 0xe3, 0x0d, 0xb0])); // deposit()
    }

    #[test]
    fn descriptor_serializes_with_correct_top_level_shape() {
        let resolved = synthesize_erc20(1, USDC_ADDR, [0x09, 0x5e, 0xa7, 0xb3], &usdc_meta())
            .expect("approve synth");
        let json = serde_json::to_value(&resolved.descriptor).expect("serialize");
        assert!(json["context"]["contract"]["deployments"][0]["chainId"] == 1);
        assert!(json["metadata"]["token"]["ticker"] == "USDC");
        assert_eq!(
            json["display"]["formats"]["approve(address spender,uint256 amount)"]["intent"],
            "Approve token spending"
        );
        assert_eq!(
            json["display"]["formats"]["approve(address spender,uint256 amount)"]
                ["interpolatedIntent"],
            "Approve {spender} to spend {amount}"
        );
    }
}
