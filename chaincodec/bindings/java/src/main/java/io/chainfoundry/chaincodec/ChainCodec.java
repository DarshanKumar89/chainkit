package io.chainfoundry.chaincodec;

/**
 * Java bindings for the chaincodec Rust library.
 *
 * <p>Load the native library before use:
 * <pre>{@code
 * System.loadLibrary("chaincodec_jni");
 * // or
 * System.load("/path/to/libchaincodec_jni.so");
 * }</pre>
 *
 * <p>All methods are thread-safe (the underlying Rust library is stateless
 * for schema loading; each call creates a fresh registry).
 */
public class ChainCodec {

    // ── Native methods ────────────────────────────────────────────────────────

    /**
     * Returns the chaincodec library version string (e.g. "0.1.0").
     */
    public static native String version();

    /**
     * Load a CSDL schema file and return a JSON summary of all schemas found.
     *
     * @param csdlPath path to a {@code .csdl} file on disk
     * @return JSON array of schema summaries
     * @throws RuntimeException on parse or IO error
     */
    public static native String loadSchema(String csdlPath);

    /**
     * Count the number of schemas in a directory of {@code .csdl} files.
     *
     * @param dirPath path to directory
     * @return number of schemas, or -1 on error
     * @throws RuntimeException on IO error
     */
    public static native int countSchemas(String dirPath);

    /**
     * Decode an EVM event log using a schema.
     *
     * @param logJson    JSON object: {@code {"address":"0x...","topics":["0x..."],"data":"0x..."}}
     * @param schemaJson schema JSON (from {@link #loadSchema})
     * @return decoded event as JSON string
     * @throws RuntimeException on decode error
     */
    public static native String decodeEvent(String logJson, String schemaJson);

    // ── Convenience wrappers ──────────────────────────────────────────────────

    /**
     * Load the library from the system library path.
     */
    public static void loadLibrary() {
        System.loadLibrary("chaincodec_jni");
    }

    /**
     * Load the library from an explicit path.
     *
     * @param path absolute path to {@code libchaincodec_jni.so} / {@code .dylib} / {@code .dll}
     */
    public static void loadLibrary(String path) {
        System.load(path);
    }
}
