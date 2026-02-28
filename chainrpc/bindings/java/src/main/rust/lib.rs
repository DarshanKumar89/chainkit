//! chainrpc JNI bindings for Java.

#![allow(non_snake_case)]

use std::sync::OnceLock;
use jni::JNIEnv;
use jni::objects::{JClass, JString, JObjectArray};
use jni::sys::jstring;

use tokio::runtime::Runtime;
use chainrpc_http::HttpRpcClient;
use chainrpc_core::{pool::ProviderPool, request::JsonRpcRequest};

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to build Tokio runtime")
    })
}

fn jstring_to_rust(env: &JNIEnv, s: JString) -> Result<String, jni::errors::Error> {
    env.get_string(s).map(|js| js.into())
}

fn rust_to_jstring<'a>(env: &'a JNIEnv<'a>, s: &str) -> jstring {
    env.new_string(s)
        .map(|js| js.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// Returns the chainrpc library version.
#[no_mangle]
pub extern "system" fn Java_io_chainfoundry_chainrpc_ChainRpc_version(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    rust_to_jstring(&env, "0.1.0")
}

/// Send a single JSON-RPC call (blocking).
///
/// url         — endpoint URL
/// method      — JSON-RPC method name
/// paramsJson  — JSON array, e.g. "[]" or "[\"0x...\",\"latest\"]"
///
/// Returns result JSON string.
/// Throws RuntimeException on error.
#[no_mangle]
pub extern "system" fn Java_io_chainfoundry_chainrpc_ChainRpc_call(
    env: JNIEnv,
    _class: JClass,
    url: JString,
    method: JString,
    params_json: JString,
) -> jstring {
    let url_str = match jstring_to_rust(&env, url) {
        Ok(s) => s,
        Err(e) => { let _ = env.throw_new("java/lang/IllegalArgumentException", e.to_string()); return std::ptr::null_mut(); }
    };
    let method_str = match jstring_to_rust(&env, method) {
        Ok(s) => s,
        Err(e) => { let _ = env.throw_new("java/lang/IllegalArgumentException", e.to_string()); return std::ptr::null_mut(); }
    };
    let params_str = match jstring_to_rust(&env, params_json) {
        Ok(s) => s,
        Err(e) => { let _ = env.throw_new("java/lang/IllegalArgumentException", e.to_string()); return std::ptr::null_mut(); }
    };

    let client = match HttpRpcClient::new(&url_str) {
        Ok(c) => c,
        Err(e) => { let _ = env.throw_new("java/lang/RuntimeException", e.to_string()); return std::ptr::null_mut(); }
    };
    let params: Vec<serde_json::Value> = match serde_json::from_str(&params_str) {
        Ok(p) => p,
        Err(e) => { let _ = env.throw_new("java/lang/IllegalArgumentException", format!("params: {e}")); return std::ptr::null_mut(); }
    };
    let req = JsonRpcRequest::new(method_str, params);

    match runtime().block_on(async move { client.send(req).await }) {
        Err(e) => { let _ = env.throw_new("java/lang/RuntimeException", e.to_string()); std::ptr::null_mut() }
        Ok(resp) => {
            if let Some(err) = resp.error {
                let _ = env.throw_new("java/lang/RuntimeException",
                    format!("JSON-RPC {}: {}", err.code, err.message));
                return std::ptr::null_mut();
            }
            let out = resp.result.map(|v| v.to_string()).unwrap_or_else(|| "null".into());
            rust_to_jstring(&env, &out)
        }
    }
}

/// Send a JSON-RPC call through a provider pool (blocking).
///
/// urlsJson    — JSON array of endpoint URL strings
/// method      — JSON-RPC method name
/// paramsJson  — JSON array of params
///
/// Returns result JSON string.
/// Throws RuntimeException on error.
#[no_mangle]
pub extern "system" fn Java_io_chainfoundry_chainrpc_ChainRpc_poolCall(
    env: JNIEnv,
    _class: JClass,
    urls_json: JString,
    method: JString,
    params_json: JString,
) -> jstring {
    let urls_str = match jstring_to_rust(&env, urls_json) {
        Ok(s) => s,
        Err(e) => { let _ = env.throw_new("java/lang/IllegalArgumentException", e.to_string()); return std::ptr::null_mut(); }
    };
    let method_str = match jstring_to_rust(&env, method) {
        Ok(s) => s,
        Err(e) => { let _ = env.throw_new("java/lang/IllegalArgumentException", e.to_string()); return std::ptr::null_mut(); }
    };
    let params_str = match jstring_to_rust(&env, params_json) {
        Ok(s) => s,
        Err(e) => { let _ = env.throw_new("java/lang/IllegalArgumentException", e.to_string()); return std::ptr::null_mut(); }
    };

    let urls: Vec<String> = match serde_json::from_str(&urls_str) {
        Ok(u) => u,
        Err(e) => { let _ = env.throw_new("java/lang/IllegalArgumentException", format!("urls: {e}")); return std::ptr::null_mut(); }
    };
    let url_refs: Vec<&str> = urls.iter().map(|s| s.as_str()).collect();
    let pool = match ProviderPool::from_urls(&url_refs) {
        Ok(p) => p,
        Err(e) => { let _ = env.throw_new("java/lang/RuntimeException", e.to_string()); return std::ptr::null_mut(); }
    };
    let params: Vec<serde_json::Value> = match serde_json::from_str(&params_str) {
        Ok(p) => p,
        Err(e) => { let _ = env.throw_new("java/lang/IllegalArgumentException", format!("params: {e}")); return std::ptr::null_mut(); }
    };
    let req = JsonRpcRequest::new(method_str, params);

    match runtime().block_on(async move { pool.send(req).await }) {
        Err(e) => { let _ = env.throw_new("java/lang/RuntimeException", e.to_string()); std::ptr::null_mut() }
        Ok(resp) => {
            if let Some(err) = resp.error {
                let _ = env.throw_new("java/lang/RuntimeException",
                    format!("JSON-RPC {}: {}", err.code, err.message));
                return std::ptr::null_mut();
            }
            let out = resp.result.map(|v| v.to_string()).unwrap_or_else(|| "null".into());
            rust_to_jstring(&env, &out)
        }
    }
}
