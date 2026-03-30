use std::collections::HashMap;

use num_bigint::{BigInt, BigUint, Sign};
use tiny_keccak::{Hasher, Keccak};

use crate::eip712::{
    coerce_typed_address_string, encode_type_for_type, parse_typed_biguint_value, TypedData,
    TypedDataDomain, TypedDataField,
};
use crate::error::Error;
use crate::types::descriptor::Descriptor;

pub(crate) fn validate_descriptor_eip712_context(
    descriptor: &Descriptor,
    data: &TypedData,
) -> Result<(), Error> {
    let crate::types::context::DescriptorContext::Eip712(ctx) = &descriptor.context else {
        return Err(Error::Descriptor(
            "typed-data descriptor must use eip712 context".to_string(),
        ));
    };

    if let Some(expected_domain) = ctx.eip712.domain.as_ref() {
        if let Some(expected_name) = expected_domain.name.as_deref() {
            match data.domain.name.as_deref() {
                Some(actual_name) if actual_name == expected_name => {}
                Some(_) => {
                    return Err(Error::Descriptor(
                        "descriptor eip712.domain.name mismatch".to_string(),
                    ));
                }
                None => {
                    return Err(Error::Descriptor(
                        "descriptor eip712.domain.name is required by descriptor but missing from typed data"
                            .to_string(),
                    ));
                }
            }
        }

        if let Some(expected_version) = expected_domain.version.as_deref() {
            match data.domain.version.as_deref() {
                Some(actual_version) if actual_version == expected_version => {}
                Some(_) => {
                    return Err(Error::Descriptor(
                        "descriptor eip712.domain.version mismatch".to_string(),
                    ));
                }
                None => {
                    return Err(Error::Descriptor(
                        "descriptor eip712.domain.version is required by descriptor but missing from typed data"
                            .to_string(),
                    ));
                }
            }
        }

        if let Some(expected_chain_id) = expected_domain.chain_id {
            match data.domain.chain_id {
                Some(actual_chain_id) if actual_chain_id == expected_chain_id => {}
                Some(_) => {
                    return Err(Error::Descriptor(
                        "descriptor eip712.domain.chainId mismatch".to_string(),
                    ));
                }
                None => {
                    return Err(Error::Descriptor(
                        "descriptor eip712.domain.chainId is required by descriptor but missing from typed data"
                            .to_string(),
                    ));
                }
            }
        }

        if let Some(expected_contract) = expected_domain.verifying_contract.as_deref() {
            match data.domain.verifying_contract.as_deref() {
                Some(actual_contract)
                    if actual_contract.eq_ignore_ascii_case(expected_contract) => {}
                Some(_) => {
                    return Err(Error::Descriptor(
                        "descriptor eip712.domain.verifyingContract mismatch".to_string(),
                    ));
                }
                None => {
                    return Err(Error::Descriptor(
                        "descriptor eip712.domain.verifyingContract is required by descriptor but missing from typed data"
                            .to_string(),
                    ));
                }
            }
        }

        if let Some(expected_salt) = expected_domain.salt.as_deref() {
            match data.domain.salt.as_deref() {
                Some(actual_salt)
                    if normalize_hex_prefix_case(actual_salt)
                        == normalize_hex_prefix_case(expected_salt) => {}
                Some(_) => {
                    return Err(Error::Descriptor(
                        "descriptor eip712.domain.salt mismatch".to_string(),
                    ));
                }
                None => {
                    return Err(Error::Descriptor(
                        "descriptor eip712.domain.salt is required by descriptor but missing from typed data"
                            .to_string(),
                    ));
                }
            }
        }
    }

    if let Some(expected_separator) = ctx.eip712.domain_separator.as_deref() {
        let expected_bytes =
            decode_descriptor_domain_separator(expected_separator).map_err(Error::Descriptor)?;
        let actual_separator = compute_domain_separator(data)?;
        if actual_separator != expected_bytes {
            return Err(Error::Descriptor(
                "descriptor eip712.domainSeparator mismatch".to_string(),
            ));
        }
    }

    Ok(())
}

fn normalize_hex_prefix_case(value: &str) -> String {
    if let Some(rest) = value.strip_prefix("0X") {
        format!("0x{rest}")
    } else {
        value.to_string()
    }
}

fn decode_descriptor_domain_separator(value: &str) -> Result<[u8; 32], String> {
    let hex_str = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .ok_or_else(|| "descriptor eip712.domainSeparator must be 32-byte hex".to_string())?;
    let bytes = hex::decode(hex_str)
        .map_err(|_| "descriptor eip712.domainSeparator must be 32-byte hex".to_string())?;
    if bytes.len() != 32 {
        return Err("descriptor eip712.domainSeparator must be 32-byte hex".to_string());
    }
    let mut result = [0u8; 32];
    result.copy_from_slice(&bytes);
    Ok(result)
}

fn compute_domain_separator(data: &TypedData) -> Result<[u8; 32], Error> {
    if !data.types.contains_key("EIP712Domain") {
        return Err(Error::Descriptor(
            "descriptor eip712.domainSeparator requires types.EIP712Domain".to_string(),
        ));
    }
    hash_typed_struct(
        "EIP712Domain",
        &typed_data_domain_to_value(&data.domain),
        &data.types,
    )
}

fn typed_data_domain_to_value(domain: &TypedDataDomain) -> serde_json::Value {
    let mut map = serde_json::Map::new();

    for (key, value) in &domain.extra {
        map.insert(key.clone(), value.clone());
    }

    if let Some(name) = &domain.name {
        map.insert("name".to_string(), serde_json::Value::String(name.clone()));
    }
    if let Some(version) = &domain.version {
        map.insert(
            "version".to_string(),
            serde_json::Value::String(version.clone()),
        );
    }
    if let Some(chain_id) = domain.chain_id {
        map.insert("chainId".to_string(), serde_json::Value::from(chain_id));
    }
    if let Some(verifying_contract) = &domain.verifying_contract {
        map.insert(
            "verifyingContract".to_string(),
            serde_json::Value::String(verifying_contract.clone()),
        );
    }
    if let Some(salt) = &domain.salt {
        map.insert("salt".to_string(), serde_json::Value::String(salt.clone()));
    }

    serde_json::Value::Object(map)
}

fn hash_typed_struct(
    type_name: &str,
    value: &serde_json::Value,
    types: &HashMap<String, Vec<TypedDataField>>,
) -> Result<[u8; 32], Error> {
    let object = value.as_object().ok_or_else(|| {
        Error::Descriptor(format!(
            "descriptor eip712.domainSeparator requires object value for struct type '{}'",
            type_name
        ))
    })?;
    let fields = types.get(type_name).ok_or_else(|| {
        Error::Descriptor(format!(
            "descriptor eip712.domainSeparator requires types.{}",
            type_name
        ))
    })?;

    let mut encoded = Vec::with_capacity(32 * (fields.len() + 1));
    let type_hash = keccak256(encode_type_for_type(types, type_name)?.as_bytes());
    encoded.extend_from_slice(&type_hash);

    for field in fields {
        let field_value = object.get(&field.name).ok_or_else(|| {
            Error::Descriptor(format!(
                "descriptor eip712.domainSeparator requires domain field '{}' in typed data",
                field.name
            ))
        })?;
        let encoded_field =
            encode_domain_value(&field.field_type, field_value, types, Some(&field.name))?;
        encoded.extend_from_slice(&encoded_field);
    }

    Ok(keccak256(&encoded))
}

fn encode_domain_value(
    field_type: &str,
    value: &serde_json::Value,
    types: &HashMap<String, Vec<TypedDataField>>,
    field_name: Option<&str>,
) -> Result<[u8; 32], Error> {
    if let Some(element_type) = array_element_type(field_type) {
        let items = value.as_array().ok_or_else(|| {
            Error::Descriptor(domain_separator_field_error(
                field_name,
                field_type,
                "must be an array",
            ))
        })?;
        let mut concatenated = Vec::with_capacity(items.len() * 32);
        for item in items {
            let encoded = encode_domain_value(element_type, item, types, field_name)?;
            concatenated.extend_from_slice(&encoded);
        }
        return Ok(keccak256(&concatenated));
    }

    if field_type == "string" {
        let value = value.as_str().ok_or_else(|| {
            Error::Descriptor(domain_separator_field_error(
                field_name,
                field_type,
                "must be a string",
            ))
        })?;
        return Ok(keccak256(value.as_bytes()));
    }

    if field_type == "bytes" {
        let bytes = decode_hex_typed_value(value, field_name, field_type)?;
        return Ok(keccak256(&bytes));
    }

    if field_type == "bool" {
        let boolean = value.as_bool().ok_or_else(|| {
            Error::Descriptor(domain_separator_field_error(
                field_name,
                field_type,
                "must be a bool",
            ))
        })?;
        let mut encoded = [0u8; 32];
        encoded[31] = u8::from(boolean);
        return Ok(encoded);
    }

    if field_type == "address" {
        let addr = coerce_typed_address_string(value).ok_or_else(|| {
            Error::Descriptor(domain_separator_field_error(
                field_name,
                field_type,
                "must be a valid address",
            ))
        })?;
        let bytes = hex::decode(
            addr.strip_prefix("0x")
                .or_else(|| addr.strip_prefix("0X"))
                .ok_or_else(|| {
                    Error::Descriptor(domain_separator_field_error(
                        field_name,
                        field_type,
                        "must be a valid address",
                    ))
                })?,
        )
        .map_err(|_| {
            Error::Descriptor(domain_separator_field_error(
                field_name,
                field_type,
                "must be a valid address",
            ))
        })?;
        if bytes.len() != 20 {
            return Err(Error::Descriptor(domain_separator_field_error(
                field_name,
                field_type,
                "must be a valid address",
            )));
        }
        let mut encoded = [0u8; 32];
        encoded[12..].copy_from_slice(&bytes);
        return Ok(encoded);
    }

    if let Some(byte_len) = fixed_bytes_len(field_type) {
        let bytes = decode_hex_typed_value(value, field_name, field_type)?;
        if bytes.len() != byte_len {
            return Err(Error::Descriptor(domain_separator_field_error(
                field_name,
                field_type,
                "has the wrong byte length",
            )));
        }
        let mut encoded = [0u8; 32];
        encoded[..byte_len].copy_from_slice(&bytes);
        return Ok(encoded);
    }

    if let Some(bits) = uint_type_bits(field_type) {
        let numeric = parse_typed_biguint_value(value, field_type).map_err(|_| {
            Error::Descriptor(domain_separator_field_error(
                field_name,
                field_type,
                "must be an unsigned integer",
            ))
        })?;
        let max = BigUint::from(1u8) << bits;
        if numeric >= max {
            return Err(Error::Descriptor(domain_separator_field_error(
                field_name,
                field_type,
                "does not fit in the declared bit width",
            )));
        }
        let bytes = numeric.to_bytes_be();
        if bytes.len() > 32 {
            return Err(Error::Descriptor(domain_separator_field_error(
                field_name,
                field_type,
                "does not fit in 32 bytes",
            )));
        }
        let mut encoded = [0u8; 32];
        encoded[32 - bytes.len()..].copy_from_slice(&bytes);
        return Ok(encoded);
    }

    if let Some(bits) = int_type_bits(field_type) {
        let numeric = parse_typed_bigint_value(value, field_type).map_err(|_| {
            Error::Descriptor(domain_separator_field_error(
                field_name,
                field_type,
                "must be an integer",
            ))
        })?;
        let limit = BigInt::from(1u8) << (bits - 1);
        if numeric < -limit.clone() || numeric >= limit {
            return Err(Error::Descriptor(domain_separator_field_error(
                field_name,
                field_type,
                "does not fit in the declared bit width",
            )));
        }
        let modulus = BigInt::from(1u8) << bits;
        let encoded_value = if numeric.sign() == Sign::Minus {
            numeric + modulus
        } else {
            numeric
        };
        let as_biguint = encoded_value.to_biguint().ok_or_else(|| {
            Error::Descriptor(domain_separator_field_error(
                field_name,
                field_type,
                "must be an integer",
            ))
        })?;
        let bytes = as_biguint.to_bytes_be();
        let mut encoded = [0u8; 32];
        encoded[32 - bytes.len()..].copy_from_slice(&bytes);
        return Ok(encoded);
    }

    if types.contains_key(field_type) {
        return hash_typed_struct(field_type, value, types);
    }

    Err(Error::Descriptor(domain_separator_field_error(
        field_name,
        field_type,
        "uses an unsupported EIP-712 type",
    )))
}

fn decode_hex_typed_value(
    value: &serde_json::Value,
    field_name: Option<&str>,
    field_type: &str,
) -> Result<Vec<u8>, Error> {
    let string = value.as_str().ok_or_else(|| {
        Error::Descriptor(domain_separator_field_error(
            field_name,
            field_type,
            "must be a 0x-prefixed hex string",
        ))
    })?;
    let hex_str = string
        .strip_prefix("0x")
        .or_else(|| string.strip_prefix("0X"))
        .ok_or_else(|| {
            Error::Descriptor(domain_separator_field_error(
                field_name,
                field_type,
                "must be a 0x-prefixed hex string",
            ))
        })?;
    hex::decode(hex_str).map_err(|_| {
        Error::Descriptor(domain_separator_field_error(
            field_name,
            field_type,
            "must be a 0x-prefixed hex string",
        ))
    })
}

fn parse_typed_bigint_value(value: &serde_json::Value, format_name: &str) -> Result<BigInt, Error> {
    match value {
        serde_json::Value::Number(n) => {
            if let Some(value) = n.as_i64() {
                Ok(BigInt::from(value))
            } else if let Some(value) = n.as_u64() {
                Ok(BigInt::from(value))
            } else {
                Err(Error::Render(format!(
                    "{format_name} field must be an integer"
                )))
            }
        }
        serde_json::Value::String(s) => {
            let trimmed = s.trim();
            if let Some(hex_str) = trimmed
                .strip_prefix("0x")
                .or_else(|| trimmed.strip_prefix("0X"))
            {
                let bytes = hex::decode(hex_str).map_err(|_| {
                    Error::Render(format!("{format_name} field must be an integer"))
                })?;
                Ok(BigInt::from_bytes_be(Sign::Plus, &bytes))
            } else {
                trimmed
                    .parse::<BigInt>()
                    .map_err(|_| Error::Render(format!("{format_name} field must be an integer")))
            }
        }
        _ => Err(Error::Render(format!(
            "{format_name} field must be an integer"
        ))),
    }
}

fn domain_separator_field_error(
    field_name: Option<&str>,
    field_type: &str,
    reason: &str,
) -> String {
    match field_name {
        Some(name) => format!(
            "descriptor eip712.domainSeparator field '{}' ({}) {}",
            name, field_type, reason
        ),
        None => format!(
            "descriptor eip712.domainSeparator value ({}) {}",
            field_type, reason
        ),
    }
}

fn array_element_type(field_type: &str) -> Option<&str> {
    let stripped = field_type.strip_suffix(']')?;
    let bracket_index = stripped.rfind('[')?;
    Some(&stripped[..bracket_index])
}

fn fixed_bytes_len(field_type: &str) -> Option<usize> {
    if field_type == "bytes" || !field_type.starts_with("bytes") {
        return None;
    }
    let len = field_type["bytes".len()..].parse::<usize>().ok()?;
    if (1..=32).contains(&len) {
        Some(len)
    } else {
        None
    }
}

fn uint_type_bits(field_type: &str) -> Option<usize> {
    if field_type == "uint" {
        return Some(256);
    }
    let bits = field_type.strip_prefix("uint")?.parse::<usize>().ok()?;
    if bits % 8 == 0 && (8..=256).contains(&bits) {
        Some(bits)
    } else {
        None
    }
}

fn int_type_bits(field_type: &str) -> Option<usize> {
    if field_type == "int" {
        return Some(256);
    }
    let bits = field_type.strip_prefix("int")?.parse::<usize>().ok()?;
    if bits % 8 == 0 && (8..=256).contains(&bits) {
        Some(bits)
    } else {
        None
    }
}

fn keccak256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak::v256();
    hasher.update(bytes);
    let mut output = [0u8; 32];
    hasher.finalize(&mut output);
    output
}
