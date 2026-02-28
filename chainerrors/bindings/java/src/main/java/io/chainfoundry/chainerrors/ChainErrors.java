package io.chainfoundry.chainerrors;

/**
 * Java bindings for the chainerrors Rust library.
 *
 * <p>Load the native library before use:
 * <pre>{@code
 * System.loadLibrary("chainerrors_jni");
 * }</pre>
 *
 * <p>Example:
 * <pre>{@code
 * ChainErrors.loadLibrary();
 * String result = ChainErrors.decode("0x08c379a0" +
 *     "0000000000000000000000000000000000000000000000000000000000000020" +
 *     "000000000000000000000000000000000000000000000000000000000000001a" +
 *     "496e73756666696369656e7420616c6c6f77616e636500000000000000000000");
 * // result: {"kind":"revert_string","message":"Insufficient allowance",...}
 * }</pre>
 */
public class ChainErrors {

    /** Returns the chainerrors library version string. */
    public static native String version();

    /**
     * Decode EVM revert data from a hex string.
     *
     * @param hexData hex-encoded revert bytes (with or without "0x" prefix).
     *                Pass {@code ""} or {@code "0x"} for an empty revert.
     * @return JSON string: {@code {"kind":"...","message":"...","selector":"0x...","confidence":0.95}}
     * @throws RuntimeException    on decode error
     * @throws IllegalArgumentException on invalid hex input
     */
    public static native String decode(String hexData);

    /**
     * Return the human-readable meaning of a Solidity panic code.
     *
     * @param code panic code as integer (e.g. {@code 17} for {@code 0x11} = arithmetic overflow)
     * @return human-readable description
     */
    public static native String panicMeaning(int code);

    /** Load the library from the system library path. */
    public static void loadLibrary() {
        System.loadLibrary("chainerrors_jni");
    }

    /** Load the library from an explicit path. */
    public static void loadLibrary(String path) {
        System.load(path);
    }
}
