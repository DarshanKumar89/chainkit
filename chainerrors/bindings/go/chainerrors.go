// Package chainerrors provides Go bindings for the chainerrors Rust library.
//
// Build the Rust library first:
//
//	cd ../../ && cargo build --release -p chainerrors-ffi
//	cp target/release/libchainerrors_ffi.{dylib,so} bindings/go/
//
// Then build Go:
//
//	CGO_LDFLAGS="-L. -lchainerrors_ffi -ldl -lm" go build .
package chainerrors

/*
#cgo LDFLAGS: -L${SRCDIR} -lchainerrors_ffi -ldl -lm
#include "chainerrors.h"
#include <stdlib.h>
*/
import "C"
import (
	"encoding/json"
	"errors"
	"unsafe"
)

// DecodedError is the result of decoding EVM revert data.
type DecodedError struct {
	Kind       string   `json:"kind"`
	Message    *string  `json:"message"`
	RawData    string   `json:"raw_data"`
	Selector   *string  `json:"selector"`
	Suggestion *string  `json:"suggestion"`
	Confidence float64  `json:"confidence"`
}

// Version returns the chainerrors library version.
func Version() string {
	return C.GoString(C.chainerrors_version())
}

func lastError() error {
	msg := C.chainerrors_last_error()
	if msg == nil {
		return errors.New("unknown FFI error")
	}
	return errors.New(C.GoString(msg))
}

// Decode decodes EVM revert data from a hex string (with or without "0x" prefix).
// Pass an empty string for an empty revert.
func Decode(hexData string) (*DecodedError, error) {
	cHex := C.CString(hexData)
	defer C.free(unsafe.Pointer(cHex))

	ptr := C.chainerrors_decode(cHex)
	if ptr == nil {
		return nil, lastError()
	}
	defer C.chainerrors_free_string(ptr)

	jsonStr := C.GoString(ptr)
	var result DecodedError
	if err := json.Unmarshal([]byte(jsonStr), &result); err != nil {
		return nil, err
	}
	return &result, nil
}

// PanicMeaning returns the human-readable meaning of a Solidity panic code.
// E.g. PanicMeaning(0x11) = "Arithmetic overflow/underflow"
func PanicMeaning(code uint32) string {
	return C.GoString(C.chainerrors_panic_meaning(C.uint32_t(code)))
}
