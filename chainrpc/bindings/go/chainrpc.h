#pragma once
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

void        chainrpc_free_string(char* ptr);
const char* chainrpc_last_error(void);
const char* chainrpc_version(void);

/**
 * Send a single JSON-RPC call (blocking).
 * url         — endpoint URL
 * method      — JSON-RPC method name
 * params_json — JSON array, e.g. "[]" or "[\"0x...\",\"latest\"]"
 * Returns JSON result string or NULL on error.
 * Caller frees with chainrpc_free_string().
 */
char* chainrpc_call(const char* url, const char* method, const char* params_json);

/**
 * Send a JSON-RPC call via a provider pool (blocking).
 * urls_json   — JSON array of endpoint URL strings
 * method      — JSON-RPC method name
 * params_json — JSON array of params
 * Returns JSON result string or NULL on error.
 * Caller frees with chainrpc_free_string().
 */
char* chainrpc_pool_call(const char* urls_json, const char* method, const char* params_json);

#ifdef __cplusplus
}
#endif
