//! chainindex C FFI — exported symbols for CGo bindings.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::cell::RefCell;
use std::sync::OnceLock;

use tokio::runtime::Runtime;
use chainindex_core::checkpoint::{Checkpoint, CheckpointStore, MemoryCheckpointStore};
use chainindex_core::indexer::IndexerConfig;
use chainindex_core::types::EventFilter;

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to build Tokio runtime")
    })
}

thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = RefCell::new(None);
    static MEMORY_STORE: RefCell<Option<MemoryCheckpointStore>> = RefCell::new(None);
}

fn set_last_error(msg: &str) {
    LAST_ERROR.with(|e| { *e.borrow_mut() = CString::new(msg).ok(); });
}

fn clear_last_error() {
    LAST_ERROR.with(|e| { *e.borrow_mut() = None; });
}

/// Last FFI error message. NULL if no error.
#[no_mangle]
pub extern "C" fn chainindex_last_error() -> *const c_char {
    LAST_ERROR.with(|e| {
        e.borrow().as_ref().map(|s| s.as_ptr()).unwrap_or(std::ptr::null())
    })
}

/// Free a string returned by any chainindex FFI function.
///
/// # Safety
/// Must only be called with pointers returned by chainindex FFI functions.
#[no_mangle]
pub unsafe extern "C" fn chainindex_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}

/// Library version (static, do NOT free).
#[no_mangle]
pub extern "C" fn chainindex_version() -> *const c_char {
    static VERSION: &[u8] = b"0.1.0\0";
    VERSION.as_ptr() as *const c_char
}

/// Create a default IndexerConfig and return it as JSON.
///
/// Returns JSON string or NULL on error. Caller frees with `chainindex_free_string`.
#[no_mangle]
pub extern "C" fn chainindex_default_config() -> *mut c_char {
    let config = IndexerConfig::default();
    match serde_json::to_string(&config) {
        Ok(json) => CString::new(json).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut()),
        Err(e) => { set_last_error(&e.to_string()); std::ptr::null_mut() }
    }
}

/// Parse and validate an IndexerConfig from JSON.
///
/// Returns the normalized JSON on success (round-tripped), NULL on error.
/// Caller frees with `chainindex_free_string`.
#[no_mangle]
pub extern "C" fn chainindex_parse_config(config_json: *const c_char) -> *mut c_char {
    clear_last_error();
    let json_str = unsafe {
        match CStr::from_ptr(config_json).to_str() {
            Ok(s) => s,
            Err(_) => { set_last_error("invalid UTF-8"); return std::ptr::null_mut(); }
        }
    };
    match serde_json::from_str::<IndexerConfig>(json_str) {
        Err(e) => { set_last_error(&format!("parse: {e}")); std::ptr::null_mut() }
        Ok(config) => {
            match serde_json::to_string(&config) {
                Err(e) => { set_last_error(&e.to_string()); std::ptr::null_mut() }
                Ok(out) => CString::new(out).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
            }
        }
    }
}

/// Save a checkpoint to the thread-local in-memory store (blocking).
///
/// `checkpoint_json` — JSON object:
///   {"chain_id":"ethereum","indexer_id":"my-idx","block_number":19000000,"block_hash":"0x..."}
///
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub extern "C" fn chainindex_save_checkpoint(checkpoint_json: *const c_char) -> c_int {
    clear_last_error();
    let json_str = unsafe {
        match CStr::from_ptr(checkpoint_json).to_str() {
            Ok(s) => s,
            Err(_) => { set_last_error("invalid UTF-8"); return -1; }
        }
    };
    let cp: Checkpoint = match serde_json::from_str(json_str) {
        Ok(c) => c,
        Err(e) => { set_last_error(&format!("parse: {e}")); return -1; }
    };

    // Ensure thread-local store exists
    MEMORY_STORE.with(|store| {
        if store.borrow().is_none() {
            *store.borrow_mut() = Some(MemoryCheckpointStore::new());
        }
    });

    let result = MEMORY_STORE.with(|store| {
        let store_ref = store.borrow();
        let store = store_ref.as_ref().unwrap();
        runtime().block_on(store.save(cp))
    });

    match result {
        Ok(()) => 0,
        Err(e) => { set_last_error(&e.to_string()); -1 }
    }
}

/// Load a checkpoint from the thread-local in-memory store (blocking).
///
/// `chain_id`   — chain slug, e.g. "ethereum"
/// `indexer_id` — indexer name, e.g. "my-indexer"
///
/// Returns a JSON checkpoint object or NULL if not found / on error.
/// Caller frees with `chainindex_free_string`.
#[no_mangle]
pub extern "C" fn chainindex_load_checkpoint(
    chain_id: *const c_char,
    indexer_id: *const c_char,
) -> *mut c_char {
    clear_last_error();
    let chain = unsafe {
        match CStr::from_ptr(chain_id).to_str() {
            Ok(s) => s,
            Err(_) => { set_last_error("invalid UTF-8 in chain_id"); return std::ptr::null_mut(); }
        }
    };
    let indexer = unsafe {
        match CStr::from_ptr(indexer_id).to_str() {
            Ok(s) => s,
            Err(_) => { set_last_error("invalid UTF-8 in indexer_id"); return std::ptr::null_mut(); }
        }
    };

    MEMORY_STORE.with(|store| {
        if store.borrow().is_none() {
            *store.borrow_mut() = Some(MemoryCheckpointStore::new());
        }
    });

    let result = MEMORY_STORE.with(|store| {
        let store_ref = store.borrow();
        let s = store_ref.as_ref().unwrap();
        runtime().block_on(s.load(chain, indexer))
    });

    match result {
        Err(e) => { set_last_error(&e.to_string()); std::ptr::null_mut() }
        Ok(None) => std::ptr::null_mut(), // not found — caller checks for NULL
        Ok(Some(cp)) => {
            match serde_json::to_string(&cp) {
                Err(e) => { set_last_error(&e.to_string()); std::ptr::null_mut() }
                Ok(json) => CString::new(json).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
            }
        }
    }
}

/// Create an EventFilter JSON object for a contract address.
///
/// `address` — contract address, e.g. "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"
///
/// Returns JSON string or NULL on error. Caller frees with `chainindex_free_string`.
#[no_mangle]
pub extern "C" fn chainindex_filter_for_address(address: *const c_char) -> *mut c_char {
    clear_last_error();
    let addr = unsafe {
        match CStr::from_ptr(address).to_str() {
            Ok(s) => s,
            Err(_) => { set_last_error("invalid UTF-8 in address"); return std::ptr::null_mut(); }
        }
    };
    let filter = EventFilter::address(addr);
    match serde_json::to_string(&filter) {
        Ok(json) => CString::new(json).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut()),
        Err(e) => { set_last_error(&e.to_string()); std::ptr::null_mut() }
    }
}
