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

pub use crate::abi::allocator::{
    xmlFree, xmlInitMemory, xmlMalloc, xmlMallocAtomic, xmlMallocAtomicZero, xmlMallocZero,
    xmlMemBlocks, xmlMemDisplay, xmlMemGet, xmlMemSetup, xmlMemShow, xmlMemStrdup, xmlMemUsed,
    xmlRealloc, xmlReallocZero,
};

/// Initialize the memory subsystem.
///
/// Called during `xmlInitParser`. Returns 0 on success.
pub fn init_memory() -> i32 {
    xmlInitMemory()
}

/// Clean up the memory subsystem.
pub fn cleanup_memory() {
    // Phase 1: no cleanup needed for the default allocator.
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use core::ffi::c_void;

    #[test]
    fn test_memory_module_delegates_to_allocator() {
        unsafe {
            let ptr = xmlMalloc(100);
            assert!(!ptr.is_null());
            xmlFree(ptr);
        }
    }

    #[test]
    fn test_memory_zero_alloc() {
        unsafe {
            let ptr = xmlMallocZero(64);
            assert!(!ptr.is_null());
            // Verify zero-initialized
            let bytes = core::slice::from_raw_parts(ptr as *const u8, 64);
            assert!(bytes.iter().all(|&b| b == 0));
            xmlFree(ptr);
        }
    }
}
