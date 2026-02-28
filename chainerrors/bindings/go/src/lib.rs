//! chainerrors C FFI — exported symbols for CGo bindings.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::cell::RefCell;

use chainerrors_evm::decoder::EvmErrorDecoder;
use chainerrors_core::types::ErrorKind;

thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = RefCell::new(None);
}

fn set_last_error(msg: &str) {
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = CString::new(msg).ok();
    });
}

fn clear_last_error() {
    LAST_ERROR.with(|e| { *e.borrow_mut() = None; });
}

/// Retrieve the last FFI error message. Returns NULL if none.
#[no_mangle]
pub extern "C" fn chainerrors_last_error() -> *const c_char {
    LAST_ERROR.with(|e| {
        e.borrow().as_ref().map(|s| s.as_ptr()).unwrap_or(std::ptr::null())
    })
}

/// Free a string returned by any chainerrors FFI function.
///
/// # Safety
/// Must only be called with pointers returned by chainerrors FFI functions.
#[no_mangle]
pub unsafe extern "C" fn chainerrors_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}

/// Decode EVM revert data into a JSON string.
///
/// `hex_data` — hex-encoded revert bytes (with or without "0x" prefix).
///              Pass empty string "" or "0x" for an empty revert.
///
/// Returns a JSON object on success, NULL on error.
/// Caller must free with `chainerrors_free_string`.
#[no_mangle]
pub extern "C" fn chainerrors_decode(hex_data: *const c_char) -> *mut c_char {
    clear_last_error();
    let hex_str = unsafe {
        match CStr::from_ptr(hex_data).to_str() {
            Ok(s) => s,
            Err(_) => { set_last_error("invalid UTF-8"); return std::ptr::null_mut(); }
        }
    };

    let stripped = hex_str.trim_start_matches("0x");
    let bytes: Vec<u8> = if stripped.is_empty() {
        vec![]
    } else {
        match hex::decode(stripped) {
            Ok(b) => b,
            Err(e) => { set_last_error(&format!("hex decode: {e}")); return std::ptr::null_mut(); }
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

    match CString::new(result.to_string()) {
        Ok(s) => s.into_raw(),
        Err(e) => { set_last_error(&e.to_string()); std::ptr::null_mut() }
    }
}

/// Return the human-readable meaning of a Solidity panic code.
///
/// `code` — decimal panic code (e.g. 17 for arithmetic overflow).
///
/// Returns a static C string. Do NOT free.
#[no_mangle]
pub extern "C" fn chainerrors_panic_meaning(code: u32) -> *const c_char {
    let meaning: &'static [u8] = match code {
        0x00 => b"Generic panic\0",
        0x01 => b"assert() violation\0",
        0x11 => b"Arithmetic overflow/underflow\0",
        0x12 => b"Division or modulo by zero\0",
        0x21 => b"Invalid enum value\0",
        0x22 => b"Storage byte array incorrectly encoded\0",
        0x31 => b"pop() on empty array\0",
        0x32 => b"Array index out of bounds\0",
        0x41 => b"Out of memory\0",
        0x51 => b"Zero-initialized function pointer called\0",
        _ => b"Unknown panic code\0",
    };
    meaning.as_ptr() as *const c_char
}

/// Return the library version string (static, do NOT free).
#[no_mangle]
pub extern "C" fn chainerrors_version() -> *const c_char {
    static VERSION: &[u8] = b"0.1.0\0";
    VERSION.as_ptr() as *const c_char
}
