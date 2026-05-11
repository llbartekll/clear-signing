use anyhow::{anyhow, Context, Result};
use rlp::Rlp;

pub struct DecodedTx {
    pub chain_id: u64,
    pub to: String,
    pub value: Vec<u8>,
    pub data: Vec<u8>,
    pub gas_limit: u64,
    pub max_fee_per_gas: u128,
}

pub fn decode_signed(raw_hex: &str) -> Result<DecodedTx> {
    let stripped = raw_hex.trim_start_matches("0x");
    let bytes = hex::decode(stripped).context("rawTx is not valid hex")?;
    if bytes.is_empty() {
        return Err(anyhow!("rawTx is empty"));
    }
    let type_byte = bytes[0];
    // Legacy tx (no type prefix) starts with an RLP list header >= 0xc0.
    if type_byte >= 0xc0 {
        return Err(anyhow!(
            "legacy transactions are not supported in v1 (rawTx starts with 0x{type_byte:02x}); only EIP-1559 type 0x02 is supported"
        ));
    }
    if type_byte != 0x02 {
        return Err(anyhow!(
            "unsupported tx type 0x{type_byte:02x} in v1 (only EIP-1559 type 0x02 is supported)"
        ));
    }
    let payload = &bytes[1..];
    let rlp = Rlp::new(payload);
    if !rlp.is_list() {
        return Err(anyhow!("EIP-1559 payload is not an RLP list"));
    }
    let item_count = rlp.item_count().context("rlp item count")?;
    // 9 fields = unsigned EIP-1559 (signing payload, what registry rawTx values typically use).
    // 12 fields = signed EIP-1559 (adds y_parity, r, s).
    if !matches!(item_count, 9 | 12) {
        return Err(anyhow!(
            "expected 9 (unsigned) or 12 (signed) RLP fields for EIP-1559, got {item_count}"
        ));
    }

    let chain_id: u64 = rlp.val_at(0).context("chain_id")?;
    let _nonce: u64 = rlp.val_at(1).context("nonce")?;
    let _max_priority_fee: u128 = u128_at(&rlp, 2).context("max_priority_fee")?;
    let max_fee_per_gas: u128 = u128_at(&rlp, 3).context("max_fee_per_gas")?;
    let gas_limit: u64 = rlp.val_at(4).context("gas_limit")?;
    let to_bytes: Vec<u8> = rlp.val_at(5).context("to")?;
    if to_bytes.len() != 20 {
        return Err(anyhow!("`to` is not a 20-byte address (got {} bytes)", to_bytes.len()));
    }
    let value: Vec<u8> = rlp.val_at(6).context("value")?;
    let data: Vec<u8> = rlp.val_at(7).context("data")?;

    Ok(DecodedTx {
        chain_id,
        to: format!("0x{}", hex::encode(to_bytes)),
        value,
        data,
        gas_limit,
        max_fee_per_gas,
    })
}

fn u128_at(rlp: &Rlp, idx: usize) -> Result<u128> {
    let raw: Vec<u8> = rlp.val_at(idx)?;
    if raw.len() > 16 {
        return Err(anyhow!("u128 field too wide: {} bytes", raw.len()));
    }
    let mut padded = [0u8; 16];
    padded[16 - raw.len()..].copy_from_slice(&raw);
    Ok(u128::from_be_bytes(padded))
}
