//! chainindex JNI bindings for Java.

#![allow(non_snake_case)]

use std::sync::OnceLock;
use jni::JNIEnv;
use jni::objects::{JClass, JString};
use jni::sys::{jint, jstring};

use tokio::runtime::Runtime;
use chainindex_core::checkpoint::{Checkpoint, CheckpointStore, MemoryCheckpointStore};
use chainindex_core::indexer::IndexerConfig;
use chainindex_core::types::EventFilter;

static RUNTIME: OnceLock<Runtime> = OnceLock::new();
static STORE: OnceLock<MemoryCheckpointStore> = OnceLock::new();

fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to build Tokio runtime")
    })
}

fn store() -> &'static MemoryCheckpointStore {
    STORE.get_or_init(MemoryCheckpointStore::new)
}

fn jstring_to_rust(env: &JNIEnv, s: JString) -> Result<String, jni::errors::Error> {
    env.get_string(s).map(|js| js.into())
}

fn rust_to_jstring<'a>(env: &'a JNIEnv<'a>, s: &str) -> jstring {
    env.new_string(s)
        .map(|js| js.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// Returns the chainindex library version.
#[no_mangle]
pub extern "system" fn Java_io_chainfoundry_chainindex_ChainIndex_version(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    rust_to_jstring(&env, "0.1.0")
}

/// Return default IndexerConfig as JSON.
#[no_mangle]
pub extern "system" fn Java_io_chainfoundry_chainindex_ChainIndex_defaultConfig(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let config = IndexerConfig::default();
    match serde_json::to_string(&config) {
        Ok(json) => rust_to_jstring(&env, &json),
        Err(e) => {
            let _ = env.throw_new("java/lang/RuntimeException", e.to_string());
            std::ptr::null_mut()
        }
    }
}

/// Save a checkpoint to the global in-memory store (blocking).
///
/// checkpointJson — JSON object with chain_id, indexer_id, block_number, block_hash
/// Returns 0 on success, -1 on error (throws).
#[no_mangle]
pub extern "system" fn Java_io_chainfoundry_chainindex_ChainIndex_saveCheckpoint(
    env: JNIEnv,
    _class: JClass,
    checkpoint_json: JString,
) -> jint {
    let json = match jstring_to_rust(&env, checkpoint_json) {
        Ok(s) => s,
        Err(e) => { let _ = env.throw_new("java/lang/IllegalArgumentException", e.to_string()); return -1; }
    };
    let cp: Checkpoint = match serde_json::from_str(&json) {
        Ok(c) => c,
        Err(e) => { let _ = env.throw_new("java/lang/IllegalArgumentException", format!("parse: {e}")); return -1; }
    };

    match runtime().block_on(store().save(cp)) {
        Ok(()) => 0,
        Err(e) => { let _ = env.throw_new("java/lang/RuntimeException", e.to_string()); -1 }
    }
}

/// Load a checkpoint from the global in-memory store (blocking).
///
/// Returns JSON checkpoint string or null if not found.
#[no_mangle]
pub extern "system" fn Java_io_chainfoundry_chainindex_ChainIndex_loadCheckpoint(
    env: JNIEnv,
    _class: JClass,
    chain_id: JString,
    indexer_id: JString,
) -> jstring {
    let chain = match jstring_to_rust(&env, chain_id) {
        Ok(s) => s,
        Err(e) => { let _ = env.throw_new("java/lang/IllegalArgumentException", e.to_string()); return std::ptr::null_mut(); }
    };
    let indexer = match jstring_to_rust(&env, indexer_id) {
        Ok(s) => s,
        Err(e) => { let _ = env.throw_new("java/lang/IllegalArgumentException", e.to_string()); return std::ptr::null_mut(); }
    };

    match runtime().block_on(store().load(&chain, &indexer)) {
        Err(e) => { let _ = env.throw_new("java/lang/RuntimeException", e.to_string()); std::ptr::null_mut() }
        Ok(None) => std::ptr::null_mut(),
        Ok(Some(cp)) => {
            match serde_json::to_string(&cp) {
                Ok(json) => rust_to_jstring(&env, &json),
                Err(e) => { let _ = env.throw_new("java/lang/RuntimeException", e.to_string()); std::ptr::null_mut() }
            }
        }
    }
}

/// Create an EventFilter JSON for a contract address.
#[no_mangle]
pub extern "system" fn Java_io_chainfoundry_chainindex_ChainIndex_filterForAddress(
    env: JNIEnv,
    _class: JClass,
    address: JString,
) -> jstring {
    let addr = match jstring_to_rust(&env, address) {
        Ok(s) => s,
        Err(e) => { let _ = env.throw_new("java/lang/IllegalArgumentException", e.to_string()); return std::ptr::null_mut(); }
    };
    let filter = EventFilter::address(addr);
    match serde_json::to_string(&filter) {
        Ok(json) => rust_to_jstring(&env, &json),
        Err(e) => { let _ = env.throw_new("java/lang/RuntimeException", e.to_string()); std::ptr::null_mut() }
    }
}
