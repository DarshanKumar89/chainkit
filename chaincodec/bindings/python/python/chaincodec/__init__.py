"""
chaincodec — Universal blockchain ABI decoder.

Production-grade EVM event log and function call decoding with 50+ built-in
DeFi/NFT/bridge protocol schemas.

Quick start::

    from chaincodec import EvmDecoder, MemoryRegistry

    registry = MemoryRegistry()
    registry.load_file("schemas/erc20.csdl")

    decoder = EvmDecoder()
    event = decoder.decode_event(raw_log, registry.get_by_fingerprint(fp))
    print(event)

Native module is exposed as ``chaincodec._chaincodec``. All public types are
re-exported from this package for a clean import surface.
"""

from chaincodec._chaincodec import (
    # Core decoder
    EvmDecoder,
    EvmCallDecoder,
    EvmEncoder,

    # Registry
    MemoryRegistry,

    # EIP-712
    Eip712Parser,

    # Types
    DecodedEvent,
    DecodedCall,
    NormalizedValue,
)

__version__ = "0.1.0"
__all__ = [
    "EvmDecoder",
    "EvmCallDecoder",
    "EvmEncoder",
    "MemoryRegistry",
    "Eip712Parser",
    "DecodedEvent",
    "DecodedCall",
    "NormalizedValue",
    "__version__",
]
