use super::ResolvedDescriptor;
use crate::eip712::TypedData;
use crate::types::descriptor::Descriptor;

pub(crate) fn safe_descriptor() -> Descriptor {
    let json = std::fs::read_to_string(format!(
        "{}/tests/fixtures/common-Safe.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("read Safe descriptor");
    Descriptor::from_json(&json).expect("parse Safe descriptor")
}

pub(crate) fn erc20_descriptor() -> Descriptor {
    let json = std::fs::read_to_string(format!(
        "{}/tests/fixtures/erc20-transfer.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("read ERC-20 descriptor");
    Descriptor::from_json(&json).expect("parse ERC-20 descriptor")
}

pub(crate) fn address_word(hex_addr: &str) -> Vec<u8> {
    let hex_str = hex_addr
        .strip_prefix("0x")
        .or_else(|| hex_addr.strip_prefix("0X"))
        .unwrap_or(hex_addr);
    let addr_bytes = hex::decode(hex_str).expect("valid hex address");
    let mut word = vec![0u8; 12];
    word.extend_from_slice(&addr_bytes);
    assert_eq!(word.len(), 32);
    word
}

pub(crate) fn uint_word(val: u128) -> Vec<u8> {
    let mut word = vec![0u8; 16];
    word.extend_from_slice(&val.to_be_bytes());
    assert_eq!(word.len(), 32);
    word
}

pub(crate) fn pad32(len: usize) -> usize {
    len.div_ceil(32) * 32
}

pub(crate) fn build_exec_transaction_calldata(to: &str, inner_calldata: &[u8]) -> Vec<u8> {
    let sig = crate::decoder::parse_signature(
        "execTransaction(address,uint256,bytes,uint8,uint256,uint256,uint256,address,address,bytes)",
    )
    .unwrap();

    let mut calldata = Vec::new();
    calldata.extend_from_slice(&sig.selector);
    calldata.extend_from_slice(&address_word(to));
    calldata.extend_from_slice(&uint_word(0));
    calldata.extend_from_slice(&uint_word(320));
    calldata.extend_from_slice(&uint_word(0));
    calldata.extend_from_slice(&uint_word(0));
    calldata.extend_from_slice(&uint_word(0));
    calldata.extend_from_slice(&uint_word(0));
    calldata.extend_from_slice(&[0u8; 32]);
    calldata.extend_from_slice(&[0u8; 32]);
    let data_offset = 320 + 32 + pad32(inner_calldata.len());
    calldata.extend_from_slice(&uint_word(data_offset as u128));

    calldata.extend_from_slice(&uint_word(inner_calldata.len() as u128));
    calldata.extend_from_slice(inner_calldata);
    let padding = pad32(inner_calldata.len()) - inner_calldata.len();
    calldata.extend_from_slice(&vec![0u8; padding]);
    calldata.extend_from_slice(&uint_word(0));

    calldata
}

pub(crate) fn build_erc20_transfer_calldata(to: &str, amount: u128) -> Vec<u8> {
    let sig = crate::decoder::parse_signature("transfer(address,uint256)").unwrap();
    let mut calldata = Vec::new();
    calldata.extend_from_slice(&sig.selector);
    calldata.extend_from_slice(&address_word(to));
    calldata.extend_from_slice(&uint_word(amount));
    calldata
}

pub(crate) fn permit2_descriptor(
    owner: &str,
    format_key: &str,
    extra_context: Option<serde_json::Value>,
) -> Descriptor {
    let mut eip712 = serde_json::json!({
        "deployments": [{
            "chainId": 1,
            "address": "0x000000000022d473030f116ddee9f6b43ac78ba3"
        }],
        "domain": {
            "name": "Permit2"
        }
    });

    if let Some(extra) = extra_context {
        let target = eip712.as_object_mut().expect("eip712 context object");
        for (key, value) in extra.as_object().expect("extra context object") {
            target.insert(key.clone(), value.clone());
        }
    }

    let descriptor = serde_json::json!({
        "context": { "eip712": eip712 },
        "metadata": {
            "owner": owner,
            "enums": {},
            "constants": {},
            "maps": {}
        },
        "display": {
            "definitions": {},
            "formats": {
                format_key: {
                    "intent": owner,
                    "fields": [{
                        "path": "spender",
                        "label": "Spender",
                        "format": "raw"
                    }]
                }
            }
        }
    });
    Descriptor::from_json(&descriptor.to_string()).expect("descriptor")
}

pub(crate) fn resolved_permit2_descriptor(
    owner: &str,
    format_key: &str,
    extra_context: Option<serde_json::Value>,
) -> ResolvedDescriptor {
    ResolvedDescriptor {
        descriptor: permit2_descriptor(owner, format_key, extra_context),
        chain_id: 1,
        address: "0x000000000022d473030f116ddee9f6b43ac78ba3".to_string(),
    }
}

pub(crate) fn exclusive_dutch_order_typed_data() -> TypedData {
    serde_json::from_value(serde_json::json!({
        "types": {
            "PermitWitnessTransferFrom": [
                { "name": "permitted", "type": "TokenPermissions" },
                { "name": "spender", "type": "address" },
                { "name": "nonce", "type": "uint256" },
                { "name": "deadline", "type": "uint256" },
                { "name": "witness", "type": "ExclusiveDutchOrder" }
            ],
            "TokenPermissions": [
                { "name": "token", "type": "address" },
                { "name": "amount", "type": "uint256" }
            ],
            "ExclusiveDutchOrder": [
                { "name": "info", "type": "OrderInfo" },
                { "name": "decayStartTime", "type": "uint256" },
                { "name": "decayEndTime", "type": "uint256" },
                { "name": "exclusiveFiller", "type": "address" },
                { "name": "exclusivityOverrideBps", "type": "uint256" },
                { "name": "inputToken", "type": "address" },
                { "name": "inputStartAmount", "type": "uint256" },
                { "name": "inputEndAmount", "type": "uint256" },
                { "name": "outputs", "type": "DutchOutput[]" }
            ],
            "OrderInfo": [
                { "name": "reactor", "type": "address" },
                { "name": "swapper", "type": "address" },
                { "name": "nonce", "type": "uint256" },
                { "name": "deadline", "type": "uint256" },
                { "name": "additionalValidationContract", "type": "address" },
                { "name": "additionalValidationData", "type": "bytes" }
            ],
            "DutchOutput": [
                { "name": "token", "type": "address" },
                { "name": "startAmount", "type": "uint256" },
                { "name": "endAmount", "type": "uint256" },
                { "name": "recipient", "type": "address" }
            ],
            "EIP712Domain": [
                { "name": "name", "type": "string" },
                { "name": "chainId", "type": "uint256" },
                { "name": "verifyingContract", "type": "address" }
            ]
        },
        "domain": {
            "name": "Permit2",
            "chainId": "1",
            "verifyingContract": "0x000000000022d473030f116ddee9f6b43ac78ba3"
        },
        "primaryType": "PermitWitnessTransferFrom",
        "message": {
            "permitted": {
                "token": "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2",
                "amount": "100000000000000"
            },
            "spender": "0x6000da47483062a0d734ba3dc7576ce6a0b645c4",
            "nonce": "1993349843209468715141873868895370562722298771555073489698616037339384894721",
            "deadline": "1774866877",
            "witness": {
                "info": {
                    "reactor": "0x6000da47483062a0d734ba3dc7576ce6a0b645c4",
                    "swapper": "0xbf01daf454dce008d3e2bfd47d5e186f71477253",
                    "nonce": "1993349843209468715141873868895370562722298771555073489698616037339384894721",
                    "deadline": "1774866877",
                    "additionalValidationContract": "0x0000000000000000000000000000000000000000",
                    "additionalValidationData": "0x"
                },
                "decayStartTime": "1774780477",
                "decayEndTime": "1774780477",
                "exclusiveFiller": "0x0000000000000000000000000000000000000000",
                "exclusivityOverrideBps": "0",
                "inputToken": "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2",
                "inputStartAmount": "100000000000000",
                "inputEndAmount": "100000000000000",
                "outputs": [{
                    "token": "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48",
                    "startAmount": "199179",
                    "endAmount": "199179",
                    "recipient": "0xbf01daf454dce008d3e2bfd47d5e186f71477253"
                }]
            }
        }
    }))
    .expect("typed data")
}
