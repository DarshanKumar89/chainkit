#pragma once
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/** Free a string returned by any chainerrors FFI function. Safe with NULL. */
void chainerrors_free_string(char* ptr);

/** Last FFI error message (thread-local). NULL if no error. */
const char* chainerrors_last_error(void);

/** Library version (static, do NOT free). */
const char* chainerrors_version(void);

/**
 * Decode EVM revert data.
 * hex_data — hex-encoded bytes (with or without "0x" prefix). Pass "" for empty.
 * Returns JSON or NULL on error. Caller frees with chainerrors_free_string().
 *
 * JSON shape:
 *   {"kind": "revert_string"|"custom_error"|"panic"|"raw_revert"|"out_of_gas"|"succeeded",
 *    "message": "...", "raw_data": "0x...", "selector": "0x...",
 *    "suggestion": "...", "confidence": 0.95}
 */
char* chainerrors_decode(const char* hex_data);

/**
 * Return human-readable meaning of a Solidity panic code.
 * code — decimal value (e.g. 17 = 0x11 = arithmetic overflow).
 * Returns static string — do NOT free.
 */
const char* chainerrors_panic_meaning(uint32_t code);

#ifdef __cplusplus
}
#endif
