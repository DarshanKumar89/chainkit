#pragma once
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

void        chainindex_free_string(char* ptr);
const char* chainindex_last_error(void);
const char* chainindex_version(void);

/** Return default IndexerConfig as JSON. Caller frees. */
char* chainindex_default_config(void);

/**
 * Parse and validate an IndexerConfig from JSON.
 * Returns normalized JSON or NULL on error. Caller frees.
 */
char* chainindex_parse_config(const char* config_json);

/**
 * Save a checkpoint to the thread-local in-memory store.
 * checkpoint_json — {"chain_id":"...","indexer_id":"...","block_number":N,"block_hash":"0x..."}
 * Returns 0 on success, -1 on error.
 */
int chainindex_save_checkpoint(const char* checkpoint_json);

/**
 * Load a checkpoint from the thread-local in-memory store.
 * Returns JSON checkpoint or NULL if not found. Caller frees.
 */
char* chainindex_load_checkpoint(const char* chain_id, const char* indexer_id);

/**
 * Create an EventFilter JSON for a contract address. Caller frees.
 */
char* chainindex_filter_for_address(const char* address);

#ifdef __cplusplus
}
#endif
