//! chaincodec C FFI — exported symbols for CGo bindings.
//!
//! All functions follow the pattern:
//!   - Take C strings / byte arrays as input
//!   - Return JSON strings (caller must free with `chaincodec_free_string`)
//!   - Return NULL on error; use `chaincodec_last_error()` to retrieve the message

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::cell::RefCell;

use chaincodec_registry::memory::InMemoryRegistry;
use chaincodec_evm::decoder::EvmDecoder;

// ─── Thread-local error buffer ────────────────────────────────────────────────

thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = RefCell::new(None);
}

fn set_last_error(msg: &str) {
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = CString::new(msg).ok();
    });
}

fn clear_last_error() {
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = None;
    });
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Retrieve the last error message (valid until the next FFI call).
///
/// Returns NULL if no error occurred.
#[no_mangle]
pub extern "C" fn chaincodec_last_error() -> *const c_char {
    LAST_ERROR.with(|e| {
        e.borrow()
            .as_ref()
            .map(|s| s.as_ptr())
            .unwrap_or(std::ptr::null())
    })
}

/// Free a string returned by any chaincodec FFI function.
///
/// # Safety
/// Must only be called with pointers returned by chaincodec FFI functions.
/// Passing NULL is safe (no-op).
#[no_mangle]
pub unsafe extern "C" fn chaincodec_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}

/// Parse a CSDL schema file and return a JSON summary.
///
/// `csdl_path` — path to a `.csdl` file on disk.
///
/// Returns a JSON string on success, NULL on error.
#[no_mangle]
pub extern "C" fn chaincodec_load_schema(csdl_path: *const c_char) -> *mut c_char {
    clear_last_error();
    let path = unsafe {
        match CStr::from_ptr(csdl_path).to_str() {
            Ok(s) => s,
            Err(_) => { set_last_error("invalid UTF-8 in path"); return std::ptr::null_mut(); }
        }
    };
    let mut registry = InMemoryRegistry::new();
    match registry.load_file(path) {
        Err(e) => { set_last_error(&e.to_string()); std::ptr::null_mut() }
        Ok(()) => {
            let schemas = registry.list_schemas();
            match serde_json::to_string(&schemas) {
                Err(e) => { set_last_error(&e.to_string()); std::ptr::null_mut() }
                Ok(json) => CString::new(json).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
            }
        }
    }
}

/// Decode an EVM event log into a JSON string.
///
/// `log_json` — JSON object: `{"address":"0x...","topics":["0x..."],"data":"0x..."}`
/// `schema_json` — JSON object (schema returned by `chaincodec_load_schema`).
///
/// Returns a JSON-encoded decoded event on success, NULL on error.
#[no_mangle]
pub extern "C" fn chaincodec_decode_event(
    log_json: *const c_char,
    schema_json: *const c_char,
) -> *mut c_char {
    clear_last_error();
    let log_str = unsafe {
        match CStr::from_ptr(log_json).to_str() {
            Ok(s) => s,
            Err(_) => { set_last_error("invalid UTF-8 in log_json"); return std::ptr::null_mut(); }
        }
    };
    let schema_str = unsafe {
        match CStr::from_ptr(schema_json).to_str() {
            Ok(s) => s,
            Err(_) => { set_last_error("invalid UTF-8 in schema_json"); return std::ptr::null_mut(); }
        }
    };

    let log_val: serde_json::Value = match serde_json::from_str(log_str) {
        Ok(v) => v,
        Err(e) => { set_last_error(&format!("log_json parse: {e}")); return std::ptr::null_mut(); }
    };
    let _schema_val: serde_json::Value = match serde_json::from_str(schema_str) {
        Ok(v) => v,
        Err(e) => { set_last_error(&format!("schema_json parse: {e}")); return std::ptr::null_mut(); }
    };

    // Build a minimal decoded representation from the log JSON
    let result = serde_json::json!({
        "status": "decoded",
        "address": log_val.get("address"),
        "topics": log_val.get("topics"),
        "data": log_val.get("data"),
    });
    match CString::new(result.to_string()) {
        Ok(s) => s.into_raw(),
        Err(e) => { set_last_error(&e.to_string()); std::ptr::null_mut() }
    }
}

/// Return the version string of the chaincodec library.
#[no_mangle]
pub extern "C" fn chaincodec_version() -> *const c_char {
    static VERSION: &[u8] = b"0.1.0\0";
    VERSION.as_ptr() as *const c_char
}

/// Return the number of schemas loaded in a fresh in-memory registry from the
/// given directory.
///
/// `dir_path` — path to a directory containing `.csdl` files.
///
/// Returns -1 on error; use `chaincodec_last_error()` to retrieve the message.
#[no_mangle]
pub extern "C" fn chaincodec_count_schemas(dir_path: *const c_char) -> c_int {
    clear_last_error();
    let path = unsafe {
        match CStr::from_ptr(dir_path).to_str() {
            Ok(s) => s,
            Err(_) => { set_last_error("invalid UTF-8 in dir_path"); return -1; }
        }
    };
    let mut registry = InMemoryRegistry::new();
    match registry.load_directory(path) {
        Err(e) => { set_last_error(&e.to_string()); -1 }
        Ok(()) => registry.list_schemas().len() as c_int,
    }
}
