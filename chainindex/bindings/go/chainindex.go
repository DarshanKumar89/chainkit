// Package chainindex provides Go bindings for the chainindex Rust library.
//
// Build the Rust library first:
//
//	cd ../../ && cargo build --release -p chainindex-ffi
//	cp target/release/libchainindex_ffi.{dylib,so} bindings/go/
//
// Then build Go:
//
//	CGO_LDFLAGS="-L. -lchainindex_ffi -ldl -lm" go build .
package chainindex

/*
#cgo LDFLAGS: -L${SRCDIR} -lchainindex_ffi -ldl -lm
#include "chainindex.h"
#include <stdlib.h>
*/
import "C"
import (
	"encoding/json"
	"errors"
	"unsafe"
)

// Checkpoint represents a persisted indexer position.
type Checkpoint struct {
	ChainID    string `json:"chain_id"`
	IndexerID  string `json:"indexer_id"`
	BlockNumber uint64 `json:"block_number"`
	BlockHash  string `json:"block_hash"`
	UpdatedAt  int64  `json:"updated_at"`
}

// IndexerConfig holds configuration for an indexer instance.
type IndexerConfig struct {
	ID                string `json:"id"`
	Chain             string `json:"chain"`
	FromBlock         uint64 `json:"from_block"`
	ToBlock           *uint64 `json:"to_block,omitempty"`
	ConfirmationDepth uint64 `json:"confirmation_depth"`
	BatchSize         uint64 `json:"batch_size"`
	CheckpointInterval uint64 `json:"checkpoint_interval"`
	PollIntervalMs    uint64 `json:"poll_interval_ms"`
}

// EventFilter holds filter criteria for indexed events.
type EventFilter struct {
	Addresses    []string `json:"addresses"`
	Topic0Values []string `json:"topic0_values"`
	FromBlock    *uint64  `json:"from_block,omitempty"`
	ToBlock      *uint64  `json:"to_block,omitempty"`
}

// Version returns the chainindex library version.
func Version() string {
	return C.GoString(C.chainindex_version())
}

func lastError() error {
	msg := C.chainindex_last_error()
	if msg == nil {
		return errors.New("unknown FFI error")
	}
	return errors.New(C.GoString(msg))
}

// DefaultConfig returns an IndexerConfig with sensible defaults.
func DefaultConfig() (*IndexerConfig, error) {
	ptr := C.chainindex_default_config()
	if ptr == nil {
		return nil, lastError()
	}
	defer C.chainindex_free_string(ptr)
	jsonStr := C.GoString(ptr)
	var cfg IndexerConfig
	if err := json.Unmarshal([]byte(jsonStr), &cfg); err != nil {
		return nil, err
	}
	return &cfg, nil
}

// ParseConfig validates and normalizes an IndexerConfig from JSON.
func ParseConfig(configJSON string) (*IndexerConfig, error) {
	cJSON := C.CString(configJSON)
	defer C.free(unsafe.Pointer(cJSON))

	ptr := C.chainindex_parse_config(cJSON)
	if ptr == nil {
		return nil, lastError()
	}
	defer C.chainindex_free_string(ptr)

	var cfg IndexerConfig
	if err := json.Unmarshal([]byte(C.GoString(ptr)), &cfg); err != nil {
		return nil, err
	}
	return &cfg, nil
}

// SaveCheckpoint persists a checkpoint to the thread-local in-memory store.
func SaveCheckpoint(cp Checkpoint) error {
	data, err := json.Marshal(cp)
	if err != nil {
		return err
	}
	cJSON := C.CString(string(data))
	defer C.free(unsafe.Pointer(cJSON))

	if C.chainindex_save_checkpoint(cJSON) != 0 {
		return lastError()
	}
	return nil
}

// LoadCheckpoint retrieves a checkpoint from the thread-local in-memory store.
// Returns nil if no checkpoint exists for the given chain/indexer pair.
func LoadCheckpoint(chainID, indexerID string) (*Checkpoint, error) {
	cChain := C.CString(chainID)
	defer C.free(unsafe.Pointer(cChain))
	cIndexer := C.CString(indexerID)
	defer C.free(unsafe.Pointer(cIndexer))

	ptr := C.chainindex_load_checkpoint(cChain, cIndexer)
	if ptr == nil {
		// Check if it's an error or just "not found"
		errMsg := C.chainindex_last_error()
		if errMsg != nil {
			return nil, errors.New(C.GoString(errMsg))
		}
		return nil, nil // not found
	}
	defer C.chainindex_free_string(ptr)

	var cp Checkpoint
	if err := json.Unmarshal([]byte(C.GoString(ptr)), &cp); err != nil {
		return nil, err
	}
	return &cp, nil
}

// FilterForAddress creates an EventFilter that matches a single contract address.
func FilterForAddress(address string) (*EventFilter, error) {
	cAddr := C.CString(address)
	defer C.free(unsafe.Pointer(cAddr))

	ptr := C.chainindex_filter_for_address(cAddr)
	if ptr == nil {
		return nil, lastError()
	}
	defer C.chainindex_free_string(ptr)

	var f EventFilter
	if err := json.Unmarshal([]byte(C.GoString(ptr)), &f); err != nil {
		return nil, err
	}
	return &f, nil
}
