use crate::error::Error;
use crate::types::display::DisplayFormat;

use super::source::ResolvedDescriptor;

pub(crate) struct SelectedTypedDescriptor<'a> {
    pub outer: &'a ResolvedDescriptor,
    pub format: &'a DisplayFormat,
}

pub(crate) struct TypedOuterNoMatch<'a> {
    pub domain_errors: Vec<String>,
    pub format_misses: Vec<&'a ResolvedDescriptor>,
}

pub(crate) enum TypedOuterSelection<'a> {
    Selected(SelectedTypedDescriptor<'a>),
    NoMatch(TypedOuterNoMatch<'a>),
}

pub(crate) fn select_typed_outer_descriptor<'a>(
    descriptors: &'a [ResolvedDescriptor],
    data: &crate::eip712::TypedData,
) -> Result<TypedOuterSelection<'a>, Error> {
    let Some(chain_id) = data.domain.chain_id else {
        return Ok(TypedOuterSelection::NoMatch(TypedOuterNoMatch {
            domain_errors: Vec::new(),
            format_misses: Vec::new(),
        }));
    };
    let Some(verifying_contract) = data.domain.verifying_contract.as_deref() else {
        return Ok(TypedOuterSelection::NoMatch(TypedOuterNoMatch {
            domain_errors: Vec::new(),
            format_misses: Vec::new(),
        }));
    };

    let mut matches = Vec::new();
    let mut domain_errors = Vec::new();
    let mut format_misses = Vec::new();

    for descriptor in descriptors {
        let deployment_matches = descriptor
            .descriptor
            .context
            .deployments()
            .iter()
            .any(|dep| {
                dep.chain_id == chain_id && dep.address.eq_ignore_ascii_case(verifying_contract)
            });
        if !deployment_matches {
            continue;
        }

        match crate::eip712::validate_descriptor_domain_binding(&descriptor.descriptor, data) {
            Ok(()) => {}
            Err(Error::Descriptor(message)) => {
                domain_errors.push(message);
                continue;
            }
            Err(err) => return Err(err),
        }

        match crate::eip712::find_typed_format_optional(&descriptor.descriptor, data)? {
            Some(format) => matches.push(SelectedTypedDescriptor {
                outer: descriptor,
                format,
            }),
            None => format_misses.push(descriptor),
        }
    }

    match matches.len() {
        1 => Ok(TypedOuterSelection::Selected(matches.pop().expect("single match"))),
        0 => Ok(TypedOuterSelection::NoMatch(TypedOuterNoMatch {
            domain_errors,
            format_misses,
        })),
        _ => Err(Error::Descriptor(format!(
            "multiple EIP-712 descriptors match chain_id={} verifying_contract={} after domain and encodeType validation",
            chain_id, verifying_contract
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::display::intent_as_string;

    use super::super::test_support::{
        exclusive_dutch_order_typed_data, resolved_permit2_descriptor,
    };

    #[test]
    fn test_select_typed_outer_descriptor_returns_selected_format() {
        let typed_data = exclusive_dutch_order_typed_data();
        let format_key = "PermitWitnessTransferFrom(TokenPermissions permitted,address spender,uint256 nonce,uint256 deadline,ExclusiveDutchOrder witness)DutchOutput(address token,uint256 startAmount,uint256 endAmount,address recipient)ExclusiveDutchOrder(OrderInfo info,uint256 decayStartTime,uint256 decayEndTime,address exclusiveFiller,uint256 exclusivityOverrideBps,address inputToken,uint256 inputStartAmount,uint256 inputEndAmount,DutchOutput[] outputs)OrderInfo(address reactor,address swapper,uint256 nonce,uint256 deadline,address additionalValidationContract,bytes additionalValidationData)TokenPermissions(address token,uint256 amount)";
        let descriptors = vec![
            resolved_permit2_descriptor("Wrong Shape", "PermitWitnessTransferFrom(TokenPermissions permitted,address spender,uint256 nonce,uint256 deadline,DutchOrder witness)DutchOrder(OrderInfo info,uint256 decayStartTime,uint256 decayEndTime,address inputToken,uint256 inputStartAmount,uint256 inputEndAmount,DutchOutput[] outputs)DutchOutput(address token,uint256 startAmount,uint256 endAmount,address recipient)OrderInfo(address reactor,address swapper,uint256 nonce,uint256 deadline,address additionalValidationContract,bytes additionalValidationData)TokenPermissions(address token,uint256 amount)", None),
            resolved_permit2_descriptor("Exclusive Dutch Order", format_key, None),
        ];

        match select_typed_outer_descriptor(&descriptors, &typed_data).expect("selection") {
            TypedOuterSelection::Selected(selected) => {
                assert_eq!(
                    selected.outer.descriptor.metadata.owner.as_deref(),
                    Some("Exclusive Dutch Order")
                );
                assert_eq!(
                    selected
                        .format
                        .intent
                        .as_ref()
                        .map(intent_as_string)
                        .as_deref(),
                    Some("Exclusive Dutch Order")
                );
            }
            TypedOuterSelection::NoMatch(_) => panic!("expected selected match"),
        }
    }

    #[test]
    fn test_select_typed_outer_descriptor_reports_single_format_miss() {
        let typed_data = exclusive_dutch_order_typed_data();
        let descriptors = vec![resolved_permit2_descriptor(
            "Wrong Shape",
            "PermitWitnessTransferFrom(TokenPermissions permitted,address spender,uint256 nonce,uint256 deadline,DutchOrder witness)DutchOrder(OrderInfo info,uint256 decayStartTime,uint256 decayEndTime,address inputToken,uint256 inputStartAmount,uint256 inputEndAmount,DutchOutput[] outputs)DutchOutput(address token,uint256 startAmount,uint256 endAmount,address recipient)OrderInfo(address reactor,address swapper,uint256 nonce,uint256 deadline,address additionalValidationContract,bytes additionalValidationData)TokenPermissions(address token,uint256 amount)",
            None,
        )];

        match select_typed_outer_descriptor(&descriptors, &typed_data).expect("selection") {
            TypedOuterSelection::Selected(_) => panic!("expected no match"),
            TypedOuterSelection::NoMatch(no_match) => {
                assert!(no_match.domain_errors.is_empty());
                assert_eq!(no_match.format_misses.len(), 1);
                let err = crate::eip712::find_typed_format(
                    &no_match.format_misses[0].descriptor,
                    &typed_data,
                )
                .expect_err("single format miss should produce exact mismatch error");
                assert!(err.to_string().contains("expected encodeType"));
            }
        }
    }

    #[test]
    fn test_select_typed_outer_descriptor_retains_domain_errors() {
        let typed_data = exclusive_dutch_order_typed_data();
        let format_key = "PermitWitnessTransferFrom(TokenPermissions permitted,address spender,uint256 nonce,uint256 deadline,ExclusiveDutchOrder witness)DutchOutput(address token,uint256 startAmount,uint256 endAmount,address recipient)ExclusiveDutchOrder(OrderInfo info,uint256 decayStartTime,uint256 decayEndTime,address exclusiveFiller,uint256 exclusivityOverrideBps,address inputToken,uint256 inputStartAmount,uint256 inputEndAmount,DutchOutput[] outputs)OrderInfo(address reactor,address swapper,uint256 nonce,uint256 deadline,address additionalValidationContract,bytes additionalValidationData)TokenPermissions(address token,uint256 amount)";
        let descriptors = vec![resolved_permit2_descriptor(
            "Wrong Domain",
            format_key,
            Some(serde_json::json!({
                "domain": { "name": "Not Permit2" }
            })),
        )];

        match select_typed_outer_descriptor(&descriptors, &typed_data).expect("selection") {
            TypedOuterSelection::Selected(_) => panic!("expected no match"),
            TypedOuterSelection::NoMatch(no_match) => {
                assert!(no_match.format_misses.is_empty());
                assert_eq!(no_match.domain_errors.len(), 1);
                assert!(no_match.domain_errors[0].contains("domain.name mismatch"));
            }
        }
    }
}
