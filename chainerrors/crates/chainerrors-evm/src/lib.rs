//! chainerrors-evm — EVM revert/panic/custom error decoder.
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use chainerrors_evm::EvmErrorDecoder;
//! use chainerrors_core::ErrorDecoder;
//!
//! let decoder = EvmErrorDecoder::new();
//! let result = decoder.decode(
//!     &hex::decode("08c379a00000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000001a4e6f7420656e6f75676820746f6b656e7320746f207472616e736665720000").unwrap(),
//!     None,
//! ).unwrap();
//! println!("{result}");  // "reverted: Not enough tokens to transfer"
//! ```

pub mod custom;
pub mod decoder;
pub mod panic;
pub mod revert;

#[cfg(feature = "fourbyte")]
pub mod fourbyte;

pub use decoder::EvmErrorDecoder;
