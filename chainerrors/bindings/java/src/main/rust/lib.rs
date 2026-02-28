//! chainerrors JNI bindings for Java.

#![allow(non_snake_case)]

use jni::JNIEnv;
use jni::objects::{JClass, JString};
use jni::sys::{jint, jstring};

use chainerrors_evm::decoder::EvmErrorDecoder;
use chainerrors_core::types::ErrorKind;

fn jstring_to_rust(env: &JNIEnv, s: JString) -> Result<String, jni::errors::Error> {
    env.get_string(s).map(|js| js.into())
}

fn rust_to_jstring<'a>(env: &'a JNIEnv<'a>, s: &str) -> jstring {
    env.new_string(s)
        .map(|js| js.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// Returns the chainerrors library version.
#[no_mangle]
pub extern "system" fn Java_io_chainfoundry_chainerrors_ChainErrors_version(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    rust_to_jstring(&env, "0.1.0")
}

/// Decode EVM revert data from a hex string.
///
/// hexData — hex-encoded revert bytes (with or without "0x" prefix).
///           Pass "" or "0x" for an empty revert.
///
/// Returns JSON: {"kind":"...","message":"...","selector":"0x...","confidence":0.95}
/// Throws RuntimeException on decode error.
#[no_mangle]
pub extern "system" fn Java_io_chainfoundry_chainerrors_ChainErrors_decode(
    env: JNIEnv,
    _class: JClass,
    hex_data: JString,
) -> jstring {
    let hex_str = match jstring_to_rust(&env, hex_data) {
        Ok(s) => s,
        Err(e) => {
            let _ = env.throw_new("java/lang/IllegalArgumentException", e.to_string());
            return std::ptr::null_mut();
        }
    };

    let stripped = hex_str.trim_start_matches("0x");
    let bytes: Vec<u8> = if stripped.is_empty() {
        vec![]
    } else {
        match hex::decode(stripped) {
            Ok(b) => b,
            Err(e) => {
                let _ = env.throw_new("java/lang/IllegalArgumentException",
                    format!("hex decode: {e}"));
                return std::ptr::null_mut();
            }
        }
    };

    let decoder = EvmErrorDecoder::new();
    let decoded = decoder.decode(&bytes);

    let kind_str = match &decoded.kind {
        ErrorKind::RevertString(_) => "revert_string",
        ErrorKind::CustomError { .. } => "custom_error",
        ErrorKind::Panic { .. } => "panic",
        ErrorKind::RawRevert(_) => "raw_revert",
        ErrorKind::OutOfGas => "out_of_gas",
        ErrorKind::ContractNotDeployed => "contract_not_deployed",
        ErrorKind::Succeeded => "succeeded",
    };

    let message = match &decoded.kind {
        ErrorKind::RevertString(s) => Some(s.clone()),
        ErrorKind::CustomError { name, .. } => Some(name.clone()),
        ErrorKind::Panic { meaning, .. } => Some(meaning.to_string()),
        _ => None,
    };

    let result = serde_json::json!({
        "kind": kind_str,
        "message": message,
        "raw_data": hex_str,
        "selector": decoded.selector.map(|s| format!("0x{}", hex::encode(s))),
        "suggestion": decoded.suggestion,
        "confidence": decoded.confidence,
    });

    rust_to_jstring(&env, &result.to_string())
}

/// Return the human-readable meaning of a Solidity panic code.
///
/// code — panic code as integer (e.g. 17 for 0x11 = arithmetic overflow)
///
/// Returns a human-readable string.
#[no_mangle]
pub extern "system" fn Java_io_chainfoundry_chainerrors_ChainErrors_panicMeaning(
    env: JNIEnv,
    _class: JClass,
    code: jint,
) -> jstring {
    let meaning = match code as u32 {
        0x00 => "Generic panic",
        0x01 => "assert() violation",
        0x11 => "Arithmetic overflow/underflow",
        0x12 => "Division or modulo by zero",
        0x21 => "Invalid enum value",
        0x22 => "Storage byte array incorrectly encoded",
        0x31 => "pop() on empty array",
        0x32 => "Array index out of bounds",
        0x41 => "Out of memory",
        0x51 => "Zero-initialized function pointer called",
        _ => "Unknown panic code",
    };
    rust_to_jstring(&env, meaning)
}
