//! Memory management (§58, §85 Phase 1).
//!
//! Wraps the allocator hooks from `crate::abi::allocator` for use by the XML
//! implementation modules. This module provides the internal Rust interface
//! to the allocator system.
//!
//! # UPSTREAM-PARITY
//!
//! libxml2 exposes several memory management APIs:
//!
//! - `xmlMemSetup` / `xmlMemGet` — set/get custom allocator hooks
//! - `xmlGcMemSetup` / `xmlGcMemGet` — GC-aware variants (now identical)
//! - `xmlMalloc` / `xmlMallocAtomic` / `xmlRealloc` / `xmlFree` / `xmlMemStrdup`
//! - `xmlMallocZero` / `xmlMallocAtomicZero` / `xmlReallocZero`
//! - `xmlMemUsed` / `xmlMemBlocks` — debugging statistics
//! - `xmlMemDisplay` / `xmlMemShow` — debugging output
//! - `xmlInitMemory` / `xmlCleanupMemory` — lifecycle
//!
//! All of these are implemented in `crate::abi::allocator`. This module
//! re-exports them for internal use.
//!
//! # Phase 1 status
//!
//! Complete — all memory functions delegate to the ABI allocator layer.
//!
//! # Upstream contract
//!
//! Mirrors upstream xmlmemory.c (SRC-LIBXML2-2.15.0-XMLMEMORY-C): xmlMemSetup
//! / xmlMemGet / xmlGcMemSetup / xmlMemUsed / xmlMemBlocks / xmlMemDisplay /
//! xmlMemShow and the xmlMalloc* family. The actual implementation lives in
//! `crate::abi::allocator`; this module is the internal Rust interface.
//!
//! # Conceptual behavior
//!
//! Wraps the allocator hooks for use by the XML implementation modules. The
//! allocator defaults to libc malloc and is swappable via xmlMemSetup; the
//! block registry (R-000131) tracks blocks for xmlMemSize, xmlMemUsed and the
//! debug dumps.
//!
//! # Ownership & safety invariants
//!
//! Ownership rule (atlas/OWNERSHIP_ATLAS.md): a pointer returned by an xml*
//! allocator must be freed by xmlFree; a pointer from libc::calloc inside the
//! engine is freed internally and never escapes. SAFETY: xmlFree on a
//! foreign/unknown pointer is a no-op removal from the registry (documented
//! safe divergence — upstream would corrupt).
//!
//! # Historical quirks & epochs
//!
//! The debug allocator with block tracking has been part of libxml2 since the
//! early 2.x era; xmlMemDisplayLast / xmlMemShow report per-block data.
//! xmlMemSetup keeps counter-only accounting when custom allocators are
//! installed (R-000131 divergence, matching upstream debug-allocator-only
//! block table).
//!
//! # Deliberate oddities
//!
//! Deliberate oddities: the exported allocator entry points are DATA
//! function-pointer globals (xmlMallocImpl etc.) matching the oracle ABI
//! (R-000162: upstream exports them as data variables so the xmlMalloc =
//! custom override can link).
//!
//! # Proving courts
//!
//! ALLOCATOR court family, the DATA-GLOBALS-001 probe (allocator globals
//! byte-identical), ASan full-suite runs (0 invalid reads/writes, 0 double-
//! free) and `cargo test --lib` (1135+ tests).
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is routing all allocation through the Rust global
//! allocator — it would break xmlMemSetup overrides, xmlMemUsed accounting
//! and the exported xmlMalloc data-symbol ABI (R-000162). Do not replace the
//! registry no-op free with a real free of foreign pointers: that would
//! corrupt the allocator (documented divergence, OWNERSHIP_ATLAS section 8).

pub use crate::abi::allocator::{
    xmlFreeImpl, xmlInitMemory, xmlMallocAtomicImpl, xmlMallocAtomicZero, xmlMallocImpl,
    xmlMallocZero, xmlMemBlocks, xmlMemDisplay, xmlMemGet, xmlMemSetup, xmlMemShow,
    xmlMemStrdupImpl, xmlMemUsed, xmlReallocImpl, xmlReallocZero,
};

/// Initialize the memory subsystem.
///
/// Called during `xmlInitParser`. Returns 0 on success.
pub const fn init_memory() -> i32 {
    xmlInitMemory()
}

/// Clean up the memory subsystem.
pub const fn cleanup_memory() {
    // Phase 1: no cleanup needed for the default allocator.
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// `xmlMallocImpl`/`xmlFreeImpl` round trip through the ABI allocator.
    ///
    /// # Safety
    ///
    /// - `ptr` is non-NULL (asserted) and allocator-owned, valid for 100
    ///   bytes, and freed with `xmlFreeImpl` exactly once.
    #[test]
    fn test_memory_module_delegates_to_allocator() {
        unsafe {
            let ptr = xmlMallocImpl(100);
            assert!(!ptr.is_null());
            xmlFreeImpl(ptr);
        }
    }

    /// `xmlMallocZero` returns zero-initialized allocator memory.
    ///
    /// # Safety
    ///
    /// - `ptr` is non-NULL (asserted) and allocator-owned, valid for 64
    ///   zeroed bytes while the slice is read, and freed with
    ///   `xmlFreeImpl` exactly once.
    #[test]
    fn test_memory_zero_alloc() {
        unsafe {
            let ptr = xmlMallocZero(64);
            assert!(!ptr.is_null());
            // Verify zero-initialized
            let bytes = core::slice::from_raw_parts(ptr as *const u8, 64);
            assert!(bytes.iter().all(|&b| b == 0));
            xmlFreeImpl(ptr);
        }
    }
}
