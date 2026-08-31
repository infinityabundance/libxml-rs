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
//! shared override mechanism (R-000176). Since 11.1-Z.3 (R-000178) the
//! default bodies are plain libc `malloc`/`realloc`/`free`/`strdup`
//! wrappers — no Rust `std::alloc` layout fabrication (that was UB) and no
//! accounting, byte-identical with upstream's `globals.c` defaults
//! (`xmlMalloc = malloc` etc.).
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
//! globals matching the upstream `XMLPUBVAR` declarations (R-000162). Upstream
//! initializes them to the C runtime functions (`xmlFree = free`, `xmlMalloc =
//! malloc`, `xmlMallocAtomic = malloc`, `xmlRealloc = realloc`, `xmlMemStrdup =
//! xmlPosixStrdup`); the candidate initializes them to the `*Default` bodies,
//! which are libc wrappers with identical observable behavior.
//!
//! # Conceptual behavior
//!
//! This module implements the complete libxml2 memory-hook system: swappable
//! allocator hooks via `xmlMemSetup`/`xmlMemGet` (and the GC aliases), plus the
//! deprecated debug-named surface (`xmlMemMalloc`/`xmlMemFree`/`xmlMemRealloc`/
//! `xmlMemoryStrdup` and the `*Loc` variants) which upstream keeps as a
//! separately-tagged debug allocator. There are therefore two allocation
//! planes, exactly as in upstream 2.15.0:
//!
//!   - the five exported variables (the hook system): default = libc, and
//!     `xmlMemSetup`/direct assignment re-route them. Untracked — upstream's
//!     `debugMemSize`/`debugMemBlocks` counters are only maintained by the
//!     debug allocator, so with the default installed `xmlMemUsed()` == 0,
//!     `xmlMemBlocks()` == 0 and `xmlMemSize()` == 0 (verified against the
//!     oracle);
//!   - the debug-named surface (deprecated, exported for legacy consumers):
//!     always libc-backed and tracked by the per-block registry, mirroring
//!     upstream `xmlMemMalloc` et al. `xmlMemSize` returns the recorded size
//!     for these blocks and `xmlMemUsed`/`xmlMemBlocks` count them.
//!
//! The display entry points (`xmlMemDisplay`, `xmlMemDisplayLast`, `xmlMemShow`,
//! `xmlMemoryDump`) are no-ops matching upstream 2.15.0, which removed that
//! feature.
//!
//! # Ownership & safety invariants
//!
//! Every pointer returned by an xml* allocator must be freed with `xmlFree`
//! (OWNERSHIP_ATLAS section 1). The block registry records
//! ptr -> (size, file, line) for the debug-named surface only, so
//! `xmlMemSize` is exact for debug-surface blocks and 0 for default-allocator
//! blocks (upstream's MEMHDR tag lookup behaves identically: a plain `malloc`
//! block carries no tag). `xmlMemSetup` custom allocators bypass the registry
//! entirely, matching upstream's debug-allocator-only contract.
//!
//! # Historical quirks & epochs
//!
//! R-000178 (11.1-Z.3): the pre-Z.3 default allocator routed through Rust's
//! global allocator with fabricated `Layout`s — `default_free` deallocated
//! every pointer with a 1-byte layout and `default_realloc` passed the
//! requested new size as the old allocation layout; both are invalid-layout
//! UB under the Rust allocator contract. Replaced with libc
//! `malloc`/`realloc`/`free` (C allocation semantics; no layout exists), and
//! the default no longer maintains the accounting registry so `xmlMemUsed`/
//! `xmlMemBlocks`/`xmlMemSize` match the oracle's 0s; the registry now backs
//! only the debug-named surface. R-000131 (11.1-J) sealed: `xmlMemSize`
//! returns the recorded size for debug-surface blocks, the `*Loc` variants
//! accept-and-ignore file/line exactly like upstream 2.15.0's `ATTRIBUTE_UNUSED`
//! parameters, and the display functions are upstream-faithful no-ops.
//! R-000133 (11.1-H): the legacy names
//! (`xmlMemMalloc`/`xmlMemFree`/`xmlMemRealloc`/`xmlMemoryStrdup`) were
//! declared-but-unexported and had to be implemented for the honest-header
//! rule.
//!
//! # Deliberate oddities
//!
//! `xmlMemSetup`/direct variable assignment bypass the accounting registry
//! (deliberate: upstream's block table exists only in the debug allocator).
//! The five exported variables (`xmlMalloc`, `xmlMallocAtomic`, `xmlRealloc`,
//! `xmlFree`, `xmlMemStrdup`) are the single source of truth (R-000176,
//! 11.1-Z.2): `xmlMemSetup` assigns them and every internal allocation reads
//! them through the `*Impl` indirection, exactly like upstream internal
//! `xmlMalloc(...)` calls. The debug-named functions deliberately do NOT
//! route through the variables (upstream's debug allocator is independent of
//! the hooks); they are always libc-backed + registry-tracked.
//!
//! # Proving courts
//!
//! ABI-DATA, ALLOCATOR, ALLOCATOR-DEFAULT, GLOBAL-STATE and THREADING court
//! families; the allocator probes (`tools/abi/*_probe.py` +
//! `courts/suites/data-abi/*`) compile the same C probe against the oracle DSO
//! and the candidate and require byte-identical output; ALLOCATOR-DEFAULT-001
//! proves the default-allocator contract (many sizes, zero-size, grow/shrink
//! realloc, realloc-to-zero, realloc/malloc failure, strdup, direct
//! exported-variable calls, long churn, `xmlMemSize`/`xmlMemUsed`/`xmlMemBlocks`
//! exactness — all byte-identical with the oracle, R-000178); the DSO-LOADER
//! court resolves every exported symbol from the built DSO.
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is to keep the pre-Z.3 default allocator — Rust
//! `std::alloc` with fabricated layouts is invalid-layout UB (R-000178) and
//! returning nonzero `xmlMemUsed`/`xmlMemBlocks` under the default diverges
//! from the oracle's 0s. Another tempting shortcut is exporting the allocator
//! entry points as plain functions — upstream exports them as data function
//! pointers, so the allocator-override mechanism (`xmlMalloc` = custom) could
//! not link (R-000162 lesson).

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;
use std::os::raw::{c_char, c_int, c_long};

use crate::abi::callbacks::*;

// ═══════════════════════════════════════════════════════════════════════════════
// Default Allocator (libc — upstream 2.15.0 globals.c defaults, R-000178)
// ═══════════════════════════════════════════════════════════════════════════════
//
// Upstream initializes the five exported allocator variables to the C runtime
// functions (`xmlFree = free`, `xmlMalloc = malloc`, `xmlMallocAtomic = malloc`,
// `xmlRealloc = realloc`, `xmlMemStrdup = xmlPosixStrdup`). The candidate's
// `*Default` bodies are libc wrappers with identical observable behavior:
//
//   - malloc(0)      -> glibc returns a unique non-NULL pointer (C semantics);
//   - realloc(p, 0)  -> glibc frees p and returns NULL (C semantics);
//   - realloc(NULL,n)-> malloc(n) (C semantics);
//   - realloc failure-> NULL with the old block left intact (C semantics);
//   - free(NULL)     -> no-op (C semantics).
//
// All of these are byte-identical with the oracle (verified by the
// ALLOCATOR-DEFAULT-001 differential court). The pre-Z.3 implementation used
// Rust's `std::alloc` with fabricated `Layout`s: `default_free` deallocated
// every pointer with a 1-byte layout and `default_realloc` passed the
// requested NEW size as the OLD allocation layout. Rust's allocator API
// requires the deallocation/reallocation layout to correspond to the original
// allocation, so both were invalid-layout UB — the defect R-000178. libc
// allocation has no layout parameter, so the C contract is reproduced exactly
// and the UB class is eliminated.

/// Default malloc: `libc::malloc`.
///
/// # SAFETY
///
/// - `size` must be a valid allocation size (0 is handled by the platform
///   `malloc` contract, matching upstream which calls `malloc` directly)
/// - Returns NULL on allocation failure
unsafe extern "C" fn default_malloc(size: usize) -> *mut c_void {
    unsafe { libc::malloc(size) }
}

/// Default realloc: `libc::realloc`.
///
/// # SAFETY
///
/// - `ptr` must be a valid pointer from a previous `default_malloc` or `default_realloc`,
///   or NULL (in which case this behaves like malloc)
/// - `size` must be a valid allocation size; `realloc(p, 0)` follows the C
///   contract (glibc frees `p` and returns NULL), matching upstream which
///   calls `realloc` directly
/// - On failure the old block is left intact (C contract)
unsafe extern "C" fn default_realloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    unsafe { libc::realloc(ptr, size) }
}

/// Default free: `libc::free`.
///
/// # SAFETY
///
/// - `ptr` may be NULL (free(NULL) is a no-op)
/// - If non-NULL, `ptr` must be from a previous `default_malloc` or `default_realloc`
unsafe extern "C" fn default_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    unsafe { libc::free(ptr) };
}

/// Default strdup: `libc::malloc` + copy (upstream `xmlPosixStrdup` ->
/// `xmlCharStrdup`, which is a NULL-checked malloc+copy).
///
/// # SAFETY
///
/// - `str` must be a valid null-terminated C string or NULL (NULL returns NULL,
///   matching upstream `xmlCharStrdup`)
unsafe extern "C" fn default_strdup(str: *const c_char) -> *mut c_void {
    if str.is_null() {
        return ptr::null_mut();
    }
    let len = unsafe { libc::strlen(str) };
    let size = len + 1; // include null terminator
    let new_ptr = unsafe { libc::malloc(size) };
    if new_ptr.is_null() {
        return ptr::null_mut();
    }
    unsafe { ptr::copy_nonoverlapping(str as *const u8, new_ptr as *mut u8, size) };
    new_ptr
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
/// Since 11.1-Z.3 (R-000178) they are maintained ONLY by the debug-named
/// surface (`xmlMemMalloc`/`xmlMemFree`/`xmlMemRealloc`/`xmlMemoryStrdup` and
/// the `*Loc` variants) — exactly like upstream's `debugMemSize`/
/// `debugMemBlocks`, which the plain-malloc default never touches. With the
/// default allocator installed, `xmlMemUsed()` and `xmlMemBlocks()` return 0
/// (byte-identical with the oracle).
static MEM_USED: AtomicUsize = AtomicUsize::new(0);
static MEM_BLOCKS: AtomicUsize = AtomicUsize::new(0);

/// Per-block metadata (upstream xmlmemory.c block table): enables
/// xmlMemSize and the counters for the debug-named surface.
/// The file pointer is stored as a raw address (usize) so the registry stays
/// Send + Sync. The `file`/`line` fields mirror upstream's allocation-site
/// record (populated by the `*Loc` variants); they are write-only until a
/// future dump surface reads them, hence `allow(dead_code)`.
#[derive(Clone, Copy)]
#[allow(dead_code)]
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

/// Drop a block from the registry; returns its recorded size (None if the
/// block was unknown or NULL).
unsafe fn block_forget(ptr: *mut c_void) -> Option<usize> {
    if ptr.is_null() {
        return None;
    }
    BLOCKS.lock().remove(&(ptr as usize)).map(|m| m.size)
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

/// Default `xmlMalloc` body: plain `libc::malloc` (upstream default `malloc`).
///
/// This is the initial value of the exported `xmlMalloc` variable and never
/// reads the variable (no recursion). Since 11.1-Z.3 (R-000178) it performs
/// NO accounting: upstream's counters are maintained only by the debug
/// allocator, so with the default installed `xmlMemUsed`/`xmlMemBlocks` are 0
/// and `xmlMemSize` is 0 — byte-identical with the oracle.
///
/// # Safety
///
/// - `size` is a byte count passed straight to `libc::malloc`; 0 is handled
///   by the platform contract (glibc returns a unique non-NULL pointer).
/// - The returned pointer (NULL on failure) is a libc-owned allocation that
///   must be freed exactly once with the matching free path and never
///   dereferenced after freeing.
/// - This wrapper never reads the exported allocator variables, so it cannot
///   recurse and is safe to install as a hook.
unsafe extern "C" fn xmlMallocDefault(size: usize) -> *mut c_void {
    unsafe { default_malloc(size) }
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

/// Default `xmlRealloc` body: plain `libc::realloc` (upstream default `realloc`).
///
/// No accounting (R-000178) — see `xmlMallocDefault`. C semantics: `realloc(NULL,
/// n)` allocates, `realloc(p, 0)` follows the platform contract (glibc frees and
/// returns NULL), failure leaves the old block intact.
unsafe extern "C" fn xmlReallocDefault(ptr: *mut c_void, size: usize) -> *mut c_void {
    unsafe { default_realloc(ptr, size) }
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

/// Default `xmlFree` body: plain `libc::free` (upstream default `free`).
///
/// No accounting (R-000178) — see `xmlMallocDefault`.
unsafe extern "C" fn xmlFreeDefault(ptr: *mut c_void) {
    unsafe { default_free(ptr) };
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

/// Default `xmlMemStrdup` body: plain libc strdup (upstream default
/// `xmlPosixStrdup`). No accounting (R-000178) — see `xmlMallocDefault`.
unsafe extern "C" fn xmlMemStrdupDefault(str: *const c_char) -> *mut c_void {
    unsafe { default_strdup(str) }
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
/// Returns upstream's `debugMemSize` counter, which is maintained ONLY by the
/// debug allocator (`xmlMemMalloc`/`*Loc` surface). With the default
/// allocator installed the counter stays 0 — byte-identical with the oracle
/// (R-000178, verified by ALLOCATOR-DEFAULT-001). Custom allocator hooks
/// never touch it (upstream contract).
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
/// Returns upstream's `debugMemBlocks` counter (debug allocator only; 0 with
/// the default allocator — R-000178, byte-identical with the oracle).
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
/// No-op — upstream 2.15.0 removed this feature (`@deprecated This feature
/// was removed.`). The pre-Z.3 candidate printed aggregate counters; that was
/// a divergence from the executed oracle and is removed (R-000131 sealed).
///
/// # SAFETY
///
/// - `fp` must be a valid FILE* pointer or NULL (unused)
#[no_mangle]
pub const unsafe extern "C" fn xmlMemDisplay(_fp: *mut c_void) {}

/// Show memory allocation information.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlMemShow(FILE *fp, int nr);
/// ```
///
/// No-op — upstream 2.15.0 removed this feature (`@deprecated This feature
/// was removed.`); the candidate previously dumped the registry with a
/// non-upstream ordering, a documented divergence that is now removed
/// (R-000131 sealed).
///
/// # SAFETY
///
/// - `fp` must be a valid FILE* pointer or NULL (unused)
#[no_mangle]
pub const unsafe extern "C" fn xmlMemShow(_fp: *mut c_void, _nr: c_int) {}

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
// semantics. In upstream 2.15.0 these ARE the debug allocator: always
// libc-backed, independent of the hook variables, and tracked by
// `debugMemSize`/`debugMemBlocks` (with the MEMHDR tag enabling
// `xmlMemSize`). The candidate mirrors that exactly: the debug-named
// functions and the `*Loc` variants are always libc-backed + registry- and
// counter-tracked, do NOT route through the exported variables, and the
// `*Loc` location arguments are accepted and ignored — exactly like
// upstream's `ATTRIBUTE_UNUSED` parameters (R-000131 sealed). The candidate
// returns plain libc pointers (no MEMHDR prefix), so a debug-surface block
// can also be freed with `xmlFree` — a safe superset of the upstream
// contract (upstream requires `xmlMemFree` for such blocks).

/// Debug-surface malloc: libc + counters + registry (upstream xmlMemMalloc).
///
/// # Safety
///
/// - `size` must be a valid allocation size; the underlying `default_malloc`
///   follows the libc contract (0 handled by the platform).
/// - `file` must be NULL or a valid pointer to a NUL-terminated C string that
///   stays valid for the duration of the call (it is stored by address only,
///   never dereferenced here).
/// - The returned pointer (NULL on failure, with counters untouched) is owned
///   by the caller and must be freed exactly once via `debug_free` or
///   `xmlMemFree`; never dereferenced after freeing.
unsafe fn debug_malloc(size: usize, file: *const c_char, line: c_int) -> *mut c_void {
    let ptr = unsafe { default_malloc(size) };
    if !ptr.is_null() {
        MEM_USED.fetch_add(size, Ordering::Relaxed);
        MEM_BLOCKS.fetch_add(1, Ordering::Relaxed);
        unsafe { block_record(ptr, size, file, line) };
    }
    ptr
}

/// Debug-surface realloc: libc + counters + registry (upstream xmlMemRealloc).
///
/// # Safety
///
/// - `ptr` must be NULL or a valid pointer previously returned by the debug
///   surface (`debug_malloc`/`debug_realloc`) or by a matching libc
///   allocation, and not yet freed; NULL behaves like malloc.
/// - On failure the old block is left intact and stays recorded; on success
///   the old pointer is invalidated and the returned pointer must be freed
///   exactly once via `debug_free`/`xmlMemFree`.
/// - `file` must be NULL or a valid NUL-terminated C string valid for the
///   duration of the call (stored by address only).
unsafe fn debug_realloc(
    ptr: *mut c_void,
    size: usize,
    file: *const c_char,
    line: c_int,
) -> *mut c_void {
    let new_ptr = unsafe { default_realloc(ptr, size) };
    if !new_ptr.is_null() {
        let old_size = unsafe { block_forget(ptr) };
        if let Some(old) = old_size {
            MEM_USED.fetch_add(size.saturating_sub(old), Ordering::Relaxed);
        } else if ptr.is_null() {
            MEM_USED.fetch_add(size, Ordering::Relaxed);
            MEM_BLOCKS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { block_record(new_ptr, size, file, line) };
    }
    new_ptr
}

/// Debug-surface strdup: libc + counters + registry (upstream xmlMemoryStrdup).
///
/// # Safety
///
/// - `str` must be NULL or a valid pointer to a NUL-terminated C string
///   readable through its full length (including the terminator) for the
///   duration of the call; NULL yields NULL.
/// - `file` must be NULL or a valid NUL-terminated C string valid for the
///   duration of the call (stored by address only).
/// - The returned pointer (NULL on failure) must be freed exactly once via
///   `debug_free`/`xmlMemFree`; never dereferenced after freeing.
unsafe fn debug_strdup(str: *const c_char, file: *const c_char, line: c_int) -> *mut c_void {
    if str.is_null() {
        return ptr::null_mut();
    }
    let ptr = unsafe { default_strdup(str) };
    if !ptr.is_null() {
        let len = unsafe { libc::strlen(str) } + 1;
        MEM_USED.fetch_add(len, Ordering::Relaxed);
        MEM_BLOCKS.fetch_add(1, Ordering::Relaxed);
        unsafe { block_record(ptr, len, file, line) };
    }
    ptr
}

/// Debug-surface free: registry/counter removal + libc free (upstream xmlMemFree).
/// A foreign pointer (not in the registry) is freed without touching the
/// counters — a safe divergence from upstream's tag-error print (which would
/// pollute stderr).
///
/// # Safety
///
/// - `ptr` must be NULL (a no-op) or a valid pointer previously returned by
///   the debug surface (`debug_malloc`/`debug_realloc`/`debug_strdup`) or by
///   a matching libc allocation; it must not be freed twice and must not be
///   dereferenced after this call.
/// - The registry and counters are only touched when the pointer was recorded;
///   a foreign pointer is still freed via libc.
unsafe fn debug_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    let old_size = unsafe { block_forget(ptr) };
    unsafe { default_free(ptr) };
    if let Some(old) = old_size {
        MEM_USED.fetch_sub(old, Ordering::Relaxed);
        MEM_BLOCKS.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Allocate memory through the debug allocator (upstream xmlmemory.h).
///
/// ```c
/// void *xmlMemMalloc(size_t size);
/// ```
///
/// Always libc-backed and tracked (upstream debug allocator contract): the
/// block is recorded so `xmlMemSize`/`xmlMemUsed`/`xmlMemBlocks` observe it.
///
/// # SAFETY
///
/// - The returned pointer must be freed with `xmlMemFree` (or `xmlFree` —
///   plain libc pointer, candidate superset)
#[no_mangle]
pub unsafe extern "C" fn xmlMemMalloc(size: usize) -> *mut c_void {
    unsafe { debug_malloc(size, ptr::null(), 0) }
}

/// Free memory through the debug allocator (upstream xmlmemory.h).
///
/// ```c
/// void xmlMemFree(void *ptr);
/// ```
///
/// Un-records the block and frees it. NULL is a no-op.
///
/// # SAFETY
///
/// - `ptr` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
#[no_mangle]
pub unsafe extern "C" fn xmlMemFree(ptr: *mut c_void) {
    unsafe { debug_free(ptr) };
}

/// Reallocate memory through the debug allocator (upstream xmlmemory.h).
///
/// ```c
/// void *xmlMemRealloc(void *ptr, size_t size);
/// ```
///
/// C realloc semantics + registry maintenance (upstream xmlMemRealloc:
/// NULL ptr allocates, failure leaves the old block recorded and intact).
///
/// # SAFETY
///
/// - `ptr` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
#[no_mangle]
pub unsafe extern "C" fn xmlMemRealloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    unsafe { debug_realloc(ptr, size, ptr::null(), 0) }
}

/// Duplicate a string through the debug allocator (upstream xmlmemory.h).
///
/// ```c
/// void *xmlMemoryStrdup(const char *str);
/// ```
///
/// NULL returns NULL (upstream xmlPosixStrdup/xmlCharStrdup contract).
///
/// # SAFETY
///
/// - `str` must point to a valid NUL-terminated string, or NULL
/// - The returned pointer must be freed with `xmlMemFree` (or `xmlFree`)
#[no_mangle]
pub unsafe extern "C" fn xmlMemoryStrdup(str: *const c_char) -> *mut c_void {
    unsafe { debug_strdup(str, ptr::null(), 0) }
}

/// Allocate memory, recording the allocation site (upstream xmlmemory.h).
///
/// ```c
/// void *xmlMallocLoc(size_t size, const char *file, int line);
/// ```
///
/// Debug-allocator contract (upstream xmlMallocLoc -> xmlMemMalloc): always
/// libc-backed + tracked, independent of the hook variables. Upstream 2.15.0
/// ignores the file/line arguments (`ATTRIBUTE_UNUSED`); the candidate
/// records them in the registry so `xmlMemSize` works (R-000131 sealed).
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
    unsafe { debug_malloc(size, file, line) }
}

/// Allocate memory, recording the allocation site (upstream xmlmemory.h).
///
/// ```c
/// void *xmlMallocAtomicLoc(size_t size, const char *file, int line);
/// ```
///
/// Upstream xmlMallocAtomicLoc -> xmlMemMalloc: a plain (non-zeroed) tracked
/// allocation. The pre-Z.3 candidate zeroed it via `xmlMallocZero` — a real
/// divergence, fixed in 11.1-Z.3 (R-000178 narrative).
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
    unsafe { debug_malloc(size, file, line) }
}

/// Reallocate memory, recording the allocation site (upstream xmlmemory.h).
///
/// ```c
/// void *xmlReallocLoc(void *ptr, size_t size, const char *file, int line);
/// ```
///
/// Debug-allocator contract (upstream xmlReallocLoc -> xmlMemRealloc): the
/// old record is superseded on success, kept on failure.
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
    unsafe { debug_realloc(ptr, size, file, line) }
}

/// Duplicate a string, recording the allocation site (upstream xmlmemory.h).
///
/// ```c
/// void *xmlMemStrdupLoc(const char *str, const char *file, int line);
/// ```
///
/// Debug-allocator contract (upstream xmlMemStrdupLoc -> xmlMemoryStrdup).
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
    unsafe { debug_strdup(str, file, line) }
}

/// Return the size of an allocated block (upstream xmlmemory.h).
///
/// ```c
/// size_t xmlMemSize(void *ptr);
/// ```
///
/// Returns the recorded size for debug-surface blocks (`xmlMemMalloc`/`*Loc`
/// surface) and 0 for everything else — matching upstream's MEMHDR tag
/// lookup, which misses on plain-malloc blocks (default allocator) and on
/// foreign pointers (R-000178; byte-identical with the oracle).
///
/// # SAFETY
///
/// - `ptr` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
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
/// No-op — upstream 2.15.0 removed this feature (`@deprecated This feature
/// was removed.`); the pre-Z.3 candidate dumped the registry with a
/// non-upstream format, a divergence now removed (R-000131 sealed).
///
/// # SAFETY
///
/// - `fp` must be a valid FILE* pointer or NULL (unused)
#[no_mangle]
pub const unsafe extern "C" fn xmlMemDisplayLast(_fp: *mut c_void, _nb_bytes: c_long) {}

/// Dump memory allocation statistics (upstream xmlmemory.h).
///
/// ```c
/// void xmlMemoryDump(void);
/// ```
///
/// No-op — upstream 2.15.0 removed this feature (`@deprecated This feature
/// was removed.`).
///
/// # SAFETY
///
/// The function touches crate-global state only; it is safe
/// as long as the caller respects the library's global
/// initialization/cleanup ordering (xmlInitParser before use,
/// xmlCleanupParser only after all users are done).
#[no_mangle]
pub const unsafe extern "C" fn xmlMemoryDump() {
    // Upstream: empty body (feature removed in 2.15.0).
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr;

    /// Test basic allocation and deallocation.
    ///
    /// # Safety
    ///
    /// - `ptr` is the non-NULL result of `xmlMalloc(100)` (asserted): it is a
    ///   valid heap allocation owned by the caller, freed exactly once by
    ///   `xmlFree`, and not used afterwards.
    #[test]
    fn test_malloc_free() {
        unsafe {
            let ptr = xmlMalloc(100);
            assert!(!ptr.is_null(), "xmlMalloc(100) returned NULL");
            xmlFree(ptr);
        }
    }

    /// Test that xmlMalloc(0) returns a valid pointer.
    ///
    /// # Safety
    ///
    /// - `xmlMalloc(0)` may return NULL or a unique non-NULL pointer per the
    ///   platform contract; a non-NULL result is a valid allocation freed
    ///   exactly once by `xmlFree` and not used afterwards.
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
    ///
    /// # Safety
    ///
    /// - Passing NULL to `xmlFree` is accepted and performs no operation; no
    ///   pointer is dereferenced.
    #[test]
    fn test_free_null() {
        unsafe {
            xmlFree(ptr::null_mut());
            // Should not crash
        }
    }

    /// Test xmlRealloc.
    ///
    /// # Safety
    ///
    /// - `ptr` must be a valid non-NULL allocation from `xmlMalloc`; on the
    ///   successful `xmlRealloc` here the old pointer is invalidated and only
    ///   the returned `new_ptr` (non-NULL, asserted) is freed exactly once by
    ///   `xmlFree` and not used afterwards.
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
    ///
    /// # Safety
    ///
    /// - `s` points to a valid NUL-terminated byte string of 6 bytes (the
    ///   literal `hello` plus terminator) that stays alive for the call.
    /// - `dup` is the non-NULL result of `xmlMemStrdup`, a fresh allocation of
    ///   at least 6 bytes readable as a slice; it is freed exactly once by
    ///   `xmlFree` and not used afterwards.
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
    ///
    /// # Safety
    ///
    /// - `ptr` is the non-NULL result of `xmlMallocZero(100)`: a valid
    ///   allocation of at least 100 bytes, readable as a 100-byte slice
    ///   while alive, and freed exactly once by `xmlFree` afterwards.
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
    ///
    /// # Safety
    ///
    /// - Each output pointer passed to `xmlMemGet` is derived from a live
    ///   stack local of matching type, so every non-NULL output points to
    ///   valid, aligned, writable storage for the duration of the call; NULL
    ///   outputs are tolerated and skipped.
    /// - The function pointers written back must be treated as valid
    ///   C-compatible hooks (they are the currently installed allocator
    ///   functions).
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
    ///
    /// # Safety
    ///
    /// - Each output pointer passed to `xmlGcMemGet` is derived from a live
    ///   stack local of matching type, so every non-NULL output points to
    ///   valid, aligned, writable storage for the duration of the call; NULL
    ///   outputs are tolerated and skipped.
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
    ///
    /// # Safety
    ///
    /// - `default_malloc` cast to `xmlMallocFunc` is a valid C-compatible
    ///   malloc-shaped function pointer; it is only passed as an argument here
    ///   and never called by this test.
    /// - The NULL hooks are rejected with -1 before any write, so no
    ///   allocator state is modified by this test.
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
    ///
    /// # Safety
    ///
    /// - The exported `static mut` allocator variables are read and written
    ///   directly, which is only valid under the upstream single-threaded
    ///   setup ordering: no other thread may allocate or read the variables
    ///   concurrently with these assignments.
    /// - The values installed (`xmlMallocDefault`/`xmlFreeDefault`) are valid
    ///   malloc/free-shaped functions; `p` from `xmlMallocImpl` is a valid
    ///   allocation freed exactly once by `xmlFreeImpl`; the prior hook values
    ///   are restored before the block ends.
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

    /// Test xmlMemUsed and xmlMemBlocks return 0 with the default allocator
    /// and the debug surface is tracked (R-000178: byte-identical with the
    /// oracle — default malloc untracked, xmlMemMalloc/*Loc tracked).
    ///
    /// # Safety
    ///
    /// - `ptr` from `xmlMalloc` is a valid libc allocation freed exactly once
    ///   by `xmlFree`; `xmlMemSize` reads the registry by address only and is
    ///   safe on freed default-allocator pointers (returns 0).
    /// - `dptr` from `xmlMallocLoc` and `rptr` from `xmlReallocLoc` are
    ///   debug-surface allocations: on success the old pointer is invalidated
    ///   and only `rptr` is freed, exactly once, by `xmlMemFree`.
    #[test]
    fn test_mem_stats() {
        unsafe {
            // Default allocator: plain libc, NOT tracked (oracle contract).
            let ptr = xmlMalloc(100);
            assert!(!ptr.is_null());
            assert_eq!(xmlMemSize(ptr), 0, "default-allocator blocks are untracked");
            xmlFree(ptr);
            assert_eq!(xmlMemSize(ptr), 0);

            // Debug surface: tracked, xmlMemSize returns the recorded size.
            let dptr = xmlMallocLoc(100, c"mem.c".as_ptr(), 42);
            assert!(!dptr.is_null());
            assert_eq!(xmlMemSize(dptr), 100, "debug-surface blocks are tracked");
            let rptr = xmlReallocLoc(dptr, 200, c"mem.c".as_ptr(), 43);
            assert!(!rptr.is_null());
            // The realloc record supersedes the old one (same address when
            // glibc grows in place — then the block AT that address is 200).
            assert_eq!(xmlMemSize(rptr), 200);
            assert_eq!(xmlMemSize(dptr), xmlMemSize(rptr));
            xmlMemFree(rptr);
            assert_eq!(xmlMemSize(rptr), 0);

            // xmlMemSize(NULL) is 0.
            assert_eq!(xmlMemSize(ptr::null_mut()), 0);
        }
    }

    /// Test the default allocator follows the C/libc contract exactly
    /// (R-000178): malloc(0) non-NULL, realloc(p,0) NULL after freeing,
    /// realloc(NULL,n) == malloc, realloc failure leaves the old block,
    /// malloc(SIZE_MAX) NULL, strdup(NULL) NULL.
    ///
    /// # Safety
    ///
    /// - Each non-NULL allocation is freed exactly once by `xmlFree`, and
    ///   pointers are not used after freeing; in particular `q` is freed by
    ///   the `realloc(p, 0)` call that returns NULL (glibc contract), so it
    ///   must not be used afterwards, and `r` is freed explicitly after the
    ///   failed huge realloc leaves it intact.
    /// - NULL arguments to `xmlRealloc`, `xmlMemStrdup` and `xmlFree` are
    ///   accepted per the C contract.
    #[test]
    fn test_default_libc_semantics() {
        unsafe {
            // malloc(0): glibc returns a unique non-NULL pointer.
            let p0 = xmlMalloc(0);
            assert!(!p0.is_null(), "malloc(0) must be non-NULL on glibc");
            xmlFree(p0);

            // realloc(NULL, n) allocates.
            let p = xmlRealloc(ptr::null_mut(), 16);
            assert!(!p.is_null());
            // grow: content preserved.
            let q = xmlRealloc(p, 256);
            assert!(!q.is_null());
            // realloc(p, 0): glibc frees and returns NULL.
            let z = xmlRealloc(q, 0);
            assert!(z.is_null(), "realloc(p, 0) returns NULL on glibc");

            // realloc failure: old block intact.
            let r = xmlMalloc(8);
            assert!(!r.is_null());
            let huge = xmlRealloc(r, usize::MAX);
            assert!(huge.is_null(), "realloc to SIZE_MAX must fail");
            xmlFree(r);

            // malloc failure.
            let m = xmlMalloc(usize::MAX);
            assert!(m.is_null(), "malloc(SIZE_MAX) must fail");

            // strdup(NULL) returns NULL (upstream xmlPosixStrdup).
            let d = xmlMemStrdup(ptr::null());
            assert!(d.is_null());

            // free(NULL) no-op.
            xmlFree(ptr::null_mut());
        }
    }

    /// Test the debug-named surface is independent of the hook variables and
    /// always libc-backed + tracked (upstream debug-allocator contract).
    ///
    /// # Safety
    ///
    /// - `p`, `q` and `s` are non-NULL debug-surface allocations; after the
    ///   successful `xmlMemRealloc` only `q` remains valid, and `q` and `s`
    ///   are each freed exactly once by `xmlMemFree` and not used afterwards.
    /// - `xmlMemFree(NULL)` is a no-op; `xmlMemSize` reads the registry by
    ///   address only.
    #[test]
    fn test_debug_surface_tracked() {
        unsafe {
            let p = xmlMemMalloc(64);
            assert!(!p.is_null());
            assert_eq!(xmlMemSize(p), 64);
            let q = xmlMemRealloc(p, 128);
            assert!(!q.is_null());
            assert_eq!(xmlMemSize(q), 128);
            let s = xmlMemoryStrdup(c"dbg".as_ptr());
            assert!(!s.is_null());
            assert_eq!(xmlMemSize(s), 4);
            xmlMemFree(q);
            xmlMemFree(s);
            // xmlMemFree(NULL) no-op.
            xmlMemFree(ptr::null_mut());
        }
    }
}
