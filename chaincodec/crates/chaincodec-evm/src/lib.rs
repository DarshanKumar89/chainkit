//! # chaincodec-evm
//!
//! EVM ABI decoder implementing the `ChainDecoder` trait.
//! Handles Ethereum, Arbitrum, Base, Polygon, Optimism and any EVM-compatible chain.
//!
//! ## Implementation notes
//! - Uses `alloy-core` for ABI decode (replaces the legacy `ethabi`)
//! - Topics[0] → event signature fingerprint (keccak256)
//! - Topics[1..] → indexed parameters (each 32 bytes, ABI-encoded)
//! - `data` → non-indexed parameters (ABI-encoded tuple)

pub mod batch;
pub mod call_decoder;
pub mod decoder;
pub mod eip712;
pub mod encoder;
pub mod fingerprint;
pub mod normalizer;
pub mod proxy;

pub use call_decoder::EvmCallDecoder;
pub use decoder::EvmDecoder;
pub use encoder::EvmEncoder;
pub use eip712::{Eip712Parser, TypedData};
pub use proxy::{classify_from_storage, detect_eip1167_clone, ProxyInfo, ProxyKind};
