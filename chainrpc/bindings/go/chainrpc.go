// Package chainrpc provides Go bindings for the chainrpc Rust library.
//
// Build the Rust library first:
//
//	cd ../../ && cargo build --release -p chainrpc-ffi
//	cp target/release/libchainrpc_ffi.{dylib,so} bindings/go/
//
// Then build Go:
//
//	CGO_LDFLAGS="-L. -lchainrpc_ffi -ldl -lm" go build .
package chainrpc

/*
#cgo LDFLAGS: -L${SRCDIR} -lchainrpc_ffi -ldl -lm
#include "chainrpc.h"
#include <stdlib.h>
*/
import "C"
import (
	"errors"
	"unsafe"
)

// Version returns the chainrpc library version.
func Version() string {
	return C.GoString(C.chainrpc_version())
}

func lastError() error {
	msg := C.chainrpc_last_error()
	if msg == nil {
		return errors.New("unknown FFI error")
	}
	return errors.New(C.GoString(msg))
}

// Call sends a single JSON-RPC request to the given URL and returns the result.
//
// paramsJSON should be a JSON array string, e.g. "[]" or `["0x...", "latest"]`.
func Call(url, method, paramsJSON string) (string, error) {
	cURL := C.CString(url)
	defer C.free(unsafe.Pointer(cURL))
	cMethod := C.CString(method)
	defer C.free(unsafe.Pointer(cMethod))
	cParams := C.CString(paramsJSON)
	defer C.free(unsafe.Pointer(cParams))

	ptr := C.chainrpc_call(cURL, cMethod, cParams)
	if ptr == nil {
		return "", lastError()
	}
	defer C.chainrpc_free_string(ptr)
	return C.GoString(ptr), nil
}

// PoolCall sends a JSON-RPC request through a provider pool with automatic failover.
//
// urlsJSON should be a JSON array of URL strings, e.g. `["https://rpc1.example.com", "https://rpc2.example.com"]`.
func PoolCall(urlsJSON, method, paramsJSON string) (string, error) {
	cURLs := C.CString(urlsJSON)
	defer C.free(unsafe.Pointer(cURLs))
	cMethod := C.CString(method)
	defer C.free(unsafe.Pointer(cMethod))
	cParams := C.CString(paramsJSON)
	defer C.free(unsafe.Pointer(cParams))

	ptr := C.chainrpc_pool_call(cURLs, cMethod, cParams)
	if ptr == nil {
		return "", lastError()
	}
	defer C.chainrpc_free_string(ptr)
	return C.GoString(ptr), nil
}
