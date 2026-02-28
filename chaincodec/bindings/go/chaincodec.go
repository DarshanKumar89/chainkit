// Package chaincodec provides Go bindings for the chaincodec Rust library.
//
// Build the Rust library first:
//
//	cd ../../ && cargo build --release -p chaincodec-ffi
//	cp target/release/libchaincodec_ffi.{dylib,so,dll} bindings/go/
//
// Then build Go:
//
//	CGO_LDFLAGS="-L. -lchaincodec_ffi" go build .
package chaincodec

/*
#cgo LDFLAGS: -L${SRCDIR} -lchaincodec_ffi -ldl -lm
#include "chaincodec.h"
#include <stdlib.h>
*/
import "C"
import (
	"errors"
	"unsafe"
)

// Version returns the chaincodec library version string.
func Version() string {
	return C.GoString(C.chaincodec_version())
}

// lastError returns the last FFI error message, or a generic error if none.
func lastError() error {
	msg := C.chaincodec_last_error()
	if msg == nil {
		return errors.New("unknown FFI error")
	}
	return errors.New(C.GoString(msg))
}

// LoadSchema loads a CSDL schema file and returns a JSON summary of all schemas.
func LoadSchema(csdlPath string) (string, error) {
	cPath := C.CString(csdlPath)
	defer C.free(unsafe.Pointer(cPath))

	ptr := C.chaincodec_load_schema(cPath)
	if ptr == nil {
		return "", lastError()
	}
	defer C.chaincodec_free_string(ptr)
	return C.GoString(ptr), nil
}

// CountSchemas counts the number of schemas in a directory of .csdl files.
func CountSchemas(dirPath string) (int, error) {
	cPath := C.CString(dirPath)
	defer C.free(unsafe.Pointer(cPath))

	n := C.chaincodec_count_schemas(cPath)
	if n < 0 {
		return 0, lastError()
	}
	return int(n), nil
}

// DecodeEvent decodes an EVM event log using the provided schema.
//
// logJSON is a JSON object: {"address":"0x...","topics":["0x..."],"data":"0x..."}
// schemaJSON is a schema JSON string (from LoadSchema).
func DecodeEvent(logJSON, schemaJSON string) (string, error) {
	cLog := C.CString(logJSON)
	defer C.free(unsafe.Pointer(cLog))
	cSchema := C.CString(schemaJSON)
	defer C.free(unsafe.Pointer(cSchema))

	ptr := C.chaincodec_decode_event(cLog, cSchema)
	if ptr == nil {
		return "", lastError()
	}
	defer C.chaincodec_free_string(ptr)
	return C.GoString(ptr), nil
}
