//! Linked list — public xmlList API (§85 Phase 1).
//!
//! Implements the libxml2 linked list.
//!
//! # UPSTREAM-PARITY
//!
//! The linked list supports:
//! - Create/delete with custom deallocator
//! - Push front/back, pop front/back
//! - Insert/append at arbitrary positions
//! - Search with custom comparator
//! - Walk with callback
//! - Remove first/last/all matching entries
//! - Clear
//! - Empty/front/back/size queries
//! - Sort, reverse, reverse splice, merge
//!
//! # Phase 1 status
//!
//! Complete — all list operations are implemented.
//!
//! # Upstream contract
//!
//! Mirrors upstream `list.c` / `list.h` (`SRC-LIBXML2-2.15.0-LIST-C`, parity
//! target libxml2 2.15.3 oracle): the `xmlList*` API with deallocator,
//! comparator and walker callbacks.
//!
//! # Conceptual behavior
//!
//! Implements a doubly-linked list whose nodes are owned by the list and
//! whose data pointers are owned by the caller unless a deallocator is
//! registered. `xmlListAppend` does a plain push_back when no comparator is
//! set but a SORTED insert once a comparator is present (upstream list.c);
//! `xmlListWalk` / `xmlListReverseWalk` stop when the walker returns 0.
//!
//! # Ownership & safety invariants
//!
//! The list owns its ListNode storage (freed by `xmlListDelete` / clear
//! paths); payload data is freed through the registered deallocator only.
//! Walkers receive borrowed data pointers valid for the walk duration.
//!
//! # Historical quirks & epochs
//!
//! The sorted-append and 0-stops-walk semantics were misimplemented in the
//! candidate and corrected to upstream behavior in the 11.1-L callback
//! audit (R-000162) — both are long-standing list.c contracts, stable
//! since the 2.6 validation era through the 2.15.3 oracle.
//!
//! # Deliberate oddities
//!
//! `xmlListAppend` becoming a sorted insert under a comparator is the
//! upstream oddity that a naive push_back implementation misses; the walk
//! stop convention (0 = stop, non-zero = continue) is the inverse of the
//! intuitive C convention and is reproduced deliberately.
//!
//! # Proving courts
//!
//! CALLBACK-001 (courts/suites/data-abi/callback-family-probe.c) exercises
//! `xmlListAppend` ordering and both walks against the oracle DSO and
//! requires byte-identical output (R-000162 evidence); cargo test runs the
//! list unit suites.
//!
//! # Tempting simplifications that would break parity
//!
//! Do not make `xmlListAppend` always push_back: with a comparator set,
//! consumers rely on sorted order. Do not invert the walk stop condition
//! back to non-zero-stops: the 11.1-L audit proved the oracle stops on 0.

use core::ffi::c_void;
use core::ptr;
use std::os::raw::c_int;

use crate::abi::allocator;

// ═══════════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════════

/// Deallocator function for list data.
pub type xmlListDeallocator = unsafe extern "C" fn(*mut c_void);

/// Data comparator function. Returns 0 if equal, non-zero if different.
pub type xmlListDataCompare = unsafe extern "C" fn(*const c_void, *const c_void) -> c_int;

/// Walker function for xmlListWalk.
pub type xmlListWalker = unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int;

/// A linked list node.
struct ListNode {
    data: *mut c_void,
    prev: *mut ListNode,
    next: *mut ListNode,
}

/// The linked list struct.
#[derive(Debug)]
pub struct List {
    front: *mut ListNode,
    back: *mut ListNode,
    count: usize,
    deallocator: Option<xmlListDeallocator>,
    comparator: Option<xmlListDataCompare>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Public API
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new linked list.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlListPtr xmlListCreate(xmlListDeallocator deallocator,
///                          xmlListDataCompare comparator);
/// ```
///
/// Creates a linked list with the given deallocator and comparator.
/// Both may be NULL.
pub fn list_create(
    deallocator: Option<xmlListDeallocator>,
    comparator: Option<xmlListDataCompare>,
) -> *mut List {
    let list = Box::new(List {
        front: ptr::null_mut(),
        back: ptr::null_mut(),
        count: 0,
        deallocator,
        comparator,
    });

    Box::into_raw(list)
}

/// Delete a linked list and all its nodes.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlListDelete(xmlListPtr l);
/// ```
///
/// # SAFETY
///
/// - `l` must be a valid pointer to a List, or NULL.
pub unsafe fn list_delete(l: *mut List) {
    if l.is_null() {
        return;
    }

    let list = unsafe { &mut *l };
    let mut cur = list.front;

    while !cur.is_null() {
        let next = unsafe { (*cur).next };
        // UPSTREAM-PARITY (list.c xmlListDeleteInternal): the deallocator
        // receives the LINK pointer, not the data.
        if let Some(dealloc) = list.deallocator {
            unsafe { dealloc(cur as *mut c_void) };
        }
        unsafe { allocator::xmlFreeImpl(cur as *mut c_void) };
        cur = next;
    }

    drop(Box::from_raw(l));
}

/// Search the list for data matching the given key.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xmlListSearch(xmlListPtr l, void *data);
/// ```
///
/// Uses the list's comparator function. Returns the matching data, or NULL.
///
/// # SAFETY
///
/// - `l` must be a valid pointer to a List, or NULL.
pub unsafe fn list_search(l: *mut List, data: *const c_void) -> *mut c_void {
    if l.is_null() {
        return ptr::null_mut();
    }

    let list = unsafe { &*l };
    let comparator = match list.comparator {
        Some(c) => c,
        None => return ptr::null_mut(),
    };

    let mut cur = list.front;
    while !cur.is_null() {
        let node = unsafe { &*cur };
        if unsafe { comparator(node.data as *const c_void, data) == 0 } {
            return node.data;
        }
        cur = node.next;
    }

    ptr::null_mut()
}

/// Return the last element of a list (upstream list.c `xmlListEnd`): the
/// data of the last node, or NULL.
///
/// # SAFETY
///
/// - `l` must be a valid list pointer or NULL.
pub unsafe fn list_end(l: *mut List) -> *mut c_void {
    if l.is_null() {
        return ptr::null_mut();
    }
    let list = unsafe { &*l };
    if list.back.is_null() {
        return ptr::null_mut();
    }
    unsafe { (*list.back).data }
}

/// Reverse-search a list with the comparator (upstream list.c
/// `xmlListReverseSearch`): scans from the back, returns the first (from
/// the end) matching node's data, or NULL.
///
/// # SAFETY
///
/// - `l` must be a valid list pointer or NULL.
/// - `data` must be valid for the comparator.
pub unsafe fn list_reverse_search(l: *mut List, data: *const c_void) -> *mut c_void {
    if l.is_null() {
        return ptr::null_mut();
    }
    let list = unsafe { &*l };
    let comparator = match list.comparator {
        Some(c) => c,
        None => return ptr::null_mut(),
    };
    let mut cur = list.back;
    while !cur.is_null() {
        // SAFETY: cur is a valid node; comparator is valid.
        let node_data = unsafe { (*cur).data };
        if unsafe { comparator(node_data as *const c_void, data) } == 0 {
            return node_data;
        }
        cur = unsafe { (*cur).prev };
    }
    ptr::null_mut()
}

/// Walk a list in reverse with a walker callback (upstream list.c
/// `xmlListReverseWalk`).
///
/// # SAFETY
///
/// - `l` must be a valid list pointer or NULL.
/// - `walker` may be NULL (no-op).
pub unsafe fn list_reverse_walk(l: *mut List, walker: Option<xmlListWalker>, data: *mut c_void) {
    if l.is_null() {
        return;
    }
    let walker = match walker {
        Some(w) => w,
        None => return,
    };
    let list = unsafe { &*l };
    let mut cur = list.back;
    while !cur.is_null() {
        // SAFETY: cur is a valid node; walker is valid.
        let node_data = unsafe { (*cur).data };
        // UPSTREAM-PARITY (list.c xmlListReverseWalk): returns 0 to stop.
        if unsafe { walker(node_data, data) == 0 } {
            return;
        }
        cur = unsafe { (*cur).prev };
    }
}

/// Duplicate a list (upstream list.c `xmlListDup`): a shallow copy using
/// the same deallocator/comparator; node data pointers are copied as-is.
/// Returns the new list or NULL on allocation failure.
///
/// # SAFETY
///
/// - `l` must be a valid list pointer or NULL.
pub unsafe fn list_dup(l: *mut List) -> *mut List {
    if l.is_null() {
        return ptr::null_mut();
    }
    let list = unsafe { &*l };
    let new_list = Box::new(List {
        front: ptr::null_mut(),
        back: ptr::null_mut(),
        count: 0,
        deallocator: list.deallocator,
        comparator: list.comparator,
    });
    let new_ptr = Box::into_raw(new_list);
    let mut cur = list.front;
    while !cur.is_null() {
        // SAFETY: cur is valid; the node data is shallow-copied.
        let data = unsafe { (*cur).data };
        if unsafe { list_push_back(new_ptr, data) } != 0 {
            unsafe { list_delete(new_ptr) };
            return ptr::null_mut();
        }
        cur = unsafe { (*cur).next };
    }
    new_ptr
}

/// Copy a list with a data copier (upstream list.c `xmlListCopy`): each
/// node's data is copied through `copier` (returns a fresh pointer or
/// NULL on failure). The result replaces the target list `l`'s content.
/// Returns 0 on success, -1 on error.
///
/// # SAFETY
///
/// - `l` must be a valid list pointer or NULL.
/// - `copier` must be a valid copier function.
pub unsafe fn list_copy(
    l: *mut List,
    copier: Option<unsafe extern "C" fn(*mut c_void) -> *mut c_void>,
) -> c_int {
    if l.is_null() {
        return -1;
    }
    let copier = match copier {
        Some(c) => c,
        None => return -1,
    };
    let list = unsafe { &*l };
    let new_list = Box::new(List {
        front: ptr::null_mut(),
        back: ptr::null_mut(),
        count: 0,
        deallocator: list.deallocator,
        comparator: list.comparator,
    });
    let new_ptr = Box::into_raw(new_list);
    let mut cur = list.front;
    while !cur.is_null() {
        // SAFETY: cur is valid; copier must return a fresh copy or NULL.
        let copied = unsafe { copier((*cur).data) };
        if copied.is_null() {
            unsafe { list_delete(new_ptr) };
            return -1;
        }
        if unsafe { list_push_back(new_ptr, copied) } != 0 {
            unsafe { list_delete(new_ptr) };
            return -1;
        }
        cur = unsafe { (*cur).next };
    }
    // Replace `l`'s content with the copy (upstream copies INTO `l`).
    unsafe {
        list_clear(l);
        let dst = &mut *l;
        let src = &mut *new_ptr;
        core::mem::swap(dst, src);
        list_delete(new_ptr);
    }
    0
}

/// Return the data stored in a link (upstream list.c `xmlLinkGetData`).
///
/// # SAFETY
///
/// - `link` must be a valid link pointer or NULL.
pub unsafe fn link_get_data(link: *mut c_void) -> *mut c_void {
    if link.is_null() {
        return ptr::null_mut();
    }
    unsafe { (*(link as *mut ListNode)).data }
}

/// Walk the list, calling the walker function for each element.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlListWalk(xmlListPtr l, xmlListWalker walker, void *data);
/// ```
///
/// # SAFETY
///
/// - `l` must be a valid pointer to a List, or NULL.
/// - `walker` must be a valid function pointer or NULL.
pub unsafe fn list_walk(l: *mut List, walker: Option<xmlListWalker>, data: *mut c_void) {
    if l.is_null() || walker.is_none() {
        return;
    }
    let walker = walker.unwrap();

    let list = unsafe { &*l };
    let mut cur = list.front;
    while !cur.is_null() {
        let node = unsafe { &*cur };
        // UPSTREAM-PARITY (list.c xmlListWalk): the walker returns 0 to stop.
        if unsafe { walker(node.data, data) == 0 } {
            break;
        }
        cur = node.next;
    }
}

/// Push data to the back of the list.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlListPushBack(xmlListPtr l, void *data);
/// ```
///
/// Returns 0 on success, -1 on failure.
///
/// # SAFETY
///
/// - `l` must be a valid pointer to a List, or NULL.
pub unsafe fn list_push_back(l: *mut List, data: *mut c_void) -> c_int {
    if l.is_null() {
        return -1;
    }

    let list = unsafe { &mut *l };

    let node = allocator::xmlMallocZero(size_of::<ListNode>() as usize) as *mut ListNode;
    if node.is_null() {
        return -1;
    }

    unsafe {
        (*node).data = data;
        (*node).prev = list.back;
        (*node).next = ptr::null_mut();
    }

    if list.back.is_null() {
        list.front = node;
        list.back = node;
    } else {
        unsafe { (*list.back).next = node };
        list.back = node;
    }

    list.count += 1;
    0
}

/// Push data to the front of the list.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlListPushFront(xmlListPtr l, void *data);
/// ```
///
/// Returns 0 on success, -1 on failure.
///
/// # SAFETY
///
/// - `l` must be a valid pointer to a List, or NULL.
pub unsafe fn list_push_front(l: *mut List, data: *mut c_void) -> c_int {
    if l.is_null() {
        return -1;
    }

    let list = unsafe { &mut *l };

    let node = allocator::xmlMallocZero(size_of::<ListNode>() as usize) as *mut ListNode;
    if node.is_null() {
        return -1;
    }

    unsafe {
        (*node).data = data;
        (*node).prev = ptr::null_mut();
        (*node).next = list.front;
    }

    if list.front.is_null() {
        list.front = node;
        list.back = node;
    } else {
        unsafe { (*list.front).prev = node };
        list.front = node;
    }

    list.count += 1;
    0
}

/// Pop data from the back of the list.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlListPopBack(xmlListPtr l);
/// ```
///
/// # SAFETY
///
/// - `l` must be a valid pointer to a List, or NULL.
pub unsafe fn list_pop_back(l: *mut List) {
    if l.is_null() {
        return;
    }

    let list = unsafe { &mut *l };
    if list.back.is_null() {
        return;
    }

    let node = list.back;
    let prev = unsafe { (*node).prev };

    if let Some(dealloc) = list.deallocator {
        unsafe { dealloc((*node).data) };
    }
    unsafe { allocator::xmlFreeImpl(node as *mut c_void) };

    list.back = prev;
    if prev.is_null() {
        list.front = ptr::null_mut();
    } else {
        unsafe { (*prev).next = ptr::null_mut() };
    }

    list.count = list.count.saturating_sub(1);
}

/// Pop data from the front of the list.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlListPopFront(xmlListPtr l);
/// ```
///
/// # SAFETY
///
/// - `l` must be a valid pointer to a List, or NULL.
pub unsafe fn list_pop_front(l: *mut List) {
    if l.is_null() {
        return;
    }

    let list = unsafe { &mut *l };
    if list.front.is_null() {
        return;
    }

    let node = list.front;
    let next = unsafe { (*node).next };

    if let Some(dealloc) = list.deallocator {
        unsafe { dealloc((*node).data) };
    }
    unsafe { allocator::xmlFreeImpl(node as *mut c_void) };

    list.front = next;
    if next.is_null() {
        list.back = ptr::null_mut();
    } else {
        unsafe { (*next).prev = ptr::null_mut() };
    }

    list.count = list.count.saturating_sub(1);
}

/// Insert data into the sorted position.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlListInsert(xmlListPtr l, void *data);
/// ```
///
/// Inserts data in sorted order using the list's comparator.
/// Returns 0 on success, -1 on failure.
///
/// # SAFETY
///
/// - `l` must be a valid pointer to a List, or NULL.
pub unsafe fn list_insert(l: *mut List, data: *mut c_void) -> c_int {
    if l.is_null() {
        return -1;
    }

    let list = unsafe { &mut *l };

    // If no comparator, just push back
    let comparator = match list.comparator {
        Some(c) => c,
        None => return list_push_back(l, data),
    };

    // Find insertion point
    let mut cur = list.front;
    while !cur.is_null() {
        let node = unsafe { &*cur };
        if unsafe { comparator(data as *const c_void, node.data as *const c_void) <= 0 } {
            // Insert before cur
            let new_node =
                allocator::xmlMallocZero(size_of::<ListNode>() as usize) as *mut ListNode;
            if new_node.is_null() {
                return -1;
            }
            unsafe {
                (*new_node).data = data;
                (*new_node).prev = node.prev;
                (*new_node).next = cur;
                if !node.prev.is_null() {
                    (*node.prev).next = new_node;
                } else {
                    list.front = new_node;
                }
                (*cur).prev = new_node;
            }
            list.count += 1;
            return 0;
        }
        cur = node.next;
    }

    // Append at end
    list_push_back(l, data)
}

/// Append data to the end of the list (alias for push_back).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlListAppend(xmlListPtr l, void *data);
/// ```
///
/// # SAFETY
///
/// - `l`, `data` must be valid pointers (or NULL
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
pub unsafe fn list_append(l: *mut List, data: *mut c_void) -> c_int {
    // UPSTREAM-PARITY (list.c xmlListAppend): with a comparator the new
    // element is inserted in sorted order (before the first node whose data
    // compares greater); without one it is pushed to the back.
    if l.is_null() {
        return -1;
    }
    let comparator = unsafe { (*l).comparator };
    match comparator {
        None => unsafe { list_push_back(l, data) },
        Some(cmp) => {
            unsafe {
                let mut cur = (*l).front;
                while !cur.is_null() {
                    if cmp((*cur).data as *const c_void, data as *const c_void) > 0 {
                        break;
                    }
                    cur = (*cur).next;
                }
                // Insert before `cur`.
                let node =
                    allocator::xmlMallocZero(size_of::<ListNode>() as usize) as *mut ListNode;
                if node.is_null() {
                    return -1;
                }
                (*node).data = data;
                (*node).next = cur;
                if cur.is_null() {
                    (*node).prev = (*l).back;
                    if !(*l).back.is_null() {
                        (*(*l).back).next = node;
                    }
                    (*l).back = node;
                    if (*l).front.is_null() {
                        (*l).front = node;
                    }
                } else {
                    (*node).prev = (*cur).prev;
                    if !(*cur).prev.is_null() {
                        (*(*cur).prev).next = node;
                    } else {
                        (*l).front = node;
                    }
                    (*cur).prev = node;
                }
            }
            0
        }
    }
}

/// Remove the first matching element.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlListRemoveFirst(xmlListPtr l, void *data);
/// ```
///
/// Returns 0 on success, -1 if not found.
///
/// # SAFETY
///
/// - `l` must be a valid pointer to a List, or NULL.
pub unsafe fn list_remove_first(l: *mut List, data: *const c_void) -> c_int {
    if l.is_null() {
        return -1;
    }

    let list = unsafe { &mut *l };
    let comparator = match list.comparator {
        Some(c) => c,
        None => return -1,
    };

    let mut cur = list.front;
    while !cur.is_null() {
        let node = unsafe { &*cur };
        let next = node.next;
        if unsafe { comparator(node.data as *const c_void, data) == 0 } {
            // Remove this node
            if !node.prev.is_null() {
                unsafe { (*node.prev).next = node.next };
            } else {
                list.front = node.next;
            }
            if !node.next.is_null() {
                unsafe { (*node.next).prev = node.prev };
            } else {
                list.back = node.prev;
            }

            if let Some(dealloc) = list.deallocator {
                unsafe { dealloc(node.data) };
            }
            unsafe { allocator::xmlFreeImpl(cur as *mut c_void) };
            list.count = list.count.saturating_sub(1);
            return 0;
        }
        cur = next;
    }

    -1
}

/// Remove the last matching element.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlListRemoveLast(xmlListPtr l, void *data);
/// ```
///
/// Returns 0 on success, -1 if not found.
///
/// # SAFETY
///
/// - `l` must be a valid pointer to a List, or NULL.
pub unsafe fn list_remove_last(l: *mut List, data: *const c_void) -> c_int {
    if l.is_null() {
        return -1;
    }

    let list = unsafe { &mut *l };
    let comparator = match list.comparator {
        Some(c) => c,
        None => return -1,
    };

    let mut cur = list.back;
    while !cur.is_null() {
        let node = unsafe { &*cur };
        let prev = node.prev;
        if unsafe { comparator(node.data as *const c_void, data) == 0 } {
            // Remove this node
            if !node.prev.is_null() {
                unsafe { (*node.prev).next = node.next };
            } else {
                list.front = node.next;
            }
            if !node.next.is_null() {
                unsafe { (*node.next).prev = node.prev };
            } else {
                list.back = node.prev;
            }

            if let Some(dealloc) = list.deallocator {
                unsafe { dealloc(node.data) };
            }
            unsafe { allocator::xmlFreeImpl(cur as *mut c_void) };
            list.count = list.count.saturating_sub(1);
            return 0;
        }
        cur = prev;
    }

    -1
}

/// Remove all matching elements.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlListRemoveAll(xmlListPtr l, void *data);
/// ```
///
/// Returns the number of elements removed.
///
/// # SAFETY
///
/// - `l` must be a valid pointer to a List, or NULL.
pub unsafe fn list_remove_all(l: *mut List, data: *const c_void) -> c_int {
    if l.is_null() {
        return 0;
    }

    let list = unsafe { &mut *l };
    let comparator = match list.comparator {
        Some(c) => c,
        None => return 0,
    };

    let mut removed = 0;
    let mut cur = list.front;

    while !cur.is_null() {
        let node = unsafe { &*cur };
        let next = node.next;

        if unsafe { comparator(node.data as *const c_void, data) == 0 } {
            // Remove this node
            if !node.prev.is_null() {
                unsafe { (*node.prev).next = node.next };
            } else {
                list.front = node.next;
            }
            if !node.next.is_null() {
                unsafe { (*node.next).prev = node.prev };
            } else {
                list.back = node.prev;
            }

            if let Some(dealloc) = list.deallocator {
                unsafe { dealloc(node.data) };
            }
            unsafe { allocator::xmlFreeImpl(cur as *mut c_void) };
            list.count = list.count.saturating_sub(1);
            removed += 1;
        }

        cur = next;
    }

    removed
}

/// Clear the list (remove all elements).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlListClear(xmlListPtr l);
/// ```
///
/// # SAFETY
///
/// - `l` must be a valid pointer to a List, or NULL.
pub unsafe fn list_clear(l: *mut List) {
    if l.is_null() {
        return;
    }

    let list = unsafe { &mut *l };
    let mut cur = list.front;

    while !cur.is_null() {
        let next = unsafe { (*cur).next };
        if let Some(dealloc) = list.deallocator {
            unsafe { dealloc((*cur).data) };
        }
        unsafe { allocator::xmlFreeImpl(cur as *mut c_void) };
        cur = next;
    }

    list.front = ptr::null_mut();
    list.back = ptr::null_mut();
    list.count = 0;
}

/// Check if the list is empty.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlListEmpty(xmlListPtr l);
/// ```
///
/// Returns 1 if empty, 0 if not empty.
pub fn list_empty(l: *mut List) -> c_int {
    if l.is_null() {
        return 1;
    }
    let list = unsafe { &*l };
    if list.front.is_null() {
        1
    } else {
        0
    }
}

/// Get the data at the front of the list.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xmlListFront(xmlListPtr l);
/// ```
///
/// Returns the data at the front, or NULL if the list is empty.
pub fn list_front(l: *mut List) -> *mut c_void {
    if l.is_null() {
        return ptr::null_mut();
    }
    let list = unsafe { &*l };
    if list.front.is_null() {
        ptr::null_mut()
    } else {
        unsafe { (*list.front).data }
    }
}

/// Get the data at the back of the list.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xmlListBack(xmlListPtr l);
/// ```
///
/// Returns the data at the back, or NULL if the list is empty.
pub fn list_back(l: *mut List) -> *mut c_void {
    if l.is_null() {
        return ptr::null_mut();
    }
    let list = unsafe { &*l };
    if list.back.is_null() {
        ptr::null_mut()
    } else {
        unsafe { (*list.back).data }
    }
}

/// Get the number of elements in the list.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlListSize(xmlListPtr l);
/// ```
///
/// Returns the number of elements, or -1 if the list is NULL.
pub fn list_size(l: *mut List) -> c_int {
    if l.is_null() {
        return -1;
    }
    let list = unsafe { &*l };
    list.count as c_int
}

/// Sort the list in-place using the comparator.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlListSort(xmlListPtr l);
/// ```
///
/// # SAFETY
///
/// - `l` must be a valid pointer to a List, or NULL.
pub unsafe fn list_sort(l: *mut List) {
    if l.is_null() {
        return;
    }

    let list = unsafe { &mut *l };
    if list.count <= 1 {
        return;
    }

    let comparator = match list.comparator {
        Some(c) => c,
        None => return,
    };

    // Convert to Vec, sort, rebuild
    let mut nodes: Vec<*mut ListNode> = Vec::with_capacity(list.count);
    let mut cur = list.front;
    while !cur.is_null() {
        nodes.push(cur);
        cur = unsafe { (*cur).next };
    }

    // Bubble sort (simple, matches upstream's simple approach)
    for i in 0..nodes.len() {
        for j in 0..nodes.len() - 1 - i {
            let a = unsafe { &*nodes[j] };
            let b = unsafe { &*nodes[j + 1] };
            if unsafe { comparator(a.data as *const c_void, b.data as *const c_void) > 0 } {
                nodes.swap(j, j + 1);
            }
        }
    }

    // Rebuild links
    list.front = nodes[0];
    list.back = nodes[nodes.len() - 1];

    for i in 0..nodes.len() {
        unsafe {
            (*nodes[i]).prev = if i > 0 { nodes[i - 1] } else { ptr::null_mut() };
            (*nodes[i]).next = if i + 1 < nodes.len() {
                nodes[i + 1]
            } else {
                ptr::null_mut()
            };
        }
    }
}

/// Reverse the list in-place.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlListReverse(xmlListPtr l);
/// ```
///
/// # SAFETY
///
/// - `l` must be a valid pointer to a List, or NULL.
pub unsafe fn list_reverse(l: *mut List) {
    if l.is_null() {
        return;
    }

    let list = unsafe { &mut *l };

    // `cur` keeps the old front (the new back): the walk below traverses the
    // old-next chain to the old back, swapping each node's links.
    let mut cur = list.front;
    std::mem::swap(&mut list.front, &mut list.back);

    while !cur.is_null() {
        let next = unsafe { (*cur).next };
        unsafe {
            (*cur).next = (*cur).prev;
            (*cur).prev = next;
        }
        cur = next;
    }
}

/// Reverse splice: move all elements from `l2` to the front of `l1` in reverse order.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlListReverseSplice(xmlListPtr l1, xmlListPtr l2);
/// ```
///
/// # SAFETY
///
/// - `l1`, `l2` must be valid pointers to Lists, or NULL.
pub unsafe fn list_reverse_splice(l1: *mut List, l2: *mut List) {
    if l1.is_null() || l2.is_null() {
        return;
    }

    let list1 = unsafe { &mut *l1 };
    let list2 = unsafe { &mut *l2 };

    if list2.front.is_null() {
        return;
    }

    // Reverse l2 first
    list_reverse(l2);

    // Move all nodes from l2 to front of l1
    unsafe {
        (*list2.back).next = list1.front;
        if !list1.front.is_null() {
            (*list1.front).prev = list2.back;
        } else {
            list1.back = list2.back;
        }
        list1.front = list2.front;
    }

    list1.count += list2.count;

    // Clear l2
    list2.front = ptr::null_mut();
    list2.back = ptr::null_mut();
    list2.count = 0;
}

/// Merge two sorted lists into one.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlListMerge(xmlListPtr l1, xmlListPtr l2);
/// ```
///
/// Merges `l2` into `l1` in sorted order. `l2` becomes empty.
///
/// # SAFETY
///
/// - `l1`, `l2` must be valid pointers to Lists, or NULL.
pub unsafe fn list_merge(l1: *mut List, l2: *mut List) {
    if l1.is_null() || l2.is_null() {
        return;
    }

    let list1 = unsafe { &mut *l1 };
    let list2 = unsafe { &mut *l2 };

    if list2.front.is_null() {
        return;
    }

    let comparator = match list1.comparator {
        Some(c) => c,
        None => {
            // No comparator — just append all of l2 to l1
            if !list1.back.is_null() {
                unsafe { (*list1.back).next = list2.front };
                unsafe { (*list2.front).prev = list1.back };
            } else {
                list1.front = list2.front;
            }
            list1.back = list2.back;
            list1.count += list2.count;
            list2.front = ptr::null_mut();
            list2.back = ptr::null_mut();
            list2.count = 0;
            return;
        }
    };

    // Merge sorted lists
    let mut cur2 = list2.front;
    let mut insert_before = list1.front;

    while !cur2.is_null() {
        let next2 = unsafe { (*cur2).next };

        // Find insertion point
        while !insert_before.is_null() {
            if unsafe {
                comparator(
                    (*cur2).data as *const c_void,
                    (*insert_before).data as *const c_void,
                ) <= 0
            } {
                break;
            }
            insert_before = unsafe { (*insert_before).next };
        }

        // Insert cur2 before insert_before
        if insert_before.is_null() {
            // Append at end
            if list1.back.is_null() {
                list1.front = cur2;
                list1.back = cur2;
                unsafe {
                    (*cur2).prev = ptr::null_mut();
                    (*cur2).next = ptr::null_mut();
                }
            } else {
                unsafe {
                    (*cur2).prev = list1.back;
                    (*cur2).next = ptr::null_mut();
                    (*list1.back).next = cur2;
                }
                list1.back = cur2;
            }
        } else {
            unsafe {
                (*cur2).prev = (*insert_before).prev;
                (*cur2).next = insert_before;
                if !(*insert_before).prev.is_null() {
                    (*(*insert_before).prev).next = cur2;
                } else {
                    list1.front = cur2;
                }
                (*insert_before).prev = cur2;
            }
        }

        list1.count += 1;
        cur2 = next2;
    }

    // Clear l2
    list2.front = ptr::null_mut();
    list2.back = ptr::null_mut();
    list2.count = 0;
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" fn int_compare(a: *const c_void, b: *const c_void) -> c_int {
        let ai = *(a as *const i32);
        let bi = *(b as *const i32);
        ai.cmp(&bi) as c_int
    }

    #[test]
    fn test_list_create_delete() {
        unsafe {
            let list = list_create(None, None);
            assert!(!list.is_null());
            list_delete(list);
        }
    }

    #[test]
    fn test_list_push_pop() {
        unsafe {
            let list = list_create(None, None);
            let v1 = &mut 1 as *mut c_int as *mut c_void;
            let v2 = &mut 2 as *mut c_int as *mut c_void;

            list_push_back(list, v1);
            list_push_back(list, v2);
            assert_eq!(list_size(list), 2);

            assert_eq!(*(list_front(list) as *const i32), 1);
            assert_eq!(*(list_back(list) as *const i32), 2);

            list_pop_back(list);
            assert_eq!(list_size(list), 1);
            assert_eq!(*(list_back(list) as *const i32), 1);

            list_pop_front(list);
            assert_eq!(list_size(list), 0);
            assert_eq!(list_empty(list), 1);

            list_delete(list);
        }
    }

    #[test]
    fn test_list_push_front() {
        unsafe {
            let list = list_create(None, None);
            let v1 = &mut 1 as *mut c_int as *mut c_void;
            let v2 = &mut 2 as *mut c_int as *mut c_void;

            list_push_front(list, v1);
            list_push_front(list, v2);
            assert_eq!(*(list_front(list) as *const i32), 2);
            assert_eq!(*(list_back(list) as *const i32), 1);

            list_delete(list);
        }
    }

    #[test]
    fn test_list_insert_sorted() {
        unsafe {
            let list = list_create(None, Some(int_compare));
            let v2 = &mut 2 as *mut c_int as *mut c_void;
            let v1 = &mut 1 as *mut c_int as *mut c_void;
            let v3 = &mut 3 as *mut c_int as *mut c_void;

            list_insert(list, v2);
            list_insert(list, v1);
            list_insert(list, v3);

            // Should be 1, 2, 3
            assert_eq!(*(list_front(list) as *const i32), 1);
            assert_eq!(*(list_back(list) as *const i32), 3);
            assert_eq!(list_size(list), 3);

            list_delete(list);
        }
    }

    #[test]
    fn test_list_remove_first() {
        unsafe {
            let list = list_create(None, Some(int_compare));
            let v1 = &mut 1 as *mut c_int as *mut c_void;
            let v2 = &mut 2 as *mut c_int as *mut c_void;

            list_push_back(list, v1);
            list_push_back(list, v2);

            let one: i32 = 1;
            let result = list_remove_first(list, &one as *const i32 as *const c_void);
            assert_eq!(result, 0);
            assert_eq!(list_size(list), 1);
            assert_eq!(*(list_front(list) as *const i32), 2);

            list_delete(list);
        }
    }

    #[test]
    fn test_list_clear() {
        unsafe {
            let list = list_create(None, None);
            list_push_back(list, &mut 1 as *mut c_int as *mut c_void);
            list_push_back(list, &mut 2 as *mut c_int as *mut c_void);
            assert_eq!(list_size(list), 2);

            list_clear(list);
            assert_eq!(list_empty(list), 1);
            assert_eq!(list_size(list), 0);

            list_delete(list);
        }
    }

    #[test]
    fn test_list_reverse() {
        unsafe {
            let list = list_create(None, None);
            let v1 = &mut 1 as *mut c_int as *mut c_void;
            let v2 = &mut 2 as *mut c_int as *mut c_void;
            let v3 = &mut 3 as *mut c_int as *mut c_void;

            list_push_back(list, v1);
            list_push_back(list, v2);
            list_push_back(list, v3);

            list_reverse(list);

            assert_eq!(*(list_front(list) as *const i32), 3);
            assert_eq!(*(list_back(list) as *const i32), 1);

            list_delete(list);
        }
    }

    #[test]
    fn test_list_null_handling() {
        unsafe {
            assert_eq!(list_empty(ptr::null_mut()), 1);
            assert_eq!(list_size(ptr::null_mut()), -1);
            assert!(list_front(ptr::null_mut()).is_null());
            assert!(list_back(ptr::null_mut()).is_null());
            list_delete(ptr::null_mut()); // Should not crash
            list_clear(ptr::null_mut()); // Should not crash
            list_pop_front(ptr::null_mut()); // Should not crash
            list_pop_back(ptr::null_mut()); // Should not crash
        }
    }
}
