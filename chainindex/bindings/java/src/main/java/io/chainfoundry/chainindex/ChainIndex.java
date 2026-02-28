package io.chainfoundry.chainindex;

/**
 * Java bindings for the chainindex Rust library.
 *
 * <p>Load the native library before use:
 * <pre>{@code
 * System.loadLibrary("chainindex_jni");
 * }</pre>
 *
 * <p>Example:
 * <pre>{@code
 * ChainIndex.loadLibrary();
 *
 * // Get default config
 * String configJson = ChainIndex.defaultConfig();
 *
 * // Save a checkpoint
 * String cpJson = "{\"chain_id\":\"ethereum\",\"indexer_id\":\"my-idx\"," +
 *     "\"block_number\":19000000,\"block_hash\":\"0xabc\",\"updated_at\":0}";
 * ChainIndex.saveCheckpoint(cpJson);
 *
 * // Load it back
 * String loaded = ChainIndex.loadCheckpoint("ethereum", "my-idx");
 * }</pre>
 */
public class ChainIndex {

    /** Returns the chainindex library version. */
    public static native String version();

    /**
     * Return the default IndexerConfig as a JSON string.
     *
     * @return JSON config with Ethereum defaults (confirmation_depth=12, batch_size=1000, etc.)
     */
    public static native String defaultConfig();

    /**
     * Save a checkpoint to the in-memory store (blocking).
     *
     * @param checkpointJson JSON: {@code {"chain_id":"...","indexer_id":"...","block_number":N,"block_hash":"0x..."}}
     * @return 0 on success, -1 on error
     * @throws RuntimeException    on storage error
     * @throws IllegalArgumentException on JSON parse error
     */
    public static native int saveCheckpoint(String checkpointJson);

    /**
     * Load a checkpoint from the in-memory store (blocking).
     *
     * @param chainId   chain slug (e.g. "ethereum")
     * @param indexerId indexer name (e.g. "my-indexer")
     * @return checkpoint JSON string, or {@code null} if no checkpoint exists
     * @throws RuntimeException on storage error
     */
    public static native String loadCheckpoint(String chainId, String indexerId);

    /**
     * Create an EventFilter JSON for a single contract address.
     *
     * @param address contract address (e.g. "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48")
     * @return JSON EventFilter string
     */
    public static native String filterForAddress(String address);

    /** Load the library from the system library path. */
    public static void loadLibrary() {
        System.loadLibrary("chainindex_jni");
    }

    /** Load the library from an explicit path. */
    public static void loadLibrary(String path) {
        System.load(path);
    }
}
