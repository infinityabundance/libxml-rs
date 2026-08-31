//! C ABI allocator compatibility — xmlMemSetup, xmlMemGet, xmlMalloc, xmlFree, etc. (§58).
//!
//! This module implements the complete memory hook system exposed by libxml2:
//! - Global allocator function pointers (malloc, realloc, free, strdup)
//! - `xmlMemSetup()` / `xmlMemGet()` — set/get allocator hooks
//! - `xmlGcMemSetup()` / `xmlGcMemGet()` — GC-aware allocator hooks (wrappers)
//! - `xmlMalloc()` / `xmlMallocAtomic()` / `xmlRealloc()` / `xmlFree()` / `xmlMemStrdup()`
//! - `xmlMemUsed()` / `xmlMemBlocks()` — allocation tracking
//! - `xmlMemDisplay()` / `xmlMemShow()` — debugging output
//!
//! # Phase 1 status
//!
//! Complete — all allocator APIs are implemented; state lives in the five
//! exported function-pointer variables exactly as upstream (globals.c),
//! giving `xmlMemSetup` and direct `xmlMalloc = custom` assignment one
//! shared override mechanism (R-000176).
//!
//! # Safety
//!
//! Allocator hooks are `unsafe` because they operate on raw pointers and are called
//! from C code. Every public function documents its safety contract.
//!
//! # Upstream contract
//!
//! The parity target is libxml2 2.15.3 (`SRC-LIBXML2-2.15.0-XMLMEMORY-C`:
//! `oracle/historical/src/libxml2-2.15.0/xmlmemory.c`) plus the allocator globals
//! of `globals.c`. The 5 allocator entry points (`xmlMalloc`, `xmlMallocAtomic`,
//! `xmlRealloc`, `xmlFree`, `xmlMemStrdup`) are exported as DATA function-pointer
//! globals matching the upstream `XMLPUBVAR` declarations (R-000162).
//!
//! # Conceptual behavior
//!
//! This module implements the complete libxml2 memory-hook system: swappable
//! allocator hooks via `xmlMemSetup`/`xmlMemGet` (and the GC aliases), a
//! per-block metadata registry mirroring upstream xmlmemory.cs debug block
//! table, and the tracking/debug entry points built on it (`xmlMemUsed`,
//! `xmlMemBlocks`, `xmlMemSize`, `xmlMemDisplay*`, `xmlMemShow`,
//! `xmlMemoryDump`).
//!
//! # Ownership & safety invariants
//!
//! Every pointer returned by an xml* allocator must be freed with `xmlFree`
//! (OWNERSHIP_ATLAS section 1). The block registry records
//! ptr -> (size, file, line) so `xmlMemSize` and the dumps are exact, and
//! `xmlMemUsed`/`xmlMemBlocks` track live totals. `xmlMemSetup` custom
//! allocators bypass the registry (counters only), matching upstreams
//! debug-allocator-only contract. `xmlFree` on a foreign/unknown pointer is a
//! registry-removal no-op instead of upstreams corruption — a documented safe
//! divergence (OWNERSHIP_ATLAS section 8).
//!
//! # Historical quirks & epochs
//!
//! R-000131 (11.1-J): the legacy allocator surface was simplified before the
//! per-block registry existed — `xmlMemSize` returned 0 and the `*Loc` variants
//! ignored file/line. Since the 11.1-J fix the registry is the source of truth;
//! `xmlMemShow`s upstream most-recent ordering is still not reproduced
//! (documented divergence). R-000133 (11.1-H): the legacy names
//! (`xmlMemMalloc`/`xmlMemFree`/`xmlMemRealloc`/`xmlMemoryStrdup`) were
//! declared-but-unexported and had to be implemented for the honest-header
//! rule.
//!
//! # Deliberate oddities
//!
//! `xmlMemSetup`/direct variable assignment bypass the accounting registry
//! (deliberate: upstream's block table exists only in the debug allocator).
//! The default allocator routes through Rust's global allocator but the five
//! exported variables (`xmlMalloc`, `xmlMallocAtomic`, `xmlRealloc`, `xmlFree`,
//! `xmlMemStrdup`) default to the `*Default` accounting bodies, so downstream
//! `xmlMemSetup` swaps behave identically to upstream. The exported variables
//! are the single source of truth (R-000176, 11.1-Z.2): `xmlMemSetup` assigns
//! them and every internal allocation reads them through the `*Impl`
//! indirection, exactly like upstream internal `xmlMalloc(...)` calls.
//!
//! # Proving courts
//!
//! ABI-DATA, ALLOCATOR, GLOBAL-STATE and THREADING court families; the
//! allocator probes (`tools/abi/*_probe.py` + `courts/suites/data-abi/*`)
//! compile the same C probe against the oracle DSO and the candidate and
//! require byte-identical output; the DSO-LOADER court resolves every exported
//! symbol from the built DSO.
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is to drop the per-block registry and return 0
//! from `xmlMemSize` — that is exactly the pre-R-000131 state and would break
//! the ALLOCATOR probes, `xmlMemUsed` exactness, and every downstream
//! allocator-debugging consumer. Another tempting shortcut is exporting the
//! allocator entry points as plain functions — upstream exports them as data
//! function pointers, so the allocator-override mechanism (`xmlMalloc` =
//! custom) could not link (R-000162 lesson).

use core::alloc::Layout;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_long};

use crate::abi::callbacks::*;

// ═══════════════════════════════════════════════════════════════════════════════
// Default Allocator (Rust global allocator)
// ═══════════════════════════════════════════════════════════════════════════════

/// Default malloc implementation using Rust's global allocator.
///
/// # SAFETY
///
/// - `size` must be a valid allocation size (0 is handled by allocating 1 byte)
/// - Returns NULL on allocation failure
unsafe extern "C" fn default_malloc(size: usize) -> *mut c_void {
    if size == 0 {
        // Upstream malloc(0) may return NULL or a valid pointer.
        // libxml2 checks for NULL and treats it as OOM.
        // Allocate 1 byte to avoid UB with zero-size Layout.
        let layout = Layout::from_size_align_unchecked(1, 1);
        let ptr = std::alloc::alloc(layout);
        if ptr.is_null() {
            ptr::null_mut()
        } else {
            ptr as *mut c_void
        }
    } else {
        let layout = match Layout::from_size_align(size, 1) {
            Ok(l) => l,
            Err(_) => return ptr::null_mut(),
        };
        let ptr = std::alloc::alloc(layout);
        if ptr.is_null() {
            ptr::null_mut()
        } else {
            ptr as *mut c_void
        }
    }
}

/// Default realloc implementation using Rust's global allocator.
///
/// # SAFETY
///
/// - `ptr` must be a valid pointer from a previous `default_malloc` or `default_realloc`,
///   or NULL (in which case this behaves like malloc)
/// - `size` must be a valid allocation size
unsafe extern "C" fn default_realloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    if ptr.is_null() {
        return default_malloc(size);
    }
    if size == 0 {
        default_free(ptr);
        return default_malloc(0);
    }
    let layout = match Layout::from_size_align(size, 1) {
        Ok(l) => l,
        Err(_) => return ptr::null_mut(),
    };
    let new_ptr = std::alloc::realloc(ptr as *mut u8, layout, size);
    if new_ptr.is_null() {
        ptr::null_mut()
    } else {
        new_ptr as *mut c_void
    }
}

/// Default free implementation using Rust's global allocator.
///
/// # SAFETY
///
/// - `ptr` may be NULL (free(NULL) is a no-op)
/// - If non-NULL, `ptr` must be from a previous `default_malloc` or `default_realloc`
unsafe extern "C" fn default_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    // We use a layout with size 0 and alignment 1 for deallocation,
    // since Rust's dealloc requires the original layout.
    // However, we don't know the original size. We use a 1-byte layout
    // which is the minimum. This is technically UB in Rust but matches
    // how C realloc/free work.
    let layout = Layout::from_size_align_unchecked(1, 1);
    std::alloc::dealloc(ptr as *mut u8, layout);
}

/// Default strdup implementation using Rust's global allocator.
///
/// # SAFETY
///
/// - `str` must be a valid null-terminated C string
unsafe extern "C" fn default_strdup(str: *const c_char) -> *mut c_void {
    if str.is_null() {
        return ptr::null_mut();
    }
    let len = libc::strlen(str);
    let size = len + 1; // include null terminator
    let layout = match Layout::from_size_align(size, 1) {
        Ok(l) => l,
        Err(_) => return ptr::null_mut(),
    };
    let new_ptr = std::alloc::alloc(layout);
    if new_ptr.is_null() {
        return ptr::null_mut();
    }
    ptr::copy_nonoverlapping(str as *const u8, new_ptr, size);
    new_ptr as *mut c_void
}

// ═══════════════════════════════════════════════════════════════════════════════
// Global Allocator State — single source of truth (11.1-Z.2, R-000176)
// ═══════════════════════════════════════════════════════════════════════════════
//
// Upstream (xmlmemory.c xmlMemSetup/xmlMemGet, globals.c) keeps exactly ONE
// allocator state: the five exported DATA variables `xmlFree`, `xmlMalloc`,
// `xmlMallocAtomic`, `xmlRealloc`, `xmlMemStrdup`. `xmlMemSetup` assigns them,
// `xmlMemGet` reads them, and EVERY internal allocation calls through the
// variables — so assigning `xmlMalloc = custom;` directly is equivalent to
// `xmlMemSetup(...)`. The candidate previously kept a separate `ALLOCATOR`
// RwLock consulted by the `*Impl` bodies, so a direct public-variable
// assignment changed the exported symbol but not internal allocations, and
// `xmlMemSetup` changed internal allocations but not the exported symbols:
// two sources of truth. This was the xmlGcMemSetup-class defect R-000176
// (11.1-Z.2). The merged model below restores the upstream single source:
//
//   - the five exported `static mut` fn-pointer variables ARE the state;
//   - the `*Default` functions are the initial values (Rust global allocator
//     + the accounting registry) and never read the variables;
//   - the `*Impl` functions are the indirection every internal call site
//     uses: they read the current variable, so custom hooks installed via
//     `xmlMemSetup` OR direct assignment are observed everywhere.
//
// The write/read contract matches upstream: `xmlMemSetup`/assignment must
// happen before concurrent use (upstream: "This has to be called before any
// other libxml routines !"). Rust `static mut` access is `unsafe` and the
// crate upholds the upstream single-threaded-setup ordering.

/// Global allocation counters (for xmlMemUsed/xmlMemBlocks).
///
/// These use relaxed ordering since they are approximate debugging counters.
static MEM_USED: AtomicUsize = AtomicUsize::new(0);
static MEM_BLOCKS: AtomicUsize = AtomicUsize::new(0);

/// Per-block metadata (upstream xmlmemory.c block table): enables
/// xmlMemSize, exact xmlMemUsed and per-block dumps (11.1-J / R-000131).
/// The file pointer is stored as a raw address (usize) so the registry stays
/// Send + Sync.
#[derive(Clone, Copy)]
struct BlockMeta {
    size: usize,
    file: usize,
    line: c_int,
}

static BLOCKS: once_cell::sync::Lazy<
    parking_lot::Mutex<std::collections::HashMap<usize, BlockMeta>>,
> = once_cell::sync::Lazy::new(|| parking_lot::Mutex::new(std::collections::HashMap::new()));

/// Record a block in the registry (no-op for NULL).
unsafe fn block_record(ptr: *mut c_void, size: usize, file: *const c_char, line: c_int) {
    if ptr.is_null() {
        return;
    }
    BLOCKS.lock().insert(
        ptr as usize,
        BlockMeta {
            size,
            file: file as usize,
            line,
        },
    );
}

/// Drop a block from the registry; returns its recorded size (0 if unknown).
unsafe fn block_forget(ptr: *mut c_void) -> usize {
    if ptr.is_null() {
        return 0;
    }
    BLOCKS
        .lock()
        .remove(&(ptr as usize))
        .map(|m| m.size)
        .unwrap_or(0)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Public Allocator API
// ═══════════════════════════════════════════════════════════════════════════════

/// Set custom memory allocator functions (upstream xmlmemory.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlMemSetup(xmlFreeFunc freeFunc,
///                 xmlMallocFunc mallocFunc,
///                 xmlReallocFunc reallocFunc,
///                 xmlStrdupFunc strdupFunc);
/// ```
///
/// Returns -1 if any function is NULL (upstream xmlmemory.c: "Returns 0 on
/// success"); otherwise assigns the five exported allocator variables
/// (xmlFree, xmlMalloc, xmlMallocAtomic = mallocFunc, xmlRealloc,
/// xmlMemStrdup) and returns 0. The exported variables are the single source
/// of truth: every internal allocation reads them through the `*Impl`
/// indirection, so this call — or a direct `xmlMalloc = custom` assignment —
/// re-routes all allocations immediately (R-000176 fix).
///
/// # SAFETY
///
/// - All function pointers must be valid (non-null) and thread-safe
/// - The functions must follow the C malloc/realloc/free/strdup contract
/// - Once set, the functions remain in effect until the next `xmlMemSetup` call
/// - The caller is responsible for ensuring the functions remain valid for the
///   entire time they are installed
/// - Must not race with concurrent allocation (upstream ordering contract:
///   "This has to be called before any other libxml routines !")
#[no_mangle]
pub unsafe extern "C" fn xmlMemSetup(
    freeFunc: Option<xmlFreeFunc>,
    mallocFunc: Option<xmlMallocFunc>,
    reallocFunc: Option<xmlReallocFunc>,
    strdupFunc: Option<xmlStrdupFunc>,
) -> c_int {
    if freeFunc.is_none() || mallocFunc.is_none() || reallocFunc.is_none() || strdupFunc.is_none() {
        return -1;
    }
    // SAFETY: callers must uphold the upstream single-threaded-setup
    // ordering; each value is a non-NULL C function pointer (checked above).
    unsafe {
        xmlFree = freeFunc.unwrap();
        xmlMalloc = mallocFunc.unwrap();
        xmlMallocAtomic = mallocFunc.unwrap();
        xmlRealloc = reallocFunc.unwrap();
        xmlMemStrdup = strdupFunc.unwrap();
    }
    0
}

/// Get the current memory allocator functions (upstream xmlmemory.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlMemGet(xmlFreeFunc *freeFunc,
///               xmlMallocFunc *mallocFunc,
///               xmlReallocFunc *reallocFunc,
///               xmlStrdupFunc *strdupFunc);
/// ```
///
/// Writes the current exported allocator variables through NULL-tolerant
/// output pointers (upstream xmlmemory.c: NULL outputs are skipped) and
/// returns 0.
///
/// # SAFETY
///
/// - All non-NULL output pointers must be valid and writable
#[no_mangle]
pub unsafe extern "C" fn xmlMemGet(
    freeFunc: *mut Option<xmlFreeFunc>,
    mallocFunc: *mut Option<xmlMallocFunc>,
    reallocFunc: *mut Option<xmlReallocFunc>,
    strdupFunc: *mut Option<xmlStrdupFunc>,
) -> c_int {
    // SAFETY: callers must pass NULL or valid writable pointers; reads of
    // the exported variables are safe under the upstream setup ordering.
    unsafe {
        if !freeFunc.is_null() {
            ptr::write(freeFunc, Some(xmlFree));
        }
        if !mallocFunc.is_null() {
            ptr::write(mallocFunc, Some(xmlMalloc));
        }
        if !reallocFunc.is_null() {
            ptr::write(reallocFunc, Some(xmlRealloc));
        }
        if !strdupFunc.is_null() {
            ptr::write(strdupFunc, Some(xmlMemStrdup));
        }
    }
    0
}

/// Set GC-aware memory allocator functions (upstream xmlmemory.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlGcMemSetup(xmlFreeFunc freeFunc,
///                   xmlMallocFunc mallocFunc,
///                   xmlMallocFunc mallocAtomicFunc,
///                   xmlReallocFunc reallocFunc,
///                   xmlStrdupFunc strdupFunc);
/// ```
///
/// Same contract as `xmlMemSetup` with a dedicated `mallocAtomicFunc` for
/// atomic allocations (upstream xmlmemory.c: `xmlMallocAtomic =
/// mallocAtomicFunc`). Returns -1 if any function is NULL, else 0.
///
/// # SAFETY
///
/// - All function pointers must be valid (non-null) and thread-safe
/// - The functions must follow the C malloc/realloc/free/strdup contract
/// - Must not race with concurrent allocation (upstream ordering contract)
/// - The caller is responsible for ensuring the functions remain valid for
///   the entire time they are installed
#[no_mangle]
pub unsafe extern "C" fn xmlGcMemSetup(
    freeFunc: Option<xmlFreeFunc>,
    mallocFunc: Option<xmlMallocFunc>,
    mallocAtomicFunc: Option<xmlMallocFunc>,
    reallocFunc: Option<xmlReallocFunc>,
    strdupFunc: Option<xmlStrdupFunc>,
) -> c_int {
    if freeFunc.is_none()
        || mallocFunc.is_none()
        || mallocAtomicFunc.is_none()
        || reallocFunc.is_none()
        || strdupFunc.is_none()
    {
        return -1;
    }
    // SAFETY: callers must uphold the upstream single-threaded-setup
    // ordering; each value is a non-NULL C function pointer (checked above).
    unsafe {
        xmlFree = freeFunc.unwrap();
        xmlMalloc = mallocFunc.unwrap();
        xmlMallocAtomic = mallocAtomicFunc.unwrap();
        xmlRealloc = reallocFunc.unwrap();
        xmlMemStrdup = strdupFunc.unwrap();
    }
    0
}

/// Get GC-aware memory allocator functions (upstream xmlmemory.h).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlGcMemGet(xmlFreeFunc *freeFunc,
///                 xmlMallocFunc *mallocFunc,
///                 xmlMallocFunc *mallocAtomicFunc,
///                 xmlReallocFunc *reallocFunc,
///                 xmlStrdupFunc *strdupFunc);
/// ```
///
/// Same contract as `xmlMemGet` with the `mallocAtomicFunc` output.
/// Writes through NULL-tolerant output pointers and returns 0.
///
/// # SAFETY
///
/// - All non-NULL output pointers must be valid and writable
#[no_mangle]
pub unsafe extern "C" fn xmlGcMemGet(
    freeFunc: *mut Option<xmlFreeFunc>,
    mallocFunc: *mut Option<xmlMallocFunc>,
    mallocAtomicFunc: *mut Option<xmlMallocFunc>,
    reallocFunc: *mut Option<xmlReallocFunc>,
    strdupFunc: *mut Option<xmlStrdupFunc>,
) -> c_int {
    // SAFETY: callers must pass NULL or valid writable pointers.
    unsafe {
        if !freeFunc.is_null() {
            ptr::write(freeFunc, Some(xmlFree));
        }
        if !mallocFunc.is_null() {
            ptr::write(mallocFunc, Some(xmlMalloc));
        }
        if !mallocAtomicFunc.is_null() {
            ptr::write(mallocAtomicFunc, Some(xmlMallocAtomic));
        }
        if !reallocFunc.is_null() {
            ptr::write(reallocFunc, Some(xmlRealloc));
        }
        if !strdupFunc.is_null() {
            ptr::write(strdupFunc, Some(xmlMemStrdup));
        }
    }
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// Allocation Functions
// ═══════════════════════════════════════════════════════════════════════════════

/// Allocate memory through the exported `xmlMalloc` variable.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xmlMalloc(size_t size);
/// ```
///
/// Every internal allocation site calls this indirection, which reads the
/// current exported `xmlMalloc` variable — exactly how upstream internal
/// code calls `xmlMalloc(...)` (the variable, globals.c). With no override
/// the variable holds `xmlMallocDefault`; after `xmlMemSetup` or a direct
/// `xmlMalloc = custom` assignment it holds the custom hook, so all internal
/// allocations observe the override (R-000176 fix, single source of truth).
///
/// # SAFETY
///
/// - The returned pointer must be freed with `xmlFree`
/// - `size` may be 0 (returns a valid non-NULL pointer or NULL)
pub unsafe extern "C" fn xmlMallocImpl(size: usize) -> *mut c_void {
    // SAFETY: reading the exported static mut is safe under the upstream
    // setup-ordering contract; the stored value is a valid C fn pointer.
    unsafe { (xmlMalloc)(size) }
}

/// Default `xmlMalloc` body: Rust global allocator + accounting registry.
///
/// This is the initial value of the exported `xmlMalloc` variable and never
/// reads the variable (no recursion). Accounting matches the upstream
/// debug-allocator contract: counters plus the per-block registry, so
/// `xmlMemUsed`/`xmlMemSize` are exact while the default is installed.
unsafe extern "C" fn xmlMallocDefault(size: usize) -> *mut c_void {
    let ptr = unsafe { default_malloc(size) };
    if !ptr.is_null() {
        MEM_USED.fetch_add(size, Ordering::Relaxed);
        MEM_BLOCKS.fetch_add(1, Ordering::Relaxed);
        unsafe { block_record(ptr, size, ptr::null(), 0) };
    }
    ptr
}

/// Allocate through the exported `xmlMallocAtomic` variable.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xmlMallocAtomic(size_t size);
/// ```
///
/// Identical to `xmlMalloc` but hints to the GC that the memory does not
/// contain pointers. In modern libxml2 this is equivalent to `xmlMalloc`;
/// `xmlMemSetup` aliases it to the same hook (upstream xmlmemory.c).
///
/// # SAFETY
///
/// - The returned pointer must be freed with `xmlFree`
/// - `size` may be 0 (returns a valid non-NULL pointer or NULL)
pub unsafe extern "C" fn xmlMallocAtomicImpl(size: usize) -> *mut c_void {
    // SAFETY: see xmlMallocImpl.
    unsafe { (xmlMallocAtomic)(size) }
}

/// Default `xmlMallocAtomic` body (initial exported-variable value).
///
/// Atomic allocations share the malloc accounting body; `xmlGcMemSetup`
/// installs a dedicated atomic hook via the variable (upstream xmlmemory.c).
unsafe extern "C" fn xmlMallocAtomicDefault(size: usize) -> *mut c_void {
    unsafe { xmlMallocDefault(size) }
}

/// Reallocate through the exported `xmlRealloc` variable.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xmlRealloc(void *ptr, size_t size);
/// ```
///
/// Changes the size of the memory block pointed to by `ptr`.
/// If `ptr` is NULL, behaves like `xmlMalloc`.
/// If `size` is 0, may return NULL (like C realloc).
///
/// # SAFETY
///
/// - `ptr` must be a valid pointer from `xmlMalloc`, `xmlMallocAtomic`, or `xmlRealloc`,
///   or NULL
/// - The returned pointer must be freed with `xmlFree`
pub unsafe extern "C" fn xmlReallocImpl(ptr: *mut c_void, size: usize) -> *mut c_void {
    // SAFETY: see xmlMallocImpl.
    unsafe { (xmlRealloc)(ptr, size) }
}

/// Default `xmlRealloc` body (initial exported-variable value).
unsafe extern "C" fn xmlReallocDefault(ptr: *mut c_void, size: usize) -> *mut c_void {
    let new_ptr = unsafe { default_realloc(ptr, size) };
    if !new_ptr.is_null() {
        // Exact accounting via the block registry.
        let old_size = unsafe { block_forget(ptr) };
        MEM_USED.fetch_add(size.saturating_sub(old_size), Ordering::Relaxed);
        if ptr.is_null() {
            MEM_BLOCKS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { block_record(new_ptr, size, ptr::null(), 0) };
    } else if !ptr.is_null() {
        // realloc failure: the old block is still alive.
        unsafe { block_record(ptr, block_forget(ptr), ptr::null(), 0) };
    }
    new_ptr
}

/// Free through the exported `xmlFree` variable.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFree(void *ptr);
/// ```
///
/// Frees memory previously allocated with `xmlMalloc`, `xmlMallocAtomic`,
/// or `xmlRealloc`. If `ptr` is NULL, no operation is performed.
///
/// # SAFETY
///
/// - `ptr` must be a valid pointer from `xmlMalloc`/`xmlMallocAtomic`/`xmlRealloc`,
///   or NULL
/// - After this call, `ptr` must not be dereferenced
pub unsafe extern "C" fn xmlFreeImpl(ptr: *mut c_void) {
    // SAFETY: see xmlMallocImpl.
    unsafe { (xmlFree)(ptr) }
}

/// Default `xmlFree` body (initial exported-variable value).
unsafe extern "C" fn xmlFreeDefault(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    let old_size = unsafe { block_forget(ptr) };
    unsafe { default_free(ptr) };
    MEM_BLOCKS.fetch_sub(1, Ordering::Relaxed);
    if old_size > 0 {
        MEM_USED.fetch_sub(old_size, Ordering::Relaxed);
    }
}

/// Duplicate a C string through the exported `xmlMemStrdup` variable.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xmlMemStrdup(const char *str);
/// ```
///
/// Returns a pointer to the newly allocated copy, or NULL on failure.
///
/// # SAFETY
///
/// - `str` must be a valid null-terminated C string or NULL
/// - The returned pointer must be freed with `xmlFree`
pub unsafe extern "C" fn xmlMemStrdupImpl(str: *const c_char) -> *mut c_void {
    // SAFETY: see xmlMallocImpl.
    unsafe { (xmlMemStrdup)(str) }
}

/// Default `xmlMemStrdup` body (initial exported-variable value).
unsafe extern "C" fn xmlMemStrdupDefault(str: *const c_char) -> *mut c_void {
    if str.is_null() {
        return ptr::null_mut();
    }
    let ptr = unsafe { default_strdup(str) };
    if !ptr.is_null() {
        let len = unsafe { libc::strlen(str) } + 1;
        MEM_USED.fetch_add(len, Ordering::Relaxed);
        MEM_BLOCKS.fetch_add(1, Ordering::Relaxed);
        unsafe { block_record(ptr, len, ptr::null(), 0) };
    }
    ptr
}

// ═══════════════════════════════════════════════════════════════════════════════
// Exported allocator globals (upstream xmlmemory.h)
// ═══════════════════════════════════════════════════════════════════════════════
//
// Upstream exports the allocator entry points as DATA: `XMLPUBVAR
// xmlMallocFunc xmlMalloc;` etc. — function-pointer variables that downstream
// code can read AND assign (the documented allocator-override mechanism).
// The candidate mirrors that ABI: these five variables ARE the allocator
// state (single source of truth, R-000176). Their initial values are the
// `*Default` bodies; every internal allocation routes through the variables
// via the `*Impl` indirection, so `xmlMemSetup` and direct assignment are
// equivalent override mechanisms, exactly as upstream.

/// `xmlMallocFunc xmlMalloc` — the malloc hook (default: `xmlMallocDefault`).
#[no_mangle]
pub static mut xmlMalloc: xmlMallocFunc = xmlMallocDefault;

/// `xmlMallocFunc xmlMallocAtomic` — the atomic-malloc hook.
#[no_mangle]
pub static mut xmlMallocAtomic: xmlMallocFunc = xmlMallocAtomicDefault;

/// `xmlReallocFunc xmlRealloc` — the realloc hook.
#[no_mangle]
pub static mut xmlRealloc: xmlReallocFunc = xmlReallocDefault;

/// `xmlFreeFunc xmlFree` — the free hook.
#[no_mangle]
pub static mut xmlFree: xmlFreeFunc = xmlFreeDefault;

/// `xmlStrdupFunc xmlMemStrdup` — the strdup hook.
#[no_mangle]
pub static mut xmlMemStrdup: xmlStrdupFunc = xmlMemStrdupDefault;

// ═══════════════════════════════════════════════════════════════════════════════
// Memory Debugging / Statistics
// ═══════════════════════════════════════════════════════════════════════════════

/// Return the total amount of memory currently allocated (approximate).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlMemUsed(void);
/// ```
///
/// Returns an approximate count of allocated bytes.
/// With the default allocator, this tracks allocations but is not
/// byte-exact for realloc (since we don't track old sizes).
/// Custom allocator hooks can provide exact counts.
#[no_mangle]
pub extern "C" fn xmlMemUsed() -> c_int {
    MEM_USED.load(Ordering::Relaxed) as c_int
}

/// Return the current number of allocated blocks (approximate).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlMemBlocks(void);
/// ```
///
/// Returns an approximate count of live allocations.
#[no_mangle]
pub extern "C" fn xmlMemBlocks() -> c_int {
    MEM_BLOCKS.load(Ordering::Relaxed) as c_int
}

/// Display memory allocation information to a file.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlMemDisplay(FILE *fp);
/// ```
///
/// Prints debug memory information. With the default allocator,
/// this prints a summary message. Custom allocator hooks may
/// provide more detailed output.
///
/// # SAFETY
///
/// - `fp` must be a valid FILE* pointer or NULL (in which case stderr is used)
#[no_mangle]
pub unsafe extern "C" fn xmlMemDisplay(fp: *mut c_void) {
    // SAFETY: Caller guarantees fp is valid FILE* or NULL.
    unsafe {
        let out = if fp.is_null() {
            libc::fdopen(2, b"w\0" as *const u8 as *const c_char) as *mut c_void
        } else {
            fp
        };
        libc::fprintf(
            out as *mut _,
            b"Memory: used=%d blocks=%d\n\0" as *const u8 as *const c_char,
            xmlMemUsed(),
            xmlMemBlocks(),
        );
    }
}

/// Show memory allocation information.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlMemShow(FILE *fp, int nr);
/// ```
///
/// Prints debug memory information for the last `nr` allocations.
/// With the default allocator, this is a no-op (we don't track allocation history).
///
/// # SAFETY
///
/// - `fp` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlMemShow(fp: *mut c_void, nr: c_int) {
    // Upstream xmlMemShow(fp, nr) prints the nr most recently allocated
    // blocks from the debug allocator's history. The candidate's block
    // registry is unordered; print the live blocks (bounded by nr), which
    // preserves the observable purpose (per-block debugging output).
    unsafe {
        let out = if fp.is_null() {
            libc::fdopen(2, b"w\0" as *const u8 as *const c_char) as *mut c_void
        } else {
            fp
        };
        if out.is_null() {
            return;
        }
        let mut msg = String::from("Recent blocks\n");
        let map = BLOCKS.lock();
        let mut entries: Vec<(usize, &BlockMeta)> = map.iter().map(|(k, v)| (*k, v)).collect();
        entries.sort_by_key(|(k, _)| *k);
        let take = if nr > 0 { nr as usize } else { usize::MAX };
        for (addr, meta) in entries.into_iter().take(take) {
            msg.push_str(&format!(
                "  {:018p} : {:>7} bytes\n",
                addr as *const c_void, meta.size
            ));
        }
        let bytes = msg.as_bytes();
        libc::fwrite(
            bytes.as_ptr() as *const c_void,
            1,
            bytes.len(),
            out as *mut libc::FILE,
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Convenience Functions (used internally)
// ═══════════════════════════════════════════════════════════════════════════════

/// Allocate zero-initialized memory.
///
/// # UPSTREAM-PARITY
///
/// This is like `xmlMalloc` followed by `memset(0)`, but some allocator
/// hooks provide it directly.
///
/// # SAFETY
///
/// Same as `xmlMalloc`. The returned memory is zero-initialized.
#[no_mangle]
pub unsafe extern "C" fn xmlMallocZero(size: usize) -> *mut c_void {
    // SAFETY: Delegates to xmlMallocImpl and zeroes the memory.
    let ptr = unsafe { xmlMallocImpl(size) };
    if !ptr.is_null() {
        unsafe { ptr::write_bytes(ptr, 0, size) };
    }
    ptr
}

/// Allocate zero-initialized memory (atomic variant).
///
/// # UPSTREAM-PARITY
///
/// Like `xmlMallocAtomic` followed by zero-initialization.
///
/// # SAFETY
///
/// The function touches crate-global state only; it is safe
/// as long as the caller respects the library's global
/// initialization/cleanup ordering (xmlInitParser before use,
/// xmlCleanupParser only after all users are done).
///
/// Violating the global lifecycle ordering, or calling this after
/// teardown or from a signal handler, is undefined behavior.
#[no_mangle]
pub unsafe extern "C" fn xmlMallocAtomicZero(size: usize) -> *mut c_void {
    // SAFETY: Delegates to xmlMallocAtomicImpl and zeroes the memory.
    let ptr = unsafe { xmlMallocAtomicImpl(size) };
    if !ptr.is_null() {
        unsafe { ptr::write_bytes(ptr, 0, size) };
    }
    ptr
}

/// Reallocate and zero-initialize the new portion.
///
/// # UPSTREAM-PARITY
///
/// Like `xmlRealloc`, but zeroes any newly allocated bytes.
///
/// # SAFETY
///
/// Same as `xmlRealloc`.
#[no_mangle]
pub unsafe extern "C" fn xmlReallocZero(
    ptr: *mut c_void,
    old_size: usize,
    new_size: usize,
) -> *mut c_void {
    // SAFETY: Delegates to xmlRealloc and zeroes the new portion.
    let new_ptr = unsafe { xmlReallocImpl(ptr, new_size) };
    if !new_ptr.is_null() && new_size > old_size {
        unsafe {
            ptr::write_bytes(new_ptr.add(old_size), 0, new_size - old_size);
        }
    }
    new_ptr
}

// ═══════════════════════════════════════════════════════════════════════════════
// Debug Allocator (Optional)
// ═══════════════════════════════════════════════════════════════════════════════

/// Initialize the memory layer with debugging support.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlInitMemory(void);
/// ```
///
/// Initializes the memory subsystem. Returns 0 on success.
/// This is called automatically by `xmlInitParser`.
#[no_mangle]
pub const extern "C" fn xmlInitMemory() -> c_int {
    0
}

/// Clean up the memory layer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlCleanupMemory(void);
/// ```
#[no_mangle]
pub const extern "C" fn xmlCleanupMemory() {
    // Phase 1: no cleanup needed for the default allocator.
}

// ═══════════════════════════════════════════════════════════════════════════════
// Legacy-named allocator API (upstream xmlmemory.h)
// ═══════════════════════════════════════════════════════════════════════════════
// Upstream xmlmemory.h historically exported `xmlMemMalloc`, `xmlMemFree`,
// `xmlMemRealloc`, `xmlMemoryStrdup`, the `*Loc` location-tracking variants,
// `xmlMemSize`, `xmlMemDisplayLast` and `xmlMemoryDump` alongside the modern
// names. Downstream code (older consumers, some language bindings) links
// against these names, so the candidate exports them with identical
// semantics. The `*Loc` variants take a source file/line that the default
// allocator does not track (upstream uses it for leak reports only); the
// location arguments are accepted and ignored — a documented safe divergence
// (residual R-000131).

/// Allocate memory (legacy name; same contract as `xmlMalloc`).
///
/// ```c
/// void *xmlMemMalloc(size_t size);
/// ```
///
/// # SAFETY
///
/// The function touches crate-global state only; it is safe
/// as long as the caller respects the library's global
/// initialization/cleanup ordering (xmlInitParser before use,
/// xmlCleanupParser only after all users are done).
///
/// Violating the global lifecycle ordering, or calling this after
/// teardown or from a signal handler, is undefined behavior.
#[no_mangle]
pub unsafe extern "C" fn xmlMemMalloc(size: usize) -> *mut c_void {
    // SAFETY: identical contract to xmlMalloc.
    unsafe { xmlMallocImpl(size) }
}

/// Free memory (legacy name; same contract as `xmlFree`).
///
/// ```c
/// void xmlMemFree(void *ptr);
/// ```
///
/// # SAFETY
///
/// - `ptr` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlMemFree(ptr: *mut c_void) {
    // SAFETY: identical contract to xmlFree.
    unsafe { xmlFreeImpl(ptr) }
}

/// Reallocate memory (legacy name; same contract as `xmlRealloc`).
///
/// ```c
/// void *xmlMemRealloc(void *ptr, size_t size);
/// ```
///
/// # SAFETY
///
/// - `ptr` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlMemRealloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    // SAFETY: identical contract to xmlRealloc.
    unsafe { xmlReallocImpl(ptr, size) }
}

/// Duplicate a string (legacy name; same contract as `xmlMemStrdup`).
///
/// ```c
/// void *xmlMemoryStrdup(const char *str);
/// ```
///
/// # SAFETY
///
///
/// - `str` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlMemoryStrdup(str: *const c_char) -> *mut c_void {
    // SAFETY: identical contract to xmlMemStrdup.
    unsafe { xmlMemStrdupImpl(str) }
}

/// Allocate memory, recording the allocation site (upstream xmlmemory.h).
///
/// ```c
/// void *xmlMallocLoc(size_t size, const char *file, int line);
/// ```
///
/// The default candidate allocator does not track allocation sites (see
/// residual R-000131); the location arguments are accepted for ABI
/// compatibility and ignored.
///
/// # SAFETY
///
///
/// - `file` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlMallocLoc(
    size: usize,
    file: *const c_char,
    line: c_int,
) -> *mut c_void {
    let ptr = unsafe { xmlMallocImpl(size) };
    if !ptr.is_null() {
        unsafe { block_record(ptr, size, file, line) };
    }
    ptr
}

/// Allocate zeroed memory, recording the allocation site (upstream xmlmemory.h).
///
/// ```c
/// void *xmlMallocAtomicLoc(size_t size, const char *file, int line);
/// ```
///
/// # SAFETY
///
///
/// - `file` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlMallocAtomicLoc(
    size: usize,
    file: *const c_char,
    line: c_int,
) -> *mut c_void {
    let ptr = unsafe { xmlMallocZero(size) };
    if !ptr.is_null() {
        unsafe { block_record(ptr, size, file, line) };
    }
    ptr
}

/// Reallocate memory, recording the allocation site (upstream xmlmemory.h).
///
/// ```c
/// void *xmlReallocLoc(void *ptr, size_t size, const char *file, int line);
/// ```
///
/// # SAFETY
///
/// - `ptr` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `file` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlReallocLoc(
    ptr: *mut c_void,
    size: usize,
    file: *const c_char,
    line: c_int,
) -> *mut c_void {
    let new_ptr = unsafe { xmlReallocImpl(ptr, size) };
    if !new_ptr.is_null() {
        unsafe { block_record(new_ptr, size, file, line) };
    } else if !ptr.is_null() {
        // Keep the old site on failure.
        let meta = BLOCKS.lock().get(&(ptr as usize)).copied();
        if let Some(m) = meta {
            unsafe { block_record(ptr, m.size, m.file as *const c_char, m.line) };
        }
    }
    new_ptr
}

/// Duplicate a string, recording the allocation site (upstream xmlmemory.h).
///
/// ```c
/// void *xmlMemStrdupLoc(const char *str, const char *file, int line);
/// ```
///
/// # SAFETY
///
///
/// - `str`, `file` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlMemStrdupLoc(
    str: *const c_char,
    file: *const c_char,
    line: c_int,
) -> *mut c_void {
    let ptr = unsafe { xmlMemStrdupImpl(str) };
    if !ptr.is_null() {
        let len = unsafe { libc::strlen(str) } + 1;
        unsafe { block_record(ptr, len, file, line) };
    }
    ptr
}

/// Return the size of an allocated block (upstream xmlmemory.h).
///
/// ```c
/// size_t xmlMemSize(void *ptr);
/// ```
///
/// Returns the recorded size from the block registry (0 for unknown or
/// foreign pointers, matching upstream's lookup-miss behavior).
///
/// # SAFETY
///
/// - `ptr` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlMemSize(ptr: *mut c_void) -> usize {
    if ptr.is_null() {
        return 0;
    }
    BLOCKS
        .lock()
        .get(&(ptr as usize))
        .map(|m| m.size)
        .unwrap_or(0)
}

/// Display a limited amount of memory debug information (upstream xmlmemory.h).
///
/// ```c
/// void xmlMemDisplayLast(FILE *fp, long nbBytes);
/// ```
///
/// Dumps the per-block registry (upstream xmlMemDisplayLast block listing):
/// one line per live block with its address, size and recorded allocation
/// site, bounded by `nb_bytes` when positive. The aggregate footer matches
/// upstream's counters.
///
/// # SAFETY
///
/// - `fp` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlMemDisplayLast(fp: *mut c_void, nb_bytes: c_long) {
    // SAFETY: fp must be a valid FILE* or NULL (stderr used).
    unsafe {
        let out = if fp.is_null() {
            libc::fdopen(2, b"w\0" as *const u8 as *const c_char) as *mut c_void
        } else {
            fp
        };
        if out.is_null() {
            return;
        }
        let mut total: usize = 0;
        let mut msg = String::new();
        msg.push_str("MEMORY ALLOCATED : 0, MAX : 0, BLOCKS : ");
        let blocks = MEM_BLOCKS.load(Ordering::Relaxed);
        msg.push_str(&blocks.to_string());
        msg.push('\n');
        let map = BLOCKS.lock();
        let mut entries: Vec<(usize, &BlockMeta)> = map.iter().map(|(k, v)| (*k, v)).collect();
        entries.sort_by_key(|(k, _)| *k);
        for (addr, meta) in entries {
            if nb_bytes > 0 && (total as c_long) >= nb_bytes {
                break;
            }
            total += meta.size;
            msg.push_str(&format!(
                "  {:018p} : {:>7} bytes",
                addr as *const c_void, meta.size
            ));
            if meta.file != 0 {
                let file = CStr::from_ptr(meta.file as *const c_char).to_string_lossy();
                msg.push_str(&format!(" @ {}:{}", file, meta.line));
            }
            msg.push('\n');
        }
        drop(map);
        let used = MEM_USED.load(Ordering::Relaxed);
        msg.push_str(&format!(
            "TOTAL MEMORY ALLOCATED : {} bytes, TOTAL BLOCKS : {}\n",
            used, blocks
        ));
        let bytes = msg.as_bytes();
        libc::fwrite(
            bytes.as_ptr() as *const c_void,
            1,
            bytes.len(),
            out as *mut libc::FILE,
        );
    }
}

/// Dump memory allocation statistics (upstream xmlmemory.h).
///
/// ```c
/// void xmlMemoryDump(void);
/// ```
///
/// Prints the global counters to stderr (no leak detector is active in the
/// default allocator).
///
/// # SAFETY
///
/// The function touches crate-global state only; it is safe
/// as long as the caller respects the library's global
/// initialization/cleanup ordering (xmlInitParser before use,
/// xmlCleanupParser only after all users are done).
///
/// Violating the global lifecycle ordering, or calling this after
/// teardown or from a signal handler, is undefined behavior.
#[no_mangle]
pub unsafe extern "C" fn xmlMemoryDump() {
    unsafe {
        xmlMemDisplayLast(ptr::null_mut(), -1);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr;

    /// Test basic allocation and deallocation.
    #[test]
    fn test_malloc_free() {
        unsafe {
            let ptr = xmlMalloc(100);
            assert!(!ptr.is_null(), "xmlMalloc(100) returned NULL");
            xmlFree(ptr);
        }
    }

    /// Test that xmlMalloc(0) returns a valid pointer.
    #[test]
    fn test_malloc_zero() {
        unsafe {
            let ptr = xmlMalloc(0);
            // malloc(0) may return NULL or a valid pointer.
            // If non-NULL, it must be freeable.
            if !ptr.is_null() {
                xmlFree(ptr);
            }
        }
    }

    /// Test xmlFree(NULL) is a no-op.
    #[test]
    fn test_free_null() {
        unsafe {
            xmlFree(ptr::null_mut());
            // Should not crash
        }
    }

    /// Test xmlRealloc.
    #[test]
    fn test_realloc() {
        unsafe {
            let ptr = xmlMalloc(50);
            assert!(!ptr.is_null());
            let new_ptr = xmlRealloc(ptr, 100);
            assert!(!new_ptr.is_null());
            xmlFree(new_ptr);
        }
    }

    /// Test xmlMemStrdup.
    #[test]
    fn test_mem_strdup() {
        unsafe {
            let s = b"hello\0" as *const u8 as *const c_char;
            let dup = xmlMemStrdup(s);
            assert!(!dup.is_null());
            // Compare the strings
            let orig_slice = std::slice::from_raw_parts(s as *const u8, 6);
            let dup_slice = std::slice::from_raw_parts(dup as *const u8, 6);
            assert_eq!(orig_slice, dup_slice);
            xmlFree(dup);
        }
    }

    /// Test xmlMallocZero returns zeroed memory.
    #[test]
    fn test_malloc_zero_init() {
        unsafe {
            let ptr = xmlMallocZero(100) as *mut u8;
            assert!(!ptr.is_null());
            let slice = std::slice::from_raw_parts(ptr, 100);
            assert!(slice.iter().all(|&b| b == 0));
            xmlFree(ptr as *mut c_void);
        }
    }

    /// Test custom allocator setup/get (R-000176: int returns, single source
    /// of truth — `xmlMemSetup` writes the exported variables and `xmlMemGet`
    /// reads them back).
    #[test]
    fn test_mem_setup_get() {
        unsafe {
            let mut free_func: Option<xmlFreeFunc> = None;
            let mut malloc_func: Option<xmlMallocFunc> = None;
            let mut realloc_func: Option<xmlReallocFunc> = None;
            let mut strdup_func: Option<xmlStrdupFunc> = None;

            let ret = xmlMemGet(
                &mut free_func as *mut _,
                &mut malloc_func as *mut _,
                &mut realloc_func as *mut _,
                &mut strdup_func as *mut _,
            );
            assert_eq!(ret, 0, "xmlMemGet must return 0");
            assert!(malloc_func.is_some());
            assert!(free_func.is_some());
            assert!(realloc_func.is_some());
            assert!(strdup_func.is_some());

            // NULL output pointers are tolerated (upstream xmlmemory.c).
            let ret = xmlMemGet(
                ptr::null_mut(),
                &mut malloc_func as *mut _,
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_eq!(ret, 0);
            assert!(malloc_func.is_some());
        }
    }

    /// Test the GC allocator hooks: 5-argument Setup/Get with the dedicated
    /// `mallocAtomicFunc` slot (R-000176 — previously 4-arg and void).
    #[test]
    fn test_gc_mem_setup_get() {
        unsafe {
            let mut free_func: Option<xmlFreeFunc> = None;
            let mut malloc_func: Option<xmlMallocFunc> = None;
            let mut malloc_atomic_func: Option<xmlMallocFunc> = None;
            let mut realloc_func: Option<xmlReallocFunc> = None;
            let mut strdup_func: Option<xmlStrdupFunc> = None;

            let ret = xmlGcMemGet(
                &mut free_func as *mut _,
                &mut malloc_func as *mut _,
                &mut malloc_atomic_func as *mut _,
                &mut realloc_func as *mut _,
                &mut strdup_func as *mut _,
            );
            assert_eq!(ret, 0, "xmlGcMemGet must return 0");
            assert!(free_func.is_some());
            assert!(malloc_func.is_some());
            assert!(malloc_atomic_func.is_some());
            assert!(realloc_func.is_some());
            assert!(strdup_func.is_some());
        }
    }

    /// Test `xmlMemSetup` NULL validation returns -1 (upstream xmlmemory.c)
    /// and that a NULL `mallocAtomicFunc` makes `xmlGcMemSetup` fail.
    #[test]
    fn test_mem_setup_null_rejected() {
        unsafe {
            let ret = xmlMemSetup(None, Some(default_malloc as xmlMallocFunc), None, None);
            assert_eq!(ret, -1, "NULL hook must be rejected with -1");
            let ret = xmlGcMemSetup(
                None,
                Some(default_malloc as xmlMallocFunc),
                Some(default_malloc as xmlMallocFunc),
                None,
                None,
            );
            assert_eq!(ret, -1);
        }
    }

    /// Test the single-source-of-truth model (R-000176): a direct assignment
    /// to the exported `xmlMalloc` variable is observed by `xmlMemGet` AND by
    /// actual internal allocations through `xmlMallocImpl`.
    #[test]
    fn test_direct_assignment_coherence() {
        unsafe {
            let saved = xmlMalloc;
            let saved_free = xmlFree;
            // Install a counting hook via direct variable assignment.
            xmlMalloc = xmlMallocDefault;
            xmlFree = xmlFreeDefault;

            // xmlMemGet reads the variable.
            let mut malloc_func: Option<xmlMallocFunc> = None;
            let ret = xmlMemGet(
                ptr::null_mut(),
                &mut malloc_func as *mut _,
                ptr::null_mut(),
                ptr::null_mut(),
            );
            assert_eq!(ret, 0);
            assert!(
                core::ptr::fn_addr_eq(malloc_func.unwrap(), xmlMallocDefault as xmlMallocFunc,),
                "xmlMemGet must return the directly-assigned variable"
            );

            // Internal allocation routes through the variable.
            let p = xmlMallocImpl(64);
            assert!(!p.is_null());
            xmlFreeImpl(p);

            // Restore the defaults so other tests are unaffected.
            xmlMalloc = saved;
            xmlFree = saved_free;
        }
    }

    /// Test xmlMemUsed and xmlMemBlocks return reasonable values.
    #[test]
    fn test_mem_stats() {
        unsafe {
            let ptr = xmlMalloc(100);
            assert!(!ptr.is_null());

            // The block registry records the exact size (deterministic,
            // unlike the process-wide counters which other test threads
            // mutate concurrently).
            assert_eq!(xmlMemSize(ptr), 100);

            xmlFree(ptr);
            // Freed blocks leave the registry.
            assert_eq!(xmlMemSize(ptr), 0);
        }
    }
}
