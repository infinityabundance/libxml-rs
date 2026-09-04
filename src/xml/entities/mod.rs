//! Entity handling (§24, §85 Phase 6).
//!
//! General entities, parameter entities, external entities, entity
//! substitution, entity references, security limits, recursive entities,
//! expansion limits.
//!
//! # Upstream contract
//!
//! Mirrors upstream entities.c and the entity paths of parser.c
//! (SRC-LIBXML2-2.15.0, oracle tree `oracle/historical/src/libxml2-2.15.0/`):
//! xmlAddEntity, xmlGetEntity, xmlGetPredefinedEntity, xmlNewReference,
//! entity content caching and the expansion limits. Parity target: the system
//! libxml2 2.15.3 oracle.
//!
//! # Conceptual behavior
//!
//! General entities, parameter entities, external entities, entity
//! substitution, entity references, security limits, recursive entities and
//! expansion limits. The entity model parses a referenced entity content once
//! into ent->children (XML_ENT_PARSED / XML_ENT_EXPANDING flags) and reuses
//! it — structural re-expansion is impossible by construction.
//!
//! # Ownership & safety invariants
//!
//! Ownership: entity declarations are owned by the DTD hash tables
//! (entities/pentities); ent->children nodes are owned by the declaration and
//! freed with the DTD; entity-ref nodes share content with the entity
//! (xmlNewReference semantics) and must never be freed separately (R-000164).
//! SAFETY: the expansion guards make entity processing loop-free.
//!
//! # Historical quirks & epochs
//!
//! Security epochs: CVE-2014-3660 (billion laughs) fix be2a7eda and its
//! regression fix 72a46a51 (SEC-0006) bounded expansion; CVE-2013-2877
//! (SEC-0004) added loop detection; the 2015 batch (SEC-0008: 69030714,
//! f1063fdb) fixed entity-boundary bugs; the recursion-depth increments came
//! from commit 8f30bdff (2016, SEC-0009). XML_ENTITY_CONTENT_DEPTH_MAX = 32
//! and XML_ENTITY_CONTENT_EXPANSION_MAX = 1,000,000 follow upstream.
//!
//! # Deliberate oddities
//!
//! Deliberate oddities: the amplification guard fires unconditionally with no
//! XML_PARSE_HUGE bypass; unloadable external entities fail silently
//! (xmlCtxtParseEntity) rather than raising undeclared-entity errors;
//! predefined entities (amp/lt/gt/quot/apos) substitute unconditionally
//! regardless of XML_PARSE_NOENT.
//!
//! # Proving courts
//!
//! PARSER-ENTITY-* court family, SECURITY-LIMITS probe (amplification sweep
//! L4..L9 x 10 matches the oracle on every boundary), TREE-001, CLI-XMLLINT-
//! 0033/0034 and `cargo test --lib`.
//!
//! # Tempting simplifications that would break parity
//!
//! Not caching entity content would re-parse per reference (exponential
//! blowup — the vulnerable pre-CVE behavior); skipping the XML_ENT_EXPANDING
//! re-entry check would loop on a self-referential entity (CVE-2013-2877). Do
//! not drop the silent-failure path for unloadable external entities — the
//! NONET oracle behavior depends on it (SD-004).
//!
//! # Safety
//!
//! - The module-level `unsafe impl Sync for SyncPtr` is only instantiated in
//!   this module as the `static PREDEFINED_ENTITIES` array of `_xmlEntity`
//!   values. That static and the byte-string literals it points to are
//!   immutable after initialization and are only ever read (never mutated or
//!   freed), so sharing references to it across threads cannot cause data
//!   races or use-after-free.

use core::ffi::c_void;
use core::mem::size_of;
use core::ptr;
use std::os::raw::{c_char, c_int, c_uint, c_ulong};

use crate::abi::allocator;
use crate::abi::structs::*;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::xmlEntityType::*;
use crate::abi::types::*;
use crate::xml::hash;
use crate::xml::string;

// ═══════════════════════════════════════════════════════════════════════════════
// Constants
// ═══════════════════════════════════════════════════════════════════════════════

/// Maximum entity recursion depth.
///
/// # UPSTREAM-PARITY
///
/// libxml2 uses XML_ENTITY_CONTENT_DEPTH_MAX (default 32).
pub const XML_ENTITY_CONTENT_DEPTH_MAX: c_int = 32;

/// Maximum entity expansion size (in bytes).
///
/// # UPSTREAM-PARITY
///
/// libxml2 uses XML_ENTITY_CONTENT_EXPANSION_MAX (default 1,000,000).
pub const XML_ENTITY_CONTENT_EXPANSION_MAX: c_uint = 1_000_000;

// ═══════════════════════════════════════════════════════════════════════════════
// Entity Declaration Functions
// ═══════════════════════════════════════════════════════════════════════════════

/// Add an entity declaration to a DTD.
///
/// # UPSTREAM-PARITY
///
/// Add an entity declaration to a DTD's hash table (DTD-level helper).
///
/// # UPSTREAM-PARITY
///
/// This is the historical tree.c `xmlAddEntity` core (dtd-level add) and
/// backs the candidate's `xmlAddDocEntity`/`xmlAddDtdEntity` and the parser
/// entity-declaration path. The exported `xmlAddEntity` (entities.h, 2.15)
/// is the document-level `add_entity_doc` with the int/error-code contract
/// (R-000176).
///
/// Adds an entity to the appropriate hash table in the DTD based on the
/// entity type (general entities go to `entities`, parameter entities to
/// `pentities`).
///
/// If an entity with the same name already exists, the existing entity
/// is returned and no new entity is created.
///
/// # SAFETY
///
/// - `dtd` must be a valid pointer to an _xmlDtd, or NULL.
/// - `name` must be a valid null-terminated string.
/// - `ExternalID`, `SystemID`, `content` may be NULL.
pub unsafe fn add_entity(
    dtd: *mut _xmlDtd,
    name: *const xmlChar,
    etype: c_int,
    ExternalID: *const xmlChar,
    SystemID: *const xmlChar,
    content: *const xmlChar,
) -> *mut _xmlEntity {
    unsafe { add_entity_impl(dtd, name, etype, ExternalID, SystemID, content, ptr::null()) }
}

/// Like `add_entity`, but also records the ORIGINAL declaration text on the
/// entity (`orig`). UPSTREAM-PARITY (parser.c xmlParseEntityDecl): internal
/// entities parsed from a DTD keep the raw source text of their value in
/// `ent->orig`, and xmlsave.c `xmlBufDumpEntityDecl` prints `orig` verbatim
/// (only `"` escaped) when present — falling back to the content path (which
/// additionally escapes `%` to `&#x25;`) only for entities without `orig`
/// (php bug67081: `<!ENTITY % attrs "%coreattrs;">` must round-trip with a
/// raw `%`).
///
/// # SAFETY
///
/// - Same as `add_entity`; `orig` may be NULL.
pub unsafe fn add_entity_with_orig(
    dtd: *mut _xmlDtd,
    name: *const xmlChar,
    etype: c_int,
    ExternalID: *const xmlChar,
    SystemID: *const xmlChar,
    content: *const xmlChar,
    orig: *const xmlChar,
) -> *mut _xmlEntity {
    unsafe { add_entity_impl(dtd, name, etype, ExternalID, SystemID, content, orig) }
}

#[allow(clippy::too_many_arguments)]
unsafe fn add_entity_impl(
    dtd: *mut _xmlDtd,
    name: *const xmlChar,
    etype: c_int,
    ExternalID: *const xmlChar,
    SystemID: *const xmlChar,
    content: *const xmlChar,
    orig: *const xmlChar,
) -> *mut _xmlEntity {
    if dtd.is_null() || name.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        let d = &*dtd;

        // Determine which hash table to use
        let is_param = is_parameter_entity(etype);
        // UPSTREAM-PARITY (entities.c xmlAddEntity): the table is created
        // lazily on first use.
        if is_param {
            if (*dtd).pentities.is_null() {
                (*dtd).pentities = hash::hash_create(8) as *mut c_void;
            }
        } else if (*dtd).entities.is_null() {
            (*dtd).entities = hash::hash_create(8) as *mut c_void;
        }
        let hash_table = if is_param {
            d.pentities as *mut hash::HashTable
        } else {
            d.entities as *mut hash::HashTable
        };

        // Check if entity already exists
        let existing = hash::hash_lookup(hash_table, name);
        if !existing.is_null() {
            return existing as *mut _xmlEntity;
        }

        // SAFETY: Allocate zero-initialized memory for the entity.
        let entity = allocator::xmlMallocZero(size_of::<_xmlEntity>() as usize) as *mut _xmlEntity;
        if entity.is_null() {
            return ptr::null_mut();
        }

        (*entity).type_ = XML_ENTITY_DECL as c_int;
        (*entity).name = string::xml_strdup(name);
        (*entity).etype = etype;
        (*entity).ExternalID = string::xml_strdup(ExternalID);
        (*entity).SystemID = string::xml_strdup(SystemID);
        (*entity).content = string::xml_strdup(content);
        (*entity).orig = if orig.is_null() {
            ptr::null_mut()
        } else {
            string::xml_strdup(orig)
        };
        (*entity).length = if content.is_null() {
            0
        } else {
            string::xml_strlen(content) as c_int
        };
        (*entity).flags = 0;
        (*entity).expandedSize = 0;
        (*entity).URI = ptr::null_mut();
        (*entity).parent = dtd;
        (*entity).doc = d.doc;
        (*entity).nexte = ptr::null_mut();
        (*entity).owner = 0;

        // Add to hash table
        let ret = hash::hash_add_entry(hash_table, name, entity as *mut c_void);
        if ret != 0 {
            // Failed to add
            free_entity_internal(entity, false);
            return ptr::null_mut();
        }

        // UPSTREAM-PARITY (entities.c xmlAddDocEntity/xmlAddDtdEntity
        // "Link it to the DTD"): the entity decl is a child node of the DTD.
        if (*dtd).last.is_null() {
            (*dtd).children = entity as *mut _xmlNode;
            (*dtd).last = entity as *mut _xmlNode;
        } else {
            (*(*dtd).last).next = entity as *mut _xmlNode;
            (*entity).prev = (*dtd).last;
            (*dtd).last = entity as *mut _xmlNode;
        }

        entity
    }
}

/// Add an entity to a document's DTD (upstream entities.c `xmlAddEntity`, 2.15).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlAddEntity(xmlDoc *doc, int extSubset, const xmlChar *name, int type,
///                  const xmlChar *publicId, const xmlChar *systemId,
///                  const xmlChar *content, xmlEntity **out);
/// ```
///
/// Mirrors the upstream control flow exactly (entities.c 2.15.0):
///
/// - `*out` is set to NULL on entry (upstream line: `if (out != NULL)
///   *out = NULL;`);
/// - NULL `doc`/`name` → `XML_ERR_ARGUMENT`;
/// - missing DTD (extSubset selects the external subset, else internal) →
///   `XML_DTD_NO_DTD`;
/// - a predefined entity (lt/gt/amp/apos/quot) may only be redeclared with
///   the exact replacement-content form upstream accepts (XML 1.0 §4.6),
///   otherwise → `XML_ERR_REDECL_PREDEF_ENTITY`;
/// - an unknown entity type → `XML_ERR_ARGUMENT`;
/// - allocation failure → `XML_ERR_NO_MEMORY`;
/// - a name already present in the selected table → `XML_WAR_ENTITY_REDEFINED`
///   (the freshly created entity is freed);
/// - success → `*out = entity` and returns 0.
///
/// The entity is created without going through the exported hooks, exactly
/// as upstream `xmlCreateEntity` allocates directly with `xmlMalloc`.
///
/// # SAFETY
///
/// - `doc` must be a valid `_xmlDoc` pointer (or NULL), `name` a valid
///   NUL-terminated string, `publicId`/`systemId`/`content` NUL-terminated
///   or NULL, and `out` a writable `xmlEntity*` slot or NULL.
#[allow(clippy::too_many_arguments)]
pub unsafe fn add_entity_doc(
    doc: *mut _xmlDoc,
    ext_subset: c_int,
    name: *const xmlChar,
    etype: c_int,
    public_id: *const xmlChar,
    system_id: *const xmlChar,
    content: *const xmlChar,
    out: *mut *mut _xmlEntity,
) -> c_int {
    use crate::abi::types::{
        XML_DTD_NO_DTD, XML_ERR_ARGUMENT, XML_ERR_NO_MEMORY, XML_ERR_REDECL_PREDEF_ENTITY,
        XML_WAR_ENTITY_REDEFINED,
    };
    unsafe {
        if !out.is_null() {
            *out = ptr::null_mut();
        }
        if doc.is_null() || name.is_null() {
            return XML_ERR_ARGUMENT;
        }
        let dtd = if ext_subset != 0 {
            (*doc).extSubset
        } else {
            (*doc).intSubset
        };
        if dtd.is_null() {
            return XML_DTD_NO_DTD;
        }

        // Select the table by entity type (upstream switch), creating it
        // lazily with the candidate's dictionary-backed hash (hash_create).
        let general = etype == XML_INTERNAL_GENERAL_ENTITY as c_int
            || etype == XML_EXTERNAL_GENERAL_PARSED_ENTITY as c_int
            || etype == XML_EXTERNAL_GENERAL_UNPARSED_ENTITY as c_int;
        let parameter = etype == XML_INTERNAL_PARAMETER_ENTITY as c_int
            || etype == XML_EXTERNAL_PARAMETER_ENTITY as c_int;
        let table_field: *mut hash::HashTable = if general {
            // XML 1.0 §4.6 predefined-entity redeclaration check.
            let predef = lookup_predefined_entity(name);
            if !predef.is_null() {
                let mut valid = 0;
                if etype == XML_INTERNAL_GENERAL_ENTITY as c_int && !content.is_null() {
                    let c = *(*predef).content;
                    if (*content == c)
                        && (*(content.add(1)) == 0)
                        && (c == b'>' || c == b'\'' || c == b'"')
                    {
                        valid = 1;
                    } else if (*content == b'&') && (*(content.add(1)) == b'#') {
                        if *(content.add(2)) == b'x' as xmlChar {
                            let hex = b"0123456789ABCDEF";
                            let mut ref_: [u8; 3] = [0, 0, b';'];
                            ref_[0] = hex[(c / 16 % 16) as usize];
                            ref_[1] = hex[(c % 16) as usize];
                            if libc::strcasecmp(
                                content.add(3) as *const c_char,
                                ref_.as_ptr() as *const c_char,
                            ) == 0
                            {
                                valid = 1;
                            }
                        } else {
                            let mut ref_: [u8; 3] = [0, 0, b';'];
                            ref_[0] = b'0' + c / 10 % 10;
                            ref_[1] = b'0' + c % 10;
                            if libc::strcmp(
                                content.add(2) as *const c_char,
                                ref_.as_ptr() as *const c_char,
                            ) == 0
                            {
                                valid = 1;
                            }
                        }
                    }
                }
                if valid == 0 {
                    return XML_ERR_REDECL_PREDEF_ENTITY;
                }
            }
            if (*dtd).entities.is_null() {
                (*dtd).entities = hash::hash_create(8) as *mut c_void;
            }
            (*dtd).entities as *mut hash::HashTable
        } else if parameter {
            if (*dtd).pentities.is_null() {
                (*dtd).pentities = hash::hash_create(8) as *mut c_void;
            }
            (*dtd).pentities as *mut hash::HashTable
        } else {
            return XML_ERR_ARGUMENT;
        };

        // Upstream xmlCreateEntity: allocate zeroed, fill fields, strdup.
        let entity = allocator::xmlMallocZero(size_of::<_xmlEntity>() as usize) as *mut _xmlEntity;
        if entity.is_null() {
            return XML_ERR_NO_MEMORY;
        }
        (*entity).doc = doc;
        (*entity).type_ = XML_ENTITY_DECL as c_int;
        (*entity).etype = etype;
        (*entity).name = string::xml_strdup(name);
        if (*entity).name.is_null() {
            free_entity_internal(entity, false);
            return XML_ERR_NO_MEMORY;
        }
        if !public_id.is_null() {
            (*entity).ExternalID = string::xml_strdup(public_id);
            if (*entity).ExternalID.is_null() {
                free_entity_internal(entity, false);
                return XML_ERR_NO_MEMORY;
            }
        }
        if !system_id.is_null() {
            (*entity).SystemID = string::xml_strdup(system_id);
            if (*entity).SystemID.is_null() {
                free_entity_internal(entity, false);
                return XML_ERR_NO_MEMORY;
            }
        }
        if !content.is_null() {
            (*entity).length = string::xml_strlen(content) as c_int;
            (*entity).content = string::xml_strdup(content);
            if (*entity).content.is_null() {
                free_entity_internal(entity, false);
                return XML_ERR_NO_MEMORY;
            }
        } else {
            (*entity).length = 0;
            (*entity).content = ptr::null_mut();
        }
        (*entity).URI = ptr::null();
        (*entity).orig = ptr::null_mut();
        (*entity).flags = 0;
        (*entity).expandedSize = 0;
        (*entity).nexte = ptr::null_mut();
        (*entity).owner = 0;

        // Upstream xmlHashAdd on the selected table: a name already present
        // frees the fresh entity and reports the redefinition warning; any
        // other add failure is treated as OOM (upstream res < 0).
        let existing = hash::hash_lookup(table_field, name);
        if !existing.is_null() {
            free_entity_internal(entity, false);
            return XML_WAR_ENTITY_REDEFINED;
        }
        let res = hash::hash_add_entry(table_field, name, entity as *mut c_void);
        if res != 0 {
            free_entity_internal(entity, false);
            return XML_ERR_NO_MEMORY;
        }

        // Link it to the DTD (upstream "Link it to the DTD" block).
        (*entity).parent = dtd;
        (*entity).doc = (*dtd).doc;
        if (*dtd).last.is_null() {
            (*dtd).children = entity as *mut _xmlNode;
            (*dtd).last = entity as *mut _xmlNode;
        } else {
            (*(*dtd).last).next = entity as *mut _xmlNode;
            (*entity).prev = (*dtd).last;
            (*dtd).last = entity as *mut _xmlNode;
        }

        if !out.is_null() {
            *out = entity;
        }
        0
    }
}

/// Get a general entity by name.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlEntityPtr xmlGetEntity(xmlDocPtr doc, const xmlChar *name);
/// ```
///
/// Searches the document's DTD for a general entity with the given name.
/// Checks both internal and external subsets.
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an _xmlDoc, or NULL.
/// - `name` must be a valid null-terminated string.
pub unsafe fn get_entity(doc: *mut _xmlDoc, name: *const xmlChar) -> *mut _xmlEntity {
    // UPSTREAM-PARITY (entities.c xmlGetDocEntity): a NULL document resolves
    // through the PREDEFINED entities only — a doc-less
    // `new DOMEntityReference("amp")` (xmlNewReference with doc NULL) still
    // binds the predefined declaration as its children/last/content, and
    // php's `dom_entity_reference_fetch_and_sync_declaration` re-syncs the
    // same way via xmlGetDocEntity(NULL, ...).
    if doc.is_null() {
        return unsafe { crate::abi::exports_misc::xmlGetPredefinedEntity(name) };
    }
    if name.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        let d = &*doc;

        // Check internal subset
        if !d.intSubset.is_null() {
            let entities = (*d.intSubset).entities as *mut hash::HashTable;
            let found = hash::hash_lookup(entities, name);
            if !found.is_null() {
                return found as *mut _xmlEntity;
            }
        }

        // Check external subset
        if !d.extSubset.is_null() {
            let entities = (*d.extSubset).entities as *mut hash::HashTable;
            let found = hash::hash_lookup(entities, name);
            if !found.is_null() {
                return found as *mut _xmlEntity;
            }
        }

        // UPSTREAM-PARITY (entities.c xmlGetDocEntity): an unregistered name
        // still resolves to the predefined entity (amp/lt/gt/quot/apos).
        unsafe { crate::abi::exports_misc::xmlGetPredefinedEntity(name) }
    }
}

/// Look up an entity in a DTD's entity hash table (upstream
/// `xmlGetDtdEntity` core).
///
/// # SAFETY
///
/// - `dtd` must be a valid DTD pointer or NULL.
/// - `name` must be a valid null-terminated string.
pub unsafe fn get_entity_from_dtd(dtd: *mut _xmlDtd, name: *const xmlChar) -> *mut _xmlEntity {
    if dtd.is_null() || name.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let entities = (*dtd).entities as *mut hash::HashTable;
        let found = hash::hash_lookup(entities, name);
        if found.is_null() {
            ptr::null_mut()
        } else {
            found as *mut _xmlEntity
        }
    }
}

/// Get a parameter entity by name.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlEntityPtr xmlGetParameterEntity(xmlDocPtr doc, const xmlChar *name);
/// ```
///
/// Searches the document's DTD for a parameter entity with the given name.
///
/// # SAFETY
///
/// - `doc` must be a valid pointer to an _xmlDoc, or NULL.
/// - `name` must be a valid null-terminated string.
pub unsafe fn get_parameter_entity(doc: *mut _xmlDoc, name: *const xmlChar) -> *mut _xmlEntity {
    if doc.is_null() || name.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        let d = &*doc;

        // Check internal subset
        if !d.intSubset.is_null() {
            let pentities = (*d.intSubset).pentities as *mut hash::HashTable;
            let found = hash::hash_lookup(pentities, name);
            if !found.is_null() {
                return found as *mut _xmlEntity;
            }
        }

        // Check external subset
        if !d.extSubset.is_null() {
            let pentities = (*d.extSubset).pentities as *mut hash::HashTable;
            let found = hash::hash_lookup(pentities, name);
            if !found.is_null() {
                return found as *mut _xmlEntity;
            }
        }

        ptr::null_mut()
    }
}

/// Deep copy an entity declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlEntityPtr xmlCopyEntity(xmlEntityPtr entity);
/// ```
///
/// # SAFETY
///
/// - `entity` must be a valid pointer to an _xmlEntity, or NULL.
pub unsafe fn copy_entity(entity: *mut _xmlEntity) -> *mut _xmlEntity {
    if entity.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        let e = &*entity;

        // SAFETY: Allocate zero-initialized memory for the copy.
        let copy = allocator::xmlMallocZero(size_of::<_xmlEntity>() as usize) as *mut _xmlEntity;
        if copy.is_null() {
            return ptr::null_mut();
        }

        (*copy).type_ = XML_ENTITY_DECL as c_int;
        (*copy).name = string::xml_strdup(e.name);
        (*copy).etype = e.etype;
        (*copy).ExternalID = string::xml_strdup(e.ExternalID);
        (*copy).SystemID = string::xml_strdup(e.SystemID);
        (*copy).content = string::xml_strdup(e.content);
        (*copy).orig = string::xml_strdup(e.orig);
        (*copy).length = e.length;
        (*copy).flags = e.flags;
        (*copy).expandedSize = e.expandedSize;
        (*copy).URI = string::xml_strdup(e.URI);
        (*copy).parent = e.parent;
        (*copy).doc = e.doc;
        (*copy).nexte = ptr::null_mut();
        (*copy).owner = e.owner;

        copy
    }
}

/// Free an entity declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeEntity(xmlEntityPtr entity);
/// ```
///
/// # SAFETY
///
/// - `entity` must be a valid pointer to an _xmlEntity, or NULL.
pub unsafe fn free_entity(entity: *mut _xmlEntity) {
    free_entity_internal(entity, true);
}

/// Internal entity free function.
///
/// If `free_children` is true, also frees the entity's children tree nodes.
/// This is separated because the hash deallocator should not free children
/// (they are owned by the document tree), but `xmlFreeEntity` from user code
/// should free everything.
///
/// # Safety
///
/// - `entity` must be NULL or a pointer to a heap-allocated `_xmlEntity` whose
///   `name`, `content`, `orig`, `ExternalID`, `SystemID`, and `URI` fields are
///   each either NULL or pointers to allocator-owned allocations. The call
///   frees every non-NULL field, then frees the entity itself, so afterwards
///   neither the entity nor any of its fields may be dereferenced or freed
///   again by the caller.
unsafe fn free_entity_internal(entity: *mut _xmlEntity, free_children: bool) {
    if entity.is_null() {
        return;
    }

    unsafe {
        let e = &*entity;

        if !e.name.is_null() {
            allocator::xmlFreeImpl(e.name as *mut c_void);
        }
        if !e.content.is_null() {
            allocator::xmlFreeImpl(e.content as *mut c_void);
        }
        if !e.orig.is_null() {
            allocator::xmlFreeImpl(e.orig as *mut c_void);
        }
        if !e.ExternalID.is_null() {
            allocator::xmlFreeImpl(e.ExternalID as *mut c_void);
        }
        if !e.SystemID.is_null() {
            allocator::xmlFreeImpl(e.SystemID as *mut c_void);
        }
        if !e.URI.is_null() {
            allocator::xmlFreeImpl(e.URI as *mut c_void);
        }

        // Free children tree nodes if requested. The materialized replacement
        // tree (upstream xmlNodeParseAttValue fills `ent->children` from
        // `ent->content` on first reference) is owned by the declaration and
        // freed with it (text/cdata nodes and entity-REF nodes; the REFs' own
        // `children` point at declarations, which free_node never descends
        // into).
        if free_children && !e.children.is_null() {
            crate::xml::tree::free_node_list(e.children);
        }

        allocator::xmlFreeImpl(entity as *mut c_void);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Entity Type Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Check if an entity type is a parameter entity.
#[inline]
pub const fn is_parameter_entity(etype: c_int) -> bool {
    etype == XML_INTERNAL_PARAMETER_ENTITY as c_int
        || etype == XML_EXTERNAL_PARAMETER_ENTITY as c_int
}

/// Check if an entity type is an external entity.
#[inline]
pub const fn is_external_entity(etype: c_int) -> bool {
    etype == XML_EXTERNAL_GENERAL_PARSED_ENTITY as c_int
        || etype == XML_EXTERNAL_GENERAL_UNPARSED_ENTITY as c_int
        || etype == XML_EXTERNAL_PARAMETER_ENTITY as c_int
}

/// Check if an entity type is a predefined entity.
#[inline]
pub const fn is_predefined_entity(etype: c_int) -> bool {
    etype == XML_INTERNAL_PREDEFINED_ENTITY as c_int
}

// ═══════════════════════════════════════════════════════════════════════════════
// Entity Substitution & Encoding
// ═══════════════════════════════════════════════════════════════════════════════

/// Encode special XML characters in a string for output.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlEncodeEntitiesReentrant(xmlDocPtr doc, const xmlChar *input);
/// ```
///
/// Encodes `<`, `>`, `&`, `"`, `'` as their corresponding XML entities.
/// The returned string must be freed with `xmlFree`.
///
/// # SAFETY
///
/// - `input` must be a valid null-terminated xmlChar string, or NULL.
pub unsafe fn encode_entities_reentrant(_doc: *mut _xmlDoc, input: *const xmlChar) -> *mut xmlChar {
    if input.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        // First pass: calculate output size
        let len = string::xml_strlen(input);
        let mut out_len: usize = 0;

        for i in 0..len {
            match *input.add(i) {
                b'<' => out_len += 4,  // &lt;
                b'>' => out_len += 4,  // &gt;
                b'&' => out_len += 5,  // &amp;
                b'"' => out_len += 6,  // &quot;
                b'\'' => out_len += 6, // &apos;
                _c => out_len += 1,
            }
        }

        // Allocate output buffer
        let output = allocator::xmlMallocImpl(out_len + 1) as *mut xmlChar;
        if output.is_null() {
            return ptr::null_mut();
        }

        // Second pass: encode
        let mut j: usize = 0;
        for i in 0..len {
            let c = *input.add(i);
            match c {
                b'<' => {
                    ptr::copy_nonoverlapping(b"&lt;" as *const u8, output.add(j), 4);
                    j += 4;
                }
                b'>' => {
                    ptr::copy_nonoverlapping(b"&gt;" as *const u8, output.add(j), 4);
                    j += 4;
                }
                b'&' => {
                    ptr::copy_nonoverlapping(b"&amp;" as *const u8, output.add(j), 5);
                    j += 5;
                }
                b'"' => {
                    ptr::copy_nonoverlapping(b"&quot;" as *const u8, output.add(j), 6);
                    j += 6;
                }
                b'\'' => {
                    ptr::copy_nonoverlapping(b"&apos;" as *const u8, output.add(j), 6);
                    j += 6;
                }
                _ => {
                    *output.add(j) = c;
                    j += 1;
                }
            }
        }

        *output.add(out_len) = 0; // null-terminate
        output
    }
}

/// Decode entity references in a string.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlStringDecodeEntities(xmlDocPtr doc, const xmlChar *input,
///                                  int what, int end, int end2, int end3);
/// ```
///
/// Replaces entity references (`&name;`) with their content from the
/// document's entity declarations. Also handles numeric character
/// references (`&#NNN;` and `&#xHHH;`).
///
/// The `what` parameter specifies which entities to substitute:
/// - 0: substitute all
/// - 1: substitute only predefined
/// - 2: substitute only general
///
/// `end`, `end2`, `end3` specify terminating characters (0 if none).
///
/// Returns the decoded string (must be freed with `xmlFree`), or NULL on error.
///
/// # SAFETY
///
/// - `doc` may be NULL (no entity lookup, only numeric refs).
/// - `input` must be a valid null-terminated xmlChar string, or NULL.
pub unsafe fn string_decode_entities(
    doc: *mut _xmlDoc,
    input: *const xmlChar,
    what: c_int,
    end: xmlChar,
    end2: xmlChar,
    end3: xmlChar,
) -> *mut xmlChar {
    if input.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        let len = string::xml_strlen(input);
        if len == 0 {
            // Return empty string
            let empty = allocator::xmlMallocImpl(1) as *mut xmlChar;
            if !empty.is_null() {
                *empty = 0;
            }
            return empty;
        }

        // Allocate a generous output buffer (input length + expansion)
        let max_out = len * 4 + 1; // Allow for some expansion
        let output = allocator::xmlMallocImpl(max_out) as *mut xmlChar;
        if output.is_null() {
            return ptr::null_mut();
        }

        let mut out_pos: usize = 0;
        let mut i: usize = 0;

        while i < len {
            let c = *input.add(i);

            // Check for terminating characters
            if (end != 0 && c == end) || (end2 != 0 && c == end2) || (end3 != 0 && c == end3) {
                break;
            }

            if c == b'&' {
                // Entity reference
                i += 1;

                // Check for numeric character reference: &#NNN; or &#xHHH;
                if i < len && *input.add(i) == b'#' {
                    i += 1;
                    let (decoded_char, _consumed) = decode_numeric_ref(input, &mut i, len);
                    if decoded_char != 0 {
                        // Encode the decoded character as UTF-8
                        if decoded_char < 0x80 {
                            if out_pos < max_out - 1 {
                                *output.add(out_pos) = decoded_char as u8;
                                out_pos += 1;
                            }
                        } else if decoded_char < 0x800 {
                            if out_pos < max_out - 2 {
                                *output.add(out_pos) = 0xC0 | ((decoded_char >> 6) as u8);
                                *output.add(out_pos + 1) = 0x80 | ((decoded_char & 0x3F) as u8);
                                out_pos += 2;
                            }
                        } else {
                            if out_pos < max_out - 3 {
                                *output.add(out_pos) = 0xE0 | ((decoded_char >> 12) as u8);
                                *output.add(out_pos + 1) =
                                    0x80 | (((decoded_char >> 6) & 0x3F) as u8);
                                *output.add(out_pos + 2) = 0x80 | ((decoded_char & 0x3F) as u8);
                                out_pos += 3;
                            }
                        }
                    }
                    // decode_numeric_ref already advanced i past ';'
                    // The continue skips the i += 1 at the bottom of the loop
                    continue;
                }

                // General entity reference: &name;
                let entity_name_start = i;
                while i < len
                    && *input.add(i) != b';'
                    && *input.add(i) != b'&'
                    && *input.add(i) != 0
                {
                    i += 1;
                }

                if i < len && *input.add(i) == b';' {
                    // We have a complete entity reference
                    let name_len = i - entity_name_start;
                    if name_len > 0 {
                        // Create a null-terminated name
                        let name_buf = allocator::xmlMallocImpl(name_len + 1) as *mut xmlChar;
                        if !name_buf.is_null() {
                            ptr::copy_nonoverlapping(
                                input.add(entity_name_start),
                                name_buf,
                                name_len,
                            );
                            *name_buf.add(name_len) = 0;

                            // Try to find the entity
                            let mut entity: *mut _xmlEntity = ptr::null_mut();
                            if what != 1 {
                                // Not just predefined
                                if !doc.is_null() {
                                    entity = get_entity(doc, name_buf as *const xmlChar);
                                }
                            }

                            if entity.is_null() {
                                // Try predefined entities
                                entity = lookup_predefined_entity(name_buf as *const xmlChar);
                            }

                            if !entity.is_null() && !(*entity).content.is_null() {
                                // Copy entity content to output
                                let content = (*entity).content;
                                let content_len = string::xml_strlen(content);
                                for j in 0..content_len {
                                    if out_pos < max_out - 1 {
                                        *output.add(out_pos) = *content.add(j);
                                        out_pos += 1;
                                    }
                                }
                            } else {
                                // Entity not found — output the reference as-is
                                if out_pos < max_out - 2 {
                                    *output.add(out_pos) = b'&';
                                    out_pos += 1;
                                }
                                for j in 0..name_len {
                                    if out_pos < max_out - 2 {
                                        *output.add(out_pos) = *input.add(entity_name_start + j);
                                        out_pos += 1;
                                    }
                                }
                                if out_pos < max_out - 1 {
                                    *output.add(out_pos) = b';';
                                    out_pos += 1;
                                }
                            }

                            allocator::xmlFreeImpl(name_buf as *mut c_void);
                        }
                    }
                    // Skip past the semicolon
                    // i is already at the semicolon, loop increment will skip it
                } else {
                    // Malformed reference — output as-is
                    if out_pos < max_out - 1 {
                        *output.add(out_pos) = b'&';
                        out_pos += 1;
                    }
                    // Back up to include all characters we scanned
                    // The loop increment will advance i
                }
            } else {
                // Regular character
                if out_pos < max_out - 1 {
                    *output.add(out_pos) = c;
                    out_pos += 1;
                }
            }

            i += 1;
        }

        *output.add(out_pos) = 0; // null-terminate
        output
    }
}

/// Decode a numeric character reference (`&#NNN;` or `&#xHHH;`).
///
/// Returns the decoded character and the number of additional characters consumed.
///
/// # Safety
///
/// - `input` must be a valid pointer to a buffer of at least `len` readable
///   bytes; `pos` must be a valid `&mut usize` in `0..=len` — the read at
///   `input[pos]` is bounds-checked against `len` before every access.
const unsafe fn decode_numeric_ref(
    input: *const xmlChar,
    pos: &mut usize,
    len: usize,
) -> (u32, usize) {
    unsafe {
        let mut consumed: usize = 0;

        if *pos >= len {
            return (0, 0);
        }

        if *input.add(*pos) == b'x' || *input.add(*pos) == b'X' {
            // Hexadecimal: &#xHHH;
            *pos += 1;
            consumed += 1;

            let mut value: u32 = 0;
            while *pos < len {
                let c = *input.add(*pos);
                if c == b';' {
                    *pos += 1;
                    consumed += 1;
                    return (value, consumed);
                }
                let digit = match c {
                    b'0'..=b'9' => c - b'0',
                    b'a'..=b'f' => c - b'a' + 10,
                    b'A'..=b'F' => c - b'A' + 10,
                    _ => break,
                };
                value = value.wrapping_mul(16).wrapping_add(digit as u32);
                *pos += 1;
                consumed += 1;
            }
            (value, consumed)
        } else {
            // Decimal: &#NNN;
            let mut value: u32 = 0;
            while *pos < len {
                let c = *input.add(*pos);
                if c == b';' {
                    *pos += 1;
                    consumed += 1;
                    return (value, consumed);
                }
                if !c.is_ascii_digit() {
                    break;
                }
                value = value.wrapping_mul(10).wrapping_add((c - b'0') as u32);
                *pos += 1;
                consumed += 1;
            }
            (value, consumed)
        }
    }
}

/// A wrapper for static data that implements Sync for raw pointer types.
struct SyncPtr<T>(pub T);
unsafe impl<T> Sync for SyncPtr<T> {}

/// Look up a predefined XML entity by name.
///
/// Returns the entity pointer (to a static entity) or NULL.
///
/// # Safety
///
/// - `name` must be NULL or a pointer to a NUL-terminated `xmlChar` string
///   that stays readable for the duration of the call: the first byte is read
///   via `name.add(0)` and the full string is compared with
///   `string::xml_strcmp`. The returned pointer aliases the immutable static
///   `PREDEFINED_ENTITIES` array, lives for the whole program, and must not be
///   freed or mutated by the caller.
unsafe fn lookup_predefined_entity(name: *const xmlChar) -> *mut _xmlEntity {
    if name.is_null() {
        return ptr::null_mut();
    }

    // UPSTREAM-PARITY: Predefined entities are: lt, gt, amp, quot, apos
    static PREDEFINED_ENTITIES: SyncPtr<[_xmlEntity; 5]> = SyncPtr([
        _xmlEntity {
            _private: ptr::null_mut(),
            type_: XML_ENTITY_DECL as c_int,
            name: b"lt\0" as *const u8 as *const xmlChar,
            children: ptr::null_mut(),
            last: ptr::null_mut(),
            parent: ptr::null_mut(),
            next: ptr::null_mut(),
            prev: ptr::null_mut(),
            doc: ptr::null_mut(),
            orig: ptr::null_mut(),
            content: b"<\0" as *const u8 as *mut xmlChar,
            length: 1,
            etype: XML_INTERNAL_PREDEFINED_ENTITY as c_int,
            ExternalID: ptr::null(),
            SystemID: ptr::null(),
            nexte: ptr::null_mut(),
            URI: ptr::null(),
            owner: 0,
            flags: 0,
            expandedSize: 0,
        },
        _xmlEntity {
            _private: ptr::null_mut(),
            type_: XML_ENTITY_DECL as c_int,
            name: b"gt\0" as *const u8 as *const xmlChar,
            children: ptr::null_mut(),
            last: ptr::null_mut(),
            parent: ptr::null_mut(),
            next: ptr::null_mut(),
            prev: ptr::null_mut(),
            doc: ptr::null_mut(),
            orig: ptr::null_mut(),
            content: b">\0" as *const u8 as *mut xmlChar,
            length: 1,
            etype: XML_INTERNAL_PREDEFINED_ENTITY as c_int,
            ExternalID: ptr::null(),
            SystemID: ptr::null(),
            nexte: ptr::null_mut(),
            URI: ptr::null(),
            owner: 0,
            flags: 0,
            expandedSize: 0,
        },
        _xmlEntity {
            _private: ptr::null_mut(),
            type_: XML_ENTITY_DECL as c_int,
            name: b"amp\0" as *const u8 as *const xmlChar,
            children: ptr::null_mut(),
            last: ptr::null_mut(),
            parent: ptr::null_mut(),
            next: ptr::null_mut(),
            prev: ptr::null_mut(),
            doc: ptr::null_mut(),
            orig: ptr::null_mut(),
            content: b"&\0" as *const u8 as *mut xmlChar,
            length: 1,
            etype: XML_INTERNAL_PREDEFINED_ENTITY as c_int,
            ExternalID: ptr::null(),
            SystemID: ptr::null(),
            nexte: ptr::null_mut(),
            URI: ptr::null(),
            owner: 0,
            flags: 0,
            expandedSize: 0,
        },
        _xmlEntity {
            _private: ptr::null_mut(),
            type_: XML_ENTITY_DECL as c_int,
            name: b"quot\0" as *const u8 as *const xmlChar,
            children: ptr::null_mut(),
            last: ptr::null_mut(),
            parent: ptr::null_mut(),
            next: ptr::null_mut(),
            prev: ptr::null_mut(),
            doc: ptr::null_mut(),
            orig: ptr::null_mut(),
            content: b"\"\0" as *const u8 as *mut xmlChar,
            length: 1,
            etype: XML_INTERNAL_PREDEFINED_ENTITY as c_int,
            ExternalID: ptr::null(),
            SystemID: ptr::null(),
            nexte: ptr::null_mut(),
            URI: ptr::null(),
            owner: 0,
            flags: 0,
            expandedSize: 0,
        },
        _xmlEntity {
            _private: ptr::null_mut(),
            type_: XML_ENTITY_DECL as c_int,
            name: b"apos\0" as *const u8 as *const xmlChar,
            children: ptr::null_mut(),
            last: ptr::null_mut(),
            parent: ptr::null_mut(),
            next: ptr::null_mut(),
            prev: ptr::null_mut(),
            doc: ptr::null_mut(),
            orig: ptr::null_mut(),
            content: b"'\0" as *const u8 as *mut xmlChar,
            length: 1,
            etype: XML_INTERNAL_PREDEFINED_ENTITY as c_int,
            ExternalID: ptr::null(),
            SystemID: ptr::null(),
            nexte: ptr::null_mut(),
            URI: ptr::null(),
            owner: 0,
            flags: 0,
            expandedSize: 0,
        },
    ]);

    unsafe {
        // Match which entity
        let idx = match *name.add(0) as char {
            'l' => {
                if string::xml_strcmp(name, b"lt\0" as *const u8 as *const xmlChar) == 0 {
                    0
                } else {
                    return ptr::null_mut();
                }
            }
            'g' => {
                if string::xml_strcmp(name, b"gt\0" as *const u8 as *const xmlChar) == 0 {
                    1
                } else {
                    return ptr::null_mut();
                }
            }
            'a' => {
                if string::xml_strcmp(name, b"amp\0" as *const u8 as *const xmlChar) == 0 {
                    2
                } else if string::xml_strcmp(name, b"apos\0" as *const u8 as *const xmlChar) == 0 {
                    4
                } else {
                    return ptr::null_mut();
                }
            }
            'q' if string::xml_strcmp(name, b"quot\0" as *const u8 as *const xmlChar) == 0 => 3,
            _ => return ptr::null_mut(),
        };

        &PREDEFINED_ENTITIES.0[idx] as *const _xmlEntity as *mut _xmlEntity
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Entity Content Retrieval
// ═══════════════════════════════════════════════════════════════════════════════

/// Get the content of an entity as a string.
///
/// # SAFETY
///
/// - `entity` must be a valid pointer to an _xmlEntity, or NULL.
pub unsafe fn get_entity_content(entity: *mut _xmlEntity) -> *mut xmlChar {
    if entity.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        let e = &*entity;
        if e.content.is_null() {
            return ptr::null_mut();
        }
        string::xml_strdup(e.content)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Security/Limit Functions
// ═══════════════════════════════════════════════════════════════════════════════

/// Check if entity expansion exceeds limits.
///
/// Returns 0 if within limits, -1 if exceeded.
pub const fn check_entity_expansion_limit(expanded_size: c_ulong) -> c_int {
    if expanded_size > XML_ENTITY_CONTENT_EXPANSION_MAX as c_ulong {
        -1
    } else {
        0
    }
}

/// Check if entity recursion depth exceeds limits.
///
/// Returns 0 if within limits, -1 if exceeded.
pub const fn check_entity_recursion_depth(depth: c_int) -> c_int {
    if depth > XML_ENTITY_CONTENT_DEPTH_MAX {
        -1
    } else {
        0
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    use core::ffi::c_void;
    use core::ptr;

    unsafe fn c_str(s: &[u8]) -> *const xmlChar {
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
        let dtd = allocator::xmlMallocZero(size_of::<_xmlDtd>() as usize) as *mut _xmlDtd;
        assert!(!dtd.is_null());
        (*dtd).type_ = XML_DTD_NODE as c_int;
        (*dtd).parent = doc;
        (*dtd).doc = doc;
        (*dtd).entities = hash::hash_create(8) as *mut c_void;
        (*dtd).pentities = hash::hash_create(8) as *mut c_void;
        (*doc).intSubset = dtd;
        (doc, dtd)
    }

    // ── Entity Declaration Tests ────────────────────────────────────────

    #[test]
    /// Tests that a general entity can be added to a DTD and looked back up.
    ///
    /// # Safety
    ///
    /// - The test dereferences only raw pointers it allocates itself: `doc`
    ///   and `dtd` are zero-initialized by `make_doc_and_dtd` and stay alive
    ///   until the final `xmlFreeImpl` calls; `name` and `content` are
    ///   NUL-terminated buffers from `c_str`; `entity` is asserted non-NULL
    ///   before `(*entity)` is read. The hash tables are freed with
    ///   `entity_deallocator` before `dtd` and `doc` are freed, so no access
    ///   touches freed memory.
    fn test_add_entity_general() {
        unsafe {
            let (doc, dtd) = make_doc_and_dtd();
            let name = c_str(b"myEntity");
            let content = c_str(b"Hello, World!");

            let entity = add_entity(
                dtd,
                name,
                XML_INTERNAL_GENERAL_ENTITY as c_int,
                ptr::null(),
                ptr::null(),
                content,
            );
            assert!(!entity.is_null());
            assert_eq!((*entity).etype, XML_INTERNAL_GENERAL_ENTITY as c_int);
            assert_eq!((*entity).length, 13);

            // Lookup
            let found = get_entity(doc, name);
            assert_eq!(found, entity);

            // Cleanup
            // We need to free the hash tables manually since we don't have free_dtd available
            hash::hash_free(
                (*dtd).entities as *mut hash::HashTable,
                Some(entity_deallocator),
            );
            hash::hash_free(
                (*dtd).pentities as *mut hash::HashTable,
                Some(entity_deallocator),
            );
            allocator::xmlFreeImpl(dtd as *mut c_void);
            allocator::xmlFreeImpl(doc as *mut c_void);
        }
    }

    /// Deallocator passed to `hash::hash_free` to release entity payloads.
    ///
    /// # Safety
    ///
    /// - `payload` must be NULL or a pointer to a heap-allocated `_xmlEntity`
    ///   that has not yet been freed, because it is forwarded to
    ///   `free_entity`; `_name` is ignored by this function.
    unsafe extern "C" fn entity_deallocator(payload: *mut c_void, _name: *mut u8) {
        if !payload.is_null() {
            free_entity(payload as *mut _xmlEntity);
        }
    }

    #[test]
    /// Tests that a parameter entity is stored in the `pentities` table.
    ///
    /// # Safety
    ///
    /// - `doc` and `dtd` are zero-initialized by `make_doc_and_dtd` and remain
    ///   live until the trailing frees; `name` and `content` are NUL-terminated
    ///   `c_str` allocations valid for the whole test; the `entity` pointer is
    ///   asserted non-NULL before `(*entity)` is read, and the hash tables are
    ///   freed with `entity_deallocator` before `dtd` and `doc` are freed.
    fn test_add_entity_parameter() {
        unsafe {
            let (doc, dtd) = make_doc_and_dtd();
            let name = c_str(b"myParam");
            let content = c_str(b"parameter content");

            let entity = add_entity(
                dtd,
                name,
                XML_INTERNAL_PARAMETER_ENTITY as c_int,
                ptr::null(),
                ptr::null(),
                content,
            );
            assert!(!entity.is_null());
            assert_eq!((*entity).etype, XML_INTERNAL_PARAMETER_ENTITY as c_int);

            // Parameter entity should be in pentities
            let found = hash::hash_lookup((*dtd).pentities as *mut hash::HashTable, name);
            assert_eq!(found, entity as *mut c_void);

            // General entity lookup should NOT find it
            let not_found = hash::hash_lookup((*dtd).entities as *mut hash::HashTable, name);
            assert!(not_found.is_null());

            hash::hash_free(
                (*dtd).entities as *mut hash::HashTable,
                Some(entity_deallocator),
            );
            hash::hash_free(
                (*dtd).pentities as *mut hash::HashTable,
                Some(entity_deallocator),
            );
            allocator::xmlFreeImpl(dtd as *mut c_void);
            allocator::xmlFreeImpl(doc as *mut c_void);
        }
    }

    #[test]
    /// Tests that `add_entity` returns NULL when the DTD is NULL.
    ///
    /// # Safety
    ///
    /// - `add_entity` checks `dtd` for NULL and returns early without
    ///   dereferencing it, and the `name` argument is a NUL-terminated `c_str`
    ///   allocation that stays alive for the duration of the call.
    fn test_add_entity_null_dtd() {
        unsafe {
            let entity = add_entity(
                ptr::null_mut(),
                c_str(b"test"),
                XML_INTERNAL_GENERAL_ENTITY as c_int,
                ptr::null(),
                ptr::null(),
                ptr::null(),
            );
            assert!(entity.is_null());
        }
    }

    #[test]
    /// Tests that re-adding an entity with the same name reuses the existing
    /// declaration.
    ///
    /// # Safety
    ///
    /// - `doc` and `dtd` are zero-initialized by `make_doc_and_dtd` and outlive
    ///   every dereference; `name` and `content` are NUL-terminated `c_str`
    ///   allocations; `e1` is asserted non-NULL before `(*e1)` is read, and
    ///   the hash tables are freed with `entity_deallocator` before `dtd` and
    ///   `doc` are freed.
    fn test_add_entity_duplicate() {
        unsafe {
            let (doc, dtd) = make_doc_and_dtd();
            let name = c_str(b"dup");
            let content = c_str(b"original");

            let e1 = add_entity(
                dtd,
                name,
                XML_INTERNAL_GENERAL_ENTITY as c_int,
                ptr::null(),
                ptr::null(),
                content,
            );
            assert!(!e1.is_null());

            let e2 = add_entity(
                dtd,
                name,
                XML_INTERNAL_GENERAL_ENTITY as c_int,
                ptr::null(),
                ptr::null(),
                c_str(b"replacement"),
            );
            assert_eq!(e1, e2); // Same pointer

            hash::hash_free(
                (*dtd).entities as *mut hash::HashTable,
                Some(entity_deallocator),
            );
            hash::hash_free(
                (*dtd).pentities as *mut hash::HashTable,
                Some(entity_deallocator),
            );
            allocator::xmlFreeImpl(dtd as *mut c_void);
            allocator::xmlFreeImpl(doc as *mut c_void);
        }
    }

    #[test]
    /// Tests that an external parsed entity keeps its SystemID.
    ///
    /// # Safety
    ///
    /// - `doc` and `dtd` come from `make_doc_and_dtd` and stay live until the
    ///   trailing frees; `name` and `sysid` are NUL-terminated `c_str`
    ///   allocations; `entity` is asserted non-NULL before `(*entity).SystemID`
    ///   is passed to `string::xml_strcmp`, and the hash tables are freed with
    ///   `entity_deallocator` before `dtd` and `doc` are freed.
    fn test_add_entity_external() {
        unsafe {
            let (doc, dtd) = make_doc_and_dtd();
            let name = c_str(b"extEntity");
            let sysid = c_str(b"http://example.com/entity.xml");

            let entity = add_entity(
                dtd,
                name,
                XML_EXTERNAL_GENERAL_PARSED_ENTITY as c_int,
                ptr::null(),
                sysid,
                ptr::null(),
            );
            assert!(!entity.is_null());
            assert_eq!((*entity).etype, XML_EXTERNAL_GENERAL_PARSED_ENTITY as c_int);
            assert_eq!(string::xml_strcmp((*entity).SystemID, sysid), 0);

            hash::hash_free(
                (*dtd).entities as *mut hash::HashTable,
                Some(entity_deallocator),
            );
            hash::hash_free(
                (*dtd).pentities as *mut hash::HashTable,
                Some(entity_deallocator),
            );
            allocator::xmlFreeImpl(dtd as *mut c_void);
            allocator::xmlFreeImpl(doc as *mut c_void);
        }
    }

    #[test]
    /// Tests that `get_entity` returns NULL when the document is NULL.
    ///
    /// # Safety
    ///
    /// - `get_entity` checks `doc` for NULL before dereferencing it, and the
    ///   `name` argument is a NUL-terminated `c_str` allocation that stays
    ///   alive for the duration of the call.
    fn test_get_entity_null_doc() {
        unsafe {
            let found = get_entity(ptr::null_mut(), c_str(b"test"));
            assert!(found.is_null());
        }
    }

    #[test]
    /// Tests that `get_entity` returns NULL for an undeclared name.
    ///
    /// # Safety
    ///
    /// - `doc` and `dtd` are zero-initialized by `make_doc_and_dtd` and stay
    ///   live until the trailing frees; the lookup name is a NUL-terminated
    ///   `c_str` allocation; the hash tables are freed with
    ///   `entity_deallocator` before `dtd` and `doc` are freed.
    fn test_get_entity_not_found() {
        unsafe {
            let (doc, dtd) = make_doc_and_dtd();
            let found = get_entity(doc, c_str(b"nonexistent"));
            assert!(found.is_null());

            hash::hash_free(
                (*dtd).entities as *mut hash::HashTable,
                Some(entity_deallocator),
            );
            hash::hash_free(
                (*dtd).pentities as *mut hash::HashTable,
                Some(entity_deallocator),
            );
            allocator::xmlFreeImpl(dtd as *mut c_void);
            allocator::xmlFreeImpl(doc as *mut c_void);
        }
    }

    #[test]
    /// Tests that `get_parameter_entity` finds a stored parameter entity.
    ///
    /// # Safety
    ///
    /// - `doc` and `dtd` come from `make_doc_and_dtd` and stay live until the
    ///   trailing frees; `name` and `content` are NUL-terminated `c_str`
    ///   allocations; `entity` is asserted non-NULL before the equality check,
    ///   and the hash tables are freed with `entity_deallocator` before `dtd`
    ///   and `doc` are freed.
    fn test_get_parameter_entity() {
        unsafe {
            let (doc, dtd) = make_doc_and_dtd();
            let name = c_str(b"param1");
            let content = c_str(b"param content");

            let entity = add_entity(
                dtd,
                name,
                XML_INTERNAL_PARAMETER_ENTITY as c_int,
                ptr::null(),
                ptr::null(),
                content,
            );
            assert!(!entity.is_null());

            let found = get_parameter_entity(doc, name);
            assert_eq!(found, entity);

            hash::hash_free(
                (*dtd).entities as *mut hash::HashTable,
                Some(entity_deallocator),
            );
            hash::hash_free(
                (*dtd).pentities as *mut hash::HashTable,
                Some(entity_deallocator),
            );
            allocator::xmlFreeImpl(dtd as *mut c_void);
            allocator::xmlFreeImpl(doc as *mut c_void);
        }
    }

    #[test]
    /// Tests that `copy_entity` produces an independent deep copy.
    ///
    /// # Safety
    ///
    /// - `entity` is a zero-initialized `xmlMallocZero` allocation whose
    ///   `name` and `content` fields are fresh `xml_strdup` allocations; the
    ///   original and the copy are both freed with `free_entity` before the
    ///   test returns, and every dereference of these raw pointers happens
    ///   while the allocations are still live.
    fn test_copy_entity() {
        unsafe {
            let name = c_str(b"srcEntity");
            let content = c_str(b"source content");

            // Create an entity without a DTD
            let entity =
                allocator::xmlMallocZero(size_of::<_xmlEntity>() as usize) as *mut _xmlEntity;
            assert!(!entity.is_null());
            (*entity).type_ = XML_ENTITY_DECL as c_int;
            (*entity).name = string::xml_strdup(name);
            (*entity).content = string::xml_strdup(content);
            (*entity).length = 14;
            (*entity).etype = XML_INTERNAL_GENERAL_ENTITY as c_int;

            let copy = copy_entity(entity);
            assert!(!copy.is_null());
            assert_ne!(copy, entity);
            assert_eq!((*copy).etype, XML_INTERNAL_GENERAL_ENTITY as c_int);
            assert_eq!((*copy).length, 14);
            assert_eq!(string::xml_strcmp((*copy).name, name), 0);
            assert_eq!(string::xml_strcmp((*copy).content, content), 0);

            free_entity(copy);
            free_entity(entity);
        }
    }

    #[test]
    /// Tests that `copy_entity` returns NULL for a NULL input.
    ///
    /// # Safety
    ///
    /// - `copy_entity` checks its argument for NULL and returns early without
    ///   dereferencing it, so passing `ptr::null_mut()` is sound.
    fn test_copy_entity_null() {
        unsafe {
            assert!(copy_entity(ptr::null_mut()).is_null());
        }
    }

    #[test]
    /// Tests that `free_entity` tolerates a NULL pointer without crashing.
    ///
    /// # Safety
    ///
    /// - `free_entity` returns early when the pointer is NULL and never
    ///   dereferences it, so the call is sound.
    fn test_free_entity_null() {
        unsafe {
            free_entity(ptr::null_mut()); // Should not crash
        }
    }

    // ── Entity Type Tests ───────────────────────────────────────────────

    #[test]
    fn test_is_parameter_entity() {
        assert!(is_parameter_entity(XML_INTERNAL_PARAMETER_ENTITY as c_int));
        assert!(is_parameter_entity(XML_EXTERNAL_PARAMETER_ENTITY as c_int));
        assert!(!is_parameter_entity(XML_INTERNAL_GENERAL_ENTITY as c_int));
        assert!(!is_parameter_entity(
            XML_EXTERNAL_GENERAL_PARSED_ENTITY as c_int
        ));
    }

    #[test]
    fn test_is_external_entity() {
        assert!(is_external_entity(
            XML_EXTERNAL_GENERAL_PARSED_ENTITY as c_int
        ));
        assert!(is_external_entity(
            XML_EXTERNAL_GENERAL_UNPARSED_ENTITY as c_int
        ));
        assert!(is_external_entity(XML_EXTERNAL_PARAMETER_ENTITY as c_int));
        assert!(!is_external_entity(XML_INTERNAL_GENERAL_ENTITY as c_int));
        assert!(!is_external_entity(XML_INTERNAL_PARAMETER_ENTITY as c_int));
    }

    #[test]
    fn test_is_predefined_entity() {
        assert!(is_predefined_entity(
            XML_INTERNAL_PREDEFINED_ENTITY as c_int
        ));
        assert!(!is_predefined_entity(XML_INTERNAL_GENERAL_ENTITY as c_int));
    }

    // ── Entity Encoding Tests ───────────────────────────────────────────

    #[test]
    /// Tests that `encode_entities_reentrant` returns NULL for NULL input.
    ///
    /// # Safety
    ///
    /// - `encode_entities_reentrant` checks `input` for NULL and returns
    ///   without dereferencing it, so passing null pointers is sound.
    fn test_encode_entities_reentrant_null() {
        unsafe {
            let result = encode_entities_reentrant(ptr::null_mut(), ptr::null());
            assert!(result.is_null());
        }
    }

    #[test]
    /// Tests that text without special characters is encoded unchanged.
    ///
    /// # Safety
    ///
    /// - `input` is a NUL-terminated `c_str` allocation that stays alive until
    ///   the comparison; `result` is the freshly allocated output of
    ///   `encode_entities_reentrant`, asserted non-NULL, and is freed with
    ///   `xmlFreeImpl` after the comparison.
    fn test_encode_entities_reentrant_no_special() {
        unsafe {
            let input = c_str(b"Hello, World!");
            let result = encode_entities_reentrant(ptr::null_mut(), input);
            assert!(!result.is_null());
            assert_eq!(string::xml_strcmp(result, input), 0);
            allocator::xmlFreeImpl(result as *mut c_void);
        }
    }

    #[test]
    /// Tests that the `lt` and `gt` characters are encoded to entities.
    ///
    /// # Safety
    ///
    /// - `input` and `expected` are NUL-terminated `c_str` allocations that
    ///   stay alive until the comparison; `result` is the freshly allocated
    ///   output of `encode_entities_reentrant`, asserted non-NULL, and is
    ///   freed with `xmlFreeImpl` after the comparison.
    fn test_encode_entities_reentrant_lt_gt() {
        unsafe {
            let input = c_str(b"a < b > c");
            let result = encode_entities_reentrant(ptr::null_mut(), input);
            assert!(!result.is_null());
            let expected = c_str(b"a &lt; b &gt; c");
            assert_eq!(string::xml_strcmp(result, expected), 0);
            allocator::xmlFreeImpl(result as *mut c_void);
        }
    }

    #[test]
    /// Tests that the `amp` character is encoded to an entity.
    ///
    /// # Safety
    ///
    /// - `input` and `expected` are NUL-terminated `c_str` allocations that
    ///   stay alive until the comparison; `result` is the freshly allocated
    ///   output of `encode_entities_reentrant`, asserted non-NULL, and is
    ///   freed with `xmlFreeImpl` after the comparison.
    fn test_encode_entities_reentrant_amp() {
        unsafe {
            let input = c_str(b"a & b");
            let result = encode_entities_reentrant(ptr::null_mut(), input);
            assert!(!result.is_null());
            let expected = c_str(b"a &amp; b");
            assert_eq!(string::xml_strcmp(result, expected), 0);
            allocator::xmlFreeImpl(result as *mut c_void);
        }
    }

    #[test]
    /// Tests that quotes and apostrophes are encoded to entities.
    ///
    /// # Safety
    ///
    /// - `input` and `expected` are NUL-terminated `c_str` allocations that
    ///   stay alive until the comparison; `result` is the freshly allocated
    ///   output of `encode_entities_reentrant`, asserted non-NULL, and is
    ///   freed with `xmlFreeImpl` after the comparison.
    fn test_encode_entities_reentrant_quotes() {
        unsafe {
            let input = c_str(b"\"hello\" 'world'");
            let result = encode_entities_reentrant(ptr::null_mut(), input);
            assert!(!result.is_null());
            let expected = c_str(b"&quot;hello&quot; &apos;world&apos;");
            assert_eq!(string::xml_strcmp(result, expected), 0);
            allocator::xmlFreeImpl(result as *mut c_void);
        }
    }

    #[test]
    /// Tests that a mixed string is fully encoded in one pass.
    ///
    /// # Safety
    ///
    /// - `input` and `expected` are NUL-terminated `c_str` allocations that
    ///   stay alive until the comparison; `result` is the freshly allocated
    ///   output of `encode_entities_reentrant`, asserted non-NULL, and is
    ///   freed with `xmlFreeImpl` after the comparison.
    fn test_encode_entities_reentrant_all() {
        unsafe {
            let input = c_str(b"<tag attr=\"value\">&'more'</tag>");
            let result = encode_entities_reentrant(ptr::null_mut(), input);
            assert!(!result.is_null());
            let expected =
                c_str(b"&lt;tag attr=&quot;value&quot;&gt;&amp;&apos;more&apos;&lt;/tag&gt;");
            assert_eq!(string::xml_strcmp(result, expected), 0);
            allocator::xmlFreeImpl(result as *mut c_void);
        }
    }

    // ── Entity Decoding Tests ───────────────────────────────────────────

    #[test]
    /// Tests that `string_decode_entities` returns NULL for a NULL input.
    ///
    /// # Safety
    ///
    /// - `string_decode_entities` returns early when `input` is NULL, and the
    ///   `doc` argument is only dereferenced for entity lookup during
    ///   decoding, so passing null pointers here is sound.
    fn test_decode_entities_null_input() {
        unsafe {
            let result = string_decode_entities(ptr::null_mut(), ptr::null(), 0, 0, 0, 0);
            assert!(result.is_null());
        }
    }

    #[test]
    /// Tests that decoding an empty string yields an empty NUL-terminated
    /// result.
    ///
    /// # Safety
    ///
    /// - `input` is a NUL-terminated `c_str` allocation valid for the call;
    ///   `result` is asserted non-NULL before the `*result` dereference and is
    ///   freed with `xmlFreeImpl` before the test ends.
    fn test_decode_entities_empty() {
        unsafe {
            let input = c_str(b"");
            let result = string_decode_entities(ptr::null_mut(), input, 0, 0, 0, 0);
            assert!(!result.is_null());
            assert_eq!(*result, 0);
            allocator::xmlFreeImpl(result as *mut c_void);
        }
    }

    #[test]
    /// Tests that input without references is decoded unchanged.
    ///
    /// # Safety
    ///
    /// - `input` is a NUL-terminated `c_str` allocation that stays alive until
    ///   the comparison; `result` is asserted non-NULL before being passed to
    ///   `string::xml_strcmp` and is freed with `xmlFreeImpl` afterwards.
    fn test_decode_entities_no_refs() {
        unsafe {
            let input = c_str(b"Hello, World!");
            let result = string_decode_entities(ptr::null_mut(), input, 0, 0, 0, 0);
            assert!(!result.is_null());
            assert_eq!(string::xml_strcmp(result, input), 0);
            allocator::xmlFreeImpl(result as *mut c_void);
        }
    }

    #[test]
    /// Tests that a decimal numeric character reference is decoded.
    ///
    /// # Safety
    ///
    /// - `input` and `expected` are NUL-terminated `c_str` allocations that
    ///   stay alive until the comparison; `result` is asserted non-NULL before
    ///   being passed to `string::xml_strcmp` and is freed with `xmlFreeImpl`
    ///   afterwards.
    fn test_decode_numeric_decimal() {
        unsafe {
            // &#65; = 'A'
            let input = c_str(b"&#65;");
            let result = string_decode_entities(ptr::null_mut(), input, 0, 0, 0, 0);
            assert!(!result.is_null());
            let expected = c_str(b"A");
            assert_eq!(string::xml_strcmp(result, expected), 0);
            allocator::xmlFreeImpl(result as *mut c_void);
        }
    }

    #[test]
    /// Tests that a hexadecimal numeric character reference is decoded.
    ///
    /// # Safety
    ///
    /// - `input` and `expected` are NUL-terminated `c_str` allocations that
    ///   stay alive until the comparison; `result` is asserted non-NULL before
    ///   being passed to `string::xml_strcmp` and is freed with `xmlFreeImpl`
    ///   afterwards.
    fn test_decode_numeric_hex() {
        unsafe {
            // &#x41; = 'A'
            let input = c_str(b"&#x41;");
            let result = string_decode_entities(ptr::null_mut(), input, 0, 0, 0, 0);
            assert!(!result.is_null());
            let expected = c_str(b"A");
            assert_eq!(string::xml_strcmp(result, expected), 0);
            allocator::xmlFreeImpl(result as *mut c_void);
        }
    }

    #[test]
    /// Tests that mixed decimal and hexadecimal references are decoded.
    ///
    /// # Safety
    ///
    /// - `input` and `expected` are NUL-terminated `c_str` allocations that
    ///   stay alive until the comparison; `result` is asserted non-NULL before
    ///   being passed to `string::xml_strcmp` and is freed with `xmlFreeImpl`
    ///   afterwards.
    fn test_decode_numeric_mixed() {
        unsafe {
            let input = c_str(b"Hello &#x57;&#111;rld!"); // Hello World!
            let result = string_decode_entities(ptr::null_mut(), input, 0, 0, 0, 0);
            assert!(!result.is_null());
            let expected = c_str(b"Hello World!");
            assert_eq!(string::xml_strcmp(result, expected), 0);
            allocator::xmlFreeImpl(result as *mut c_void);
        }
    }

    #[test]
    /// Tests that predefined general entities in the DTD are substituted.
    ///
    /// # Safety
    ///
    /// - `doc` and `dtd` are zero-initialized by `make_doc_and_dtd` and stay
    ///   live until the trailing frees; the `input` and `expected` buffers are
    ///   NUL-terminated `c_str` allocations; `result` is asserted non-NULL
    ///   before the comparison and freed with `xmlFreeImpl`; the hash tables
    ///   are freed with `entity_deallocator` before `dtd` and `doc` are freed.
    fn test_decode_predefined_entities() {
        unsafe {
            let (doc, dtd) = make_doc_and_dtd();
            let input = c_str(b"a &lt; b &gt; c &amp; d");

            // Add the entities to the DTD so get_entity can find them
            add_entity(
                dtd,
                c_str(b"lt"),
                XML_INTERNAL_GENERAL_ENTITY as c_int,
                ptr::null(),
                ptr::null(),
                c_str(b"<"),
            );
            add_entity(
                dtd,
                c_str(b"gt"),
                XML_INTERNAL_GENERAL_ENTITY as c_int,
                ptr::null(),
                ptr::null(),
                c_str(b">"),
            );
            add_entity(
                dtd,
                c_str(b"amp"),
                XML_INTERNAL_GENERAL_ENTITY as c_int,
                ptr::null(),
                ptr::null(),
                c_str(b"&"),
            );

            let result = string_decode_entities(doc, input, 0, 0, 0, 0);
            assert!(!result.is_null());
            let expected = c_str(b"a < b > c & d");
            assert_eq!(string::xml_strcmp(result, expected), 0);

            allocator::xmlFreeImpl(result as *mut c_void);

            hash::hash_free(
                (*dtd).entities as *mut hash::HashTable,
                Some(entity_deallocator),
            );
            hash::hash_free(
                (*dtd).pentities as *mut hash::HashTable,
                Some(entity_deallocator),
            );
            allocator::xmlFreeImpl(dtd as *mut c_void);
            allocator::xmlFreeImpl(doc as *mut c_void);
        }
    }

    // ── Security/Limit Tests ────────────────────────────────────────────

    #[test]
    fn test_check_entity_expansion_limit() {
        assert_eq!(check_entity_expansion_limit(100), 0);
        assert_eq!(check_entity_expansion_limit(1_000_000), 0);
        assert_eq!(check_entity_expansion_limit(1_000_001), -1);
    }

    #[test]
    fn test_check_entity_recursion_depth() {
        assert_eq!(check_entity_recursion_depth(10), 0);
        assert_eq!(check_entity_recursion_depth(32), 0);
        assert_eq!(check_entity_recursion_depth(33), -1);
    }

    // ── Entity Content Tests ────────────────────────────────────────────

    #[test]
    /// Tests that `get_entity_content` returns a copy of the entity content.
    ///
    /// # Safety
    ///
    /// - `entity` is a zero-initialized `xmlMallocZero` allocation whose
    ///   `content` field is a fresh `xml_strdup` allocation; `retrieved` is
    ///   asserted non-NULL before the comparison and freed with `xmlFreeImpl`,
    ///   and `entity` is freed with `free_entity` before the test returns.
    fn test_get_entity_content() {
        unsafe {
            let content = c_str(b"entity content");
            let entity =
                allocator::xmlMallocZero(size_of::<_xmlEntity>() as usize) as *mut _xmlEntity;
            assert!(!entity.is_null());
            (*entity).content = string::xml_strdup(content);

            let retrieved = get_entity_content(entity);
            assert!(!retrieved.is_null());
            assert_eq!(string::xml_strcmp(retrieved, content), 0);

            allocator::xmlFreeImpl(retrieved as *mut c_void);
            free_entity(entity);
        }
    }

    #[test]
    /// Tests that `get_entity_content` returns NULL for a NULL entity.
    ///
    /// # Safety
    ///
    /// - `get_entity_content` checks its argument for NULL and returns early
    ///   without dereferencing it, so passing `ptr::null_mut()` is sound.
    fn test_get_entity_content_null() {
        unsafe {
            assert!(get_entity_content(ptr::null_mut()).is_null());
        }
    }
}
