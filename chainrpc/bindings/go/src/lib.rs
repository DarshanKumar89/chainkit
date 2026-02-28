//! chainrpc C FFI — exported symbols for CGo bindings.
//!
//! Async operations are bridged to synchronous C calls using a
//! Tokio runtime created once per process.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::cell::RefCell;
use std::sync::OnceLock;

use tokio::runtime::Runtime;
use chainrpc_http::HttpRpcClient;
use chainrpc_core::{pool::ProviderPool, request::JsonRpcRequest};

// ─── Global Tokio runtime ─────────────────────────────────────────────────────

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to build Tokio runtime")
    })
}

// ─── Thread-local error buffer ────────────────────────────────────────────────

thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = RefCell::new(None);
}

fn set_last_error(msg: &str) {
    LAST_ERROR.with(|e| { *e.borrow_mut() = CString::new(msg).ok(); });
}

fn clear_last_error() {
    LAST_ERROR.with(|e| { *e.borrow_mut() = None; });
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Last FFI error message (thread-local). NULL if no error.
#[no_mangle]
pub extern "C" fn chainrpc_last_error() -> *const c_char {
    LAST_ERROR.with(|e| {
        e.borrow().as_ref().map(|s| s.as_ptr()).unwrap_or(std::ptr::null())
    })
}

/// Free a string returned by any chainrpc FFI function.
///
/// # Safety
/// Must only be called with pointers returned by chainrpc FFI functions.
#[no_mangle]
pub unsafe extern "C" fn chainrpc_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}

/// Library version (static, do NOT free).
#[no_mangle]
pub extern "C" fn chainrpc_version() -> *const c_char {
    static VERSION: &[u8] = b"0.1.0\0";
    VERSION.as_ptr() as *const c_char
}

/// Send a JSON-RPC call to a single HTTP endpoint (blocking).
///
/// `url`         — endpoint URL, e.g. "https://eth-mainnet.g.alchemy.com/v2/KEY"
/// `method`      — JSON-RPC method, e.g. "eth_blockNumber"
/// `params_json` — JSON array of params, e.g. "[]" or "[\"0x...\", \"latest\"]"
///
/// Returns a JSON string with the result, or NULL on error.
/// Caller must free with `chainrpc_free_string`.
#[no_mangle]
pub extern "C" fn chainrpc_call(
    url: *const c_char,
    method: *const c_char,
    params_json: *const c_char,
) -> *mut c_char {
    clear_last_error();
    let url_str = unsafe {
        match CStr::from_ptr(url).to_str() {
            Ok(s) => s.to_owned(),
            Err(_) => { set_last_error("invalid UTF-8 in url"); return std::ptr::null_mut(); }
        }
    };
    let method_str = unsafe {
        match CStr::from_ptr(method).to_str() {
            Ok(s) => s.to_owned(),
            Err(_) => { set_last_error("invalid UTF-8 in method"); return std::ptr::null_mut(); }
        }
    };
    let params_str = unsafe {
        match CStr::from_ptr(params_json).to_str() {
            Ok(s) => s.to_owned(),
            Err(_) => { set_last_error("invalid UTF-8 in params_json"); return std::ptr::null_mut(); }
        }
    };

    let client = match HttpRpcClient::new(&url_str) {
        Ok(c) => c,
        Err(e) => { set_last_error(&e.to_string()); return std::ptr::null_mut(); }
    };

    let params: Vec<serde_json::Value> = match serde_json::from_str(&params_str) {
        Ok(p) => p,
        Err(e) => { set_last_error(&format!("params parse: {e}")); return std::ptr::null_mut(); }
    };

    let req = JsonRpcRequest::new(method_str, params);
    let result = runtime().block_on(async move { client.send(req).await });

    match result {
        Err(e) => { set_last_error(&e.to_string()); std::ptr::null_mut() }
        Ok(resp) => {
            if let Some(err) = resp.error {
                set_last_error(&format!("JSON-RPC {}: {}", err.code, err.message));
                return std::ptr::null_mut();
            }
            let out = resp.result
                .map(|v| v.to_string())
                .unwrap_or_else(|| "null".into());
            match CString::new(out) {
                Ok(s) => s.into_raw(),
                Err(e) => { set_last_error(&e.to_string()); std::ptr::null_mut() }
            }
        }
    }
}

/// Send a JSON-RPC call through a provider pool (blocking).
///
/// `urls_json`   — JSON array of endpoint URLs
/// `method`      — JSON-RPC method
/// `params_json` — JSON array of params
///
/// Returns result JSON string or NULL on error.
/// Caller must free with `chainrpc_free_string`.
#[no_mangle]
pub extern "C" fn chainrpc_pool_call(
    urls_json: *const c_char,
    method: *const c_char,
    params_json: *const c_char,
) -> *mut c_char {
    clear_last_error();
    let urls_str = unsafe {
        match CStr::from_ptr(urls_json).to_str() {
            Ok(s) => s.to_owned(),
            Err(_) => { set_last_error("invalid UTF-8 in urls_json"); return std::ptr::null_mut(); }
        }
    };
    let method_str = unsafe {
        match CStr::from_ptr(method).to_str() {
            Ok(s) => s.to_owned(),
            Err(_) => { set_last_error("invalid UTF-8 in method"); return std::ptr::null_mut(); }
        }
    };
    let params_str = unsafe {
        match CStr::from_ptr(params_json).to_str() {
            Ok(s) => s.to_owned(),
            Err(_) => { set_last_error("invalid UTF-8 in params_json"); return std::ptr::null_mut(); }
        }
    };

    let urls: Vec<String> = match serde_json::from_str(&urls_str) {
        Ok(u) => u,
        Err(e) => { set_last_error(&format!("urls_json parse: {e}")); return std::ptr::null_mut(); }
    };
    let url_refs: Vec<&str> = urls.iter().map(|s| s.as_str()).collect();
    let pool = match ProviderPool::from_urls(&url_refs) {
        Ok(p) => p,
        Err(e) => { set_last_error(&e.to_string()); return std::ptr::null_mut(); }
    };

    let params: Vec<serde_json::Value> = match serde_json::from_str(&params_str) {
        Ok(p) => p,
        Err(e) => { set_last_error(&format!("params parse: {e}")); return std::ptr::null_mut(); }
    };

    let req = JsonRpcRequest::new(method_str, params);
    let result = runtime().block_on(async move { pool.send(req).await });

    match result {
        Err(e) => { set_last_error(&e.to_string()); std::ptr::null_mut() }
        Ok(resp) => {
            if let Some(err) = resp.error {
                set_last_error(&format!("JSON-RPC {}: {}", err.code, err.message));
                return std::ptr::null_mut();
            }
            let out = resp.result
                .map(|v| v.to_string())
                .unwrap_or_else(|| "null".into());
            match CString::new(out) {
                Ok(s) => s.into_raw(),
                Err(e) => { set_last_error(&e.to_string()); std::ptr::null_mut() }
            }
        }
    }
}
