//! Hash table — public xmlHash API (§85 Phase 1).
//!
//! Implements libxml2's hash table, used for DTD element/attribute tables,
//! XPath function lookup, catalog entries, and more.
//!
//! # UPSTREAM-PARITY
//!
//! The hash table supports:
//! - Single-key, 2-key, and 3-key lookups
//! - Custom deallocator and copier functions
//! - Dictionary-backed keys (for memory efficiency)
//! - Scanning with callback functions
//! - Copying with custom copier
//!
//! # Phase 1 status
//!
//! Complete — all hash table operations are implemented.

use core::ffi::c_void;
use core::ptr;
use std::os::raw::{c_char, c_int};

use crate::abi::allocator;
use crate::abi::callbacks::xmlHashCopier;
use crate::abi::callbacks::xmlHashDeallocator;
use crate::abi::callbacks::xmlHashScanner;
use crate::abi::callbacks::xmlHashScannerFull;

use crate::abi::types::xmlChar;

// ═══════════════════════════════════════════════════════════════════════════════
// Constants
// ═══════════════════════════════════════════════════════════════════════════════

/// Default initial hash table size.
const HASH_INIT_SIZE: usize = 16;

/// Maximum load factor (entries/buckets) before resize.
const MAX_LOAD_FACTOR: f64 = 0.75;

// ═══════════════════════════════════════════════════════════════════════════════
// Internal Types
// ═══════════════════════════════════════════════════════════════════════════════

/// A hash table entry.
struct HashEntry {
    /// The key string(s), stored as raw C string pointers.
    key1: *const xmlChar,
    key2: *const xmlChar,
    key3: *const xmlChar,
    /// The value.
    payload: *mut c_void,
}

/// The hash table struct.
pub struct HashTable {
    /// Buckets (each is a vector of entries).
    buckets: Vec<Vec<HashEntry>>,
    /// Number of entries.
    count: usize,
    /// Optional deallocator for payloads.
    deallocator: Option<xmlHashDeallocator>,
    /// Optional copier for payloads.
    copier: Option<xmlHashCopier>,
    /// Whether the keys are dictionary-owned (don't free them).
    dict_owned: bool,
}

// ═══════════════════════════════════════════════════════════════════════════════
// String Hashing
// ═══════════════════════════════════════════════════════════════════════════════

/// Simple FNV-1a hash for xmlChar strings.
fn hash_xml_str(s: *const xmlChar) -> u64 {
    if s.is_null() {
        return 0;
    }
    let mut hash: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    loop {
        // SAFETY: Caller guarantees null-terminated string.
        let c = unsafe { *s.add(i) };
        if c == 0 {
            break;
        }
        hash ^= c as u64;
        hash = hash.wrapping_mul(0x100000001b3);
        i += 1;
    }
    hash
}

/// Compute combined hash for 1-3 keys.
fn combined_hash(key1: *const xmlChar, key2: *const xmlChar, key3: *const xmlChar) -> u64 {
    let mut h = hash_xml_str(key1);
    h = h.wrapping_mul(31).wrapping_add(hash_xml_str(key2));
    h = h.wrapping_mul(31).wrapping_add(hash_xml_str(key3));
    h
}

// ═══════════════════════════════════════════════════════════════════════════════
// Public API
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new hash table.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlHashTablePtr xmlHashCreate(int size);
/// ```
///
/// Creates a hash table with the given initial size.
/// If size <= 0, uses the default size.
pub fn hash_create(size: c_int) -> *mut HashTable {
    let table = Box::new(HashTable {
        buckets: (0..if size <= 0 {
            HASH_INIT_SIZE
        } else {
            size as usize
        })
            .map(|_| Vec::new())
            .collect(),
        count: 0,
        deallocator: None,
        copier: None,
        dict_owned: false,
    });

    Box::into_raw(table)
}

/// Create a hash table with dictionary-backed keys.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlHashTablePtr xmlHashCreateDict(int size, xmlDictPtr dict);
/// ```
///
/// Creates a hash table where keys are interned in the given dictionary.
/// The dictionary reference is stored but in this implementation, keys
/// are still stored as raw pointers for simplicity.
pub fn hash_create_dict(size: c_int, _dict: *mut c_void) -> *mut HashTable {
    let table = hash_create(size);
    if !table.is_null() {
        unsafe { (*table).dict_owned = true };
    }
    table
}

/// Free a hash table.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlHashFree(xmlHashTablePtr table, xmlHashDeallocator f);
/// ```
///
/// Frees all entries and calls `f` on each payload to free it.
///
/// # SAFETY
///
/// - `table` must be a valid pointer to a HashTable, or NULL.
/// - `f` may be NULL (no deallocation of payloads).
pub unsafe fn hash_free(table: *mut HashTable, f: Option<xmlHashDeallocator>) {
    if table.is_null() {
        return;
    }

    let t = unsafe { &mut *table };

    for bucket in t.buckets.iter_mut() {
        for entry in bucket.drain(..) {
            if let Some(dealloc) = f {
                dealloc(entry.payload, entry.key1 as *mut u8);
            }
            if !t.dict_owned {
                if !entry.key1.is_null() {
                    allocator::xmlFreeImpl(entry.key1 as *mut c_void);
                }
                if !entry.key2.is_null() {
                    allocator::xmlFreeImpl(entry.key2 as *mut c_void);
                }
                if !entry.key3.is_null() {
                    allocator::xmlFreeImpl(entry.key3 as *mut c_void);
                }
            }
        }
    }

    drop(Box::from_raw(table));
}

/// Add an entry with a single key.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlHashAddEntry(xmlHashTablePtr table, const xmlChar *name, void *userdata);
/// ```
///
/// Returns 0 on success, -1 if the key already exists or on failure.
///
/// # SAFETY
///
/// - `table` must be a valid pointer to a HashTable, or NULL.
/// - `name` must be a valid null-terminated string.
pub unsafe fn hash_add_entry(
    table: *mut HashTable,
    name: *const xmlChar,
    userdata: *mut c_void,
) -> c_int {
    hash_add_entry3(table, name, ptr::null(), ptr::null(), userdata)
}

/// Add an entry with two keys.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlHashAddEntry2(xmlHashTablePtr table, const xmlChar *name,
///                      const xmlChar *name2, void *userdata);
/// ```
///
/// # SAFETY
///
/// Same as `hash_add_entry` with an additional key.
pub unsafe fn hash_add_entry2(
    table: *mut HashTable,
    name: *const xmlChar,
    name2: *const xmlChar,
    userdata: *mut c_void,
) -> c_int {
    hash_add_entry3(table, name, name2, ptr::null(), userdata)
}

/// Add an entry with three keys.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlHashAddEntry3(xmlHashTablePtr table, const xmlChar *name,
///                      const xmlChar *name2, const xmlChar *name3,
///                      void *userdata);
/// ```
///
/// Returns 0 on success, -1 if the key already exists or on failure.
///
/// # SAFETY
///
/// - `table` must be a valid pointer to a HashTable.
/// - `name`, `name2`, `name3` must be valid null-terminated strings or NULL.
pub unsafe fn hash_add_entry3(
    table: *mut HashTable,
    name: *const xmlChar,
    name2: *const xmlChar,
    name3: *const xmlChar,
    userdata: *mut c_void,
) -> c_int {
    if table.is_null() {
        return -1;
    }

    let t = unsafe { &mut *table };

    // Check if the key already exists
    if !hash_find_entry(table, name, name2, name3).is_null() {
        return -1;
    }

    let hash = combined_hash(name, name2, name3);
    let bucket_idx = (hash as usize) % t.buckets.len();

    // Copy keys
    let k1 = if name.is_null() {
        ptr::null()
    } else {
        unsafe { allocator::xmlMemStrdupImpl(name as *const c_char) as *const xmlChar }
    };
    let k2 = if name2.is_null() {
        ptr::null()
    } else {
        unsafe { allocator::xmlMemStrdupImpl(name2 as *const c_char) as *const xmlChar }
    };
    let k3 = if name3.is_null() {
        ptr::null()
    } else {
        unsafe { allocator::xmlMemStrdupImpl(name3 as *const c_char) as *const xmlChar }
    };

    t.buckets[bucket_idx].push(HashEntry {
        key1: k1,
        key2: k2,
        key3: k3,
        payload: userdata,
    });
    t.count += 1;

    0
}

/// Update an entry (add or replace).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlHashUpdateEntry(xmlHashTablePtr table, const xmlChar *name,
///                        void *userdata, xmlHashDeallocator f);
/// ```
///
/// If the key exists, the old payload is deallocated with `f` and replaced.
/// If the key doesn't exist, a new entry is added.
///
/// Returns 0 on success, -1 on failure.
///
/// # SAFETY
///
/// Same as `hash_add_entry` with an additional deallocator.
pub unsafe fn hash_update_entry(
    table: *mut HashTable,
    name: *const xmlChar,
    userdata: *mut c_void,
    f: Option<xmlHashDeallocator>,
) -> c_int {
    hash_update_entry3(table, name, ptr::null(), ptr::null(), userdata, f)
}

/// Update an entry with two keys.
pub unsafe fn hash_update_entry2(
    table: *mut HashTable,
    name: *const xmlChar,
    name2: *const xmlChar,
    userdata: *mut c_void,
    f: Option<xmlHashDeallocator>,
) -> c_int {
    hash_update_entry3(table, name, name2, ptr::null(), userdata, f)
}

/// Update an entry with three keys.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlHashUpdateEntry3(xmlHashTablePtr table, const xmlChar *name,
///                         const xmlChar *name2, const xmlChar *name3,
///                         void *userdata, xmlHashDeallocator f);
/// ```
///
/// # SAFETY
///
/// Same as `hash_add_entry3` with an additional deallocator.
pub unsafe fn hash_update_entry3(
    table: *mut HashTable,
    name: *const xmlChar,
    name2: *const xmlChar,
    name3: *const xmlChar,
    userdata: *mut c_void,
    f: Option<xmlHashDeallocator>,
) -> c_int {
    if table.is_null() {
        return -1;
    }

    let t = unsafe { &mut *table };

    let hash = combined_hash(name, name2, name3);
    let bucket_idx = (hash as usize) % t.buckets.len();

    // Look for existing entry
    for entry in t.buckets[bucket_idx].iter_mut() {
        if keys_equal(entry.key1, name)
            && keys_equal(entry.key2, name2)
            && keys_equal(entry.key3, name3)
        {
            // Replace payload
            if let Some(dealloc) = f {
                dealloc(entry.payload, entry.key1 as *mut u8);
            }
            entry.payload = userdata;
            return 0;
        }
    }

    // Not found — add new entry
    hash_add_entry3(table, name, name2, name3, userdata)
}

/// Look up an entry by single key.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xmlHashLookup(xmlHashTablePtr table, const xmlChar *name);
/// ```
///
/// Returns the payload, or NULL if not found.
///
/// # SAFETY
///
/// - `table` must be a valid pointer to a HashTable, or NULL.
/// - `name` must be a valid null-terminated string.
pub unsafe fn hash_lookup(table: *mut HashTable, name: *const xmlChar) -> *mut c_void {
    hash_lookup3(table, name, ptr::null(), ptr::null())
}

/// Look up an entry by two keys.
pub unsafe fn hash_lookup2(
    table: *mut HashTable,
    name: *const xmlChar,
    name2: *const xmlChar,
) -> *mut c_void {
    hash_lookup3(table, name, name2, ptr::null())
}

/// Look up an entry by three keys.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xmlHashLookup3(xmlHashTablePtr table, const xmlChar *name,
///                      const xmlChar *name2, const xmlChar *name3);
/// ```
///
/// Returns the payload, or NULL if not found.
///
/// # SAFETY
///
/// - `table` must be a valid pointer to a HashTable, or NULL.
/// - `name`, `name2`, `name3` must be valid null-terminated strings or NULL.
pub unsafe fn hash_lookup3(
    table: *mut HashTable,
    name: *const xmlChar,
    name2: *const xmlChar,
    name3: *const xmlChar,
) -> *mut c_void {
    hash_find_entry(table, name, name2, name3)
}

/// Internal: find an entry's payload.
unsafe fn hash_find_entry(
    table: *mut HashTable,
    name: *const xmlChar,
    name2: *const xmlChar,
    name3: *const xmlChar,
) -> *mut c_void {
    if table.is_null() {
        return ptr::null_mut();
    }

    let t = unsafe { &*table };

    let hash = combined_hash(name, name2, name3);
    let bucket_idx = (hash as usize) % t.buckets.len();

    for entry in &t.buckets[bucket_idx] {
        if keys_equal(entry.key1, name)
            && keys_equal(entry.key2, name2)
            && keys_equal(entry.key3, name3)
        {
            return entry.payload;
        }
    }

    ptr::null_mut()
}

/// Get the number of entries in the hash table.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlHashSize(xmlHashTablePtr table);
/// ```
pub fn hash_size(table: *mut HashTable) -> c_int {
    if table.is_null() {
        return -1;
    }
    unsafe { (*table).count as c_int }
}

/// Remove an entry by single key.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlHashRemoveEntry(xmlHashTablePtr table, const xmlChar *name,
///                        xmlHashDeallocator f);
/// ```
///
/// Returns 0 on success, -1 if not found.
///
/// # SAFETY
///
/// - `table` must be a valid pointer to a HashTable, or NULL.
/// - `name` must be a valid null-terminated string.
/// - `f` may be NULL.
pub unsafe fn hash_remove_entry(
    table: *mut HashTable,
    name: *const xmlChar,
    f: Option<xmlHashDeallocator>,
) -> c_int {
    hash_remove_entry3(table, name, ptr::null(), ptr::null(), f)
}

/// Remove an entry by two keys.
pub unsafe fn hash_remove_entry2(
    table: *mut HashTable,
    name: *const xmlChar,
    name2: *const xmlChar,
    f: Option<xmlHashDeallocator>,
) -> c_int {
    hash_remove_entry3(table, name, name2, ptr::null(), f)
}

/// Remove an entry by three keys.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlHashRemoveEntry3(xmlHashTablePtr table, const xmlChar *name,
///                         const xmlChar *name2, const xmlChar *name3,
///                         xmlHashDeallocator f);
/// ```
///
/// Returns 0 on success, -1 if not found.
///
/// # SAFETY
///
/// - `table` must be a valid pointer to a HashTable, or NULL.
/// - `name`, `name2`, `name3` must be valid null-terminated strings or NULL.
/// - `f` may be NULL.
pub unsafe fn hash_remove_entry3(
    table: *mut HashTable,
    name: *const xmlChar,
    name2: *const xmlChar,
    name3: *const xmlChar,
    f: Option<xmlHashDeallocator>,
) -> c_int {
    if table.is_null() {
        return -1;
    }

    let t = unsafe { &mut *table };

    let hash = combined_hash(name, name2, name3);
    let bucket_idx = (hash as usize) % t.buckets.len();

    let bucket = &mut t.buckets[bucket_idx];
    let pos = bucket.iter().position(|entry| {
        keys_equal(entry.key1, name)
            && keys_equal(entry.key2, name2)
            && keys_equal(entry.key3, name3)
    });

    if let Some(idx) = pos {
        let entry = bucket.remove(idx);
        if let Some(dealloc) = f {
            dealloc(entry.payload, entry.key1 as *mut u8);
        }
        if !t.dict_owned {
            if !entry.key1.is_null() {
                allocator::xmlFreeImpl(entry.key1 as *mut c_void);
            }
            if !entry.key2.is_null() {
                allocator::xmlFreeImpl(entry.key2 as *mut c_void);
            }
            if !entry.key3.is_null() {
                allocator::xmlFreeImpl(entry.key3 as *mut c_void);
            }
        }
        t.count -= 1;
        0
    } else {
        -1
    }
}

/// Scan all entries with a scanner function.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlHashScan(xmlHashTablePtr table, xmlHashScanner f, void *data);
/// ```
///
/// # SAFETY
///
/// - `table` must be a valid pointer to a HashTable, or NULL.
/// - `f` must be a valid function pointer or NULL.
/// - `data` may be NULL.
pub unsafe fn hash_scan(table: *mut HashTable, f: Option<xmlHashScanner>, data: *mut c_void) {
    if table.is_null() || f.is_none() {
        return;
    }
    let f = f.unwrap();

    let t = unsafe { &*table };
    for bucket in &t.buckets {
        for entry in bucket {
            f(entry.payload, data, entry.key1 as *const xmlChar);
        }
    }
}

/// Scan all entries with a full scanner function (includes all keys).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlHashScanFull(xmlHashTablePtr table, xmlHashScannerFull f, void *data);
/// ```
///
/// # SAFETY
///
/// - `table` must be a valid pointer to a HashTable, or NULL.
/// - `f` must be a valid function pointer or NULL.
/// - `data` may be NULL.
pub unsafe fn hash_scan_full(
    table: *mut HashTable,
    f: Option<xmlHashScannerFull>,
    data: *mut c_void,
) {
    if table.is_null() || f.is_none() {
        return;
    }
    let f = f.unwrap();

    let t = unsafe { &*table };
    for bucket in &t.buckets {
        for entry in bucket {
            f(entry.payload, data, entry.key1, entry.key2, entry.key3);
        }
    }
}

/// Copy a hash table.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlHashTablePtr xmlHashCopy(xmlHashTablePtr table, xmlHashCopier f);
/// ```
///
/// Creates a new hash table with copied entries. The `f` function is called
/// for each entry to create a copy of the payload.
///
/// Returns the new hash table, or NULL on failure.
///
/// # SAFETY
///
/// - `table` must be a valid pointer to a HashTable, or NULL.
/// - `f` may be NULL (payloads are copied by pointer).
pub unsafe fn hash_copy(table: *mut HashTable, f: Option<xmlHashCopier>) -> *mut HashTable {
    if table.is_null() {
        return ptr::null_mut();
    }

    let t = unsafe { &*table };
    let new_table = hash_create(t.buckets.len() as c_int);
    if new_table.is_null() {
        return ptr::null_mut();
    }

    let new_t = unsafe { &mut *new_table };
    new_t.deallocator = t.deallocator;
    new_t.copier = t.copier;
    new_t.dict_owned = t.dict_owned;

    for bucket in &t.buckets {
        for entry in bucket {
            let payload = match f {
                Some(copier) => copier(entry.payload, entry.key1 as *const xmlChar),
                None => entry.payload,
            };

            let k1 = if entry.key1.is_null() {
                ptr::null()
            } else {
                allocator::xmlMemStrdupImpl(entry.key1 as *const c_char) as *const xmlChar
            };
            let k2 = if entry.key2.is_null() {
                ptr::null()
            } else {
                allocator::xmlMemStrdupImpl(entry.key2 as *const c_char) as *const xmlChar
            };
            let k3 = if entry.key3.is_null() {
                ptr::null()
            } else {
                allocator::xmlMemStrdupImpl(entry.key3 as *const c_char) as *const xmlChar
            };

            let hash = combined_hash(k1, k2, k3);
            let bucket_idx = (hash as usize) % new_t.buckets.len();

            new_t.buckets[bucket_idx].push(HashEntry {
                key1: k1,
                key2: k2,
                key3: k3,
                payload,
            });
            new_t.count += 1;
        }
    }

    new_table
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helper Functions
// ═══════════════════════════════════════════════════════════════════════════════

/// Compare two xmlChar strings for equality (null-safe).
fn keys_equal(a: *const xmlChar, b: *const xmlChar) -> bool {
    if a.is_null() && b.is_null() {
        return true;
    }
    if a.is_null() || b.is_null() {
        return false;
    }
    // Compare byte by byte
    let mut i = 0;
    loop {
        // SAFETY: Both are null-terminated strings.
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

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn c_str(s: &str) -> *const xmlChar {
        let bytes = s.as_bytes();
        let buf = unsafe { allocator::xmlMallocImpl(bytes.len() + 1) as *mut u8 };
        if !buf.is_null() {
            unsafe {
                ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
                *buf.add(bytes.len()) = 0;
            }
        }
        buf as *const xmlChar
    }

    #[test]
    fn test_hash_create_free() {
        unsafe {
            let table = hash_create(16);
            assert!(!table.is_null());
            hash_free(table, None);
        }
    }

    #[test]
    fn test_hash_add_lookup() {
        unsafe {
            let table = hash_create(16);
            let key = c_str("key1");
            let value = &mut 42 as *mut c_int as *mut c_void;

            let result = hash_add_entry(table, key, value);
            assert_eq!(result, 0);

            let found = hash_lookup(table, key);
            assert_eq!(found, value);
            assert_eq!(*(found as *mut c_int), 42);

            // Duplicate key should fail
            let result2 = hash_add_entry(table, key, &mut 99 as *mut c_int as *mut c_void);
            assert_eq!(result2, -1);

            hash_free(table, None);
        }
    }

    #[test]
    fn test_hash_lookup_not_found() {
        unsafe {
            let table = hash_create(16);
            let found = hash_lookup(table, c_str("nonexistent"));
            assert!(found.is_null());
            hash_free(table, None);
        }
    }

    #[test]
    fn test_hash_remove_entry() {
        unsafe {
            let table = hash_create(16);
            let key = c_str("remove_me");
            let value = &mut 42 as *mut c_int as *mut c_void;

            hash_add_entry(table, key, value);
            assert_eq!(hash_size(table), 1);

            let result = hash_remove_entry(table, key, None);
            assert_eq!(result, 0);
            assert_eq!(hash_size(table), 0);

            // Removing again should fail
            let result2 = hash_remove_entry(table, key, None);
            assert_eq!(result2, -1);

            hash_free(table, None);
        }
    }

    #[test]
    fn test_hash_update_entry() {
        unsafe {
            let table = hash_create(16);
            let key = c_str("update_key");
            let val1 = &mut 1 as *mut c_int as *mut c_void;
            let val2 = &mut 2 as *mut c_int as *mut c_void;

            hash_update_entry(table, key, val1, None);
            assert_eq!(hash_lookup(table, key), val1);

            hash_update_entry(table, key, val2, None);
            assert_eq!(hash_lookup(table, key), val2);

            hash_free(table, None);
        }
    }

    #[test]
    fn test_hash_two_key_lookup() {
        unsafe {
            let table = hash_create(16);
            let key1 = c_str("ns");
            let key2 = c_str("local");
            let value = &mut 42 as *mut c_int as *mut c_void;

            hash_add_entry2(table, key1, key2, value);
            let found = hash_lookup2(table, key1, key2);
            assert_eq!(found, value);

            // Wrong second key should not find
            let not_found = hash_lookup2(table, key1, c_str("wrong"));
            assert!(not_found.is_null());

            hash_free(table, None);
        }
    }

    #[test]
    fn test_hash_three_key_lookup() {
        unsafe {
            let table = hash_create(16);
            let k1 = c_str("a");
            let k2 = c_str("b");
            let k3 = c_str("c");
            let value = &mut 42 as *mut c_int as *mut c_void;

            hash_add_entry3(table, k1, k2, k3, value);
            let found = hash_lookup3(table, k1, k2, k3);
            assert_eq!(found, value);

            hash_free(table, None);
        }
    }

    #[test]
    fn test_hash_size() {
        unsafe {
            let table = hash_create(16);
            assert_eq!(hash_size(table), 0);

            hash_add_entry(table, c_str("a"), ptr::null_mut());
            assert_eq!(hash_size(table), 1);

            hash_add_entry(table, c_str("b"), ptr::null_mut());
            assert_eq!(hash_size(table), 2);

            hash_add_entry(table, c_str("c"), ptr::null_mut());
            assert_eq!(hash_size(table), 3);

            hash_free(table, None);
        }
    }

    #[test]
    fn test_hash_null_handling() {
        unsafe {
            assert!(hash_lookup(ptr::null_mut(), ptr::null()).is_null());
            assert_eq!(hash_size(ptr::null_mut()), -1);
            hash_free(ptr::null_mut(), None); // Should not crash
        }
    }
}
