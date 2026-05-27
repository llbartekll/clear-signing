use clear_signing::eip712::{TypedData, TypedDataDomain, TypedDataField};
use clear_signing::resolver::ResolvedDescriptor;
use clear_signing::types::descriptor::Descriptor;
use clear_signing::{format_calldata, format_typed_data, EmptyDataProvider, TransactionContext};

const ERC20_DESCRIPTOR_WITH_CONTRACT_NAME: &str = r#"{
    "context": {
        "contract": {
            "deployments": [{"chainId": 1, "address": "0xdac17f958d2ee523a2206206994597c13d831ec7"}]
        }
    },
    "metadata": {
        "owner": "Tether",
        "contractName": "USDT"
    },
    "display": {
        "definitions": {},
        "formats": {
            "transfer(address _to, uint256 _value)": {
                "intent": "Send",
                "fields": [
                    {"path": "_to", "label": "To", "format": "raw"},
                    {"path": "_value", "label": "Amount", "format": "raw"}
                ]
            }
        }
    }
}"#;

const ERC20_DESCRIPTOR_WITHOUT_CONTRACT_NAME: &str = r#"{
    "context": {
        "contract": {
            "deployments": [{"chainId": 1, "address": "0xdac17f958d2ee523a2206206994597c13d831ec7"}]
        }
    },
    "metadata": {
        "owner": "Tether"
    },
    "display": {
        "definitions": {},
        "formats": {
            "transfer(address _to, uint256 _value)": {
                "intent": "Send",
                "fields": [
                    {"path": "_to", "label": "To", "format": "raw"},
                    {"path": "_value", "label": "Amount", "format": "raw"}
                ]
            }
        }
    }
}"#;

const PERMIT_DESCRIPTOR_WITH_CONTRACT_NAME: &str = r#"{
    "context": {
        "eip712": {
            "deployments": [{"chainId": 1, "address": "0xabc"}]
        }
    },
    "metadata": {
        "owner": "Tether",
        "contractName": "USDT"
    },
    "display": {
        "definitions": {},
        "formats": {
            "Permit(address spender,uint256 value)": {
                "intent": "Permit token spending",
                "fields": [
                    {"path": "spender", "label": "Spender", "format": "raw"},
                    {"path": "value", "label": "Amount", "format": "raw"}
                ]
            }
        }
    }
}"#;

fn wrap_rd(descriptor: Descriptor, chain_id: u64, address: &str) -> Vec<ResolvedDescriptor> {
    vec![ResolvedDescriptor {
        descriptor,
        chain_id,
        address: address.to_lowercase(),
    }]
}

fn transfer_calldata() -> Vec<u8> {
    let selector = [0xa9, 0x05, 0x9c, 0xbb];
    let mut data = Vec::with_capacity(4 + 64);
    data.extend_from_slice(&selector);
    let mut to_word = [0u8; 32];
    to_word[31] = 1;
    data.extend_from_slice(&to_word);
    let mut amount_word = [0u8; 32];
    amount_word[31] = 0x64;
    data.extend_from_slice(&amount_word);
    data
}

fn permit_typed_data() -> TypedData {
    TypedData {
        types: std::collections::HashMap::from([(
            "Permit".to_string(),
            vec![
                TypedDataField {
                    name: "spender".to_string(),
                    field_type: "address".to_string(),
                },
                TypedDataField {
                    name: "value".to_string(),
                    field_type: "uint256".to_string(),
                },
            ],
        )]),
        primary_type: "Permit".to_string(),
        domain: TypedDataDomain {
            name: Some("USDT".to_string()),
            version: None,
            chain_id: Some(1),
            verifying_contract: Some("0xabc".to_string()),
            salt: None,
            extra: std::collections::HashMap::new(),
        },
        container: None,
        message: serde_json::json!({
            "spender": "0x1234567890123456789012345678901234567890",
            "value": "1000000"
        }),
    }
}

#[tokio::test]
async fn contract_name_surfaced_calldata_when_present() {
    let descriptor = Descriptor::from_json(ERC20_DESCRIPTOR_WITH_CONTRACT_NAME).unwrap();
    let to = "0xdac17f958d2ee523a2206206994597c13d831ec7";
    let descriptors = wrap_rd(descriptor, 1, to);
    let calldata = transfer_calldata();
    let tx = TransactionContext {
        chain_id: 1,
        to,
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };

    let result = format_calldata(&descriptors, &tx, &EmptyDataProvider)
        .await
        .unwrap();

    assert_eq!(result.contract_name.as_deref(), Some("USDT"));
    assert_eq!(result.owner.as_deref(), Some("Tether"));
}

#[tokio::test]
async fn contract_name_none_calldata_when_absent() {
    let descriptor = Descriptor::from_json(ERC20_DESCRIPTOR_WITHOUT_CONTRACT_NAME).unwrap();
    let to = "0xdac17f958d2ee523a2206206994597c13d831ec7";
    let descriptors = wrap_rd(descriptor, 1, to);
    let calldata = transfer_calldata();
    let tx = TransactionContext {
        chain_id: 1,
        to,
        calldata: &calldata,
        value: None,
        from: None,
        implementation_address: None,
    };

    let result = format_calldata(&descriptors, &tx, &EmptyDataProvider)
        .await
        .unwrap();

    assert!(result.contract_name.is_none());
    assert_eq!(result.owner.as_deref(), Some("Tether"));
}

#[tokio::test]
async fn contract_name_surfaced_typed_data_when_present() {
    let descriptor = Descriptor::from_json(PERMIT_DESCRIPTOR_WITH_CONTRACT_NAME).unwrap();
    let typed_data = permit_typed_data();
    let descriptors = wrap_rd(descriptor, 1, "0xabc");

    let result = format_typed_data(&descriptors, &typed_data, &EmptyDataProvider)
        .await
        .unwrap();

    assert_eq!(result.contract_name.as_deref(), Some("USDT"));
    assert_eq!(result.owner.as_deref(), Some("Tether"));
}
