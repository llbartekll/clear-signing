use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use crate::decoder::ArgumentValue;
use crate::error::{Error, ResolveError};
use crate::types::display::{DisplayField, FieldFormat};

use super::source::{DescriptorSource, ResolvedDescriptor, TypedDescriptorLookup};
use super::{select_typed_outer_descriptor, TypedOuterSelection};

/// Maximum recursion depth for nested descriptor resolution.
const MAX_RESOLVE_DEPTH: u8 = 3;

/// Resolve all descriptors needed to format a transaction, including nested calldata.
pub async fn resolve_descriptors_for_tx(
    tx: &crate::TransactionContext<'_>,
    source: &dyn DescriptorSource,
) -> Result<Vec<ResolvedDescriptor>, ResolveError> {
    let mut results = Vec::new();
    let address = tx.implementation_address.unwrap_or(tx.to);
    resolve_recursive(
        tx.chain_id,
        address,
        tx.calldata,
        source,
        MAX_RESOLVE_DEPTH,
        &mut results,
    )
    .await?;
    Ok(results)
}

fn resolve_recursive<'a>(
    chain_id: u64,
    address: &'a str,
    calldata: &'a [u8],
    source: &'a dyn DescriptorSource,
    depth: u8,
    results: &'a mut Vec<ResolvedDescriptor>,
) -> Pin<Box<dyn Future<Output = Result<(), ResolveError>> + Send + 'a>> {
    Box::pin(async move {
        if depth == 0 || calldata.len() < 4 {
            return Ok(());
        }

        let resolved = match source.resolve_calldata(chain_id, address).await {
            Ok(r) => r,
            Err(ResolveError::NotFound { .. }) => return Ok(()),
            Err(e) => return Err(e),
        };

        let selector = &calldata[..4];
        let (sig, format_key) = match crate::find_matching_signature(&resolved.descriptor, selector)
        {
            Ok(r) => r,
            Err(_) => {
                results.push(resolved);
                return Ok(());
            }
        };

        let decoded = match crate::decoder::decode_calldata(&sig, calldata) {
            Ok(d) => d,
            Err(_) => {
                results.push(resolved);
                return Ok(());
            }
        };

        let format = resolved.descriptor.display.formats.get(&format_key);
        let calldata_fields = format
            .map(|fmt| {
                collect_calldata_fields(&fmt.fields, &resolved.descriptor.display.definitions)
            })
            .unwrap_or_default();

        results.push(resolved);

        for field in &calldata_fields {
            let data_path = match &field.data_path {
                Some(p) => p,
                None => continue,
            };

            let callee = resolve_resolver_nested_callee(field, &decoded)
                .map_err(|e| ResolveError::Parse(e.to_string()))?;

            let inner_data =
                crate::engine::resolve_path(&decoded, data_path).and_then(|v| match v {
                    ArgumentValue::Bytes(b) => Some(b),
                    _ => None,
                });

            let inner_chain = resolve_resolver_nested_chain_id(field, &decoded, chain_id)
                .map_err(|e| ResolveError::Parse(e.to_string()))?;
            let selector_override = resolve_resolver_nested_selector(field, &decoded)
                .map_err(|e| ResolveError::Parse(e.to_string()))?;

            if let (Some(addr), Some(data)) = (callee, inner_data) {
                let normalized =
                    crate::engine::normalized_nested_calldata(&data, selector_override);
                resolve_recursive(inner_chain, &addr, &normalized, source, depth - 1, results)
                    .await?;
            }
        }

        Ok(())
    })
}

/// Resolve all descriptors needed to format EIP-712 typed data, including nested calldata.
///
/// Uses the full typed-data schema so outer descriptor selection follows the same
/// `domain` / `domainSeparator` / exact `encodeType` rules as rendering.
pub async fn resolve_descriptors_for_typed_data(
    typed_data: &crate::eip712::TypedData,
    source: &dyn DescriptorSource,
) -> Result<Vec<ResolvedDescriptor>, ResolveError> {
    let mut results = Vec::new();

    let Some(chain_id) = typed_data.domain.chain_id else {
        return Ok(results);
    };
    let Some(verifying_contract) = typed_data.domain.verifying_contract.as_deref() else {
        return Ok(results);
    };

    let lookup = TypedDescriptorLookup {
        chain_id,
        verifying_contract: verifying_contract.to_string(),
        primary_type: typed_data.primary_type.clone(),
        encode_type_hash: Some(
            crate::eip712::encode_type_hash_hex_for_primary_type(typed_data)
                .map_err(|e| ResolveError::Parse(e.to_string()))?,
        ),
    };

    let candidates = match source.resolve_typed_candidates(lookup).await {
        Ok(r) => r,
        Err(ResolveError::NotFound { .. }) => return Ok(results),
        Err(e) => return Err(e),
    };

    let selection = select_typed_outer_descriptor(&candidates, typed_data)
        .map_err(|e| ResolveError::Parse(e.to_string()))?;
    let selected = match selection {
        TypedOuterSelection::Selected(selected) => selected,
        TypedOuterSelection::NoMatch(_) => return Ok(results),
    };

    let mut warnings = Vec::new();
    let expanded = crate::engine::expand_display_fields(
        &selected.outer.descriptor,
        &selected.format.fields,
        &mut warnings,
    );
    let calldata_fields =
        collect_calldata_fields(&expanded, &selected.outer.descriptor.display.definitions);

    results.push(selected.outer.clone());

    for field in &calldata_fields {
        let callee_addr = match resolve_typed_nested_callee_for_resolver(
            field,
            &typed_data.message,
            chain_id,
            verifying_contract,
        )
        .map_err(|e| ResolveError::Parse(e.to_string()))?
        {
            Some(addr) => addr,
            None => continue,
        };

        let inner_chain = resolve_typed_nested_chain_id_for_resolver(
            field,
            &typed_data.message,
            chain_id,
            verifying_contract,
        )
        .map_err(|e| ResolveError::Parse(e.to_string()))?;
        let selector_override = resolve_typed_nested_selector_for_resolver(
            field,
            &typed_data.message,
            chain_id,
            verifying_contract,
        )
        .map_err(|e| ResolveError::Parse(e.to_string()))?;

        if let Some(data_path) = &field.data_path {
            if let Some(inner_hex) = crate::eip712::resolve_typed_path(
                &typed_data.message,
                data_path,
                chain_id,
                Some(verifying_contract),
            )
            .and_then(|v| v.as_str().map(String::from))
            {
                let hex_str = inner_hex
                    .strip_prefix("0x")
                    .or_else(|| inner_hex.strip_prefix("0X"))
                    .unwrap_or(&inner_hex);
                if let Ok(inner_bytes) = hex::decode(hex_str) {
                    let normalized =
                        crate::engine::normalized_nested_calldata(&inner_bytes, selector_override);
                    let _ = resolve_recursive(
                        inner_chain,
                        &callee_addr,
                        &normalized,
                        source,
                        MAX_RESOLVE_DEPTH - 1,
                        &mut results,
                    )
                    .await;
                    continue;
                }
            }
        }

        match source.resolve_calldata(inner_chain, &callee_addr).await {
            Ok(r) => {
                if !results
                    .iter()
                    .any(|e| e.chain_id == r.chain_id && e.address.eq_ignore_ascii_case(&r.address))
                {
                    results.push(r);
                }
            }
            Err(ResolveError::NotFound { .. }) => {}
            Err(e) => return Err(e),
        }
    }

    Ok(results)
}

/// Info extracted from a `FieldFormat::Calldata` display field.
struct CalldataFieldInfo {
    callee_path: Option<String>,
    callee: Option<String>,
    data_path: Option<String>,
    selector_path: Option<String>,
    selector: Option<String>,
    chain_id: Option<u64>,
    chain_id_path: Option<String>,
}

/// Walk display fields (resolving `$ref` references) and collect calldata-format fields.
fn collect_calldata_fields(
    fields: &[DisplayField],
    definitions: &HashMap<String, DisplayField>,
) -> Vec<CalldataFieldInfo> {
    let mut result = Vec::new();
    collect_calldata_fields_recursive(fields, definitions, &mut result);
    result
}

fn collect_calldata_fields_recursive(
    fields: &[DisplayField],
    definitions: &HashMap<String, DisplayField>,
    result: &mut Vec<CalldataFieldInfo>,
) {
    for field in fields {
        match field {
            DisplayField::Simple {
                path,
                format,
                params,
                ..
            } => {
                if matches!(format.as_ref(), Some(FieldFormat::Calldata)) {
                    let fp = params.as_ref();
                    result.push(CalldataFieldInfo {
                        callee_path: fp.and_then(|p| p.callee_path.clone()),
                        callee: fp.and_then(|p| p.callee.clone()),
                        data_path: path.clone(),
                        selector_path: fp.and_then(|p| p.selector_path.clone()),
                        selector: fp.and_then(|p| p.selector.clone()),
                        chain_id: fp.and_then(|p| p.chain_id),
                        chain_id_path: fp.and_then(|p| p.chain_id_path.clone()),
                    });
                }
            }
            DisplayField::Reference {
                reference,
                path,
                params: ref_params,
                ..
            } => {
                let def_key = reference
                    .strip_prefix("$.display.definitions.")
                    .unwrap_or(reference);
                if let Some(DisplayField::Simple {
                    format: def_format,
                    params: def_params,
                    ..
                }) = definitions.get(def_key)
                {
                    if matches!(def_format.as_ref(), Some(FieldFormat::Calldata)) {
                        let callee_path = ref_params
                            .as_ref()
                            .and_then(|p| p.callee_path.clone())
                            .or_else(|| def_params.as_ref().and_then(|p| p.callee_path.clone()));
                        let callee = ref_params
                            .as_ref()
                            .and_then(|p| p.callee.clone())
                            .or_else(|| def_params.as_ref().and_then(|p| p.callee.clone()));
                        let selector_path = ref_params
                            .as_ref()
                            .and_then(|p| p.selector_path.clone())
                            .or_else(|| def_params.as_ref().and_then(|p| p.selector_path.clone()));
                        let selector = ref_params
                            .as_ref()
                            .and_then(|p| p.selector.clone())
                            .or_else(|| def_params.as_ref().and_then(|p| p.selector.clone()));
                        let chain_id = ref_params
                            .as_ref()
                            .and_then(|p| p.chain_id)
                            .or_else(|| def_params.as_ref().and_then(|p| p.chain_id));
                        let chain_id_path = ref_params
                            .as_ref()
                            .and_then(|p| p.chain_id_path.clone())
                            .or_else(|| def_params.as_ref().and_then(|p| p.chain_id_path.clone()));
                        result.push(CalldataFieldInfo {
                            callee_path,
                            callee,
                            data_path: path.clone(),
                            selector_path,
                            selector,
                            chain_id,
                            chain_id_path,
                        });
                    }
                }
            }
            DisplayField::Group { field_group } => {
                collect_calldata_fields_recursive(&field_group.fields, definitions, result);
            }
            DisplayField::Scope { fields: sub, .. } => {
                collect_calldata_fields_recursive(sub, definitions, result);
            }
        }
    }
}

fn resolve_resolver_nested_callee(
    field: &CalldataFieldInfo,
    decoded: &crate::decoder::DecodedArguments,
) -> Result<Option<String>, Error> {
    crate::engine::ensure_single_nested_param_source(
        field.callee.is_some(),
        field.callee_path.is_some(),
        "callee",
    )?;
    if let Some(callee) = field.callee.as_deref() {
        return crate::engine::parse_nested_address_param(callee, "callee").map(Some);
    }
    Ok(field
        .callee_path
        .as_ref()
        .and_then(|p| crate::engine::resolve_path(decoded, p))
        .and_then(|value| crate::engine::address_string_from_argument_value(&value)))
}

fn resolve_resolver_nested_chain_id(
    field: &CalldataFieldInfo,
    decoded: &crate::decoder::DecodedArguments,
    default_chain_id: u64,
) -> Result<u64, Error> {
    crate::engine::ensure_single_nested_param_source(
        field.chain_id.is_some(),
        field.chain_id_path.is_some(),
        "chainId",
    )?;
    if let Some(chain_id) = field.chain_id {
        return Ok(chain_id);
    }
    Ok(field
        .chain_id_path
        .as_ref()
        .and_then(|p| crate::engine::resolve_path(decoded, p))
        .and_then(|value| crate::engine::chain_id_from_argument_value(&value))
        .unwrap_or(default_chain_id))
}

fn resolve_resolver_nested_selector(
    field: &CalldataFieldInfo,
    decoded: &crate::decoder::DecodedArguments,
) -> Result<Option<[u8; 4]>, Error> {
    crate::engine::ensure_single_nested_param_source(
        field.selector.is_some(),
        field.selector_path.is_some(),
        "selector",
    )?;
    if let Some(selector) = field.selector.as_deref() {
        return crate::engine::parse_nested_selector_param(selector, "selector").map(Some);
    }
    Ok(field
        .selector_path
        .as_ref()
        .and_then(|p| crate::engine::resolve_path(decoded, p))
        .and_then(|value| crate::engine::selector_from_argument_value(&value)))
}

fn resolve_typed_nested_callee_for_resolver(
    field: &CalldataFieldInfo,
    message: &serde_json::Value,
    chain_id: u64,
    verifying_contract: &str,
) -> Result<Option<String>, Error> {
    crate::engine::ensure_single_nested_param_source(
        field.callee.is_some(),
        field.callee_path.is_some(),
        "callee",
    )?;
    if let Some(callee) = field.callee.as_deref() {
        return crate::engine::parse_nested_address_param(callee, "callee").map(Some);
    }
    Ok(field
        .callee_path
        .as_ref()
        .and_then(|p| {
            crate::eip712::resolve_typed_path(message, p, chain_id, Some(verifying_contract))
        })
        .as_ref()
        .and_then(crate::eip712::coerce_typed_address_string))
}

fn resolve_typed_nested_chain_id_for_resolver(
    field: &CalldataFieldInfo,
    message: &serde_json::Value,
    chain_id: u64,
    verifying_contract: &str,
) -> Result<u64, Error> {
    crate::engine::ensure_single_nested_param_source(
        field.chain_id.is_some(),
        field.chain_id_path.is_some(),
        "chainId",
    )?;
    if let Some(chain_id) = field.chain_id {
        return Ok(chain_id);
    }
    Ok(field
        .chain_id_path
        .as_ref()
        .and_then(|p| {
            crate::eip712::resolve_typed_path(message, p, chain_id, Some(verifying_contract))
        })
        .as_ref()
        .and_then(crate::eip712::coerce_typed_numeric_string)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(chain_id))
}

fn resolve_typed_nested_selector_for_resolver(
    field: &CalldataFieldInfo,
    message: &serde_json::Value,
    chain_id: u64,
    verifying_contract: &str,
) -> Result<Option<[u8; 4]>, Error> {
    crate::engine::ensure_single_nested_param_source(
        field.selector.is_some(),
        field.selector_path.is_some(),
        "selector",
    )?;
    if let Some(selector) = field.selector.as_deref() {
        return crate::engine::parse_nested_selector_param(selector, "selector").map(Some);
    }
    Ok(field
        .selector_path
        .as_ref()
        .and_then(|p| {
            crate::eip712::resolve_typed_path(message, p, chain_id, Some(verifying_contract))
        })
        .as_ref()
        .and_then(crate::eip712::selector_from_typed_value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EmptyDataProvider;

    use super::super::source::StaticSource;
    use super::super::test_support::{
        build_erc20_transfer_calldata, build_exec_transaction_calldata, erc20_descriptor,
        exclusive_dutch_order_typed_data, permit2_descriptor, safe_descriptor,
    };

    #[tokio::test]
    async fn test_resolve_descriptors_safe_wrapping_erc20() {
        let safe_addr = "0xd9Db270c1B5E3Bd161E8c8503c55cEABeE709552";
        let usdc_addr = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";
        let recipient = "0x1234567890123456789012345678901234567890";

        let mut source = StaticSource::new();
        source.add_calldata(1, safe_addr, safe_descriptor());
        source.add_calldata(1, usdc_addr, erc20_descriptor());

        let inner = build_erc20_transfer_calldata(recipient, 1_000_000);
        let outer = build_exec_transaction_calldata(usdc_addr, &inner);

        let tx = crate::TransactionContext {
            chain_id: 1,
            to: safe_addr,
            calldata: &outer,
            value: None,
            from: None,
            implementation_address: None,
        };

        let descriptors = resolve_descriptors_for_tx(&tx, &source).await.unwrap();

        assert_eq!(descriptors.len(), 2, "should resolve outer + inner");
        assert_eq!(descriptors[0].address, safe_addr.to_lowercase());
        assert_eq!(descriptors[1].address, usdc_addr.to_lowercase());
    }

    #[tokio::test]
    async fn test_resolve_descriptors_unknown_inner() {
        let safe_addr = "0xd9Db270c1B5E3Bd161E8c8503c55cEABeE709552";
        let unknown_addr = "0x0000000000000000000000000000000000000042";

        let mut source = StaticSource::new();
        source.add_calldata(1, safe_addr, safe_descriptor());

        let inner =
            hex::decode("12345678000000000000000000000000000000000000000000000000000000000000002a")
                .unwrap();
        let outer = build_exec_transaction_calldata(unknown_addr, &inner);

        let tx = crate::TransactionContext {
            chain_id: 1,
            to: safe_addr,
            calldata: &outer,
            value: None,
            from: None,
            implementation_address: None,
        };

        let descriptors = resolve_descriptors_for_tx(&tx, &source).await.unwrap();

        assert_eq!(
            descriptors.len(),
            1,
            "should resolve outer only, gracefully skip unknown inner"
        );
        assert_eq!(descriptors[0].address, safe_addr.to_lowercase());
    }

    #[tokio::test]
    async fn test_resolve_descriptors_no_outer() {
        let source = StaticSource::new();

        let calldata = hex::decode("a9059cbb").unwrap();
        let tx = crate::TransactionContext {
            chain_id: 1,
            to: "0x0000000000000000000000000000000000000099",
            calldata: &calldata,
            value: None,
            from: None,
            implementation_address: None,
        };

        let descriptors = resolve_descriptors_for_tx(&tx, &source).await.unwrap();
        assert!(descriptors.is_empty(), "no outer descriptor → empty vec");
    }

    #[tokio::test]
    async fn test_resolve_descriptors_uses_implementation_address() {
        let safe_addr = "0xd9Db270c1B5E3Bd161E8c8503c55cEABeE709552";
        let proxy_addr = "0x1111111111111111111111111111111111111111";
        let usdc_addr = "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48";

        let mut source = StaticSource::new();
        source.add_calldata(1, safe_addr, safe_descriptor());
        source.add_calldata(1, usdc_addr, erc20_descriptor());

        let inner =
            build_erc20_transfer_calldata("0x1234567890123456789012345678901234567890", 1_000_000);
        let outer = build_exec_transaction_calldata(usdc_addr, &inner);

        let tx = crate::TransactionContext {
            chain_id: 1,
            to: proxy_addr,
            calldata: &outer,
            value: None,
            from: None,
            implementation_address: Some(safe_addr),
        };

        let descriptors = resolve_descriptors_for_tx(&tx, &source).await.unwrap();

        assert_eq!(
            descriptors.len(),
            2,
            "should use implementation_address for outer resolution"
        );
    }

    #[tokio::test]
    async fn test_resolve_typed_data_selects_exact_encode_type_candidate() {
        let typed_data = exclusive_dutch_order_typed_data();
        let mut source = StaticSource::new();
        source.add_typed(
            1,
            "0x000000000022d473030f116ddee9f6b43ac78ba3",
            permit2_descriptor(
                "Dutch Order",
                "PermitWitnessTransferFrom(TokenPermissions permitted,address spender,uint256 nonce,uint256 deadline,DutchOrder witness)DutchOrder(OrderInfo info,uint256 decayStartTime,uint256 decayEndTime,address inputToken,uint256 inputStartAmount,uint256 inputEndAmount,DutchOutput[] outputs)DutchOutput(address token,uint256 startAmount,uint256 endAmount,address recipient)OrderInfo(address reactor,address swapper,uint256 nonce,uint256 deadline,address additionalValidationContract,bytes additionalValidationData)TokenPermissions(address token,uint256 amount)",
                None,
            ),
        );
        source.add_typed(
            1,
            "0x000000000022d473030f116ddee9f6b43ac78ba3",
            permit2_descriptor(
                "Exclusive Dutch Order",
                "PermitWitnessTransferFrom(TokenPermissions permitted,address spender,uint256 nonce,uint256 deadline,ExclusiveDutchOrder witness)DutchOutput(address token,uint256 startAmount,uint256 endAmount,address recipient)ExclusiveDutchOrder(OrderInfo info,uint256 decayStartTime,uint256 decayEndTime,address exclusiveFiller,uint256 exclusivityOverrideBps,address inputToken,uint256 inputStartAmount,uint256 inputEndAmount,DutchOutput[] outputs)OrderInfo(address reactor,address swapper,uint256 nonce,uint256 deadline,address additionalValidationContract,bytes additionalValidationData)TokenPermissions(address token,uint256 amount)",
                None,
            ),
        );
        source.add_typed(
            1,
            "0x000000000022d473030f116ddee9f6b43ac78ba3",
            permit2_descriptor(
                "Limit Order",
                "PermitWitnessTransferFrom(TokenPermissions permitted,address spender,uint256 nonce,uint256 deadline,LimitOrder witness)LimitOrder(OrderInfo info,address inputToken,uint256 inputAmount,OutputToken[] outputs)OrderInfo(address reactor,address swapper,uint256 nonce,uint256 deadline,address additionalValidationContract,bytes additionalValidationData)OutputToken(address token,uint256 amount,address recipient)TokenPermissions(address token,uint256 amount)",
                None,
            ),
        );

        let descriptors = resolve_descriptors_for_typed_data(&typed_data, &source)
            .await
            .expect("resolve");
        assert_eq!(descriptors.len(), 1);
        assert_eq!(
            descriptors[0].descriptor.metadata.owner.as_deref(),
            Some("Exclusive Dutch Order")
        );

        let model = crate::format_typed_data(&descriptors, &typed_data, &EmptyDataProvider)
            .await
            .expect("format");
        assert_eq!(model.intent, "Exclusive Dutch Order");
    }

    #[tokio::test]
    async fn test_resolve_typed_data_returns_empty_when_no_exact_encode_type_candidate_matches() {
        let typed_data = exclusive_dutch_order_typed_data();
        let mut source = StaticSource::new();
        source.add_typed(
            1,
            "0x000000000022d473030f116ddee9f6b43ac78ba3",
            permit2_descriptor(
                "Dutch Order",
                "PermitWitnessTransferFrom(TokenPermissions permitted,address spender,uint256 nonce,uint256 deadline,DutchOrder witness)DutchOrder(OrderInfo info,uint256 decayStartTime,uint256 decayEndTime,address inputToken,uint256 inputStartAmount,uint256 inputEndAmount,DutchOutput[] outputs)DutchOutput(address token,uint256 startAmount,uint256 endAmount,address recipient)OrderInfo(address reactor,address swapper,uint256 nonce,uint256 deadline,address additionalValidationContract,bytes additionalValidationData)TokenPermissions(address token,uint256 amount)",
                None,
            ),
        );

        let descriptors = resolve_descriptors_for_typed_data(&typed_data, &source)
            .await
            .expect("resolve");
        assert!(descriptors.is_empty());
    }

    #[tokio::test]
    async fn test_resolve_typed_data_rejects_domain_separator_mismatch_before_exact_match() {
        let typed_data = exclusive_dutch_order_typed_data();
        let mut source = StaticSource::new();
        source.add_typed(
            1,
            "0x000000000022d473030f116ddee9f6b43ac78ba3",
            permit2_descriptor(
                "Wrong Separator",
                "PermitWitnessTransferFrom(TokenPermissions permitted,address spender,uint256 nonce,uint256 deadline,ExclusiveDutchOrder witness)DutchOutput(address token,uint256 startAmount,uint256 endAmount,address recipient)ExclusiveDutchOrder(OrderInfo info,uint256 decayStartTime,uint256 decayEndTime,address exclusiveFiller,uint256 exclusivityOverrideBps,address inputToken,uint256 inputStartAmount,uint256 inputEndAmount,DutchOutput[] outputs)OrderInfo(address reactor,address swapper,uint256 nonce,uint256 deadline,address additionalValidationContract,bytes additionalValidationData)TokenPermissions(address token,uint256 amount)",
                Some(serde_json::json!({
                    "domainSeparator": "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
                })),
            ),
        );
        source.add_typed(
            1,
            "0x000000000022d473030f116ddee9f6b43ac78ba3",
            permit2_descriptor(
                "Correct Candidate",
                "PermitWitnessTransferFrom(TokenPermissions permitted,address spender,uint256 nonce,uint256 deadline,ExclusiveDutchOrder witness)DutchOutput(address token,uint256 startAmount,uint256 endAmount,address recipient)ExclusiveDutchOrder(OrderInfo info,uint256 decayStartTime,uint256 decayEndTime,address exclusiveFiller,uint256 exclusivityOverrideBps,address inputToken,uint256 inputStartAmount,uint256 inputEndAmount,DutchOutput[] outputs)OrderInfo(address reactor,address swapper,uint256 nonce,uint256 deadline,address additionalValidationContract,bytes additionalValidationData)TokenPermissions(address token,uint256 amount)",
                None,
            ),
        );

        let descriptors = resolve_descriptors_for_typed_data(&typed_data, &source)
            .await
            .expect("resolve");
        assert_eq!(descriptors.len(), 1);
        assert_eq!(
            descriptors[0].descriptor.metadata.owner.as_deref(),
            Some("Correct Candidate")
        );
    }

    #[tokio::test]
    async fn test_resolve_typed_data_errors_when_multiple_exact_candidates_survive() {
        let typed_data = exclusive_dutch_order_typed_data();
        let mut source = StaticSource::new();
        let format_key = "PermitWitnessTransferFrom(TokenPermissions permitted,address spender,uint256 nonce,uint256 deadline,ExclusiveDutchOrder witness)DutchOutput(address token,uint256 startAmount,uint256 endAmount,address recipient)ExclusiveDutchOrder(OrderInfo info,uint256 decayStartTime,uint256 decayEndTime,address exclusiveFiller,uint256 exclusivityOverrideBps,address inputToken,uint256 inputStartAmount,uint256 inputEndAmount,DutchOutput[] outputs)OrderInfo(address reactor,address swapper,uint256 nonce,uint256 deadline,address additionalValidationContract,bytes additionalValidationData)TokenPermissions(address token,uint256 amount)";
        source.add_typed(
            1,
            "0x000000000022d473030f116ddee9f6b43ac78ba3",
            permit2_descriptor("Candidate A", format_key, None),
        );
        source.add_typed(
            1,
            "0x000000000022d473030f116ddee9f6b43ac78ba3",
            permit2_descriptor("Candidate B", format_key, None),
        );

        let err = resolve_descriptors_for_typed_data(&typed_data, &source)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("multiple EIP-712 descriptors match"));
    }
}
