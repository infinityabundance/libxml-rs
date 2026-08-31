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
//! There are two allocation planes, exactly as in upstream 2.15.0. The five
//! exported variables (`xmlMalloc`, `xmlMallocAtomic`, `xmlRealloc`,
//! `xmlFree`, `xmlMemStrdup`) are the hook system: their default bodies are
//! plain libc `malloc`/`realloc`/`free`/`strdup` wrappers and are UNTRACKED —
//! with the default installed `xmlMemUsed()`/`xmlMemBlocks()`/`xmlMemSize()`
//! all return 0, byte-identical with the oracle (R-000178). `xmlMemSetup` /
//! direct variable assignment re-route the hooks, and custom allocators
//! bypass accounting entirely, matching upstream's debug-allocator-only block
//! table. The debug-named surface (`xmlMemMalloc`/`xmlMemFree`/`xmlMemRealloc`/
//! `xmlMemoryStrdup` and the `*Loc` variants) is the second plane: always
//! libc-backed and tracked by the per-block registry (R-000131), which is
//! what `xmlMemSize` returns sizes from for those blocks. The display entry
//! points (`xmlMemDisplay`, `xmlMemDisplayLast`, `xmlMemShow`,
//! `xmlMemoryDump`) are no-ops matching upstream 2.15.0, which removed that
//! feature.
//!
//! # Ownership & safety invariants
//!
//! Ownership rule (atlas/OWNERSHIP_ATLAS.md): a pointer returned by an xml*
//! allocator must be freed with xmlFree; a pointer from libc::calloc inside
//! the engine is freed internally and never escapes. SAFETY: `xmlFree` on a
//! foreign/unknown pointer is a plain libc `free` — the default free body
//! does not consult the registry at all (the registry is only consulted by
//! the debug-named `xmlMemFree`), exactly like upstream's default
//! `xmlFree = free` (R-000178).
//!
//! # Historical quirks & epochs
//!
//! R-000178 (11.1-Z.3): the pre-Z.3 default allocator routed through Rust's
//! global allocator with fabricated `Layout`s — invalid-layout UB under the
//! Rust allocator contract; replaced with plain libc
//! `malloc`/`realloc`/`free`/`strdup` (C allocation semantics; no layout
//! exists). The pre-Z.3 claim that `xmlFree` on a foreign pointer was a
//! "no-op removal from the registry" is obsolete: the default free is now
//! untracked libc `free`. R-000131 (11.1-J): `xmlMemSize` returns the
//! recorded size for debug-surface blocks and the `*Loc` variants
//! accept-and-ignore file/line exactly like upstream 2.15.0's
//! `ATTRIBUTE_UNUSED` parameters. R-000133 (11.1-H): the legacy debug names
//! were declared-but-unexported and had to be implemented for the
//! honest-header rule.
//!
//! # Deliberate oddities
//!
//! Deliberate oddities: the exported allocator entry points are DATA
//! function-pointer globals matching the oracle ABI (R-000162: upstream
//! exports them as data variables so the `xmlMalloc = custom` override can
//! link), and since 11.1-Z.2 they are the single source of truth —
//! `xmlMemSetup` assigns them and every internal allocation reads them
//! through the `*Impl` indirection, so `xmlMemSetup` and direct
//! `xmlMalloc = custom` assignment share one override mechanism (R-000176).
//! The debug-named functions deliberately do NOT route through the variables
//! (upstream's debug allocator is independent of the hooks).
//!
//! # Proving courts
//!
//! ABI-DATA, ALLOCATOR, GLOBAL-STATE and THREADING court families;
//! ALLOCATOR-DEFAULT-001 (default-allocator contract: many sizes, zero-size,
//! grow/shrink realloc, realloc-to-zero, failure, strdup, direct
//! exported-variable calls, long churn, `xmlMemSize`/`xmlMemUsed`/
//! `xmlMemBlocks` exactness — byte-identical with the oracle, R-000178);
//! ALLOCATOR-HOOK (custom-hook differential, byte-identical); DATA-GLOBALS-001
//! (allocator globals byte-identical); DSO-LOADER (every exported symbol
//! resolved from the built DSO); and `cargo test --lib` (counts generated
//! into atlas/TEST_COUNTS.json by tools/evidence/test_counts.py).
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is routing all allocation through the Rust
//! global allocator — the pre-Z.3 `std::alloc` fabricated-Layout approach was
//! invalid-layout UB (R-000178), and any Rust-allocator route would break
//! xmlMemSetup overrides and the exported xmlMalloc data-symbol ABI
//! (R-000162). Do not make the default tracked: returning nonzero
//! `xmlMemUsed`/`xmlMemBlocks` under the default diverges from the oracle's
//! 0s. Do not restore the display dumps: upstream 2.15.0 removed them, so a
//! per-block dump would diverge.

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
