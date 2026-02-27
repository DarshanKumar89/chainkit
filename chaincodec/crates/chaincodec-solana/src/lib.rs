//! # chaincodec-solana
//!
//! Solana Anchor IDL event decoder for ChainCodec.
//!
//! ## Event format
//!
//! Solana programs emit events via CPI logs. Anchor-generated events are:
//! - Discriminator: first 8 bytes of `SHA-256("event:<EventName>")` (Anchor standard)
//! - Payload: remaining bytes, Borsh-encoded fields in schema order
//!
//! ## RawEvent mapping
//! - `topics[0]`: hex discriminator (8 bytes = 16 hex chars, used as fingerprint)
//! - `data`: Borsh-encoded payload bytes (NOT including the 8-byte discriminator)
//!
//! ## CSDL fingerprint
//! Must be set to `keccak256("event:<EventName>")[..8]` as a hex string.

use chaincodec_core::{
    chain::ChainFamily,
    decoder::{BatchDecodeResult, ChainDecoder, ErrorMode, ProgressCallback},
    error::{BatchDecodeError, DecodeError},
    event::{DecodedEvent, EventFingerprint, RawEvent},
    schema::{CanonicalType, Schema, SchemaRegistry},
    types::NormalizedValue,
};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Solana/Anchor event decoder.
#[derive(Debug, Default, Clone)]
pub struct SolanaDecoder;

impl SolanaDecoder {
    pub fn new() -> Self {
        Self
    }

    /// Compute the Anchor discriminator for an event name.
    ///
    /// Discriminator = first 8 bytes of SHA-256(`"event:<name>"`)
    pub fn anchor_discriminator(event_name: &str) -> [u8; 8] {
        let preimage = format!("event:{event_name}");
        let hash = Sha256::digest(preimage.as_bytes());
        hash[..8].try_into().expect("slice is 8 bytes")
    }

    /// Compute the fingerprint hex string for an Anchor event name.
    pub fn fingerprint_for(event_name: &str) -> EventFingerprint {
        let disc = Self::anchor_discriminator(event_name);
        EventFingerprint::new(format!("0x{}", hex::encode(disc)))
    }
}

impl ChainDecoder for SolanaDecoder {
    fn chain_family(&self) -> ChainFamily {
        ChainFamily::Solana
    }

    fn fingerprint(&self, raw: &RawEvent) -> EventFingerprint {
        // Discriminator is stored in topics[0] as a hex string
        raw.topics
            .first()
            .cloned()
            .map(EventFingerprint::new)
            .unwrap_or_else(|| {
                // Fallback: compute from first 8 bytes of raw.data
                if raw.data.len() >= 8 {
                    EventFingerprint::new(format!("0x{}", hex::encode(&raw.data[..8])))
                } else {
                    EventFingerprint::new("0x0000000000000000".to_string())
                }
            })
    }

    fn decode_event(&self, raw: &RawEvent, schema: &Schema) -> Result<DecodedEvent, DecodeError> {
        // Borsh-decode fields in schema order from raw.data
        let mut reader = BorshReader::new(&raw.data);
        let mut fields: HashMap<String, NormalizedValue> = HashMap::new();
        let mut decode_errors: HashMap<String, String> = HashMap::new();

        for (field_name, field_def) in schema.fields.iter() {
            match decode_borsh_field(&mut reader, &field_def.ty) {
                Ok(val) => {
                    fields.insert(field_name.clone(), val);
                }
                Err(e) => {
                    decode_errors.insert(field_name.clone(), e.to_string());
                    // Insert null placeholder and continue (partial decode)
                    fields.insert(field_name.clone(), NormalizedValue::Null);
                }
            }
        }

        Ok(DecodedEvent {
            chain: raw.chain.clone(),
            schema: schema.name.clone(),
            schema_version: schema.version,
            tx_hash: raw.tx_hash.clone(),
            block_number: raw.block_number,
            block_timestamp: raw.block_timestamp,
            log_index: raw.log_index,
            address: raw.address.clone(),
            fields,
            fingerprint: raw
                .topics
                .first()
                .cloned()
                .map(EventFingerprint::new)
                .unwrap_or_else(|| EventFingerprint::new("0x00".to_string())),
            decode_errors,
        })
    }

    fn supports_abi_guess(&self) -> bool {
        false
    }
}

// ─── Borsh reader ─────────────────────────────────────────────────────────────

/// Minimal Borsh decoder for ChainCodec canonical types.
///
/// Borsh encoding rules:
/// - Integers: little-endian fixed width
/// - bool: 1 byte (0=false, 1=true)
/// - String: u32 LE length prefix + UTF-8 bytes
/// - Fixed bytes: N bytes as-is
/// - Vec<T>: u32 LE length prefix + elements
/// - Option<T>: 1 byte (0=None, 1=Some) + T if Some
struct BorshReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BorshReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn read_slice(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        if self.pos + n > self.data.len() {
            return Err(DecodeError::AbiDecodeFailed(format!(
                "Borsh EOF: need {} bytes at pos {}, have {}",
                n,
                self.pos,
                self.data.len()
            )));
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    fn read_u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.read_slice(1)?[0])
    }

    fn read_u16_le(&mut self) -> Result<u16, DecodeError> {
        let b = self.read_slice(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn read_u32_le(&mut self) -> Result<u32, DecodeError> {
        let b = self.read_slice(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn read_u64_le(&mut self) -> Result<u64, DecodeError> {
        let b = self.read_slice(8)?;
        Ok(u64::from_le_bytes(b.try_into().unwrap()))
    }

    fn read_u128_le(&mut self) -> Result<u128, DecodeError> {
        let b = self.read_slice(16)?;
        Ok(u128::from_le_bytes(b.try_into().unwrap()))
    }

    fn read_i8(&mut self) -> Result<i8, DecodeError> {
        Ok(self.read_u8()? as i8)
    }

    fn read_i16_le(&mut self) -> Result<i16, DecodeError> {
        let b = self.read_slice(2)?;
        Ok(i16::from_le_bytes([b[0], b[1]]))
    }

    fn read_i32_le(&mut self) -> Result<i32, DecodeError> {
        let b = self.read_slice(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn read_i64_le(&mut self) -> Result<i64, DecodeError> {
        let b = self.read_slice(8)?;
        Ok(i64::from_le_bytes(b.try_into().unwrap()))
    }

    fn read_i128_le(&mut self) -> Result<i128, DecodeError> {
        let b = self.read_slice(16)?;
        Ok(i128::from_le_bytes(b.try_into().unwrap()))
    }

    fn read_string(&mut self) -> Result<String, DecodeError> {
        let len = self.read_u32_le()? as usize;
        let bytes = self.read_slice(len)?;
        String::from_utf8(bytes.to_vec())
            .map_err(|e| DecodeError::AbiDecodeFailed(format!("invalid UTF-8: {e}")))
    }

    fn read_pubkey(&mut self) -> Result<String, DecodeError> {
        let bytes = self.read_slice(32)?;
        Ok(bs58::encode(bytes).into_string())
    }
}

// ─── Field decoder ────────────────────────────────────────────────────────────

fn decode_borsh_field(
    reader: &mut BorshReader,
    ty: &CanonicalType,
) -> Result<NormalizedValue, DecodeError> {
    match ty {
        CanonicalType::Bool => {
            let b = reader.read_u8()?;
            Ok(NormalizedValue::Bool(b != 0))
        }

        CanonicalType::Uint(bits) => match bits {
            8 => Ok(NormalizedValue::Uint(reader.read_u8()? as u128)),
            16 => Ok(NormalizedValue::Uint(reader.read_u16_le()? as u128)),
            32 => Ok(NormalizedValue::Uint(reader.read_u32_le()? as u128)),
            64 => Ok(NormalizedValue::Uint(reader.read_u64_le()? as u128)),
            128 => Ok(NormalizedValue::Uint(reader.read_u128_le()?)),
            256 => {
                // Non-standard: read 32 bytes LE
                let b = reader.read_slice(32)?;
                Ok(NormalizedValue::BigUint(u256_le_bytes_to_decimal(b)))
            }
            _ => Err(DecodeError::AbiDecodeFailed(format!(
                "unsupported uint width: {}",
                bits
            ))),
        },

        CanonicalType::Int(bits) => match bits {
            8 => Ok(NormalizedValue::Int(reader.read_i8()? as i128)),
            16 => Ok(NormalizedValue::Int(reader.read_i16_le()? as i128)),
            32 => Ok(NormalizedValue::Int(reader.read_i32_le()? as i128)),
            64 => Ok(NormalizedValue::Int(reader.read_i64_le()? as i128)),
            128 => Ok(NormalizedValue::Int(reader.read_i128_le()?)),
            _ => Err(DecodeError::AbiDecodeFailed(format!(
                "unsupported int width: {}",
                bits
            ))),
        },

        CanonicalType::Bytes(n) => {
            let bytes = reader.read_slice(*n as usize)?.to_vec();
            Ok(NormalizedValue::Bytes(bytes))
        }

        CanonicalType::BytesVec => {
            let len = reader.read_u32_le()? as usize;
            let bytes = reader.read_slice(len)?.to_vec();
            Ok(NormalizedValue::Bytes(bytes))
        }

        CanonicalType::Str => {
            let s = reader.read_string()?;
            Ok(NormalizedValue::Str(s))
        }

        CanonicalType::Address => {
            // EVM address stored as 20 bytes in Solana cross-chain contexts
            let bytes = reader.read_slice(20)?;
            Ok(NormalizedValue::Address(format!("0x{}", hex::encode(bytes))))
        }

        CanonicalType::Pubkey => {
            let pk = reader.read_pubkey()?;
            Ok(NormalizedValue::Pubkey(pk))
        }

        CanonicalType::Bech32Address => {
            // Read as string (bech32 is variable length, stored as Borsh string)
            let s = reader.read_string()?;
            Ok(NormalizedValue::Bech32(s))
        }

        CanonicalType::Hash256 => {
            let bytes = reader.read_slice(32)?.to_vec();
            Ok(NormalizedValue::Hash256(format!("0x{}", hex::encode(&bytes))))
        }

        CanonicalType::Timestamp => {
            // Stored as i64 (Unix seconds) in Borsh
            let t = reader.read_i64_le()?;
            Ok(NormalizedValue::Timestamp(t))
        }

        CanonicalType::Decimal { .. } => {
            // Treated as u128 in Borsh (amount with scale applied at interpretation)
            let v = reader.read_u128_le()?;
            Ok(NormalizedValue::Uint(v))
        }

        CanonicalType::Array { elem, len } => {
            let mut items = Vec::with_capacity(*len as usize);
            for _ in 0..*len {
                items.push(decode_borsh_field(reader, elem)?);
            }
            Ok(NormalizedValue::Array(items))
        }

        CanonicalType::Vec(elem) => {
            let len = reader.read_u32_le()? as usize;
            let mut items = Vec::with_capacity(len);
            for _ in 0..len {
                items.push(decode_borsh_field(reader, elem)?);
            }
            Ok(NormalizedValue::Array(items))
        }

        CanonicalType::Tuple(fields) => {
            let mut result = Vec::with_capacity(fields.len());
            for (name, field_ty) in fields {
                let val = decode_borsh_field(reader, field_ty)?;
                result.push((name.clone(), val));
            }
            Ok(NormalizedValue::Tuple(result))
        }
    }
}

// ─── u256 little-endian to decimal string ────────────────────────────────────

fn u256_le_bytes_to_decimal(bytes: &[u8]) -> String {
    if bytes.len() < 32 {
        return "0".to_string();
    }
    let mut limbs: [u64; 4] = [0u64; 4];
    for i in 0..4 {
        let b = &bytes[i * 8..(i + 1) * 8];
        limbs[i] = u64::from_le_bytes(b.try_into().unwrap_or([0u8; 8]));
    }

    // Fast path: fits in u128
    if limbs[2] == 0 && limbs[3] == 0 {
        let v = (limbs[0] as u128) | ((limbs[1] as u128) << 64);
        return v.to_string();
    }

    // Slow path: full u256 to decimal via repeated division
    let mut digits: Vec<u8> = Vec::new();
    loop {
        if limbs.iter().all(|&x| x == 0) {
            break;
        }
        let mut rem: u128 = 0;
        for i in (0..4).rev() {
            let cur = (rem << 64) | (limbs[i] as u128);
            limbs[i] = (cur / 10) as u64;
            rem = cur % 10;
        }
        digits.push(rem as u8 + b'0');
    }

    if digits.is_empty() {
        return "0".to_string();
    }
    digits.reverse();
    String::from_utf8(digits).unwrap_or_else(|_| "0".to_string())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_discriminator_deterministic() {
        let disc = SolanaDecoder::anchor_discriminator("Transfer");
        assert_eq!(disc.len(), 8);
        // Second call produces the same result
        assert_eq!(disc, SolanaDecoder::anchor_discriminator("Transfer"));
    }

    #[test]
    fn borsh_reader_bool() {
        let data = &[1u8];
        let mut r = BorshReader::new(data);
        assert_eq!(
            decode_borsh_field(&mut r, &CanonicalType::Bool).unwrap(),
            NormalizedValue::Bool(true)
        );
    }

    #[test]
    fn borsh_reader_u64() {
        let v: u64 = 1_000_000;
        let data = v.to_le_bytes();
        let mut r = BorshReader::new(&data);
        assert_eq!(
            decode_borsh_field(&mut r, &CanonicalType::Uint(64)).unwrap(),
            NormalizedValue::Uint(1_000_000)
        );
    }

    #[test]
    fn borsh_reader_string() {
        let text = "hello";
        let mut data = (text.len() as u32).to_le_bytes().to_vec();
        data.extend_from_slice(text.as_bytes());
        let mut r = BorshReader::new(&data);
        assert_eq!(
            decode_borsh_field(&mut r, &CanonicalType::Str).unwrap(),
            NormalizedValue::Str("hello".to_string())
        );
    }

    #[test]
    fn u256_decimal_small() {
        let mut bytes = [0u8; 32];
        let v: u128 = 1_000_000_000_000_000_000;
        bytes[..16].copy_from_slice(&v.to_le_bytes());
        assert_eq!(u256_le_bytes_to_decimal(&bytes), "1000000000000000000");
    }
}
