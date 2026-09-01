//! exports_hash — family closure (11.1-I). Filled by workstream.
//!
//! C ABI exports for the libxml2 hash-table and dictionary families:
//! `xmlHashAdd`, `xmlHashAdd2`, `xmlHashAdd3`, `xmlHashCopySafe`,
//! `xmlHashDefaultDeallocator`, `xmlHashQLookup`, `xmlHashQLookup2`,
//! `xmlHashQLookup3`, `xmlHashScan3`, `xmlHashScanFull3` and
//! `xmlDictCleanup`, `xmlDictOwns`, `xmlDictQLookup`, `xmlDictReference`.
//!
//! The public `xmlHashTable` / `xmlDict` types are opaque (`*mut c_void` at
//! the ABI boundary); they are cast to the internal
//! `crate::xml::hash::HashTable` and `crate::xml::dictionary::Dict`.
//!
//! # UPSTREAM-PARITY
//!
//! All semantics follow upstream libxml2 2.15
//! (`oracle/historical/src/libxml2-2.15.0/hash.c` and `dict.c`).
//!
//! # Upstream contract
//!
//! Parity target is upstream `hash.c` and `dict.c` (libxml2 2.15.3,
//! SRC-LIBXML2-2.15.0-HASH-C / SRC-LIBXML2-2.15.0-DICT-C) with the signatures
//! of `hash.h`/`dict.h`. The public `xmlHashTable`/`xmlDict` types are opaque;
//! this module casts them to the internal `crate::xml::hash::HashTable` and
//! `crate::xml::dictionary::Dict`.
//!
//! # Conceptual behavior
//!
//! This module implements the hash-table and dictionary export surface:
//! add/update/lookup/scan/copy with the Q-variants (qualified-name lookups),
//! deallocators, and the dict lifecycle (`xmlDictCreate*`, `xmlDictReference`,
//! `xmlDictOwns`, `xmlDictQLookup`, `xmlDictCleanup`). The internal engines
//! hold the semantics; the exports translate the opaque-handle ABI.
//!
//! # Ownership & safety invariants
//!
//! Hash tables and dicts are caller-owned (freed with `xmlHashFree`/
//! `xmlDictFree`); values stored in a hash are owned per the deallocator the
//! caller installed at creation. Dict lookups return borrowed strings stable
//! for the dicts lifetime — freeing the dict invalidates them (OWNERSHIP_ATLAS
//! section 3). `xmlDictReference` adds a caller-owned reference counted
//! against `xmlDictFree`.
//!
//! # Historical quirks & epochs
//!
//! The dict subsystem was hardened during the security epoch: SEC-0008 records
//! CVE-2015-7497 (commit `6360a31a`, 2015-11-20, heap buffer overflow in
//! `xmlDictComputeFastQKey`), and the hash/dict ABI has been stable since the
//! 2.6 era. The HIST-SURFACE-EPOCH court family covers this surface.
//!
//! # Deliberate oddities
//!
//! `xmlDictCleanup` is a deliberate no-op (R-000138 family: upstreams own
//! body is empty after lazy init); `xmlDictReference` on NULL returns NULL
//! exactly like upstream. QLookup with NULL prefix is the unqualified lookup
//! path, matching upstreams prefix==NULL fast path.
//!
//! # Proving courts
//!
//! The HIST-SURFACE-EPOCH court family plus the DSO-LOADER and
//! HEADER-COMPILE courts cover this module; the dict/hash unit tests
//! in `src/xml/dictionary` and `src/xml/hash` run under cargo test.
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is to return the internal Rust object as the
//! handle — the opaque `*mut c_void` casts exist because upstream hides these
//! structs; exposing them would break the ABI for consumers that pass handles
//! between the candidate and code compiled against the headers. Dropping the
//! QLookup fast paths would change qualified-name resolution order that
//! downstream XPath consumers depend on.

#![allow(
    missing_docs,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals
)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

// SAFETY-SCOPE: EXPORT-HASH-MECHANICAL-001
// (11.1-Z.3 proof scope, classified-generated) — this module is the
// mechanical extern-"C" export surface: every `unsafe` block in it is
// the documented indirection/registry-access pattern whose validity
// rests on the upstream C contract, and the exported signatures are
// machine-measured by the ABI-FUNCTION-SIGNATURE and DSO-LOADER
// courts and the C-API differential probes. The safety contract of
// each export is stated in its own doc comment; this scope covers the
// mechanical wrappers' unsafe blocks.

use core::ffi::c_void;
use core::ptr;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::os::raw::c_int;

use crate::abi::allocator;
use crate::abi::callbacks::{
    xmlHashCopier, xmlHashDeallocator, xmlHashScanner, xmlHashScannerFull,
};
use crate::abi::types::xmlChar;
use crate::xml::dictionary::{dict_lookup, Dict};
use crate::xml::hash::{
    hash_add_entry3, hash_create, hash_free, hash_lookup3, hash_scan_full, HashTable,
};

// ═══════════════════════════════════════════════════════════════════════════════
// Shared Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Byte-wise comparison of two null-terminated xmlChar strings (null-safe).
const unsafe fn c_str_eq(a: *const xmlChar, b: *const xmlChar) -> bool {
    if a.is_null() && b.is_null() {
        return true;
    }
    if a.is_null() || b.is_null() {
        return false;
    }
    let mut i = 0usize;
    loop {
        // SAFETY: Both pointers are valid null-terminated strings.
        let ca = unsafe { *a.add(i) };
        let cb = unsafe { *b.add(i) };
        if ca != cb {
            return false;
        }
        if ca == 0 {
            return true;
        }
        i += 1;
    }
}

/// Append the bytes of a null-terminated xmlChar string (excluding the
/// terminator) to a byte vector.
unsafe fn push_c_str(v: &mut Vec<u8>, s: *const xmlChar) {
    if s.is_null() {
        return;
    }
    let mut i = 0usize;
    loop {
        // SAFETY: `s` is a valid null-terminated string.
        let c = unsafe { *s.add(i) };
        if c == 0 {
            break;
        }
        v.push(c);
        i += 1;
    }
}

/// Snapshot of a single hash-table entry collected during a scan.
///
/// The internal hash table's buckets are private, so scans that need to
/// filter entries (or invoke user callbacks afterwards) go through the
/// module's full-scanner API and buffer the entries here.
#[derive(Clone, Copy)]
struct HashEntrySnapshot {
    payload: *mut c_void,
    key1: *const xmlChar,
    key2: *const xmlChar,
    key3: *const xmlChar,
}

/// Full-scanner callback that appends entries to a `Vec<HashEntrySnapshot>`
/// whose pointer is passed via the scanner `data` argument.
unsafe extern "C" fn collect_entry(
    payload: *mut c_void,
    data: *mut c_void,
    name: *const xmlChar,
    name2: *const xmlChar,
    name3: *const xmlChar,
) {
    let collected = unsafe { &mut *(data as *mut Vec<HashEntrySnapshot>) };
    collected.push(HashEntrySnapshot {
        payload,
        key1: name,
        key2: name2,
        key3: name3,
    });
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlHashAdd / xmlHashAdd2 / xmlHashAdd3
// ═══════════════════════════════════════════════════════════════════════════════

/// Shared core for the modern (2.13+) `xmlHashAdd*` family.
///
/// Mirrors upstream `xmlHashUpdateInternal(..., update = 0)`:
/// - `-1` if `hash`/`name` is NULL (or on allocation failure),
/// - `0` if an entry with the key already exists (payload untouched),
/// - `1` on success.
unsafe fn hash_add_impl(
    hash: *mut c_void,
    name: *const xmlChar,
    name2: *const xmlChar,
    name3: *const xmlChar,
    userdata: *mut c_void,
) -> c_int {
    if hash.is_null() || name.is_null() {
        return -1;
    }
    let table = hash as *mut HashTable;
    // The internal add_entry3 returns -1 exactly when the key is already
    // present (NULL hash/name were rejected above) and 0 on success.
    if unsafe { hash_add_entry3(table, name, name2, name3, userdata) } == 0 {
        1
    } else {
        0
    }
}

/// Add a hash table entry.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlHashAdd(xmlHashTable *hash, const xmlChar *name, void *userdata);
/// ```
///
/// Returns 1 on success, 0 if an entry exists and -1 in case of error.
#[no_mangle]
pub unsafe extern "C" fn xmlHashAdd(
    hash: *mut c_void,
    name: *const xmlChar,
    userdata: *mut c_void,
) -> c_int {
    unsafe { hash_add_impl(hash, name, ptr::null(), ptr::null(), userdata) }
}

/// Add a hash table entry with two strings as key.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlHashAdd2(xmlHashTable *hash, const xmlChar *name,
///                 const xmlChar *name2, void *userdata);
/// ```
///
/// Returns 1 on success, 0 if an entry exists and -1 in case of error.
#[no_mangle]
pub unsafe extern "C" fn xmlHashAdd2(
    hash: *mut c_void,
    name: *const xmlChar,
    name2: *const xmlChar,
    userdata: *mut c_void,
) -> c_int {
    unsafe { hash_add_impl(hash, name, name2, ptr::null(), userdata) }
}

/// Add a hash table entry with three strings as key.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlHashAdd3(xmlHashTable *hash, const xmlChar *name,
///                 const xmlChar *name2, const xmlChar *name3,
///                 void *userdata);
/// ```
///
/// Returns 1 on success, 0 if an entry exists and -1 in case of error.
#[no_mangle]
pub unsafe extern "C" fn xmlHashAdd3(
    hash: *mut c_void,
    name: *const xmlChar,
    name2: *const xmlChar,
    name3: *const xmlChar,
    userdata: *mut c_void,
) -> c_int {
    unsafe { hash_add_impl(hash, name, name2, name3, userdata) }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlHashCopySafe
// ═══════════════════════════════════════════════════════════════════════════════

/// Copy a hash table using `copy` to copy payloads; on error the partial
/// table is freed with `dealloc` and NULL is returned.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlHashTable *xmlHashCopySafe(xmlHashTable *hash, xmlHashCopier copy,
///                               xmlHashDeallocator dealloc);
/// ```
///
/// Returns the new table or NULL if a memory allocation failed.
#[no_mangle]
pub unsafe extern "C" fn xmlHashCopySafe(
    hash: *mut c_void,
    copy: Option<xmlHashCopier>,
    dealloc: Option<xmlHashDeallocator>,
) -> *mut c_void {
    if hash.is_null() || copy.is_none() {
        return ptr::null_mut();
    }
    let copy_fn = copy.unwrap();

    // Snapshot every entry first so the copier runs without holding a borrow
    // on the table (the internal buckets are not publicly reachable).
    let mut collected: Vec<HashEntrySnapshot> = Vec::new();
    unsafe {
        hash_scan_full(
            hash as *mut HashTable,
            Some(collect_entry),
            &mut collected as *mut Vec<HashEntrySnapshot> as *mut c_void,
        );
    }

    let new_table = { hash_create(0) };
    if new_table.is_null() {
        return ptr::null_mut();
    }

    for e in &collected {
        // SAFETY: `copy_fn` is a valid C callback supplied by the caller.
        let copied = unsafe { copy_fn(e.payload, e.key1) };
        if copied.is_null()
            || unsafe { hash_add_entry3(new_table, e.key1, e.key2, e.key3, copied) } != 0
        {
            // Upstream: deallocate the failed copy, then free the partial
            // table (which deallocates every successfully copied payload).
            if let Some(f) = dealloc {
                // SAFETY: `f` is a valid C deallocator supplied by the caller.
                unsafe { f(copied, e.key1 as *mut xmlChar) };
            }
            unsafe { hash_free(new_table, dealloc) };
            return ptr::null_mut();
        }
    }

    new_table as *mut c_void
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlHashDefaultDeallocator
// ═══════════════════════════════════════════════════════════════════════════════

/// Free a hash table entry with `xmlFree`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlHashDefaultDeallocator(void *entry, const xmlChar *name);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlHashDefaultDeallocator(entry: *mut c_void, _name: *const xmlChar) {
    if !entry.is_null() {
        unsafe { allocator::xmlFreeImpl(entry) };
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlHashQLookup / xmlHashQLookup2 / xmlHashQLookup3
// ═══════════════════════════════════════════════════════════════════════════════

/// Find the payload specified by the QNames tuple.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xmlHashQLookup(xmlHashTable *hash, const xmlChar *prefix,
///                      const xmlChar *name);
/// ```
///
/// The entry key is matched against the concatenation `prefix:name` (the
/// colon is only included when `prefix` is non-NULL); a NULL `prefix` makes
/// this a plain single-key lookup.
///
/// Returns a pointer to the payload or NULL if no entry was found.
#[no_mangle]
pub unsafe extern "C" fn xmlHashQLookup(
    hash: *mut c_void,
    prefix: *const xmlChar,
    name: *const xmlChar,
) -> *mut c_void {
    unsafe {
        qlookup3(
            hash,
            prefix,
            name,
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
        )
    }
}

/// Find the payload specified by two QNames.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xmlHashQLookup2(xmlHashTable *hash, const xmlChar *prefix,
///                       const xmlChar *name, const xmlChar *prefix2,
///                       const xmlChar *name2);
/// ```
///
/// Returns a pointer to the payload or NULL if no entry was found.
#[no_mangle]
pub unsafe extern "C" fn xmlHashQLookup2(
    hash: *mut c_void,
    prefix: *const xmlChar,
    name: *const xmlChar,
    prefix2: *const xmlChar,
    name2: *const xmlChar,
) -> *mut c_void {
    unsafe { qlookup3(hash, prefix, name, prefix2, name2, ptr::null(), ptr::null()) }
}

/// Find the payload specified by three QNames.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xmlHashQLookup3(xmlHashTable *hash, const xmlChar *prefix,
///                       const xmlChar *name, const xmlChar *prefix2,
///                       const xmlChar *name2, const xmlChar *prefix3,
///                       const xmlChar *name3);
/// ```
///
/// Returns a pointer to the payload or NULL if no entry was found.
#[no_mangle]
pub unsafe extern "C" fn xmlHashQLookup3(
    hash: *mut c_void,
    prefix: *const xmlChar,
    name: *const xmlChar,
    prefix2: *const xmlChar,
    name2: *const xmlChar,
    prefix3: *const xmlChar,
    name3: *const xmlChar,
) -> *mut c_void {
    unsafe { qlookup3(hash, prefix, name, prefix2, name2, prefix3, name3) }
}

/// Core QName lookup: each entry key is compared against the `prefix:name`
/// concatenation (prefix omitted when NULL), matching upstream
/// `xmlStrQEqual`/`xmlHashQNameValue` semantics.
unsafe fn qlookup3(
    hash: *mut c_void,
    prefix: *const xmlChar,
    name: *const xmlChar,
    prefix2: *const xmlChar,
    name2: *const xmlChar,
    prefix3: *const xmlChar,
    name3: *const xmlChar,
) -> *mut c_void {
    if hash.is_null() || name.is_null() {
        return ptr::null_mut();
    }
    // Upstream xmlStrQEqual: a NULL local name with a non-NULL prefix can
    // never match any stored key, so the whole lookup fails.
    if (!prefix2.is_null() && name2.is_null()) || (!prefix3.is_null() && name3.is_null()) {
        return ptr::null_mut();
    }

    let table = hash as *mut HashTable;

    // Build the three qualified keys. The buffers are only needed for the
    // duration of the lookup (the internal lookup copies nothing).
    let mut buf1: Vec<u8> = Vec::new();
    if !prefix.is_null() {
        unsafe { push_c_str(&mut buf1, prefix) };
        buf1.push(b':');
    }
    unsafe { push_c_str(&mut buf1, name) };
    buf1.push(0);

    let mut buf2: Vec<u8> = Vec::new();
    let key2: *const xmlChar = if prefix2.is_null() {
        name2
    } else {
        unsafe { push_c_str(&mut buf2, prefix2) };
        buf2.push(b':');
        unsafe { push_c_str(&mut buf2, name2) };
        buf2.push(0);
        buf2.as_ptr() as *const xmlChar
    };

    let mut buf3: Vec<u8> = Vec::new();
    let key3: *const xmlChar = if prefix3.is_null() {
        name3
    } else {
        unsafe { push_c_str(&mut buf3, prefix3) };
        buf3.push(b':');
        unsafe { push_c_str(&mut buf3, name3) };
        buf3.push(0);
        buf3.as_ptr() as *const xmlChar
    };

    unsafe { hash_lookup3(table, buf1.as_ptr() as *const xmlChar, key2, key3) }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlHashScan3 / xmlHashScanFull3
// ═══════════════════════════════════════════════════════════════════════════════

/// Collect the entries matching a (`name`, `name2`, `name3`) triple; a NULL
/// key acts as a wildcard. Entries with a NULL payload are skipped, matching
/// upstream `xmlHashScanFull3`.
///
/// The table is fully scanned into a snapshot first, so user callbacks never
/// run while the internal buckets are being iterated.
unsafe fn scan3_collect(
    hash: *mut c_void,
    name: *const xmlChar,
    name2: *const xmlChar,
    name3: *const xmlChar,
) -> Vec<HashEntrySnapshot> {
    let mut collected: Vec<HashEntrySnapshot> = Vec::new();
    if hash.is_null() {
        return collected;
    }
    unsafe {
        hash_scan_full(
            hash as *mut HashTable,
            Some(collect_entry),
            &mut collected as *mut Vec<HashEntrySnapshot> as *mut c_void,
        );
    }
    collected.retain(|e| {
        if e.payload.is_null() {
            return false;
        }
        if !name.is_null() && !unsafe { c_str_eq(name, e.key1) } {
            return false;
        }
        if !name2.is_null() && !unsafe { c_str_eq(name2, e.key2) } {
            return false;
        }
        if !name3.is_null() && !unsafe { c_str_eq(name3, e.key3) } {
            return false;
        }
        true
    });
    collected
}

/// Scan the hash `table` and apply `scan` to each value matching the
/// (`name`, `name2`, `name3`) triple. A NULL key matches any value.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlHashScan3(xmlHashTable *hash, const xmlChar *name,
///                   const xmlChar *name2, const xmlChar *name3,
///                   xmlHashScanner scan, void *data);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlHashScan3(
    hash: *mut c_void,
    name: *const xmlChar,
    name2: *const xmlChar,
    name3: *const xmlChar,
    scan: Option<xmlHashScanner>,
    data: *mut c_void,
) {
    if scan.is_none() {
        return;
    }
    let scan = scan.unwrap();
    let matches = unsafe { scan3_collect(hash, name, name2, name3) };
    for e in &matches {
        // Upstream xmlHashScan3 passes a stub that forwards only key1.
        unsafe { scan(e.payload, data, e.key1) };
    }
}

/// Scan the hash `table` and apply `scan` to each value matching the
/// (`name`, `name2`, `name3`) triple. A NULL key matches any value.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlHashScanFull3(xmlHashTable *hash, const xmlChar *name,
///                       const xmlChar *name2, const xmlChar *name3,
///                       xmlHashScannerFull scan, void *data);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlHashScanFull3(
    hash: *mut c_void,
    name: *const xmlChar,
    name2: *const xmlChar,
    name3: *const xmlChar,
    scan: Option<xmlHashScannerFull>,
    data: *mut c_void,
) {
    if scan.is_none() {
        return;
    }
    let scan = scan.unwrap();
    let matches = unsafe { scan3_collect(hash, name, name2, name3) };
    for e in &matches {
        unsafe { scan(e.payload, data, e.key1, e.key2, e.key3) };
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Dictionary Exports (xmlDict*)
// ═══════════════════════════════════════════════════════════════════════════════

/// Reference counts tracked for `xmlDictReference`.
///
/// Upstream keeps `ref_counter` inside the `xmlDict` struct; the internal
/// `Dict` has no self-refcount field (its refcounts are per-entry), so the
/// Reference counts for dictionaries (upstream xmlDictReference). The
/// reference count is tracked here in a side table keyed by the opaque dict
/// pointer.
pub static DICT_REFS: Lazy<Mutex<HashMap<usize, u32>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// Interned string pointers owned by each dictionary, keyed by dict pointer.
///
/// Used by `xmlDictOwns`; upstream checks whether `str` falls inside the
/// dict's string pool, which the internal `Dict` does not track, so pointers
/// returned by `xmlDictQLookup` are recorded here.
static DICT_OWNED: Lazy<Mutex<HashMap<usize, HashSet<usize>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Record an interned string pointer as owned by `dict` (for `xmlDictOwns`).
pub(crate) fn register_owned(dict: *mut c_void, s: *const xmlChar) {
    if dict.is_null() || s.is_null() {
        return;
    }
    DICT_OWNED
        .lock()
        .entry(dict as usize)
        .or_default()
        .insert(s as usize);
}

/// Increment the reference counter of a dictionary.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlDictReference(xmlDict *dict);
/// ```
///
/// Returns 0 in case of success and -1 in case of error.
#[no_mangle]
pub unsafe extern "C" fn xmlDictReference(dict: *mut c_void) -> c_int {
    if dict.is_null() {
        return -1;
    }
    *DICT_REFS.lock().entry(dict as usize).or_insert(0) += 1;
    if std::env::var_os("LX_DICT_TRACE").is_some() {
        eprintln!(
            "[dict] ref   {:p} now={}",
            dict,
            DICT_REFS.lock().get(&(dict as usize)).copied().unwrap_or(0)
        );
    }
    0
}

/// Look up a QName (`prefix:name`) in the dictionary, adding it if not found.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const xmlChar *xmlDictQLookup(xmlDict *dict, const xmlChar *prefix,
///                               const xmlChar *name);
/// ```
///
/// If `prefix` is NULL this is a plain lookup of `name`. Returns the interned
/// copy of the string or NULL in case of error.
#[no_mangle]
pub unsafe extern "C" fn xmlDictQLookup(
    dict: *mut c_void,
    prefix: *const xmlChar,
    name: *const xmlChar,
) -> *const xmlChar {
    if dict.is_null() || name.is_null() {
        return ptr::null();
    }
    if prefix.is_null() {
        let ret = unsafe { dict_lookup(dict as *mut Dict, name, -1) };
        register_owned(dict, ret);
        return ret;
    }

    // Build "prefix:name", intern it, then drop the temporary buffer.
    let mut qname: Vec<u8> = Vec::new();
    unsafe { push_c_str(&mut qname, prefix) };
    qname.push(b':');
    unsafe { push_c_str(&mut qname, name) };
    qname.push(0);

    let ret = unsafe { dict_lookup(dict as *mut Dict, qname.as_ptr() as *const xmlChar, -1) };
    register_owned(dict, ret);
    ret
}

/// Check if a string is owned by the dictionary.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlDictOwns(xmlDict *dict, const xmlChar *str);
/// ```
///
/// Returns 1 if `str` points into the dictionary's memory, 0 if not, and
/// -1 in case of error (NULL `dict` or `str`).
#[no_mangle]
pub unsafe extern "C" fn xmlDictOwns(dict: *mut c_void, str: *const xmlChar) -> c_int {
    if dict.is_null() || str.is_null() {
        return -1;
    }
    let owned = DICT_OWNED
        .lock()
        .get(&(dict as usize))
        .is_some_and(|set| set.contains(&(str as usize)));
    if owned {
        1
    } else {
        0
    }
}

/// Internal dict-ownership predicate (no FFI/c_int): true when `s` is an
/// interned string owned by `dict`. Used by the tree teardown paths
/// (UPSTREAM-PARITY `DICT_FREE` in tree.c).
pub(crate) fn dict_owns_str(dict: *mut c_void, s: *const xmlChar) -> bool {
    if dict.is_null() || s.is_null() {
        return false;
    }
    DICT_OWNED
        .lock()
        .get(&(dict as usize))
        .is_some_and(|set| set.contains(&(s as usize)))
}

/// Free the dictionary data.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlDictCleanup(void);
/// ```
///
/// Upstream deprecated this function in 2.13 when the global dictionary was
/// removed; it is a no-op (takes no arguments) in the oracle 2.15 headers.
#[no_mangle]
pub const unsafe extern "C" fn xmlDictCleanup() {
    // No-op: there is no global dictionary to clean up (upstream 2.13+).
}
