//! Entity handling (§24, §85 Phase 6).
//!
//! General entities, parameter entities, external entities, entity
//! substitution, entity references, security limits, recursive entities,
//! expansion limits.

use core::ffi::c_void;
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
/// ```c
/// xmlEntityPtr xmlAddEntity(xmlDtdPtr dtd, const xmlChar *name, int type,
///                           const xmlChar *ExternalID, const xmlChar *SystemID,
///                           const xmlChar *content);
/// ```
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
        (*entity).orig = ptr::null_mut();
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
    if doc.is_null() || name.is_null() {
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

        ptr::null_mut()
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

        // Free children tree nodes if requested
        if free_children && !e.children.is_null() {
            // In a full implementation, we'd recursively free the node tree.
            // For now, we simply note that children should be freed.
            // The tree module's free functions would be called here.
        }

        allocator::xmlFreeImpl(entity as *mut c_void);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Entity Type Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Check if an entity type is a parameter entity.
#[inline]
pub fn is_parameter_entity(etype: c_int) -> bool {
    etype == XML_INTERNAL_PARAMETER_ENTITY as c_int
        || etype == XML_EXTERNAL_PARAMETER_ENTITY as c_int
}

/// Check if an entity type is an external entity.
#[inline]
pub fn is_external_entity(etype: c_int) -> bool {
    etype == XML_EXTERNAL_GENERAL_PARSED_ENTITY as c_int
        || etype == XML_EXTERNAL_GENERAL_UNPARSED_ENTITY as c_int
        || etype == XML_EXTERNAL_PARAMETER_ENTITY as c_int
}

/// Check if an entity type is a predefined entity.
#[inline]
pub fn is_predefined_entity(etype: c_int) -> bool {
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
                c => out_len += 1,
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
                let mut entity_name_start = i;
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
unsafe fn decode_numeric_ref(input: *const xmlChar, pos: &mut usize, len: usize) -> (u32, usize) {
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
            'q' => {
                if string::xml_strcmp(name, b"quot\0" as *const u8 as *const xmlChar) == 0 {
                    3
                } else {
                    return ptr::null_mut();
                }
            }
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
pub fn check_entity_expansion_limit(expanded_size: c_ulong) -> c_int {
    if expanded_size > XML_ENTITY_CONTENT_EXPANSION_MAX as c_ulong {
        -1
    } else {
        0
    }
}

/// Check if entity recursion depth exceeds limits.
///
/// Returns 0 if within limits, -1 if exceeded.
pub fn check_entity_recursion_depth(depth: c_int) -> c_int {
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
    use crate::abi::allocator::xmlFreeImpl;
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

    unsafe extern "C" fn entity_deallocator(payload: *mut c_void, _name: *mut u8) {
        if !payload.is_null() {
            free_entity(payload as *mut _xmlEntity);
        }
    }

    #[test]
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
    fn test_get_entity_null_doc() {
        unsafe {
            let found = get_entity(ptr::null_mut(), c_str(b"test"));
            assert!(found.is_null());
        }
    }

    #[test]
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
    fn test_copy_entity_null() {
        unsafe {
            assert!(copy_entity(ptr::null_mut()).is_null());
        }
    }

    #[test]
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
    fn test_encode_entities_reentrant_null() {
        unsafe {
            let result = encode_entities_reentrant(ptr::null_mut(), ptr::null());
            assert!(result.is_null());
        }
    }

    #[test]
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
    fn test_decode_entities_null_input() {
        unsafe {
            let result = string_decode_entities(ptr::null_mut(), ptr::null(), 0, 0, 0, 0);
            assert!(result.is_null());
        }
    }

    #[test]
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
    fn test_get_entity_content_null() {
        unsafe {
            assert!(get_entity_content(ptr::null_mut()).is_null());
        }
    }
}
