#pragma once
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ── Memory management ─────────────────────────────────────────────────────── */

/** Free a string returned by any chaincodec FFI function. Safe to call with NULL. */
void chaincodec_free_string(char* ptr);

/* ── Error handling ─────────────────────────────────────────────────────────── */

/** Return the last error message (thread-local). NULL if no error. */
const char* chaincodec_last_error(void);

/* ── Core API ───────────────────────────────────────────────────────────────── */

/** Return the library version string (static, do NOT free). */
const char* chaincodec_version(void);

/**
 * Load a CSDL schema file and return a JSON summary of loaded schemas.
 * Returns NULL on error; call chaincodec_last_error() for details.
 * Caller must free the returned string with chaincodec_free_string().
 */
char* chaincodec_load_schema(const char* csdl_path);

/**
 * Count schemas in a directory of .csdl files.
 * Returns -1 on error; call chaincodec_last_error() for details.
 */
int chaincodec_count_schemas(const char* dir_path);

/**
 * Decode an EVM event log.
 * log_json   — {"address":"0x...","topics":["0x..."],"data":"0x..."}
 * schema_json — schema JSON returned by chaincodec_load_schema
 * Returns JSON-encoded decoded event or NULL on error.
 * Caller must free with chaincodec_free_string().
 */
char* chaincodec_decode_event(const char* log_json, const char* schema_json);

#ifdef __cplusplus
}
#endif
