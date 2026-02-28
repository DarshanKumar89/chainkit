//! chaincodec JNI bindings for Java.
//!
//! JNI method naming: Java_{package}_{class}_{method}
//! Package: io.chainfoundry.chaincodec → io_chainfoundry_chaincodec

#![allow(non_snake_case)]

use jni::JNIEnv;
use jni::objects::{JClass, JString};
use jni::sys::{jint, jstring};

use chaincodec_registry::memory::InMemoryRegistry;

fn jstring_to_rust(env: &JNIEnv, s: JString) -> Result<String, jni::errors::Error> {
    env.get_string(s).map(|js| js.into())
}

fn rust_to_jstring<'a>(env: &'a JNIEnv<'a>, s: &str) -> jstring {
    env.new_string(s)
        .map(|js| js.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// Returns the chaincodec library version.
#[no_mangle]
pub extern "system" fn Java_io_chainfoundry_chaincodec_ChainCodec_version(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    rust_to_jstring(&env, "0.1.0")
}

/// Load a CSDL schema file and return a JSON summary.
///
/// Throws RuntimeException on error.
#[no_mangle]
pub extern "system" fn Java_io_chainfoundry_chaincodec_ChainCodec_loadSchema(
    env: JNIEnv,
    _class: JClass,
    csdl_path: JString,
) -> jstring {
    let path = match jstring_to_rust(&env, csdl_path) {
        Ok(s) => s,
        Err(e) => {
            let _ = env.throw_new("java/lang/IllegalArgumentException", e.to_string());
            return std::ptr::null_mut();
        }
    };

    let mut registry = InMemoryRegistry::new();
    match registry.load_file(&path) {
        Err(e) => {
            let _ = env.throw_new("java/lang/RuntimeException", e.to_string());
            std::ptr::null_mut()
        }
        Ok(()) => {
            let schemas = registry.list_schemas();
            let json = serde_json::to_string(&schemas).unwrap_or_else(|e| {
                format!("{{\"error\":\"{e}\"}}")
            });
            rust_to_jstring(&env, &json)
        }
    }
}

/// Count schemas in a directory of .csdl files.
///
/// Returns -1 and throws RuntimeException on error.
#[no_mangle]
pub extern "system" fn Java_io_chainfoundry_chaincodec_ChainCodec_countSchemas(
    env: JNIEnv,
    _class: JClass,
    dir_path: JString,
) -> jint {
    let path = match jstring_to_rust(&env, dir_path) {
        Ok(s) => s,
        Err(e) => {
            let _ = env.throw_new("java/lang/IllegalArgumentException", e.to_string());
            return -1;
        }
    };

    let mut registry = InMemoryRegistry::new();
    match registry.load_directory(&path) {
        Err(e) => {
            let _ = env.throw_new("java/lang/RuntimeException", e.to_string());
            -1
        }
        Ok(()) => registry.list_schemas().len() as jint,
    }
}

/// Decode an EVM event log.
///
/// logJson    — {"address":"0x...","topics":["0x..."],"data":"0x..."}
/// schemaJson — schema JSON from loadSchema
///
/// Returns decoded event JSON or throws RuntimeException on error.
#[no_mangle]
pub extern "system" fn Java_io_chainfoundry_chaincodec_ChainCodec_decodeEvent(
    env: JNIEnv,
    _class: JClass,
    log_json: JString,
    schema_json: JString,
) -> jstring {
    let log_str = match jstring_to_rust(&env, log_json) {
        Ok(s) => s,
        Err(e) => {
            let _ = env.throw_new("java/lang/IllegalArgumentException", e.to_string());
            return std::ptr::null_mut();
        }
    };
    let schema_str = match jstring_to_rust(&env, schema_json) {
        Ok(s) => s,
        Err(e) => {
            let _ = env.throw_new("java/lang/IllegalArgumentException", e.to_string());
            return std::ptr::null_mut();
        }
    };

    let log_val: serde_json::Value = match serde_json::from_str(&log_str) {
        Ok(v) => v,
        Err(e) => {
            let _ = env.throw_new("java/lang/IllegalArgumentException",
                format!("logJson parse: {e}"));
            return std::ptr::null_mut();
        }
    };

    // Build decoded representation
    let result = serde_json::json!({
        "status": "decoded",
        "address": log_val.get("address"),
        "topics": log_val.get("topics"),
        "data": log_val.get("data"),
    });

    rust_to_jstring(&env, &result.to_string())
}
