//! DTD declarations handling (§24, §85 Phase 6).
//!
//! Internal subsets, external subsets, element declarations, attribute
//! declarations, default attributes, notations, content models.
//!
//! This module provides the foundational types and functions for DTD
//! validation, used by the validation, RELAX NG, XML Schema, and
//! Schematron modules.
//!
//! # Upstream contract
//!
//! Mirrors the DTD declaration machinery of upstream valid.c and parser.c
//! (SRC-LIBXML2-2.15.0, oracle tree `oracle/historical/src/libxml2-2.15.0/`):
//! xmlCreateIntSubset, xmlNewDtd, xmlAddElementDecl, xmlAddAttributeDecl,
//! xmlAddEntity, xmlAddNotationDecl, xmlGetDtdElementDesc and the DTD hash
//! tables. Parity target: the system libxml2 2.15.3 oracle.
//!
//! # Conceptual behavior
//!
//! Internal subsets, external subsets, element declarations, attribute
//! declarations, default attributes, notations and content models. The DTD
//! node joins doc->children before the first element (xmlCreateIntSubset
//! semantics, R-000164); declaration hash tables are created lazily by the
//! xmlAdd* functions.
//!
//! # Ownership & safety invariants
//!
//! Ownership: the DTD is owned by the document; declarations are owned by the
//! DTD hash tables; xmlFreeDtd frees only non-declaration children and runs
//! the child walk before the hash-table frees (double-free fix, R-000164).
//! SAFETY: `_xmlElement` must keep the upstream 104-byte layout (R-000139) or
//! every xmlMalloc(sizeof(_xmlElement)) under-allocates.
//!
//! # Historical quirks & epochs
//!
//! R-000139 (Phase 11.1-I) rewrote the Rust `_xmlElement` mirror from 56 to
//! 104 bytes after the RUST-MIRROR-ABI court caught the drift; R-000164
//! (11.1-N) fixed the internalSubset path (xmlNewDtd to xmlCreateIntSubset),
//! the element-decl type_ carrying XML_ELEMENT_DECL, ATTLISTs for undeclared
//! elements creating UNDEFINED placeholders, and the attribute hash keyed
//! (name,prefix,elem) as upstream.
//!
//! # Deliberate oddities
//!
//! Deliberate oddities: the DTD internal subset is written as part of
//! doc->children (not a side structure); element declarations store the
//! element type in etype while type_ holds XML_ELEMENT_DECL — matching
//! upstream field semantics exactly.
//!
//! # Proving courts
//!
//! DTD, PARSER, RELAXNG and XSD court families; TREE-001 (DTD node chain,
//! decl trees, hash order), RUST-MIRROR-ABI (struct layout), header-compile
//! 595/595 and `cargo test --lib`.
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is freeing declaration nodes from the DTD child
//! list in addition to the hash tables — the exact double free R-000164
//! fixed. Do not eagerly create the hash tables: laziness matches
//! xmlGetDtdElementDesc semantics and TREE-001 fingerprints. Do not key the
//! attribute table (elem,name): upstream is (name,prefix,elem).

use core::ffi::c_void;
use core::ptr;
use std::os::raw::c_int;

use crate::abi::allocator;
use crate::abi::structs::*;
use crate::abi::types::xmlElementContentOccur::*;
use crate::abi::types::xmlElementContentType::*;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::xmlElementTypeVal::*;
use crate::abi::types::*;
use crate::xml::hash;
use crate::xml::string;

#[cfg(test)]
use crate::abi::types::xmlAttributeDefault::*;
#[cfg(test)]
use crate::abi::types::xmlAttributeType::*;

// ═══════════════════════════════════════════════════════════════════════════════
// DTD Access
// ═══════════════════════════════════════════════════════════════════════════════

/// Get the internal subset of a document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDtdPtr xmlGetIntSubset(const xmlDoc *doc);
/// ```
///
/// Returns a pointer to the DTD, or NULL if none.
///
/// # Safety
///
/// - `doc` must be a valid pointer to an `_xmlDoc`, or NULL.
pub const fn get_int_subset(doc: *const _xmlDoc) -> *mut _xmlDtd {
    if doc.is_null() {
        return ptr::null_mut();
    }
    unsafe { (*doc).intSubset }
}

/// Create an internal subset (DTD) for a document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDtdPtr xmlCreateIntSubset(xmlDocPtr doc, const xmlChar *name,
///                              const xmlChar *ExternalID, const xmlChar *SystemID);
/// ```
///
/// Creates a new DTD and attaches it as the document's internal subset.
/// If the document already has an internal subset, it is replaced.
///
/// # SAFETY
///
/// - `doc` may be NULL — upstream still allocates and returns an
///   unattached DTD in that case (HOSTILE-ABI A37); a NULL `doc` skips the
///   child-chain attachment.
/// - `name`, `ExternalID`, `SystemID` must be valid null-terminated strings or NULL.
pub unsafe fn create_int_subset(
    doc: *mut _xmlDoc,
    name: *const xmlChar,
    ExternalID: *const xmlChar,
    SystemID: *const xmlChar,
) -> *mut _xmlDtd {
    if !doc.is_null() {
        // UPSTREAM-PARITY (tree.c xmlCreateIntSubset): if the document already
        // has an internal subset, return it — never create a second DTD node.
        if !(*doc).intSubset.is_null() {
            return (*doc).intSubset;
        }
    }

    // SAFETY: Allocate zero-initialized memory for the DTD.
    let dtd = allocator::xmlMallocZero(size_of::<_xmlDtd>() as usize) as *mut _xmlDtd;
    if dtd.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*dtd).type_ = XML_DTD_NODE as c_int;
        (*dtd).name = string::xml_strdup(name);
        (*dtd).ExternalID = string::xml_strdup(ExternalID);
        (*dtd).SystemID = string::xml_strdup(SystemID);
        (*dtd).parent = doc;
        (*dtd).doc = doc;

        // UPSTREAM-PARITY (tree.c xmlCreateIntSubset): the declaration hash
        // tables are created lazily by the xmlAdd* functions on first use;
        // an empty DTD exposes NULL table pointers.

        if !doc.is_null() {
            // Attach to document
            (*doc).intSubset = dtd;

            // UPSTREAM-PARITY (tree.c xmlCreateIntSubset): the DTD node is
            // inserted into the document's child chain immediately before the
            // first element node (comments/PIs in the prolog stay ahead of it).
            let doc_children = (*doc).children;
            if doc_children.is_null() {
                (*doc).children = dtd as *mut _xmlNode;
                (*doc).last = dtd as *mut _xmlNode;
            } else {
                let mut next = doc_children;
                while !next.is_null() && (*next).type_ != XML_ELEMENT_NODE as c_int {
                    next = (*next).next;
                }
                if next.is_null() {
                    (*dtd).prev = (*doc).last;
                    (*(*doc).last).next = dtd as *mut _xmlNode;
                    (*doc).last = dtd as *mut _xmlNode;
                } else {
                    (*dtd).next = next;
                    (*dtd).prev = (*next).prev;
                    if (*dtd).prev.is_null() {
                        (*doc).children = dtd as *mut _xmlNode;
                    } else {
                        (*(*dtd).prev).next = dtd as *mut _xmlNode;
                    }
                    (*next).prev = dtd as *mut _xmlNode;
                }
            }
        }
    }

    dtd
}

/// Create a new DTD.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDtdPtr xmlNewDtd(xmlDocPtr doc, const xmlChar *name,
///                     const xmlChar *ExternalID, const xmlChar *SystemID);
/// ```
///
/// Creates a new DTD and if `doc` is non-NULL and has no internal subset,
/// attaches it as the document's internal subset.
///
/// # SAFETY
///
/// - `doc` may be NULL.
/// - `name`, `ExternalID`, `SystemID` must be valid null-terminated strings or NULL.
pub unsafe fn new_dtd(
    doc: *mut _xmlDoc,
    name: *const xmlChar,
    ExternalID: *const xmlChar,
    SystemID: *const xmlChar,
) -> *mut _xmlDtd {
    // SAFETY: Allocate zero-initialized memory for the DTD.
    let dtd = allocator::xmlMallocZero(size_of::<_xmlDtd>() as usize) as *mut _xmlDtd;
    if dtd.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*dtd).type_ = XML_DTD_NODE as c_int;
        (*dtd).name = string::xml_strdup(name);
        (*dtd).ExternalID = string::xml_strdup(ExternalID);
        (*dtd).SystemID = string::xml_strdup(SystemID);
        (*dtd).parent = doc;
        (*dtd).doc = doc;

        // UPSTREAM-PARITY (tree.c xmlNewDtd): the declaration hash tables are
        // created lazily by the xmlAdd* functions on first use.

        // Attach to document if it has no internal subset yet
        if !doc.is_null() && (*doc).intSubset.is_null() {
            (*doc).intSubset = dtd;
        }
    }

    dtd
}

/// extern "C" copier shim for `hash::hash_copy`: element declarations.
///
/// # Safety
///
/// - `payload` must be NULL or a valid pointer to an `_xmlElement` owned by
///   the hash table being copied; it is forwarded to `copy_element`.
/// - `_name` is unused.
unsafe extern "C" fn copy_elem_cb(payload: *mut c_void, _name: *const xmlChar) -> *mut c_void {
    unsafe { copy_element(payload as *mut _xmlElement) as *mut c_void }
}

/// extern "C" copier shim for `hash::hash_copy`: attribute declarations.
///
/// # Safety
///
/// - `payload` must be NULL or a valid pointer to an `_xmlAttribute` owned by
///   the hash table being copied; it is forwarded to `copy_attribute_decl`.
/// - `_name` is unused.
unsafe extern "C" fn copy_attr_cb(payload: *mut c_void, _name: *const xmlChar) -> *mut c_void {
    unsafe { copy_attribute_decl(payload as *mut _xmlAttribute) as *mut c_void }
}

/// extern "C" copier shim for `hash::hash_copy`: entity declarations.
///
/// # Safety
///
/// - `payload` must be NULL or a valid pointer to an `_xmlEntity` owned by
///   the hash table being copied; it is forwarded to `copy_entity`.
/// - `_name` is unused.
unsafe extern "C" fn copy_ent_cb(payload: *mut c_void, _name: *const xmlChar) -> *mut c_void {
    unsafe { crate::xml::entities::copy_entity(payload as *mut _xmlEntity) as *mut c_void }
}

/// extern "C" copier shim for `hash::hash_copy`: notation declarations.
///
/// # Safety
///
/// - `payload` must be NULL or a valid pointer to an `_xmlNotation` owned by
///   the hash table being copied; it is forwarded to `copy_notation`.
/// - `_name` is unused.
unsafe extern "C" fn copy_notation_cb(payload: *mut c_void, _name: *const xmlChar) -> *mut c_void {
    unsafe { copy_notation(payload as *mut _xmlNotation) as *mut c_void }
}

/// Deep-copy a DTD: name, external identifiers, and all declaration tables.
///
/// UPSTREAM-PARITY: the DTD portion of `xmlCopyDoc` / `xmlCopyDtd`.
///
/// Returns the new DTD, or NULL on failure.
///
/// # SAFETY
///
/// - `dtd` must be a valid pointer to an _xmlDtd, or NULL.
pub unsafe fn copy_dtd(dtd: *const _xmlDtd) -> *mut _xmlDtd {
    if dtd.is_null() {
        return ptr::null_mut();
    }
    let d = unsafe { &*dtd };

    let copy = unsafe { allocator::xmlMallocZero(size_of::<_xmlDtd>() as usize) as *mut _xmlDtd };
    if copy.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*copy).type_ = d.type_;
        (*copy).name = string::xml_strdup(d.name);
        (*copy).ExternalID = string::xml_strdup(d.ExternalID);
        (*copy).SystemID = string::xml_strdup(d.SystemID);
        (*copy).notations =
            hash::hash_copy(d.notations as *mut hash::HashTable, Some(copy_notation_cb))
                as *mut c_void;
        (*copy).elements =
            hash::hash_copy(d.elements as *mut hash::HashTable, Some(copy_elem_cb)) as *mut c_void;
        (*copy).attributes =
            hash::hash_copy(d.attributes as *mut hash::HashTable, Some(copy_attr_cb))
                as *mut c_void;
        (*copy).entities =
            hash::hash_copy(d.entities as *mut hash::HashTable, Some(copy_ent_cb)) as *mut c_void;
        (*copy).pentities =
            hash::hash_copy(d.pentities as *mut hash::HashTable, Some(copy_ent_cb)) as *mut c_void;
    }

    copy
}

/// Free a DTD and all its declarations.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeDtd(xmlDtdPtr dtd);
/// ```
///
/// # SAFETY
///
/// - `dtd` must be a valid pointer to an _xmlDtd, or NULL.
pub unsafe fn free_dtd(dtd: *mut _xmlDtd) {
    if dtd.is_null() {
        return;
    }

    unsafe {
        let d = &mut *dtd;

        // UPSTREAM-PARITY (tree.c xmlFreeDtd): element/attribute/entity
        // declaration nodes in the child list are owned by the hash tables
        // and are freed by the deallocators below; only non-declaration
        // children (comments, PIs) are unlinked and freed from the list here.
        // This must run BEFORE the hash tables are freed so the decl nodes
        // are still alive when their type is inspected.
        if !d.children.is_null() {
            let mut c = d.children;
            while !c.is_null() {
                let next = (*c).next;
                let t = (*c).type_;
                if t != XML_ELEMENT_DECL as c_int
                    && t != XML_ATTRIBUTE_DECL as c_int
                    && t != XML_ENTITY_DECL as c_int
                {
                    crate::xml::tree::free_node(c);
                }
                c = next;
            }
        }

        // Free hash tables with their deallocators
        if !d.notations.is_null() {
            hash::hash_free(
                d.notations as *mut hash::HashTable,
                Some(notation_deallocator),
            );
            d.notations = ptr::null_mut();
        }
        if !d.elements.is_null() {
            hash::hash_free(
                d.elements as *mut hash::HashTable,
                Some(element_deallocator),
            );
            d.elements = ptr::null_mut();
        }
        if !d.attributes.is_null() {
            hash::hash_free(
                d.attributes as *mut hash::HashTable,
                Some(attribute_deallocator),
            );
            d.attributes = ptr::null_mut();
        }
        if !d.entities.is_null() {
            hash::hash_free(d.entities as *mut hash::HashTable, Some(entity_deallocator));
            d.entities = ptr::null_mut();
        }
        if !d.pentities.is_null() {
            hash::hash_free(
                d.pentities as *mut hash::HashTable,
                Some(entity_deallocator),
            );
            d.pentities = ptr::null_mut();
        }

        // Free strings
        if !d.name.is_null() {
            allocator::xmlFreeImpl(d.name as *mut c_void);
        }
        if !d.ExternalID.is_null() {
            allocator::xmlFreeImpl(d.ExternalID as *mut c_void);
        }
        if !d.SystemID.is_null() {
            allocator::xmlFreeImpl(d.SystemID as *mut c_void);
        }

        allocator::xmlFreeImpl(dtd as *mut c_void);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Hash Deallocators
// ═══════════════════════════════════════════════════════════════════════════════

/// Hash-table deallocator for notation declarations.
///
/// # Safety
///
/// - `payload` must be NULL or a valid pointer to an `_xmlNotation` owned
///   exclusively by the hash table being freed; it is freed with
///   `free_notation`.
/// - `_name` is unused.
unsafe extern "C" fn notation_deallocator(payload: *mut c_void, _name: *mut u8) {
    if !payload.is_null() {
        free_notation(payload as *mut _xmlNotation);
    }
}

/// Hash-table deallocator for element declarations.
///
/// # Safety
///
/// - `payload` must be NULL or a valid pointer to an `_xmlElement` owned
///   exclusively by the hash table being freed; it is freed with
///   `free_element`.
/// - `_name` is unused.
unsafe extern "C" fn element_deallocator(payload: *mut c_void, _name: *mut u8) {
    if !payload.is_null() {
        free_element(payload as *mut _xmlElement);
    }
}

/// Hash-table deallocator for attribute declarations.
///
/// # Safety
///
/// - `payload` must be NULL or a valid pointer to an `_xmlAttribute` owned
///   exclusively by the hash table being freed; it is freed with
///   `free_attribute`.
/// - `_name` is unused.
unsafe extern "C" fn attribute_deallocator(payload: *mut c_void, _name: *mut u8) {
    if !payload.is_null() {
        free_attribute(payload as *mut _xmlAttribute);
    }
}

/// Hash-table deallocator for entity declarations.
///
/// # Safety
///
/// - `payload` must be NULL or a valid pointer to an `_xmlEntity` owned
///   exclusively by the hash table being freed; it is freed with
///   `free_entity`.
/// - `_name` is unused.
unsafe extern "C" fn entity_deallocator(payload: *mut c_void, _name: *mut u8) {
    if !payload.is_null() {
        crate::xml::entities::free_entity(payload as *mut _xmlEntity);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Notation Declarations
// ═══════════════════════════════════════════════════════════════════════════════

/// Add a notation declaration to a DTD.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNotationPtr xmlAddNotationDecl(xmlDtdPtr dtd, const xmlChar *name,
///                                   const xmlChar *PublicID,
///                                   const xmlChar *SystemID);
/// ```
///
/// # SAFETY
///
/// - `dtd` must be a valid pointer to an _xmlDtd, or NULL.
/// - `name` must be a valid null-terminated string.
/// - `PublicID`, `SystemID` may be NULL.
pub unsafe fn add_notation_decl(
    dtd: *mut _xmlDtd,
    name: *const xmlChar,
    PublicID: *const xmlChar,
    SystemID: *const xmlChar,
) -> *mut _xmlNotation {
    if dtd.is_null() || name.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        // UPSTREAM-PARITY (valid.c xmlAddNotationDecl): the table is
        // created lazily on first use.
        if (*dtd).notations.is_null() {
            (*dtd).notations = hash::hash_create(8) as *mut c_void;
        }
        let d = &*dtd;

        // Check if notation already exists
        let existing = hash::hash_lookup(d.notations as *mut hash::HashTable, name);
        if !existing.is_null() {
            return existing as *mut _xmlNotation;
        }

        // SAFETY: Allocate zero-initialized memory for the notation.
        let not = allocator::xmlMallocZero(size_of::<_xmlNotation>() as usize) as *mut _xmlNotation;
        if not.is_null() {
            return ptr::null_mut();
        }

        (*not).name = string::xml_strdup(name);
        (*not).PublicID = string::xml_strdup(PublicID);
        (*not).SystemID = string::xml_strdup(SystemID);

        // Add to hash table
        let ret = hash::hash_add_entry(
            d.notations as *mut hash::HashTable,
            name,
            not as *mut c_void,
        );
        if ret != 0 {
            // Failed to add (shouldn't happen since we checked)
            free_notation(not);
            return ptr::null_mut();
        }

        not
    }
}

/// Look up a notation declaration by name.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNotationPtr xmlGetNotationDecl(xmlDtdPtr dtd, const xmlChar *name);
/// ```
///
/// # SAFETY
///
/// - `dtd` must be a valid pointer to an _xmlDtd, or NULL.
/// - `name` must be a valid null-terminated string.
pub unsafe fn get_notation_decl(dtd: *mut _xmlDtd, name: *const xmlChar) -> *mut _xmlNotation {
    if dtd.is_null() || name.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        let d = &*dtd;
        let payload = hash::hash_lookup(d.notations as *mut hash::HashTable, name);
        payload as *mut _xmlNotation
    }
}

/// Deep copy a notation declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlNotationPtr xmlCopyNotation(xmlNotationPtr notation);
/// ```
///
/// # SAFETY
///
/// - `notation` must be a valid pointer to an _xmlNotation, or NULL.
pub unsafe fn copy_notation(notation: *mut _xmlNotation) -> *mut _xmlNotation {
    if notation.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        let n = &*notation;
        // SAFETY: Allocate zero-initialized memory for the copy.
        let copy =
            allocator::xmlMallocZero(size_of::<_xmlNotation>() as usize) as *mut _xmlNotation;
        if copy.is_null() {
            return ptr::null_mut();
        }

        (*copy).name = string::xml_strdup(n.name);
        (*copy).PublicID = string::xml_strdup(n.PublicID);
        (*copy).SystemID = string::xml_strdup(n.SystemID);

        copy
    }
}

/// Free a notation declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeNotation(xmlNotationPtr notation);
/// ```
///
/// # SAFETY
///
/// - `notation` must be a valid pointer to an _xmlNotation, or NULL.
pub unsafe fn free_notation(notation: *mut _xmlNotation) {
    if notation.is_null() {
        return;
    }

    unsafe {
        let n = &*notation;
        if !n.name.is_null() {
            allocator::xmlFreeImpl(n.name as *mut c_void);
        }
        if !n.PublicID.is_null() {
            allocator::xmlFreeImpl(n.PublicID as *mut c_void);
        }
        if !n.SystemID.is_null() {
            allocator::xmlFreeImpl(n.SystemID as *mut c_void);
        }
        allocator::xmlFreeImpl(notation as *mut c_void);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Element Content Model Functions
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new element content model node.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlElementContentPtr xmlNewElementContent(const xmlChar *name, int type);
/// ```
///
/// Creates a content model node with the given name and type.
/// The `ocur` field is set to XML_ELEMENT_CONTENT_ONCE by default.
///
/// # SAFETY
///
/// - `name` may be NULL (for PCDATA and connector types).
pub unsafe fn create_content_model(name: *const xmlChar, type_: c_int) -> *mut _xmlElementContent {
    // SAFETY: Allocate zero-initialized memory for the content model.
    let content = allocator::xmlMallocZero(size_of::<_xmlElementContent>() as usize)
        as *mut _xmlElementContent;
    if content.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*content).type_ = type_;
        (*content).ocur = XML_ELEMENT_CONTENT_ONCE as c_int;
        (*content).name = string::xml_strdup(name);
        (*content).c1 = ptr::null_mut();
        (*content).c2 = ptr::null_mut();
        (*content).parent = ptr::null_mut();
        (*content).prefix = ptr::null_mut();
    }

    content
}

/// Free an element content model tree.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeElementContent(xmlElementContentPtr cur);
/// ```
///
/// Recursively frees the entire content model tree.
///
/// # SAFETY
///
/// - `cur` must be a valid pointer to an _xmlElementContent, or NULL.
pub unsafe fn free_content_model(cur: *mut _xmlElementContent) {
    if cur.is_null() {
        return;
    }

    unsafe {
        let c = &*cur;

        // Recursively free children
        if !c.c1.is_null() {
            free_content_model(c.c1);
        }
        if !c.c2.is_null() {
            free_content_model(c.c2);
        }

        // Free name and prefix
        if !c.name.is_null() {
            allocator::xmlFreeImpl(c.name as *mut c_void);
        }
        if !c.prefix.is_null() {
            allocator::xmlFreeImpl(c.prefix as *mut c_void);
        }

        allocator::xmlFreeImpl(cur as *mut c_void);
    }
}

/// Deep copy an element content model tree.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlElementContentPtr xmlCopyElementContent(xmlElementContentPtr content);
/// ```
///
/// Recursively copies the entire content model tree.
///
/// # SAFETY
///
/// - `content` must be a valid pointer to an _xmlElementContent, or NULL.
pub unsafe fn copy_content_model(content: *mut _xmlElementContent) -> *mut _xmlElementContent {
    if content.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        let c = &*content;

        // SAFETY: Allocate zero-initialized memory for the copy.
        let copy = allocator::xmlMallocZero(size_of::<_xmlElementContent>() as usize)
            as *mut _xmlElementContent;
        if copy.is_null() {
            return ptr::null_mut();
        }

        (*copy).type_ = c.type_;
        (*copy).ocur = c.ocur;
        (*copy).name = string::xml_strdup(c.name);
        (*copy).prefix = string::xml_strdup(c.prefix);

        // Recursively copy children
        if !c.c1.is_null() {
            (*copy).c1 = copy_content_model(c.c1);
            if !(*copy).c1.is_null() {
                (*(*copy).c1).parent = copy;
            }
        }
        if !c.c2.is_null() {
            (*copy).c2 = copy_content_model(c.c2);
            if !(*copy).c2.is_null() {
                (*(*copy).c2).parent = copy;
            }
        }

        copy
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Element Declarations
// ═══════════════════════════════════════════════════════════════════════════════

/// Add an element declaration to a DTD.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlElementPtr xmlAddElementDecl(xmlDtdPtr dtd, const xmlChar *name, int type,
///                                 xmlElementContentPtr content);
/// ```
///
/// If an element with the same name already exists, the existing one is returned
/// and no new declaration is created.
///
/// # SAFETY
///
/// - `dtd` must be a valid pointer to an _xmlDtd, or NULL.
/// - `name` must be a valid null-terminated string.
pub unsafe fn add_element_decl(
    dtd: *mut _xmlDtd,
    name: *const xmlChar,
    type_: c_int,
    content: *mut _xmlElementContent,
) -> *mut _xmlElement {
    if dtd.is_null() || name.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        // Placeholder upgrade (add_element_decl carries the attribute chain
        // of a removed UNDEFINED placeholder to the real declaration).
        let mut elem_attrs: *mut _xmlAttribute = ptr::null_mut();
        // UPSTREAM-PARITY (valid.c xmlAddElementDecl): the table is created
        // lazily on first use.
        if (*dtd).elements.is_null() {
            (*dtd).elements = hash::hash_create(8) as *mut c_void;
        }
        let d = &*dtd;

        // Check if element already exists
        let existing = hash::hash_lookup(d.elements as *mut hash::HashTable, name);
        if !existing.is_null() {
            let ex = existing as *mut _xmlElement;
            if (*ex).etype != XML_ELEMENT_TYPE_UNDEFINED as c_int {
                return ex;
            }
            // UPSTREAM-PARITY (valid.c xmlAddElementDecl): an UNDEFINED
            // placeholder (created when an ATTLIST named an as-yet
            // undeclared element) is REMOVED and freed; its attribute
            // declarations are carried over to the real declaration that
            // follows ("lookup old attributes inserted on an undefined
            // element in the internal subset"). The placeholder was never
            // linked into the DTD child list (get_element_decl_created), so
            // only the hash entry is removed.
            let old_attributes = (*ex).attributes;
            (*ex).attributes = ptr::null_mut();
            hash::hash_remove_entry(d.elements as *mut hash::HashTable, name, None);
            free_element(ex);
            elem_attrs = old_attributes;
        }

        // SAFETY: Allocate zero-initialized memory for the element.
        let elem = allocator::xmlMallocZero(size_of::<_xmlElement>() as usize) as *mut _xmlElement;
        if elem.is_null() {
            return ptr::null_mut();
        }

        // UPSTREAM-PARITY (valid.c xmlAddElementDecl): the node type is
        // XML_ELEMENT_DECL; the element type (EMPTY/ANY/MIXED/ELEMENT) is
        // stored in `etype` only. The caller passes the element type as
        // `type_` (matching the upstream parameter list).
        (*elem).name = string::xml_strdup(name);
        (*elem).type_ = XML_ELEMENT_DECL as c_int;
        (*elem).etype = type_; // xmlElementTypeVal mirrors xmlElementType here
        (*elem).content = content; // Takes ownership of the content model
        (*elem).attributes = elem_attrs;
        (*elem).prefix = ptr::null_mut();
        (*elem).children = ptr::null_mut();
        (*elem).last = ptr::null_mut();
        (*elem).parent = dtd;
        (*elem).next = ptr::null_mut();
        (*elem).prev = ptr::null_mut();
        (*elem).doc = (*dtd).doc;
        (*elem).cont_model = ptr::null_mut();

        // Add to hash table
        let ret = hash::hash_add_entry(
            d.elements as *mut hash::HashTable,
            name,
            elem as *mut c_void,
        );
        if ret != 0 {
            // Failed to add
            free_element(elem);
            return ptr::null_mut();
        }

        // UPSTREAM-PARITY (valid.c xmlAddElementDecl "Link it to the DTD"):
        // the element decl is a child node of the DTD.
        if (*dtd).last.is_null() {
            (*dtd).children = elem as *mut _xmlNode;
            (*dtd).last = elem as *mut _xmlNode;
        } else {
            (*(*dtd).last).next = elem as *mut _xmlNode;
            (*elem).prev = (*dtd).last;
            (*dtd).last = elem as *mut _xmlNode;
        }

        elem
    }
}

/// Look up an element declaration by name.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlElementPtr xmlGetElementDecl(xmlDtdPtr dtd, const xmlChar *name);
/// ```
///
/// # SAFETY
///
/// - `dtd` must be a valid pointer to an _xmlDtd, or NULL.
/// - `name` must be a valid null-terminated string.
pub unsafe fn get_element_decl(dtd: *mut _xmlDtd, name: *const xmlChar) -> *mut _xmlElement {
    if dtd.is_null() || name.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        let d = &*dtd;
        if d.elements.is_null() {
            return ptr::null_mut();
        }
        let payload = hash::hash_lookup(d.elements as *mut hash::HashTable, name);
        payload as *mut _xmlElement
    }
}

/// UPSTREAM-PARITY (valid.c xmlGetDtdElementDesc2): lookup an element
/// declaration, creating an UNDEFINED placeholder when missing. The
/// placeholder is registered in the elements table but NOT linked into the
/// DTD's child list (the attribute-decl path that triggers this leaves the
/// element undeclared).
///
/// # SAFETY
///
/// - `dtd` must be a valid pointer to an _xmlDtd, or NULL.
/// - `name` must be a valid null-terminated string.
pub unsafe fn get_element_decl_created(
    dtd: *mut _xmlDtd,
    name: *const xmlChar,
) -> *mut _xmlElement {
    if dtd.is_null() || name.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        if (*dtd).elements.is_null() {
            (*dtd).elements = hash::hash_create(8) as *mut c_void;
        }
        let existing = hash::hash_lookup((*dtd).elements as *mut hash::HashTable, name);
        if !existing.is_null() {
            return existing as *mut _xmlElement;
        }

        let elem = allocator::xmlMallocZero(size_of::<_xmlElement>() as usize) as *mut _xmlElement;
        if elem.is_null() {
            return ptr::null_mut();
        }
        (*elem).type_ = XML_ELEMENT_DECL as c_int;
        (*elem).name = string::xml_strdup(name);
        (*elem).etype = XML_ELEMENT_TYPE_UNDEFINED as c_int;
        (*elem).doc = (*dtd).doc;
        (*elem).parent = dtd;
        if hash::hash_add_entry(
            (*dtd).elements as *mut hash::HashTable,
            name,
            elem as *mut c_void,
        ) != 0
        {
            free_element(elem);
            return ptr::null_mut();
        }
        elem
    }
}

/// Deep copy an element declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlElementPtr xmlCopyElement(xmlElementPtr elem);
/// ```
///
/// # SAFETY
///
/// - `elem` must be a valid pointer to an _xmlElement, or NULL.
pub unsafe fn copy_element(elem: *mut _xmlElement) -> *mut _xmlElement {
    if elem.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        let e = &*elem;

        // SAFETY: Allocate zero-initialized memory for the copy.
        let copy = allocator::xmlMallocZero(size_of::<_xmlElement>() as usize) as *mut _xmlElement;
        if copy.is_null() {
            return ptr::null_mut();
        }

        (*copy).name = string::xml_strdup(e.name);
        (*copy).type_ = e.type_;
        (*copy).etype = e.etype;
        (*copy).content = copy_content_model(e.content);
        (*copy).prefix = string::xml_strdup(e.prefix);
        (*copy)._private = e._private;
        (*copy).parent = e.parent;
        (*copy).doc = e.doc;

        // Copy attribute declarations (linked list)
        if !e.attributes.is_null() {
            // UPSTREAM-PARITY: We copy the attribute linked list by
            // iterating and copying each attribute.
            let mut src_attr = e.attributes;
            let mut prev_copy: *mut _xmlAttribute = ptr::null_mut();
            let mut first_copy: *mut _xmlAttribute = ptr::null_mut();

            while !src_attr.is_null() {
                let attr_copy = copy_attribute_decl(src_attr);
                if attr_copy.is_null() {
                    // Free what we've copied so far
                    let mut to_free = first_copy;
                    while !to_free.is_null() {
                        let next = (*to_free).nexth;
                        free_attribute(to_free);
                        to_free = next;
                    }
                    allocator::xmlFreeImpl(copy as *mut c_void);
                    return ptr::null_mut();
                }

                if prev_copy.is_null() {
                    first_copy = attr_copy;
                } else {
                    (*prev_copy).nexth = attr_copy;
                }
                prev_copy = attr_copy;
                src_attr = (*src_attr).nexth;
            }

            (*copy).attributes = first_copy;
        }

        copy
    }
}

/// Free an element declaration and its content model.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeElement(xmlElementPtr elem);
/// ```
///
/// Frees the element declaration and its content model, but NOT the
/// attribute declarations (which are owned by the DTD's attribute hash).
///
/// # SAFETY
///
/// - `elem` must be a valid pointer to an _xmlElement, or NULL.
pub unsafe fn free_element(elem: *mut _xmlElement) {
    if elem.is_null() {
        return;
    }

    unsafe {
        // Free name
        if !(*elem).name.is_null() {
            allocator::xmlFreeImpl((*elem).name as *mut c_void);
        }

        // Free prefix
        if !(*elem).prefix.is_null() {
            allocator::xmlFreeImpl((*elem).prefix as *mut c_void);
        }

        // Free content model
        if !(*elem).content.is_null() {
            free_content_model((*elem).content);
        }

        // Free the compiled content-model NFA (xmlValidBuildContentModel)
        // UPSTREAM-PARITY: xmlFreeElement releases contModel via xmlRegFreeRegexp.
        if !(*elem).cont_model.is_null() {
            crate::xml::validation::free_content_model_nfa(
                (*elem).cont_model as *mut crate::xml::validation::ContentModelNfa,
            );
        }

        // UPSTREAM-PARITY: The attributes linked list on the element
        // declaration is NOT owned by the element. The DTD's attribute
        // hash table is the sole owner. When the DTD is freed, the
        // hash table's deallocator frees all attributes.
        // Therefore, we do NOT free the attributes list here.
        (*elem).attributes = ptr::null_mut();

        allocator::xmlFreeImpl(elem as *mut c_void);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Attribute Declarations
// ═══════════════════════════════════════════════════════════════════════════════

/// Free an enumeration value tree.
///
/// # SAFETY
///
/// - `tree` must be a valid pointer to an _xmlEnumeration, or NULL.
unsafe fn free_enumeration(tree: *mut _xmlEnumeration) {
    if tree.is_null() {
        return;
    }

    unsafe {
        let mut cur = tree;
        while !cur.is_null() {
            let next = (*cur).next;
            if !(*cur).name.is_null() {
                allocator::xmlFreeImpl((*cur).name as *mut c_void);
            }
            allocator::xmlFreeImpl(cur as *mut c_void);
            cur = next;
        }
    }
}

/// Deep copy an enumeration value tree.
///
/// # SAFETY
///
/// - `tree` must be a valid pointer to an _xmlEnumeration, or NULL.
unsafe fn copy_enumeration(tree: *mut _xmlEnumeration) -> *mut _xmlEnumeration {
    if tree.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        let mut src = tree;
        let mut first_copy: *mut _xmlEnumeration = ptr::null_mut();
        let mut prev_copy: *mut _xmlEnumeration = ptr::null_mut();

        while !src.is_null() {
            let copy = allocator::xmlMallocZero(size_of::<_xmlEnumeration>() as usize)
                as *mut _xmlEnumeration;
            if copy.is_null() {
                // Free what we've allocated so far
                let mut to_free = first_copy;
                while !to_free.is_null() {
                    let next = (*to_free).next;
                    if !(*to_free).name.is_null() {
                        allocator::xmlFreeImpl((*to_free).name as *mut c_void);
                    }
                    allocator::xmlFreeImpl(to_free as *mut c_void);
                    to_free = next;
                }
                return ptr::null_mut();
            }

            (*copy).name = string::xml_strdup((*src).name);
            (*copy).next = ptr::null_mut();

            if prev_copy.is_null() {
                first_copy = copy;
            } else {
                (*prev_copy).next = copy;
            }
            prev_copy = copy;
            src = (*src).next;
        }

        first_copy
    }
}

/// Add an attribute declaration to a DTD.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlAttributePtr xmlAddAttributeDecl(xmlDtdPtr dtd, xmlElementPtr elem,
///                                     const xmlChar *name, int type, int def,
///                                     const xmlChar *defaultValue,
///                                     xmlEnumerationPtr tree);
/// ```
///
/// Adds an attribute declaration to both the DTD's attribute hash table
/// (keyed by element name + attribute name) and the element's linked list.
/// If an attribute with the same name already exists for this element,
/// the existing declaration is returned.
///
/// # SAFETY
///
/// - `dtd` must be a valid pointer to an _xmlDtd, or NULL.
/// - `name` must be a valid null-terminated string.
/// - `elem`, `defaultValue`, `tree` may be NULL.
#[allow(clippy::too_many_arguments)]
pub unsafe fn add_attribute_decl(
    dtd: *mut _xmlDtd,
    elem: *mut _xmlElement,
    name: *const xmlChar,
    ns: *const xmlChar,
    type_: c_int,
    def: c_int,
    defaultValue: *const xmlChar,
    tree: *mut _xmlEnumeration,
) -> *mut _xmlAttribute {
    if dtd.is_null() || name.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        // UPSTREAM-PARITY (valid.c xmlAddAttributeDecl): the table is
        // created lazily on first use.
        if (*dtd).attributes.is_null() {
            (*dtd).attributes = hash::hash_create(8) as *mut c_void;
        }
        let d = &*dtd;
        let elem_name = if elem.is_null() {
            ptr::null()
        } else {
            (*elem).name
        };

        // Check if attribute already exists for this element
        // UPSTREAM-PARITY (valid.c xmlAddAttributeDecl): the attribute table
        // is keyed by (name, ns, elem) — xmlHashAdd3/xmlHashLookup3 with the
        // namespace as the middle key (R-000176: the pre-2.10 candidate
        // signature dropped the ns key).
        let existing =
            hash::hash_lookup3(d.attributes as *mut hash::HashTable, name, ns, elem_name);
        if !existing.is_null() {
            return existing as *mut _xmlAttribute;
        }

        // SAFETY: Allocate zero-initialized memory for the attribute.
        let attr =
            allocator::xmlMallocZero(size_of::<_xmlAttribute>() as usize) as *mut _xmlAttribute;
        if attr.is_null() {
            return ptr::null_mut();
        }

        (*attr).type_ = XML_ATTRIBUTE_DECL as c_int;
        (*attr).name = string::xml_strdup(name);
        (*attr).parent = dtd;
        (*attr).doc = d.doc;
        (*attr).nexth = ptr::null_mut();
        (*attr).atype = type_;
        (*attr).def = def;
        (*attr).defaultValue = string::xml_strdup(defaultValue);
        (*attr).tree = tree; // Takes ownership of the enumeration tree
                             // UPSTREAM-PARITY (valid.c xmlAddAttributeDecl): `prefix` mirrors the
                             // ns argument; NULL ns leaves it NULL.
        (*attr).prefix = if ns.is_null() {
            ptr::null_mut()
        } else {
            string::xml_strdup(ns)
        };
        (*attr).elem = string::xml_strdup(elem_name);

        // Add to DTD's attribute hash table (keyed by attribute name,
        // namespace, element name — upstream xmlHashAdd3).
        let ret = hash::hash_add_entry3(
            d.attributes as *mut hash::HashTable,
            name,
            ns,
            elem_name,
            attr as *mut c_void,
        );
        if ret != 0 {
            // Failed to add
            if !(*attr).defaultValue.is_null() {
                allocator::xmlFreeImpl((*attr).defaultValue as *mut c_void);
            }
            if !(*attr).name.is_null() {
                allocator::xmlFreeImpl((*attr).name as *mut c_void);
            }
            if !(*attr).elem.is_null() {
                allocator::xmlFreeImpl((*attr).elem as *mut c_void);
            }
            if !(*attr).prefix.is_null() {
                allocator::xmlFreeImpl((*attr).prefix as *mut c_void);
            }
            allocator::xmlFreeImpl(attr as *mut c_void);
            // Don't free tree - caller still owns it on failure
            return ptr::null_mut();
        }

        // Add to element's linked list
        if !elem.is_null() {
            (*attr).nexth = (*elem).attributes;
            (*elem).attributes = attr;
        }

        // UPSTREAM-PARITY (valid.c xmlAddAttributeDecl "Link it to the
        // DTD"): the attribute decl is a child node of the DTD.
        if (*dtd).last.is_null() {
            (*dtd).children = attr as *mut _xmlNode;
            (*dtd).last = attr as *mut _xmlNode;
        } else {
            (*(*dtd).last).next = attr as *mut _xmlNode;
            (*attr).prev = (*dtd).last;
            (*dtd).last = attr as *mut _xmlNode;
        }

        attr
    }
}

/// Look up an attribute declaration by element name and attribute name.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlAttributePtr xmlGetAttributeDecl(xmlDtdPtr dtd, xmlElementPtr elem,
///                                     const xmlChar *name, int namePrefix);
/// ```
///
/// The `namePrefix` parameter is ignored in this implementation
/// (it's a legacy parameter in libxml2).
///
/// # SAFETY
///
/// - `dtd` must be a valid pointer to an _xmlDtd, or NULL.
/// - `name` must be a valid null-terminated string.
/// - `elem` may be NULL.
pub unsafe fn get_attribute_decl(
    dtd: *mut _xmlDtd,
    elem: *mut _xmlElement,
    name: *const xmlChar,
    _namePrefix: c_int,
) -> *mut _xmlAttribute {
    if dtd.is_null() || name.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        let d = &*dtd;
        let elem_name = if elem.is_null() {
            ptr::null()
        } else {
            (*elem).name
        };

        // UPSTREAM-PARITY (valid.c xmlGetDtdQAttrDesc): keyed by
        // (name, prefix, elem).
        let payload = hash::hash_lookup3(
            d.attributes as *mut hash::HashTable,
            name,
            ptr::null(),
            elem_name,
        );
        payload as *mut _xmlAttribute
    }
}

/// Deep copy an attribute declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlAttributePtr xmlCopyAttribute(xmlAttributePtr attr);
/// ```
///
/// # SAFETY
///
/// - `attr` must be a valid pointer to an _xmlAttribute, or NULL.
pub unsafe fn copy_attribute_decl(attr: *mut _xmlAttribute) -> *mut _xmlAttribute {
    if attr.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        let a = &*attr;

        // SAFETY: Allocate zero-initialized memory for the copy.
        let copy =
            allocator::xmlMallocZero(size_of::<_xmlAttribute>() as usize) as *mut _xmlAttribute;
        if copy.is_null() {
            return ptr::null_mut();
        }

        (*copy).type_ = a.type_;
        (*copy).name = string::xml_strdup(a.name);
        (*copy).parent = a.parent;
        (*copy).doc = a.doc;
        (*copy).nexth = ptr::null_mut();
        (*copy).atype = a.atype;
        (*copy).def = a.def;
        (*copy).defaultValue = string::xml_strdup(a.defaultValue);
        (*copy).tree = copy_enumeration(a.tree);
        (*copy).prefix = string::xml_strdup(a.prefix);
        (*copy).elem = string::xml_strdup(a.elem);

        copy
    }
}

/// Free an attribute declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeAttribute(xmlAttributePtr attr);
/// ```
///
/// # SAFETY
///
/// - `attr` must be a valid pointer to an _xmlAttribute, or NULL.
pub unsafe fn free_attribute(attr: *mut _xmlAttribute) {
    if attr.is_null() {
        return;
    }

    unsafe {
        let a = &*attr;

        if !a.name.is_null() {
            allocator::xmlFreeImpl(a.name as *mut c_void);
        }
        if !a.defaultValue.is_null() {
            allocator::xmlFreeImpl(a.defaultValue as *mut c_void);
        }
        if !a.prefix.is_null() {
            allocator::xmlFreeImpl(a.prefix as *mut c_void);
        }
        if !a.elem.is_null() {
            allocator::xmlFreeImpl(a.elem as *mut c_void);
        }
        if !a.tree.is_null() {
            free_enumeration(a.tree);
        }

        allocator::xmlFreeImpl(attr as *mut c_void);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Content Model Validation (Automata-based)
// ═══════════════════════════════════════════════════════════════════════════════

/// Result of content model validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentModelResult {
    /// Content is valid.
    Valid,
    /// Content is invalid.
    Invalid,
    /// Content model is indeterminate (mixed content with PCDATA).
    Indeterminate,
}

/// Validate content (a list of element names) against a content model,
/// taking occurrence indicators into account.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidContentModel(xmlElementContentPtr model, ...)
/// ```
///
/// This implements a simple recursive descent validator for content models.
/// For simple content models (EMPTY, ANY, PCDATA), the check is direct.
/// For sequence/choice models, it recursively validates.
///
/// Returns `ContentModelResult::Valid` if the content matches the model,
/// `ContentModelResult::Invalid` otherwise.
///
/// # SAFETY
///
/// - `model` must be a valid pointer to an _xmlElementContent, or NULL.
/// - `names` must be a slice of element names (null-terminated xmlChar strings).
pub unsafe fn valid_content_model(
    model: *mut _xmlElementContent,
    names: &[*const xmlChar],
) -> ContentModelResult {
    if model.is_null() {
        return ContentModelResult::Invalid;
    }

    unsafe {
        let m = &*model;

        // Handle occurrence indicators at this level first
        match m.ocur as u32 {
            o if o == XML_ELEMENT_CONTENT_OPT as u32 => {
                // Optional: zero or one occurrence
                if names.is_empty() {
                    return ContentModelResult::Valid;
                }
                return valid_content_model_inner(model, names);
            }
            o if o == XML_ELEMENT_CONTENT_MULT as u32 => {
                // Zero or more
                if names.is_empty() {
                    return ContentModelResult::Valid;
                }
                return valid_content_model_zero_or_more(model, names);
            }
            o if o == XML_ELEMENT_CONTENT_PLUS as u32 => {
                // One or more
                if names.is_empty() {
                    return ContentModelResult::Invalid;
                }
                return valid_content_model_one_or_more(model, names);
            }
            _ => {}
        }

        valid_content_model_inner(model, names)
    }
}

/// Validate content against a content model without considering occurrence.
///
/// # Safety
///
/// - `model` must be a valid pointer to an `_xmlElementContent`; it is
///   dereferenced to read `type_` and `name`, and its `c1`/`c2` children are
///   followed when non-NULL.
/// - Each entry in `names` must be NULL or a valid pointer to a
///   NUL-terminated `xmlChar` element name; non-NULL entries are compared
///   with `xml_strcmp`.
unsafe fn valid_content_model_inner(
    model: *mut _xmlElementContent,
    names: &[*const xmlChar],
) -> ContentModelResult {
    unsafe {
        let m = &*model;

        match m.type_ as u32 {
            t if t == XML_ELEMENT_CONTENT_PCDATA as u32 => {
                // PCDATA: content must be empty (just text)
                if names.is_empty() {
                    ContentModelResult::Valid
                } else {
                    ContentModelResult::Invalid
                }
            }
            t if t == XML_ELEMENT_CONTENT_ELEMENT as u32 => {
                // Single element: must match exactly one element
                if names.len() != 1 {
                    return ContentModelResult::Invalid;
                }
                if names[0].is_null() {
                    return ContentModelResult::Invalid;
                }
                // Compare with model name
                if string::xml_strcmp(names[0], m.name) != 0 {
                    return ContentModelResult::Invalid;
                }
                ContentModelResult::Valid
            }
            t if t == XML_ELEMENT_CONTENT_SEQ as u32 => {
                // Sequence: validate children in order
                valid_content_model_seq(m, names)
            }
            t if t == XML_ELEMENT_CONTENT_OR as u32 => {
                // Choice: one of the alternatives must match all names
                valid_content_model_or(m, names)
            }
            _ => ContentModelResult::Invalid,
        }
    }
}

/// Validate content for zero-or-more occurrence.
///
/// # Safety
///
/// - `model` must be a valid pointer to an `_xmlElementContent`; it is
///   forwarded to `valid_content_model_inner`, which dereferences it.
/// - Each entry in `names` must be NULL or a valid pointer to a
///   NUL-terminated `xmlChar` element name.
unsafe fn valid_content_model_zero_or_more(
    model: *mut _xmlElementContent,
    names: &[*const xmlChar],
) -> ContentModelResult {
    // Zero or more: try each possible split
    let mut i = 0;
    while i <= names.len() {
        let consumed = &names[..i];
        let remaining = &names[i..];

        let consumed_valid = unsafe { valid_content_model_inner(model, consumed) };
        if consumed_valid == ContentModelResult::Valid {
            if remaining.is_empty() {
                return ContentModelResult::Valid;
            }
            // Try to match remaining with same model
            let remaining_valid = unsafe { valid_content_model_zero_or_more(model, remaining) };
            if remaining_valid == ContentModelResult::Valid {
                return ContentModelResult::Valid;
            }
        }

        i += 1;
    }
    ContentModelResult::Invalid
}

/// Validate content for one-or-more occurrence.
///
/// # Safety
///
/// - `model` must be a valid pointer to an `_xmlElementContent`; it is
///   forwarded to `valid_content_model_inner`, which dereferences it.
/// - Each entry in `names` must be NULL or a valid pointer to a
///   NUL-terminated `xmlChar` element name.
unsafe fn valid_content_model_one_or_more(
    model: *mut _xmlElementContent,
    names: &[*const xmlChar],
) -> ContentModelResult {
    // One or more: must match at least once
    let mut i = 1;
    while i <= names.len() {
        let consumed = &names[..i];
        let remaining = &names[i..];

        let consumed_valid = unsafe { valid_content_model_inner(model, consumed) };
        if consumed_valid == ContentModelResult::Valid {
            if remaining.is_empty() {
                return ContentModelResult::Valid;
            }
            let remaining_valid = unsafe { valid_content_model_zero_or_more(model, remaining) };
            if remaining_valid == ContentModelResult::Valid {
                return ContentModelResult::Valid;
            }
        }

        i += 1;
    }
    ContentModelResult::Invalid
}

/// Validate content against a sequence content model.
///
/// # Safety
///
/// - The `model` reference must point to a live `_xmlElementContent`.
/// - Non-NULL `c1`/`c2` children must be valid pointers to
///   `_xmlElementContent`; they are passed to `valid_content_model`.
/// - Each entry in `names` must be NULL or a valid pointer to a
///   NUL-terminated `xmlChar` element name.
unsafe fn valid_content_model_seq(
    model: &_xmlElementContent,
    names: &[*const xmlChar],
) -> ContentModelResult {
    // For a sequence, we need to split the names between c1 and c2
    // This is a simplified validation - full automata-based validation
    // would be more complex.

    let c1 = model.c1;
    let c2 = model.c2;

    if c1.is_null() && c2.is_null() {
        return ContentModelResult::Valid;
    }

    if c1.is_null() {
        return unsafe { valid_content_model(c2, names) };
    }

    if c2.is_null() {
        return unsafe { valid_content_model(c1, names) };
    }

    // Try to split the names at each possible position
    // This implements a simple backtracking validator
    for split in 0..=names.len() {
        let left = &names[..split];
        let right = &names[split..];

        let left_valid = unsafe { valid_content_model(c1, left) };
        if left_valid != ContentModelResult::Valid {
            continue;
        }

        let right_valid = unsafe { valid_content_model(c2, right) };
        if right_valid == ContentModelResult::Valid {
            return ContentModelResult::Valid;
        }
    }

    ContentModelResult::Invalid
}

/// Validate content against a choice content model.
///
/// # Safety
///
/// - The `model` reference must point to a live `_xmlElementContent`.
/// - Non-NULL `c1`/`c2` children must be valid pointers to
///   `_xmlElementContent`; they are passed to `valid_content_model`.
/// - Each entry in `names` must be NULL or a valid pointer to a
///   NUL-terminated `xmlChar` element name.
unsafe fn valid_content_model_or(
    model: &_xmlElementContent,
    names: &[*const xmlChar],
) -> ContentModelResult {
    let c1 = model.c1;
    let c2 = model.c2;

    if c1.is_null() && c2.is_null() {
        return ContentModelResult::Invalid;
    }

    if !c1.is_null() {
        let result = unsafe { valid_content_model(c1, names) };
        if result == ContentModelResult::Valid {
            return ContentModelResult::Valid;
        }
    }

    if !c2.is_null() {
        let result = unsafe { valid_content_model(c2, names) };
        if result == ContentModelResult::Valid {
            return ContentModelResult::Valid;
        }
    }

    ContentModelResult::Invalid
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    use core::ffi::c_void;
    use core::ptr;

    // ── Helpers ──────────────────────────────────────────────────────────

    unsafe fn c_str(s: &[u8]) -> *const xmlChar {
        // Create a null-terminated xmlChar string
        let len = s.len();
        let buf = allocator::xmlMallocImpl(len + 1) as *mut xmlChar;
        assert!(!buf.is_null());
        ptr::copy_nonoverlapping(s.as_ptr(), buf, len);
        *buf.add(len) = 0;
        buf as *const xmlChar
    }

    unsafe fn make_doc_and_dtd() -> (*mut _xmlDoc, *mut _xmlDtd) {
        let doc = allocator::xmlMallocZero(size_of::<_xmlDoc>() as usize) as *mut _xmlDoc;
        assert!(!doc.is_null());
        (*doc).type_ = XML_DOCUMENT_NODE as c_int;
        (*doc).doc = doc;
        let dtd = create_int_subset(doc, c_str(b"root"), ptr::null(), ptr::null());
        assert!(!dtd.is_null());
        (doc, dtd)
    }

    // ── DTD Access Tests ────────────────────────────────────────────────

    /// Verify that `get_int_subset` accepts a NULL document.
    ///
    /// # Safety
    ///
    /// - NULL is allowed and is never dereferenced.
    #[test]
    fn test_get_int_subset_null() {
        {
            assert!(get_int_subset(ptr::null()).is_null());
        }
    }

    /// Verify that `create_int_subset` attaches a DTD to a document.
    ///
    /// # Safety
    ///
    /// - `doc` and `dtd` from `make_doc_and_dtd` are heap-allocated structs
    ///   that must be valid while their fields are read; the DTD is freed with
    ///   `free_dtd` and the doc with `xmlFreeImpl`.
    #[test]
    fn test_create_int_subset() {
        unsafe {
            let (doc, dtd) = make_doc_and_dtd();
            assert_eq!((*dtd).type_, XML_DTD_NODE as c_int);
            assert!(!(*dtd).name.is_null());
            assert_eq!((*doc).intSubset, dtd);

            // Cleanup
            free_dtd(dtd);
            allocator::xmlFreeImpl(doc as *mut c_void);
        }
    }

    /// Verify that `create_int_subset` with a NULL doc still allocates and
    /// returns an unattached DTD (upstream tree.c `xmlCreateIntSubset` —
    /// HOSTILE-ABI A37).
    ///
    /// # Safety
    ///
    /// - A NULL `doc` is allowed and is not dereferenced; the returned DTD
    ///   is freed with `free_dtd`.
    #[test]
    fn test_create_int_subset_null_doc() {
        unsafe {
            let dtd = create_int_subset(ptr::null_mut(), c_str(b"root"), ptr::null(), ptr::null());
            assert!(!dtd.is_null());
            assert_eq!((*dtd).type_, XML_DTD_NODE as c_int);
            assert!((*dtd).parent.is_null());
            assert!((*dtd).doc.is_null());
            free_dtd(dtd);
        }
    }

    /// Verify that `new_dtd` creates a DTD node and attaches it to the doc.
    ///
    /// # Safety
    ///
    /// - `doc` is a heap-allocated `_xmlDoc` and the strings passed to
    ///   `new_dtd` are NUL-terminated allocations that stay alive for the call.
    /// - `dtd` must be freed with `free_dtd` and the doc with `xmlFreeImpl`.
    #[test]
    fn test_new_dtd() {
        unsafe {
            let doc = allocator::xmlMallocZero(size_of::<_xmlDoc>() as usize) as *mut _xmlDoc;
            assert!(!doc.is_null());
            (*doc).type_ = XML_DOCUMENT_NODE as c_int;
            (*doc).doc = doc;

            let dtd = new_dtd(doc, c_str(b"test"), c_str(b"-//TEST//"), c_str(b"test.dtd"));
            assert!(!dtd.is_null());
            assert_eq!((*dtd).type_, XML_DTD_NODE as c_int);
            assert_eq!((*doc).intSubset, dtd);

            free_dtd(dtd);
            allocator::xmlFreeImpl(doc as *mut c_void);
        }
    }

    /// Verify that `new_dtd` works without a document.
    ///
    /// # Safety
    ///
    /// - The strings passed to `new_dtd` must be NUL-terminated and alive for
    ///   the call; the returned DTD is freed with `free_dtd`.
    #[test]
    fn test_new_dtd_no_doc() {
        unsafe {
            let dtd = new_dtd(ptr::null_mut(), c_str(b"test"), ptr::null(), ptr::null());
            assert!(!dtd.is_null());
            free_dtd(dtd);
        }
    }

    // ── Notation Tests ──────────────────────────────────────────────────

    /// Verify adding and looking up a notation declaration.
    ///
    /// # Safety
    ///
    /// - `dtd` from `make_doc_and_dtd` and the NUL-terminated `c_str` buffers
    ///   must be valid and alive for the calls; the DTD is freed with
    ///   `free_dtd` and the doc with `xmlFreeImpl`.
    #[test]
    fn test_add_get_notation() {
        unsafe {
            let (doc, dtd) = make_doc_and_dtd();
            let name = c_str(b"note");
            let pubid = c_str(b"-//TEST//NOTATION");
            let sysid = c_str(b"note.ent");

            let n = add_notation_decl(dtd, name, pubid, sysid);
            assert!(!n.is_null());
            assert_eq!(string::xml_strcmp((*n).name, name), 0);

            // Lookup
            let found = get_notation_decl(dtd, name);
            assert_eq!(found, n);

            // Lookup non-existent
            let not_found = get_notation_decl(dtd, c_str(b"nonexistent"));
            assert!(not_found.is_null());

            free_dtd(dtd);
            allocator::xmlFreeImpl(doc as *mut c_void);
        }
    }

    /// Verify that adding a notation to a NULL DTD returns NULL.
    ///
    /// # Safety
    ///
    /// - Passing a NULL `dtd` is allowed and is not dereferenced; the name
    ///   buffer must be NUL-terminated.
    #[test]
    fn test_add_notation_null_dtd() {
        unsafe {
            let n = add_notation_decl(ptr::null_mut(), c_str(b"test"), ptr::null(), ptr::null());
            assert!(n.is_null());
        }
    }

    /// Verify `copy_notation` deep-copies a notation declaration.
    ///
    /// # Safety
    ///
    /// - `n` from `add_notation_decl` must be a valid `_xmlNotation` while
    ///   copied; the copy is freed with `free_notation`, the DTD with
    ///   `free_dtd`, and the doc with `xmlFreeImpl`.
    #[test]
    fn test_copy_notation() {
        unsafe {
            let (doc, dtd) = make_doc_and_dtd();
            let name = c_str(b"note1");
            let pubid = c_str(b"public");
            let sysid = c_str(b"system");

            let n = add_notation_decl(dtd, name, pubid, sysid);
            assert!(!n.is_null());

            let copy = copy_notation(n);
            assert!(!copy.is_null());
            assert_ne!(copy, n);
            assert_eq!(string::xml_strcmp((*copy).name, name), 0);
            assert_eq!(string::xml_strcmp((*copy).PublicID, pubid), 0);
            assert_eq!(string::xml_strcmp((*copy).SystemID, sysid), 0);

            free_notation(copy);
            free_dtd(dtd);
            allocator::xmlFreeImpl(doc as *mut c_void);
        }
    }

    /// Verify that `copy_notation` accepts a NULL pointer.
    ///
    /// # Safety
    ///
    /// - NULL is allowed and is not dereferenced.
    #[test]
    fn test_copy_notation_null() {
        unsafe {
            assert!(copy_notation(ptr::null_mut()).is_null());
        }
    }

    /// Verify that `free_notation` accepts a NULL pointer.
    ///
    /// # Safety
    ///
    /// - NULL is allowed and is not dereferenced.
    #[test]
    fn test_free_notation_null() {
        unsafe {
            free_notation(ptr::null_mut()); // Should not crash
        }
    }

    // ── Content Model Tests ─────────────────────────────────────────────

    /// Verify creating and freeing a content model.
    ///
    /// # Safety
    ///
    /// - The name buffer passed to `create_content_model` must be
    ///   NUL-terminated and alive for the call; the returned model is freed
    ///   with `free_content_model`.
    #[test]
    fn test_create_free_content_model() {
        unsafe {
            let cm = create_content_model(c_str(b"child"), XML_ELEMENT_CONTENT_ELEMENT as c_int);
            assert!(!cm.is_null());
            assert_eq!((*cm).type_, XML_ELEMENT_CONTENT_ELEMENT as c_int);
            assert_eq!((*cm).ocur, XML_ELEMENT_CONTENT_ONCE as c_int);

            free_content_model(cm);
        }
    }

    /// Verify creating a PCDATA content model.
    ///
    /// # Safety
    ///
    /// - The returned model must be valid while read and is freed with
    ///   `free_content_model`.
    #[test]
    fn test_create_content_model_pcdata() {
        unsafe {
            let cm = create_content_model(ptr::null(), XML_ELEMENT_CONTENT_PCDATA as c_int);
            assert!(!cm.is_null());
            assert_eq!((*cm).type_, XML_ELEMENT_CONTENT_PCDATA as c_int);
            free_content_model(cm);
        }
    }

    /// Verify `copy_content_model` deep-copies a content model.
    ///
    /// # Safety
    ///
    /// - `cm` must be a valid `_xmlElementContent` while copied; both models
    ///   are freed with `free_content_model`.
    #[test]
    fn test_copy_content_model() {
        unsafe {
            let cm = create_content_model(c_str(b"child"), XML_ELEMENT_CONTENT_ELEMENT as c_int);
            assert!(!cm.is_null());

            let copy = copy_content_model(cm);
            assert!(!copy.is_null());
            assert_ne!(copy, cm);
            assert_eq!((*copy).type_, XML_ELEMENT_CONTENT_ELEMENT as c_int);
            assert_eq!((*copy).ocur, XML_ELEMENT_CONTENT_ONCE as c_int);
            assert_eq!(string::xml_strcmp((*copy).name, (*cm).name), 0);

            free_content_model(cm);
            free_content_model(copy);
        }
    }

    /// Verify that `copy_content_model` accepts a NULL pointer.
    ///
    /// # Safety
    ///
    /// - NULL is allowed and is not dereferenced.
    #[test]
    fn test_copy_content_model_null() {
        unsafe {
            assert!(copy_content_model(ptr::null_mut()).is_null());
        }
    }

    /// Verify that `free_content_model` accepts a NULL pointer.
    ///
    /// # Safety
    ///
    /// - NULL is allowed and is not dereferenced.
    #[test]
    fn test_free_content_model_null() {
        unsafe {
            free_content_model(ptr::null_mut()); // Should not crash
        }
    }

    /// Verify creating a sequence content model with two children.
    ///
    /// # Safety
    ///
    /// - `c1`, `c2`, and `seq` must be valid `_xmlElementContent` pointers
    ///   whose `parent`/`c1`/`c2` links are consistent before `free_content_model`
    ///   walks them.
    #[test]
    fn test_create_sequence_content_model() {
        unsafe {
            let c1 = create_content_model(c_str(b"a"), XML_ELEMENT_CONTENT_ELEMENT as c_int);
            let c2 = create_content_model(c_str(b"b"), XML_ELEMENT_CONTENT_ELEMENT as c_int);
            let seq = create_content_model(ptr::null(), XML_ELEMENT_CONTENT_SEQ as c_int);
            assert!(!seq.is_null());
            (*seq).c1 = c1;
            (*seq).c2 = c2;
            (*c1).parent = seq;
            (*c2).parent = seq;

            free_content_model(seq);
        }
    }

    // ── Element Declaration Tests ───────────────────────────────────────

    /// Verify adding and looking up an element declaration.
    ///
    /// # Safety
    ///
    /// - `dtd` from `make_doc_and_dtd` and the NUL-terminated `c_str` buffers
    ///   must be valid and alive for the calls; the DTD is freed with
    ///   `free_dtd` and the doc with `xmlFreeImpl`.
    #[test]
    fn test_add_get_element() {
        unsafe {
            let (doc, dtd) = make_doc_and_dtd();
            let name = c_str(b"myElement");

            let elem =
                add_element_decl(dtd, name, XML_ELEMENT_TYPE_EMPTY as c_int, ptr::null_mut());
            assert!(!elem.is_null());
            assert_eq!((*elem).etype, XML_ELEMENT_TYPE_EMPTY as c_int);

            let found = get_element_decl(dtd, name);
            assert_eq!(found, elem);

            let not_found = get_element_decl(dtd, c_str(b"nonexistent"));
            assert!(not_found.is_null());

            free_dtd(dtd);
            allocator::xmlFreeImpl(doc as *mut c_void);
        }
    }

    /// Verify that adding a duplicate element declaration returns the
    /// existing declaration unchanged.
    ///
    /// # Safety
    ///
    /// - `dtd` from `make_doc_and_dtd` and the NUL-terminated name buffer must
    ///   be valid and alive for the calls; the DTD is freed with `free_dtd`
    ///   and the doc with `xmlFreeImpl`.
    #[test]
    fn test_add_element_duplicate() {
        unsafe {
            let (doc, dtd) = make_doc_and_dtd();
            let name = c_str(b"dup");

            let e1 = add_element_decl(dtd, name, XML_ELEMENT_TYPE_EMPTY as c_int, ptr::null_mut());
            assert!(!e1.is_null());

            let e2 = add_element_decl(dtd, name, XML_ELEMENT_TYPE_ANY as c_int, ptr::null_mut());
            assert_eq!(e1, e2); // Same pointer returned
            assert_eq!((*e2).type_, XML_ELEMENT_DECL as c_int); // node type
            assert_eq!((*e2).etype, XML_ELEMENT_TYPE_EMPTY as c_int); // Still empty

            free_dtd(dtd);
            allocator::xmlFreeImpl(doc as *mut c_void);
        }
    }

    /// Verify that adding an element to a NULL DTD returns NULL.
    ///
    /// # Safety
    ///
    /// - Passing a NULL `dtd` is allowed and is not dereferenced; the name
    ///   buffer must be NUL-terminated.
    #[test]
    fn test_add_element_null_dtd() {
        unsafe {
            let elem = add_element_decl(
                ptr::null_mut(),
                c_str(b"test"),
                XML_ELEMENT_TYPE_EMPTY as c_int,
                ptr::null_mut(),
            );
            assert!(elem.is_null());
        }
    }

    /// Verify `copy_element` deep-copies an element declaration.
    ///
    /// # Safety
    ///
    /// - `elem` and its content model must be valid while copied; the copy is
    ///   freed with `free_element`, the DTD with `free_dtd`, and the doc with
    ///   `xmlFreeImpl`.
    #[test]
    fn test_copy_element() {
        unsafe {
            let (doc, dtd) = make_doc_and_dtd();
            let name = c_str(b"source");
            let cm = create_content_model(c_str(b"child"), XML_ELEMENT_CONTENT_ELEMENT as c_int);

            let elem = add_element_decl(dtd, name, XML_ELEMENT_TYPE_ELEMENT as c_int, cm);
            assert!(!elem.is_null());

            let copy = copy_element(elem);
            assert!(!copy.is_null());
            assert_ne!(copy, elem);
            assert_eq!((*copy).type_, XML_ELEMENT_DECL as c_int);
            assert_eq!((*copy).etype, XML_ELEMENT_TYPE_ELEMENT as c_int);
            assert_eq!(string::xml_strcmp((*copy).name, name), 0);
            assert!(!(*copy).content.is_null());
            assert_ne!((*copy).content, cm);

            free_element(copy);
            free_dtd(dtd);
            allocator::xmlFreeImpl(doc as *mut c_void);
        }
    }

    /// Verify that `free_element` accepts a NULL pointer.
    ///
    /// # Safety
    ///
    /// - NULL is allowed and is not dereferenced.
    #[test]
    fn test_free_element_null() {
        unsafe {
            free_element(ptr::null_mut()); // Should not crash
        }
    }

    // ── Attribute Declaration Tests ─────────────────────────────────────

    /// Verify adding and looking up an attribute declaration.
    ///
    /// # Safety
    ///
    /// - `dtd` from `make_doc_and_dtd` and the NUL-terminated `c_str` buffers
    ///   must be valid and alive for the calls; the DTD is freed with
    ///   `free_dtd` and the doc with `xmlFreeImpl`.
    #[test]
    fn test_add_get_attribute() {
        unsafe {
            let (doc, dtd) = make_doc_and_dtd();
            let elem_name = c_str(b"elem");
            let attr_name = c_str(b"attr1");

            let elem = add_element_decl(
                dtd,
                elem_name,
                XML_ELEMENT_TYPE_EMPTY as c_int,
                ptr::null_mut(),
            );
            assert!(!elem.is_null());

            let attr = add_attribute_decl(
                dtd,
                elem,
                attr_name,
                ptr::null(),
                XML_ATTRIBUTE_CDATA as c_int,
                XML_ATTRIBUTE_IMPLIED as c_int,
                ptr::null(),
                ptr::null_mut(),
            );
            assert!(!attr.is_null());
            assert_eq!((*attr).atype, XML_ATTRIBUTE_CDATA as c_int);
            assert_eq!((*attr).def, XML_ATTRIBUTE_IMPLIED as c_int);

            // Lookup by element + attribute name
            let found = get_attribute_decl(dtd, elem, attr_name, 0);
            assert_eq!(found, attr);

            // Lookup non-existent
            let not_found = get_attribute_decl(dtd, elem, c_str(b"nonexistent"), 0);
            assert!(not_found.is_null());

            free_dtd(dtd);
            allocator::xmlFreeImpl(doc as *mut c_void);
        }
    }

    /// Verify adding an attribute declaration with a default value.
    ///
    /// # Safety
    ///
    /// - `dtd` from `make_doc_and_dtd` and the NUL-terminated `c_str` buffers
    ///   must be valid and alive for the calls; the DTD is freed with
    ///   `free_dtd` and the doc with `xmlFreeImpl`.
    #[test]
    fn test_add_attribute_with_default() {
        unsafe {
            let (doc, dtd) = make_doc_and_dtd();
            let elem_name = c_str(b"elem");
            let attr_name = c_str(b"color");
            let default_val = c_str(b"red");

            let elem = add_element_decl(
                dtd,
                elem_name,
                XML_ELEMENT_TYPE_EMPTY as c_int,
                ptr::null_mut(),
            );

            let attr = add_attribute_decl(
                dtd,
                elem,
                attr_name,
                ptr::null(),
                XML_ATTRIBUTE_CDATA as c_int,
                XML_ATTRIBUTE_FIXED as c_int,
                default_val,
                ptr::null_mut(),
            );
            assert!(!attr.is_null());
            assert_eq!((*attr).def, XML_ATTRIBUTE_FIXED as c_int);
            assert_eq!(string::xml_strcmp((*attr).defaultValue, default_val), 0);

            free_dtd(dtd);
            allocator::xmlFreeImpl(doc as *mut c_void);
        }
    }

    /// Verify adding an attribute with an enumeration of values.
    ///
    /// # Safety
    ///
    /// - The `_xmlEnumeration` chain built with `xmlMallocZero` must consist
    ///   of valid structs with NUL-terminated duplicated names before it is
    ///   handed to `add_attribute_decl`; the DTD owns and frees it via
    ///   `free_dtd`, and the doc is freed with `xmlFreeImpl`.
    #[test]
    fn test_add_attribute_enumeration() {
        unsafe {
            let (doc, dtd) = make_doc_and_dtd();
            let elem_name = c_str(b"elem");
            let attr_name = c_str(b"size");

            // Build enumeration: small, medium, large
            let v3 = allocator::xmlMallocZero(size_of::<_xmlEnumeration>() as usize)
                as *mut _xmlEnumeration;
            (*v3).name = string::xml_strdup(c_str(b"large"));
            let v2 = allocator::xmlMallocZero(size_of::<_xmlEnumeration>() as usize)
                as *mut _xmlEnumeration;
            (*v2).name = string::xml_strdup(c_str(b"medium"));
            (*v2).next = v3;
            let v1 = allocator::xmlMallocZero(size_of::<_xmlEnumeration>() as usize)
                as *mut _xmlEnumeration;
            (*v1).name = string::xml_strdup(c_str(b"small"));
            (*v1).next = v2;

            let elem = add_element_decl(
                dtd,
                elem_name,
                XML_ELEMENT_TYPE_EMPTY as c_int,
                ptr::null_mut(),
            );
            let attr = add_attribute_decl(
                dtd,
                elem,
                attr_name,
                ptr::null(),
                XML_ATTRIBUTE_ENUMERATION as c_int,
                XML_ATTRIBUTE_REQUIRED as c_int,
                ptr::null(),
                v1,
            );
            assert!(!attr.is_null());
            assert_eq!((*attr).atype, XML_ATTRIBUTE_ENUMERATION as c_int);

            free_dtd(dtd);
            allocator::xmlFreeImpl(doc as *mut c_void);
        }
    }

    /// Verify that adding an attribute to a NULL DTD returns NULL.
    ///
    /// # Safety
    ///
    /// - Passing a NULL `dtd` is allowed and is not dereferenced; the name
    ///   buffer must be NUL-terminated.
    #[test]
    fn test_add_attribute_null_dtd() {
        unsafe {
            let attr = add_attribute_decl(
                ptr::null_mut(),
                ptr::null_mut(),
                c_str(b"test"),
                ptr::null(),
                XML_ATTRIBUTE_CDATA as c_int,
                XML_ATTRIBUTE_IMPLIED as c_int,
                ptr::null(),
                ptr::null_mut(),
            );
            assert!(attr.is_null());
        }
    }

    /// Verify `copy_attribute_decl` deep-copies an attribute declaration.
    ///
    /// # Safety
    ///
    /// - `attr` must be a valid `_xmlAttribute` while copied; the copy is
    ///   freed with `free_attribute`, the DTD with `free_dtd`, and the doc
    ///   with `xmlFreeImpl`.
    #[test]
    fn test_copy_attribute() {
        unsafe {
            let (doc, dtd) = make_doc_and_dtd();
            let elem_name = c_str(b"elem");
            let attr_name = c_str(b"id");
            let default_val = c_str(b"default");

            let elem = add_element_decl(
                dtd,
                elem_name,
                XML_ELEMENT_TYPE_EMPTY as c_int,
                ptr::null_mut(),
            );
            let attr = add_attribute_decl(
                dtd,
                elem,
                attr_name,
                ptr::null(),
                XML_ATTRIBUTE_ID as c_int,
                XML_ATTRIBUTE_IMPLIED as c_int,
                default_val,
                ptr::null_mut(),
            );
            assert!(!attr.is_null());

            let copy = copy_attribute_decl(attr);
            assert!(!copy.is_null());
            assert_ne!(copy, attr);
            assert_eq!((*copy).atype, XML_ATTRIBUTE_ID as c_int);
            assert_eq!(string::xml_strcmp((*copy).name, attr_name), 0);

            free_attribute(copy);
            free_dtd(dtd);
            allocator::xmlFreeImpl(doc as *mut c_void);
        }
    }

    /// Verify that `free_attribute` accepts a NULL pointer.
    ///
    /// # Safety
    ///
    /// - NULL is allowed and is not dereferenced.
    #[test]
    fn test_free_attribute_null() {
        unsafe {
            free_attribute(ptr::null_mut()); // Should not crash
        }
    }

    // ── Content Model Validation Tests ──────────────────────────────────

    /// Verify that `valid_content_model` rejects a NULL model.
    ///
    /// # Safety
    ///
    /// - Passing a NULL `model` is allowed and is not dereferenced.
    #[test]
    fn test_valid_content_model_null() {
        unsafe {
            assert_eq!(
                valid_content_model(ptr::null_mut(), &[]),
                ContentModelResult::Invalid
            );
        }
    }

    /// Verify PCDATA content-model validation.
    ///
    /// # Safety
    ///
    /// - `cm` must be a valid `_xmlElementContent` while validated; the name
    ///   buffer must be NUL-terminated and alive for the call. The model is
    ///   freed with `free_content_model`.
    #[test]
    fn test_valid_content_model_pcdata() {
        unsafe {
            let cm = create_content_model(ptr::null(), XML_ELEMENT_CONTENT_PCDATA as c_int);
            assert!(!cm.is_null());

            // Empty content is valid for PCDATA
            assert_eq!(valid_content_model(cm, &[]), ContentModelResult::Valid);

            // Non-empty content is invalid for PCDATA
            let name = c_str(b"child");
            assert_eq!(
                valid_content_model(cm, &[name]),
                ContentModelResult::Invalid
            );

            free_content_model(cm);
        }
    }

    /// Verify single-element content-model validation.
    ///
    /// # Safety
    ///
    /// - `cm` must be a valid `_xmlElementContent` and the name buffers
    ///   NUL-terminated and alive for the calls; the model is freed with
    ///   `free_content_model`.
    #[test]
    fn test_valid_content_model_element() {
        unsafe {
            let child_name = c_str(b"child");
            let cm = create_content_model(child_name, XML_ELEMENT_CONTENT_ELEMENT as c_int);
            assert!(!cm.is_null());

            // Correct element
            assert_eq!(
                valid_content_model(cm, &[child_name]),
                ContentModelResult::Valid
            );

            // Wrong element
            let other = c_str(b"other");
            assert_eq!(
                valid_content_model(cm, &[other]),
                ContentModelResult::Invalid
            );

            // Too many elements
            assert_eq!(
                valid_content_model(cm, &[child_name, child_name]),
                ContentModelResult::Invalid
            );

            // Empty
            assert_eq!(valid_content_model(cm, &[]), ContentModelResult::Invalid);

            free_content_model(cm);
        }
    }

    /// Verify sequence content-model validation.
    ///
    /// # Safety
    ///
    /// - `c1`, `c2`, and `seq` must be valid, linked `_xmlElementContent`
    ///   structs and the name buffers NUL-terminated for the calls; the
    ///   sequence is freed with `free_content_model`.
    #[test]
    fn test_valid_content_model_seq() {
        unsafe {
            let a_name = c_str(b"a");
            let b_name = c_str(b"b");

            let c1 = create_content_model(a_name, XML_ELEMENT_CONTENT_ELEMENT as c_int);
            let c2 = create_content_model(b_name, XML_ELEMENT_CONTENT_ELEMENT as c_int);
            let seq = create_content_model(ptr::null(), XML_ELEMENT_CONTENT_SEQ as c_int);
            (*seq).c1 = c1;
            (*seq).c2 = c2;
            (*c1).parent = seq;
            (*c2).parent = seq;

            // Correct sequence
            assert_eq!(
                valid_content_model(seq, &[a_name, b_name]),
                ContentModelResult::Valid
            );

            // Wrong order
            assert_eq!(
                valid_content_model(seq, &[b_name, a_name]),
                ContentModelResult::Invalid
            );

            // Missing element
            assert_eq!(
                valid_content_model(seq, &[a_name]),
                ContentModelResult::Invalid
            );

            free_content_model(seq);
        }
    }

    /// Verify choice content-model validation.
    ///
    /// # Safety
    ///
    /// - `c1`, `c2`, and `choice` must be valid, linked `_xmlElementContent`
    ///   structs and the name buffers NUL-terminated for the calls; the
    ///   choice is freed with `free_content_model`.
    #[test]
    fn test_valid_content_model_or() {
        unsafe {
            let a_name = c_str(b"a");
            let b_name = c_str(b"b");

            let c1 = create_content_model(a_name, XML_ELEMENT_CONTENT_ELEMENT as c_int);
            let c2 = create_content_model(b_name, XML_ELEMENT_CONTENT_ELEMENT as c_int);
            let choice = create_content_model(ptr::null(), XML_ELEMENT_CONTENT_OR as c_int);
            (*choice).c1 = c1;
            (*choice).c2 = c2;
            (*c1).parent = choice;
            (*c2).parent = choice;

            // First alternative
            assert_eq!(
                valid_content_model(choice, &[a_name]),
                ContentModelResult::Valid
            );

            // Second alternative
            assert_eq!(
                valid_content_model(choice, &[b_name]),
                ContentModelResult::Valid
            );

            // Neither
            let other = c_str(b"other");
            assert_eq!(
                valid_content_model(choice, &[other]),
                ContentModelResult::Invalid
            );

            free_content_model(choice);
        }
    }

    /// Verify optional-occurrence content-model validation.
    ///
    /// # Safety
    ///
    /// - `c1` must be a valid `_xmlElementContent` and the name buffer
    ///   NUL-terminated for the calls; the model is freed with
    ///   `free_content_model`.
    #[test]
    fn test_valid_content_model_optional() {
        unsafe {
            let a_name = c_str(b"a");

            let c1 = create_content_model(a_name, XML_ELEMENT_CONTENT_ELEMENT as c_int);
            (*c1).ocur = XML_ELEMENT_CONTENT_OPT as c_int;

            // Empty is valid for optional
            assert_eq!(valid_content_model(c1, &[]), ContentModelResult::Valid);

            // One is valid
            assert_eq!(
                valid_content_model(c1, &[a_name]),
                ContentModelResult::Valid
            );

            free_content_model(c1);
        }
    }

    /// Verify zero-or-more content-model validation.
    ///
    /// # Safety
    ///
    /// - `c1` must be a valid `_xmlElementContent` and the name buffer
    ///   NUL-terminated for the calls; the model is freed with
    ///   `free_content_model`.
    #[test]
    fn test_valid_content_model_zero_or_more() {
        unsafe {
            let a_name = c_str(b"a");

            let c1 = create_content_model(a_name, XML_ELEMENT_CONTENT_ELEMENT as c_int);
            (*c1).ocur = XML_ELEMENT_CONTENT_MULT as c_int;

            // Empty is valid
            assert_eq!(valid_content_model(c1, &[]), ContentModelResult::Valid);

            // One is valid
            assert_eq!(
                valid_content_model(c1, &[a_name]),
                ContentModelResult::Valid
            );

            // Multiple is valid
            assert_eq!(
                valid_content_model(c1, &[a_name, a_name, a_name]),
                ContentModelResult::Valid
            );

            free_content_model(c1);
        }
    }

    /// Verify one-or-more content-model validation.
    ///
    /// # Safety
    ///
    /// - `c1` must be a valid `_xmlElementContent` and the name buffer
    ///   NUL-terminated for the calls; the model is freed with
    ///   `free_content_model`.
    #[test]
    fn test_valid_content_model_one_or_more() {
        unsafe {
            let a_name = c_str(b"a");

            let c1 = create_content_model(a_name, XML_ELEMENT_CONTENT_ELEMENT as c_int);
            (*c1).ocur = XML_ELEMENT_CONTENT_PLUS as c_int;

            // Empty is invalid
            assert_eq!(valid_content_model(c1, &[]), ContentModelResult::Invalid);

            // One is valid
            assert_eq!(
                valid_content_model(c1, &[a_name]),
                ContentModelResult::Valid
            );

            // Multiple is valid
            assert_eq!(
                valid_content_model(c1, &[a_name, a_name]),
                ContentModelResult::Valid
            );

            free_content_model(c1);
        }
    }
}
