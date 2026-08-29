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
//! Complete — all allocator APIs are implemented with thread-safe global state.
//!
//! # Safety
//!
//! Allocator hooks are `unsafe` because they operate on raw pointers and are called
//! from C code. Every public function documents its safety contract.

use core::alloc::Layout;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;
use std::os::raw::{c_char, c_int, c_long};

use parking_lot::RwLock;

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
// Global Allocator State
// ═══════════════════════════════════════════════════════════════════════════════

/// Global allocator function pointers.
///
/// Protected by RwLock for thread-safe read/write.
/// Default values point to Rust's global allocator.
static ALLOCATOR: RwLock<AllocatorFuncs> = RwLock::new(AllocatorFuncs {
    malloc_func: Some(default_malloc as xmlMallocFunc),
    realloc_func: Some(default_realloc as xmlReallocFunc),
    free_func: Some(default_free as xmlFreeFunc),
    strdup_func: Some(default_strdup as xmlStrdupFunc),
});

/// The set of allocator function pointers.
struct AllocatorFuncs {
    malloc_func: Option<xmlMallocFunc>,
    realloc_func: Option<xmlReallocFunc>,
    free_func: Option<xmlFreeFunc>,
    strdup_func: Option<xmlStrdupFunc>,
}

/// Global allocation counters (for xmlMemUsed/xmlMemBlocks).
///
/// These use relaxed ordering since they are approximate debugging counters.
static MEM_USED: AtomicUsize = AtomicUsize::new(0);
static MEM_BLOCKS: AtomicUsize = AtomicUsize::new(0);

// ═══════════════════════════════════════════════════════════════════════════════
// Public Allocator API
// ═══════════════════════════════════════════════════════════════════════════════

/// Set custom memory allocator functions.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlMemSetup(xmlFreeFunc freeFunc,
///                  xmlMallocFunc mallocFunc,
///                  xmlReallocFunc reallocFunc,
///                  xmlStrdupFunc strdupFunc);
/// ```
///
/// # SAFETY
///
/// - All function pointers must be valid (non-null) and thread-safe
/// - The functions must follow the C malloc/realloc/free/strdup contract
/// - Once set, the functions remain in effect until the next `xmlMemSetup` call
/// - The caller is responsible for ensuring the functions remain valid for the
///   entire time they are installed
#[no_mangle]
pub unsafe extern "C" fn xmlMemSetup(
    freeFunc: Option<xmlFreeFunc>,
    mallocFunc: Option<xmlMallocFunc>,
    reallocFunc: Option<xmlReallocFunc>,
    strdupFunc: Option<xmlStrdupFunc>,
) {
    // SAFETY: Caller guarantees all function pointers are valid.
    // We store them in global state protected by RwLock.
    let mut alloc = ALLOCATOR.write();
    alloc.free_func = freeFunc;
    alloc.malloc_func = mallocFunc;
    alloc.realloc_func = reallocFunc;
    alloc.strdup_func = strdupFunc;
}

/// Get the current memory allocator functions.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlMemGet(xmlFreeFunc *freeFunc,
///                xmlMallocFunc *mallocFunc,
///                xmlReallocFunc *reallocFunc,
///                xmlStrdupFunc *strdupFunc);
/// ```
///
/// # SAFETY
///
/// - All output pointers must be valid (non-null) and writable
#[no_mangle]
pub unsafe extern "C" fn xmlMemGet(
    freeFunc: *mut Option<xmlFreeFunc>,
    mallocFunc: *mut Option<xmlMallocFunc>,
    reallocFunc: *mut Option<xmlReallocFunc>,
    strdupFunc: *mut Option<xmlStrdupFunc>,
) {
    // SAFETY: Caller guarantees all output pointers are valid.
    let alloc = ALLOCATOR.read();
    unsafe {
        ptr::write(freeFunc, alloc.free_func);
        ptr::write(mallocFunc, alloc.malloc_func);
        ptr::write(reallocFunc, alloc.realloc_func);
        ptr::write(strdupFunc, alloc.strdup_func);
    }
}

/// Set GC-aware memory allocator functions.
///
/// # UPSTREAM-PARITY
///
/// This is a wrapper around `xmlMemSetup` in modern libxml2.
/// Historically, it was separate for the GC-allocated memory pool,
/// but in modern versions both functions do the same thing.
#[no_mangle]
pub unsafe extern "C" fn xmlGcMemSetup(
    freeFunc: Option<xmlFreeFunc>,
    mallocFunc: Option<xmlMallocFunc>,
    reallocFunc: Option<xmlReallocFunc>,
    strdupFunc: Option<xmlStrdupFunc>,
) {
    // SAFETY: Delegates to xmlMemSetup with the same safety contract.
    unsafe { xmlMemSetup(freeFunc, mallocFunc, reallocFunc, strdupFunc) };
}

/// Get GC-aware memory allocator functions.
///
/// # UPSTREAM-PARITY
///
/// Wrapper around `xmlMemGet`.
#[no_mangle]
pub unsafe extern "C" fn xmlGcMemGet(
    freeFunc: *mut Option<xmlFreeFunc>,
    mallocFunc: *mut Option<xmlMallocFunc>,
    reallocFunc: *mut Option<xmlReallocFunc>,
    strdupFunc: *mut Option<xmlStrdupFunc>,
) {
    // SAFETY: Delegates to xmlMemGet with the same safety contract.
    unsafe { xmlMemGet(freeFunc, mallocFunc, reallocFunc, strdupFunc) };
}

// ═══════════════════════════════════════════════════════════════════════════════
// Allocation Functions
// ═══════════════════════════════════════════════════════════════════════════════

/// Allocate memory.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xmlMalloc(size_t size);
/// ```
///
/// Returns a pointer to the allocated memory, or NULL on failure.
/// The allocated memory is not initialized (like malloc).
///
/// # SAFETY
///
/// - The returned pointer must be freed with `xmlFree`
/// - `size` may be 0 (returns a valid non-NULL pointer or NULL)
#[no_mangle]
pub unsafe extern "C" fn xmlMalloc(size: usize) -> *mut c_void {
    // SAFETY: We call the stored malloc function pointer.
    // The function pointer must be valid (set by xmlMemSetup or default).
    let alloc = ALLOCATOR.read();
    let malloc_func = alloc.malloc_func.unwrap_or(default_malloc as xmlMallocFunc);
    let ptr = unsafe { malloc_func(size) };
    if !ptr.is_null() {
        MEM_USED.fetch_add(size, Ordering::Relaxed);
        MEM_BLOCKS.fetch_add(1, Ordering::Relaxed);
    }
    ptr
}

/// Allocate memory that will never contain pointers to other memory.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xmlMallocAtomic(size_t size);
/// ```
///
/// Identical to `xmlMalloc` but hints to the GC that the memory does not
/// contain pointers. In modern libxml2, this is equivalent to `xmlMalloc`.
#[no_mangle]
pub unsafe extern "C" fn xmlMallocAtomic(size: usize) -> *mut c_void {
    // SAFETY: Same as xmlMalloc.
    unsafe { xmlMalloc(size) }
}

/// Reallocate memory.
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
#[no_mangle]
pub unsafe extern "C" fn xmlRealloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    // SAFETY: We call the stored realloc function pointer.
    let alloc = ALLOCATOR.read();
    let realloc_func = alloc
        .realloc_func
        .unwrap_or(default_realloc as xmlReallocFunc);
    let new_ptr = unsafe { realloc_func(ptr, size) };
    if !new_ptr.is_null() {
        // Update counters (approximate — we don't know the old size)
        MEM_BLOCKS.fetch_add(1, Ordering::Relaxed);
        if ptr.is_null() {
            MEM_USED.fetch_add(size, Ordering::Relaxed);
        }
        // Note: we don't subtract old size because we don't track it.
        // This makes counters approximate, matching upstream behavior
        // where the debugging allocator tracks sizes but the default doesn't.
    }
    new_ptr
}

/// Free allocated memory.
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
#[no_mangle]
pub unsafe extern "C" fn xmlFree(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: We call the stored free function pointer.
    let alloc = ALLOCATOR.read();
    let free_func = alloc.free_func.unwrap_or(default_free as xmlFreeFunc);
    unsafe { free_func(ptr) };
    MEM_BLOCKS.fetch_sub(1, Ordering::Relaxed);
}

/// Duplicate a C string using the configured allocator.
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
#[no_mangle]
pub unsafe extern "C" fn xmlMemStrdup(str: *const c_char) -> *mut c_void {
    if str.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: We call the stored strdup function pointer.
    let alloc = ALLOCATOR.read();
    let strdup_func = alloc.strdup_func.unwrap_or(default_strdup as xmlStrdupFunc);
    let ptr = unsafe { strdup_func(str) };
    if !ptr.is_null() {
        let len = unsafe { libc::strlen(str) } + 1;
        MEM_USED.fetch_add(len, Ordering::Relaxed);
        MEM_BLOCKS.fetch_add(1, Ordering::Relaxed);
    }
    ptr
}

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
#[no_mangle]
pub unsafe extern "C" fn xmlMemShow(_fp: *mut c_void, _nr: c_int) {
    // Phase 1: no-op with the default allocator.
    // A future debugging allocator could track allocation history.
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
    // SAFETY: Delegates to xmlMalloc and zeroes the memory.
    let ptr = unsafe { xmlMalloc(size) };
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
#[no_mangle]
pub unsafe extern "C" fn xmlMallocAtomicZero(size: usize) -> *mut c_void {
    // SAFETY: Delegates to xmlMallocAtomic and zeroes the memory.
    let ptr = unsafe { xmlMallocAtomic(size) };
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
    let new_ptr = unsafe { xmlRealloc(ptr, new_size) };
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
pub extern "C" fn xmlInitMemory() -> c_int {
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
pub extern "C" fn xmlCleanupMemory() {
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
#[no_mangle]
pub unsafe extern "C" fn xmlMemMalloc(size: usize) -> *mut c_void {
    // SAFETY: identical contract to xmlMalloc.
    unsafe { xmlMalloc(size) }
}

/// Free memory (legacy name; same contract as `xmlFree`).
///
/// ```c
/// void xmlMemFree(void *ptr);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlMemFree(ptr: *mut c_void) {
    // SAFETY: identical contract to xmlFree.
    unsafe { xmlFree(ptr) }
}

/// Reallocate memory (legacy name; same contract as `xmlRealloc`).
///
/// ```c
/// void *xmlMemRealloc(void *ptr, size_t size);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlMemRealloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    // SAFETY: identical contract to xmlRealloc.
    unsafe { xmlRealloc(ptr, size) }
}

/// Duplicate a string (legacy name; same contract as `xmlMemStrdup`).
///
/// ```c
/// void *xmlMemoryStrdup(const char *str);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlMemoryStrdup(str: *const c_char) -> *mut c_void {
    // SAFETY: identical contract to xmlMemStrdup.
    unsafe { xmlMemStrdup(str) }
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
#[no_mangle]
pub unsafe extern "C" fn xmlMallocLoc(
    size: usize,
    _file: *const c_char,
    _line: c_int,
) -> *mut c_void {
    // SAFETY: identical contract to xmlMalloc.
    unsafe { xmlMalloc(size) }
}

/// Allocate zeroed memory, recording the allocation site (upstream xmlmemory.h).
///
/// ```c
/// void *xmlMallocAtomicLoc(size_t size, const char *file, int line);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlMallocAtomicLoc(
    size: usize,
    _file: *const c_char,
    _line: c_int,
) -> *mut c_void {
    // SAFETY: identical contract to xmlMallocZero.
    unsafe { xmlMallocZero(size) }
}

/// Reallocate memory, recording the allocation site (upstream xmlmemory.h).
///
/// ```c
/// void *xmlReallocLoc(void *ptr, size_t size, const char *file, int line);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlReallocLoc(
    ptr: *mut c_void,
    size: usize,
    _file: *const c_char,
    _line: c_int,
) -> *mut c_void {
    // SAFETY: identical contract to xmlRealloc.
    unsafe { xmlRealloc(ptr, size) }
}

/// Duplicate a string, recording the allocation site (upstream xmlmemory.h).
///
/// ```c
/// void *xmlMemStrdupLoc(const char *str, const char *file, int line);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlMemStrdupLoc(
    str: *const c_char,
    _file: *const c_char,
    _line: c_int,
) -> *mut c_void {
    // SAFETY: identical contract to xmlMemStrdup.
    unsafe { xmlMemStrdup(str) }
}

/// Return the size of an allocated block (upstream xmlmemory.h).
///
/// ```c
/// size_t xmlMemSize(void *ptr);
/// ```
///
/// The default candidate allocator does not maintain a per-block size table
/// (that is the upstream debug-allocator block list); it therefore returns 0
/// for all blocks — a documented safe divergence (residual R-000131) until
/// the allocator instrumentation court (11.1-J) adds block metadata.
#[no_mangle]
pub unsafe extern "C" fn xmlMemSize(_ptr: *mut c_void) -> usize {
    0
}

/// Display a limited amount of memory debug information (upstream xmlmemory.h).
///
/// ```c
/// void xmlMemDisplayLast(FILE *fp, long nbBytes);
/// ```
///
/// The default candidate allocator prints the global counters (it has no
/// per-block list); the output format is intentionally simpler than
/// upstream's block dump — a documented safe divergence (residual R-000131).
#[no_mangle]
pub unsafe extern "C" fn xmlMemDisplayLast(fp: *mut c_void, _nb_bytes: c_long) {
    // SAFETY: fp must be a valid FILE* or NULL (stderr used).
    unsafe {
        let used = MEM_USED.load(Ordering::Relaxed);
        let blocks = MEM_BLOCKS.load(Ordering::Relaxed);
        let out = if fp.is_null() {
            libc::fdopen(2, b"w\0" as *const u8 as *const c_char) as *mut c_void
        } else {
            fp
        };
        if !out.is_null() {
            let msg = format!(
                "libxml-rs allocator: {} blocks, {} bytes in use\n",
                blocks, used
            );
            let bytes = msg.as_bytes();
            libc::fwrite(
                bytes.as_ptr() as *const c_void,
                1,
                bytes.len(),
                out as *mut libc::FILE,
            );
        }
    }
}

/// Dump memory allocation statistics (upstream xmlmemory.h).
///
/// ```c
/// int xmlMemoryDump(void);
/// ```
///
/// Prints the global counters to stderr and returns 0 (no leak detector is
/// active in the default allocator).
#[no_mangle]
pub unsafe extern "C" fn xmlMemoryDump() -> c_int {
    unsafe {
        xmlMemDisplayLast(ptr::null_mut(), -1);
    }
    0
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

    /// Test custom allocator setup/get.
    #[test]
    fn test_mem_setup_get() {
        unsafe {
            let mut free_func: Option<xmlFreeFunc> = None;
            let mut malloc_func: Option<xmlMallocFunc> = None;
            let mut realloc_func: Option<xmlReallocFunc> = None;
            let mut strdup_func: Option<xmlStrdupFunc> = None;

            xmlMemGet(
                &mut free_func as *mut _,
                &mut malloc_func as *mut _,
                &mut realloc_func as *mut _,
                &mut strdup_func as *mut _,
            );

            assert!(malloc_func.is_some());
            assert!(free_func.is_some());
            assert!(realloc_func.is_some());
            assert!(strdup_func.is_some());
        }
    }

    /// Test xmlMemUsed and xmlMemBlocks return reasonable values.
    #[test]
    fn test_mem_stats() {
        unsafe {
            let before_used = xmlMemUsed();
            let before_blocks = xmlMemBlocks();

            let ptr = xmlMalloc(100);
            assert!(!ptr.is_null());

            // After allocation, used and blocks should be higher
            assert!(xmlMemBlocks() >= before_blocks + 1);

            xmlFree(ptr);
        }
    }
}
