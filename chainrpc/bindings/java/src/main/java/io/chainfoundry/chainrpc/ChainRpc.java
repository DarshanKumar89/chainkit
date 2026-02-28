package io.chainfoundry.chainrpc;

import java.util.List;
import com.fasterxml.jackson.databind.ObjectMapper;

/**
 * Java bindings for the chainrpc Rust library.
 *
 * <p>Load the native library before use:
 * <pre>{@code
 * System.loadLibrary("chainrpc_jni");
 * }</pre>
 *
 * <p>Example:
 * <pre>{@code
 * ChainRpc.loadLibrary();
 * String blockNumber = ChainRpc.call(
 *     "https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY",
 *     "eth_blockNumber",
 *     "[]"
 * );
 * System.out.println(blockNumber); // "0x12a05f2"
 * }</pre>
 */
public class ChainRpc {

    /** Returns the chainrpc library version. */
    public static native String version();

    /**
     * Send a single JSON-RPC call (blocking).
     *
     * @param url        RPC endpoint URL
     * @param method     JSON-RPC method name (e.g. "eth_blockNumber")
     * @param paramsJson JSON array of params (e.g. "[]" or {@code "[\"0x...\",\"latest\"]"})
     * @return result JSON string
     * @throws RuntimeException on RPC error
     */
    public static native String call(String url, String method, String paramsJson);

    /**
     * Send a JSON-RPC call through a provider pool with automatic failover.
     *
     * @param urlsJson   JSON array of endpoint URL strings
     * @param method     JSON-RPC method name
     * @param paramsJson JSON array of params
     * @return result JSON string
     * @throws RuntimeException on RPC error
     */
    public static native String poolCall(String urlsJson, String method, String paramsJson);

    // ── Convenience helpers ───────────────────────────────────────────────────

    /**
     * Convenience wrapper — converts List&lt;String&gt; urls to JSON automatically.
     *
     * @param urls       list of endpoint URLs
     * @param method     JSON-RPC method name
     * @param paramsJson JSON array of params
     * @return result JSON string
     */
    public static String poolCall(List<String> urls, String method, String paramsJson) {
        try {
            ObjectMapper mapper = new ObjectMapper();
            String urlsJson = mapper.writeValueAsString(urls);
            return poolCall(urlsJson, method, paramsJson);
        } catch (Exception e) {
            throw new RuntimeException("Failed to serialize URLs: " + e.getMessage(), e);
        }
    }

    /** Load the library from the system library path. */
    public static void loadLibrary() {
        System.loadLibrary("chainrpc_jni");
    }

    /** Load the library from an explicit path. */
    public static void loadLibrary(String path) {
        System.load(path);
    }
}
