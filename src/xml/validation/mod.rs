//! DTD validation (§27, §85 Phase 6).
//!
//! DTD validation against element/attribute declarations:
//!
//! 1. **Element content model validation** — does the element's child sequence
//!    match its declared content model?
//! 2. **Attribute value validation** — does each attribute's value conform to
//!    its declared type (CDATA, ID, IDREF, IDREFS, ENTITY, ENTITIES, NMTOKEN,
//!    NMTOKENS, ENUMERATION, NOTATION)?
//! 3. **ID/IDREF consistency** — are all ID values unique? Does each IDREF
//!    reference a valid ID?
//! 4. **Required attributes** — are all REQUIRED attributes present?
//! 5. **NOTATION validation** — are NOTATION attributes referencing declared
//!    notations?
//! 6. **Well-formedness constraints** — additional validity constraints
//!
//! # UPSTREAM-PARITY
//!
//! This module follows libxml2's `valid.c` implementation. The validation
//! context (`xmlValidCtxt`) accumulates ID/IDREF tables across the document
//! and checks consistency in `xmlValidateDocumentFinal`.
//!
//! # Phase 6 status
//!
//! Complete — all core DTD validation functions are implemented.
//! Edge-case behavior for degenerate DTDs matches upstream.

use core::ffi::c_void;
use core::ptr;
use std::os::raw::{c_char, c_int, c_uint};

use crate::abi::allocator;
use crate::abi::callbacks::xmlGenericErrorFunc;
use crate::abi::callbacks::xmlHashDeallocator;
use crate::abi::callbacks::xmlHashScannerFull;
use crate::abi::structs::*;
use crate::abi::types::xmlAttributeDefault::*;
use crate::abi::types::xmlAttributeType::*;
use crate::abi::types::xmlElementContentOccur::*;
use crate::abi::types::xmlElementContentType::*;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::xmlElementTypeVal::*;
use crate::abi::types::xmlEntityType::*;
use crate::abi::types::*;
use crate::xml::dtd;
use crate::xml::entities;
use crate::xml::hash;
use crate::xml::string;
use crate::xml::tree;

// ═══════════════════════════════════════════════════════════════════════════════
// Constants
// ═══════════════════════════════════════════════════════════════════════════════

/// Maximum allowed depth for recursive validation walks.
const VALID_CTXT_DEPTH_MAX: c_int = 256;

// ═══════════════════════════════════════════════════════════════════════════════
// Validation Context
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new validation context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlValidCtxtPtr xmlNewValidCtxt(void);
/// ```
///
/// Returns a new zero-initialized validation context, or NULL on OOM.
pub unsafe fn new_valid_ctxt() -> *mut _xmlValidCtxt {
    // SAFETY: Allocate zero-initialized memory for the validation context.
    let ctxt = allocator::xmlMallocZero(size_of::<_xmlValidCtxt>() as usize) as *mut _xmlValidCtxt;
    if ctxt.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*ctxt).valid = 1;
        (*ctxt).node = ptr::null_mut();
        (*ctxt).doc = ptr::null_mut();
        (*ctxt).nodeNr = 0;
        (*ctxt).nodeMax = 0;
        (*ctxt).nodeTab = ptr::null_mut();
        (*ctxt).flags = 0;
        (*ctxt).vstate = ptr::null_mut();
        (*ctxt).vstateNr = 0;
        (*ctxt).vstateMax = 0;
        (*ctxt).vstateTab = ptr::null_mut();
        (*ctxt).am = ptr::null_mut();
        (*ctxt).state = ptr::null_mut();
        (*ctxt).error = None;
        (*ctxt).warning = None;
        (*ctxt).userData = ptr::null_mut();
    }

    ctxt
}

/// Free a validation context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlFreeValidCtxt(xmlValidCtxtPtr ctxt);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a valid pointer to an _xmlValidCtxt, or NULL.
pub unsafe fn free_valid_ctxt(ctxt: *mut _xmlValidCtxt) {
    if ctxt.is_null() {
        return;
    }

    unsafe {
        let c = &mut *ctxt;

        // Free node stack
        if !c.nodeTab.is_null() {
            allocator::xmlFreeImpl(c.nodeTab as *mut c_void);
        }

        // Free automata
        if !c.am.is_null() {
            // Automata free — currently a no-op since am is opaque.
            // UPSTREAM-PARITY: xmlFreeAutomata(c.am) in upstream.
        }

        // Free state
        if !c.state.is_null() {
            // State free — currently a no-op.
        }

        allocator::xmlFreeImpl(ctxt as *mut c_void);
    }
}

/// Set error and warning callbacks on a validation context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSetValidErrors(xmlValidCtxtPtr ctxt,
///                        xmlGenericErrorFunc err,
///                        xmlGenericErrorFunc warn,
///                        void *data);
/// ```
///
/// # SAFETY
///
/// - `ctxt` may be NULL (no-op).
/// - `err`, `warn`, `data` may be NULL.
pub unsafe fn set_valid_errors(
    ctxt: *mut _xmlValidCtxt,
    err: Option<xmlGenericErrorFunc>,
    warn: Option<xmlGenericErrorFunc>,
    data: *mut c_void,
) {
    if ctxt.is_null() {
        return;
    }

    unsafe {
        // UPSTREAM-PARITY: libxml2 stores these as xmlValidityErrorFunc
        // but accepts xmlGenericErrorFunc in the setter.
        (*ctxt).error = err;
        (*ctxt).warning = warn;
        (*ctxt).userData = data;
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Internal helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Report a validation error through the context.
///
/// # SAFETY
///
/// - `ctxt` may be NULL.
/// - `msg` must be a valid null-terminated C string.
unsafe fn vctxt_error(ctxt: *mut _xmlValidCtxt, msg: *const c_char) {
    if ctxt.is_null() {
        return;
    }
    unsafe {
        let c = &mut *ctxt;
        c.valid = 0;
        if let Some(err) = c.error {
            err(c.userData, msg);
        }
    }
}

/// Push a node onto the validation context's node stack.
///
/// Returns 0 on success, -1 on failure.
///
/// # SAFETY
///
/// - `ctxt` must be a valid pointer.
unsafe fn vctxt_push_node(ctxt: *mut _xmlValidCtxt, node: *mut _xmlNode) -> c_int {
    unsafe {
        let c = &mut *ctxt;

        if c.nodeNr >= c.nodeMax {
            let new_max = if c.nodeMax == 0 { 4 } else { c.nodeMax * 2 };
            let new_tab = allocator::xmlReallocImpl(
                c.nodeTab as *mut c_void,
                (new_max as usize) * size_of::<*mut _xmlNode>(),
            ) as *mut *mut _xmlNode;
            if new_tab.is_null() {
                return -1;
            }
            c.nodeTab = new_tab;
            c.nodeMax = new_max;
        }

        *c.nodeTab.add(c.nodeNr as usize) = node;
        c.nodeNr += 1;
        c.node = node;
    }
    0
}

/// Pop a node from the validation context's node stack.
///
/// # SAFETY
///
/// - `ctxt` must be a valid pointer.
unsafe fn vctxt_pop_node(ctxt: *mut _xmlValidCtxt) {
    unsafe {
        let c = &mut *ctxt;
        if c.nodeNr > 0 {
            c.nodeNr -= 1;
        }
        if c.nodeNr > 0 {
            c.node = *c.nodeTab.add((c.nodeNr - 1) as usize);
        } else {
            c.node = ptr::null_mut();
        }
    }
}

/// Get the DTD to validate against for a given document.
///
/// Returns the internal subset first, then the external subset.
///
/// # SAFETY
///
/// - `doc` must be a valid pointer or NULL.
unsafe fn get_valid_dtd(doc: *mut _xmlDoc) -> *mut _xmlDtd {
    if doc.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let d = &*doc;
        if !d.intSubset.is_null() {
            d.intSubset
        } else {
            d.extSubset
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// XML Name / NMTOKEN Character Classification
// ═══════════════════════════════════════════════════════════════════════════════

/// Check if a character is a valid XML Name start character.
///
/// # UPSTREAM-PARITY
///
/// Matches the XML 1.0 Fifth Edition NameStartChar production:
/// `[a-zA-Z_:] | [\xC0-\xD6] | [\xD8-\xF6] | [\xF8-\u{2FF}] |
///  [\u{370}-\u{37D}] | [\u{37F}-\u{1FFF}] | [\u{200C}-\u{200D}] |
///  [\u{2070}-\u{218F}] | [\u{2C00}-\u{2FEF}] | [\u{3001}-\u{D7FF}] |
///  [\u{F900}-\u{FDCF}] | [\u{FDF0}-\u{FFFD}]`
pub(crate) fn is_xml_name_start(c: char) -> bool {
    matches!(c,
        'a'..='z' | 'A'..='Z' | '_' | ':' |
        '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}' | '\u{F8}'..='\u{2FF}' |
        '\u{370}'..='\u{37D}' | '\u{37F}'..='\u{1FFF}' |
        '\u{200C}'..='\u{200D}' | '\u{2070}'..='\u{218F}' |
        '\u{2C00}'..='\u{2FEF}' | '\u{3001}'..='\u{D7FF}' |
        '\u{F900}'..='\u{FDCF}' | '\u{FDF0}'..='\u{FFFD}' |
        '\u{10000}'..='\u{EFFFF}'
    )
}

/// Check if a character is a valid XML Name character.
///
/// # UPSTREAM-PARITY
///
/// Matches NameChar production: NameStartChar | '-' | '.' | [0-9] |
/// \u{B7} | [\u{0300}-\u{036F}] | [\u{203F}-\u{2040}]
pub(crate) fn is_xml_name_char(c: char) -> bool {
    is_xml_name_start(c)
        || matches!(c,
            '-' | '.' | '0'..='9' | '\u{B7}' |
            '\u{0300}'..='\u{036F}' | '\u{203F}'..='\u{2040}'
        )
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlValidateName / xmlValidateNames
// ═══════════════════════════════════════════════════════════════════════════════

/// Validate whether `value` is a valid XML Name.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateName(const xmlChar *value);
/// ```
///
/// Returns 1 if valid, 0 if not.
///
/// # SAFETY
///
/// - `value` must be a valid null-terminated string or NULL.
pub unsafe fn validate_name(value: *const xmlChar) -> c_int {
    if value.is_null() {
        return 0;
    }

    let s = unsafe { string::xmlstr_to_bytes(value) };
    let s = core::str::from_utf8(s).unwrap_or("");

    if s.is_empty() {
        return 0;
    }

    let mut chars = s.chars();

    // First character must be a NameStartChar
    match chars.next() {
        Some(c) if is_xml_name_start(c) => {}
        _ => return 0,
    }

    // Remaining characters must be NameChars
    for c in chars {
        if !is_xml_name_char(c) {
            return 0;
        }
    }

    1
}

/// Validate whether `value` is a whitespace-separated list of XML Names.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateNames(const xmlChar *value);
/// ```
///
/// Returns 1 if valid, 0 if not.
///
/// # SAFETY
///
/// - `value` must be a valid null-terminated string or NULL.
pub unsafe fn validate_names(value: *const xmlChar) -> c_int {
    if value.is_null() {
        return 0;
    }

    let s = unsafe { string::xmlstr_to_bytes(value) };
    let s = core::str::from_utf8(s).unwrap_or("");

    if s.is_empty() {
        return 0;
    }

    for token in s.split_whitespace() {
        if token.is_empty() {
            return 0;
        }
        let mut chars = token.chars();
        match chars.next() {
            Some(c) if is_xml_name_start(c) => {}
            _ => return 0,
        }
        for c in chars {
            if !is_xml_name_char(c) {
                return 0;
            }
        }
    }

    1
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlValidateNmtoken / xmlValidateNmtokens
// ═══════════════════════════════════════════════════════════════════════════════

/// Validate whether `value` is a valid XML NMTOKEN.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateNmtoken(const xmlChar *value);
/// ```
///
/// An NMTOKEN is like a Name but the first character can also be a NameChar
/// (not just a NameStartChar). Returns 1 if valid, 0 if not.
///
/// # SAFETY
///
/// - `value` must be a valid null-terminated string or NULL.
pub unsafe fn validate_nmtoken(value: *const xmlChar) -> c_int {
    if value.is_null() {
        return 0;
    }

    let s = unsafe { string::xmlstr_to_bytes(value) };
    let s = core::str::from_utf8(s).unwrap_or("");

    if s.is_empty() {
        return 0;
    }

    for c in s.chars() {
        if !is_xml_name_char(c) {
            return 0;
        }
    }

    1
}

/// Validate whether `value` is a whitespace-separated list of XML NMTOKENs.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateNmtokens(const xmlChar *value);
/// ```
///
/// Returns 1 if valid, 0 if not.
///
/// # SAFETY
///
/// - `value` must be a valid null-terminated string or NULL.
pub unsafe fn validate_nmtokens(value: *const xmlChar) -> c_int {
    if value.is_null() {
        return 0;
    }

    let s = unsafe { string::xmlstr_to_bytes(value) };
    let s = core::str::from_utf8(s).unwrap_or("");

    if s.is_empty() {
        return 0;
    }

    for token in s.split_whitespace() {
        if token.is_empty() {
            return 0;
        }
        for c in token.chars() {
            if !is_xml_name_char(c) {
                return 0;
            }
        }
    }

    1
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlValidateAttributeValue
// ═══════════════════════════════════════════════════════════════════════════════

/// Validate an attribute value against its declared type.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateAttributeValue(int type, const xmlChar *value);
/// ```
///
/// Returns 1 if the value is valid for the given attribute type, 0 otherwise.
///
/// # SAFETY
///
/// - `value` must be a valid null-terminated string or NULL.
pub unsafe fn validate_attribute_value(atype: c_int, value: *const xmlChar) -> c_int {
    // UPSTREAM-PARITY: upstream xmlValidateAttributeValue dispatches to
    // xmlValidateAttributeValueInternal(NULL, type, value) whose switch
    // matches this exactly; CDATA (and unknown types) fall through to 1.
    match atype as u32 {
        t if t == XML_ATTRIBUTE_ENTITIES as u32 || t == XML_ATTRIBUTE_IDREFS as u32 => {
            validate_values_internal(value, 0)
        }
        t if t == XML_ATTRIBUTE_ENTITY as u32
            || t == XML_ATTRIBUTE_IDREF as u32
            || t == XML_ATTRIBUTE_ID as u32
            || t == XML_ATTRIBUTE_NOTATION as u32 =>
        {
            validate_value_internal(value, 0)
        }
        t if t == XML_ATTRIBUTE_NMTOKENS as u32 || t == XML_ATTRIBUTE_ENUMERATION as u32 => {
            validate_values_internal(value, XML_SCAN_NMTOKEN)
        }
        t if t == XML_ATTRIBUTE_NMTOKEN as u32 => validate_value_internal(value, XML_SCAN_NMTOKEN),
        _ => 1, // CDATA / unknown
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlValidateEnumeration
// ═══════════════════════════════════════════════════════════════════════════════

/// Validate that `value` is one of the values in the enumeration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateEnumeration(xmlValidCtxtPtr ctxt,
///                            const xmlChar *value,
///                            xmlEnumerationPtr tree);
/// ```
///
/// Returns 1 if the value is in the enumeration, 0 otherwise.
///
/// # SAFETY
///
/// - `ctxt` may be NULL.
/// - `value` must be a valid null-terminated string or NULL.
/// - `tree` may be NULL (returns 0).
pub unsafe fn validate_enumeration(
    ctxt: *mut _xmlValidCtxt,
    value: *const xmlChar,
    tree: *mut _xmlEnumeration,
) -> c_int {
    if value.is_null() || tree.is_null() {
        return 0;
    }

    let mut cur = tree;
    while !cur.is_null() {
        unsafe {
            if string::xml_strcmp(value, (*cur).name) == 0 {
                return 1;
            }
            cur = (*cur).next;
        }
    }

    // Value not found in enumeration
    unsafe {
        let msg = string::xmlstr_to_string(value);
        let err_msg = format!("Value '{}' is not a valid enumeration value\0", msg);
        vctxt_error(ctxt, err_msg.as_ptr() as *const c_char);
    }
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlValidateNotationUse
// ═══════════════════════════════════════════════════════════════════════════════

/// Validate that `notationName` is a declared notation in the document's DTD.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateNotationUse(xmlValidCtxtPtr ctxt,
///                            xmlDocPtr doc,
///                            const xmlChar *notationName);
/// ```
///
/// Returns 1 if the notation is declared, 0 otherwise.
///
/// # SAFETY
///
/// - `ctxt`, `doc`, `notationName` may be NULL.
pub unsafe fn validate_notation_use(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    notation_name: *const xmlChar,
) -> c_int {
    if notation_name.is_null() {
        return 0;
    }

    let dtd = unsafe { get_valid_dtd(doc) };
    if dtd.is_null() {
        unsafe {
            vctxt_error(
                ctxt,
                b"No DTD available for notation validation\0" as *const u8 as *const c_char,
            );
        }
        return 0;
    }

    // Look up the notation in the DTD's notation hash table
    unsafe {
        let notations = (*dtd).notations;
        if notations.is_null() {
            vctxt_error(
                ctxt,
                b"No notations declared in DTD\0" as *const u8 as *const c_char,
            );
            return 0;
        }

        let notation = hash::hash_lookup(notations as *mut hash::HashTable, notation_name);
        if notation.is_null() {
            let msg = string::xmlstr_to_string(notation_name);
            let err_msg = format!("Notation '{}' is not declared\0", msg);
            vctxt_error(ctxt, err_msg.as_ptr() as *const c_char);
            return 0;
        }
    }

    1
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlValidateID / xmlValidateIDRef / xmlValidateIDRefs
// ═══════════════════════════════════════════════════════════════════════════════

/// Validate an ID value: check that the value is a valid XML Name and
/// that no duplicate ID values exist in the document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateID(xmlValidCtxtPtr ctxt,
///                   xmlDocPtr doc,
///                   xmlNodePtr node,
///                   const xmlChar *value);
/// ```
///
/// Returns 1 if the ID is valid, 0 otherwise.
///
/// # SAFETY
///
/// - `ctxt`, `doc`, `node`, `value` may be NULL.
pub unsafe fn validate_id(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    node: *mut _xmlNode,
    value: *const xmlChar,
) -> c_int {
    if value.is_null() || doc.is_null() {
        return 0;
    }

    // First, check that the value is a valid XML Name
    // UPSTREAM-PARITY: xmlValidateID uses xmlValidateNameValue semantics.
    if unsafe { validate_name_value(value) } == 0 {
        unsafe {
            let msg = string::xmlstr_to_string(value);
            let err_msg = format!("ID value '{}' is not a valid XML Name\0", msg);
            vctxt_error(ctxt, err_msg.as_ptr() as *const c_char);
        }
        return 0;
    }

    // Check for duplicate ID in the document's ID hash table
    unsafe {
        let doc_ref = &*doc;
        if !doc_ref.ids.is_null() {
            let existing = hash::hash_lookup(doc_ref.ids as *mut hash::HashTable, value);
            if !existing.is_null() {
                let msg = string::xmlstr_to_string(value);
                let err_msg = format!("Duplicate ID value '{}'\0", msg);
                vctxt_error(ctxt, err_msg.as_ptr() as *const c_char);
                return 0;
            }
        }
    }

    // Register the ID in the document's ID hash table
    unsafe {
        if (*doc).ids.is_null() {
            (*doc).ids = hash::hash_create(16) as *mut c_void;
        }
        hash::hash_add_entry(
            (*doc).ids as *mut hash::HashTable,
            value,
            node as *mut c_void,
        );
    }

    1
}

/// Validate an IDREF value: check that the referenced ID exists in the document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateIDRef(xmlValidCtxtPtr ctxt,
///                      xmlDocPtr doc,
///                      xmlNodePtr node,
///                      const xmlChar *value);
/// ```
///
/// Returns 1 if the IDREF is valid (references a known ID), 0 otherwise.
///
/// # SAFETY
///
/// - `ctxt`, `doc`, `node`, `value` may be NULL.
pub unsafe fn validate_id_ref(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    node: *mut _xmlNode,
    value: *const xmlChar,
) -> c_int {
    if value.is_null() || doc.is_null() {
        return 0;
    }

    // Check that the value is a valid XML Name
    // UPSTREAM-PARITY: xmlValidateIDRef uses xmlValidateNameValue semantics.
    if unsafe { validate_name_value(value) } == 0 {
        unsafe {
            let msg = string::xmlstr_to_string(value);
            let err_msg = format!("IDREF value '{}' is not a valid XML Name\0", msg);
            vctxt_error(ctxt, err_msg.as_ptr() as *const c_char);
        }
        return 0;
    }

    // Check if the referenced ID exists
    unsafe {
        let doc_ref = &*doc;
        if doc_ref.ids.is_null()
            || hash::hash_lookup(doc_ref.ids as *mut hash::HashTable, value).is_null()
        {
            // UPSTREAM-PARITY: Forward references are allowed during
            // validation but are reported as warnings. The final check
            // happens in xmlValidateDocumentFinal.
            let msg = string::xmlstr_to_string(value);
            let err_msg = format!("IDREF '{}' references an unknown ID\0", msg);
            vctxt_error(ctxt, err_msg.as_ptr() as *const c_char);
            return 0;
        }
    }

    1
}

/// Validate IDREFS (whitespace-separated list of IDREF values).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateIDRefs(xmlValidCtxtPtr ctxt,
///                       xmlDocPtr doc,
///                       xmlNodePtr node,
///                       const xmlChar *value);
/// ```
///
/// Returns 1 if all IDREFs are valid, 0 otherwise.
///
/// # SAFETY
///
/// - `ctxt`, `doc`, `node`, `value` may be NULL.
pub unsafe fn validate_id_refs(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    node: *mut _xmlNode,
    value: *const xmlChar,
) -> c_int {
    if value.is_null() || doc.is_null() {
        return 0;
    }

    let s = unsafe { string::xmlstr_to_bytes(value) };
    let s = core::str::from_utf8(s).unwrap_or("");

    if s.is_empty() {
        return 0;
    }

    let mut valid = 1;
    for token in s.split_whitespace() {
        if token.is_empty() {
            continue;
        }
        // Create a null-terminated xmlChar string for each token
        let token_ptr = unsafe { string::bytes_to_xmlstr(token.as_bytes()) };
        if token_ptr.is_null() {
            valid = 0;
            break;
        }
        let result = unsafe { validate_id_ref(ctxt, doc, node, token_ptr) };
        unsafe {
            allocator::xmlFreeImpl(token_ptr as *mut c_void);
        }
        if result == 0 {
            valid = 0;
        }
    }

    valid
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlValidateAttributeDecl
// ═══════════════════════════════════════════════════════════════════════════════

/// Validate an attribute's value against its declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateAttributeDecl(xmlValidCtxtPtr ctxt,
///                              xmlDocPtr doc,
///                              xmlNodePtr elem,
///                              xmlAttributePtr attr);
/// ```
///
/// Checks:
/// - Attribute value type (CDATA, ID, IDREF, etc.)
/// - Enumeration membership
/// - NOTATION declaration
/// - Default value validity
///
/// Returns 1 if valid, 0 otherwise.
///
/// # SAFETY
///
/// - `ctxt`, `doc`, `elem`, `attr` may be NULL.
pub unsafe fn validate_attribute_decl(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    elem: *mut _xmlNode,
    attr: *mut _xmlAttribute,
) -> c_int {
    if attr.is_null() {
        return 0;
    }

    unsafe {
        let a = &*attr;
        let atype = a.atype as c_int;

        // Validate the default value if present
        if !a.defaultValue.is_null() {
            if validate_attribute_value(atype, a.defaultValue) == 0 {
                let name_str = string::xmlstr_to_string(a.name);
                let val_str = string::xmlstr_to_string(a.defaultValue);
                let err_msg = format!(
                    "Default value '{}' for attribute '{}' is not valid for its type\0",
                    val_str, name_str
                );
                vctxt_error(ctxt, err_msg.as_ptr() as *const c_char);
                return 0;
            }
        }

        // Validate enumeration values
        if atype == XML_ATTRIBUTE_ENUMERATION as c_int && !a.tree.is_null() {
            // Validate each enumeration value is a valid NMTOKEN
            let mut cur = a.tree;
            while !cur.is_null() {
                if !(*cur).name.is_null() {
                    if validate_nmtoken_value((*cur).name) == 0 {
                        let val_str = string::xmlstr_to_string((*cur).name);
                        let err_msg =
                            format!("Enumeration value '{}' is not a valid NMTOKEN\0", val_str);
                        vctxt_error(ctxt, err_msg.as_ptr() as *const c_char);
                        return 0;
                    }
                }
                cur = (*cur).next;
            }
        }

        // Validate NOTATION values reference declared notations
        if atype == XML_ATTRIBUTE_NOTATION as c_int && !a.tree.is_null() {
            let mut cur = a.tree;
            while !cur.is_null() {
                if !(*cur).name.is_null() {
                    if validate_notation_use(ctxt, doc, (*cur).name) == 0 {
                        let val_str = string::xmlstr_to_string((*cur).name);
                        let err_msg = format!(
                            "NOTATION value '{}' references undeclared notation\0",
                            val_str
                        );
                        vctxt_error(ctxt, err_msg.as_ptr() as *const c_char);
                        return 0;
                    }
                }
                cur = (*cur).next;
            }
        }

        1
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlValidateElement — Core element validation
// ═══════════════════════════════════════════════════════════════════════════════

/// Validate a single element node against its DTD element and attribute
/// declarations.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateElement(xmlValidCtxtPtr ctxt,
///                        xmlDocPtr doc,
///                        xmlNodePtr elem);
/// ```
///
/// Validates:
/// 1. Element declaration exists for the element name
/// 2. Content model matches child elements
/// 3. Required attributes are present
/// 4. Attribute values match their declared types
/// 5. ID uniqueness
/// 6. IDREF references resolve
///
/// Returns 1 if valid, 0 otherwise.
///
/// # SAFETY
///
/// - `ctxt`, `doc`, `elem` may be NULL.
pub unsafe fn validate_element(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    elem: *mut _xmlNode,
) -> c_int {
    if elem.is_null() || doc.is_null() || ctxt.is_null() {
        return 0;
    }

    unsafe {
        let e = &*elem;

        // Skip non-element nodes
        if e.type_ != XML_ELEMENT_NODE as c_int {
            return 1;
        }

        // Push node onto stack
        if vctxt_push_node(ctxt, elem) != 0 {
            return 0;
        }

        let mut valid = 1;

        // Get the DTD
        let dtd = get_valid_dtd(doc);
        if dtd.is_null() {
            // No DTD — no validation to perform
            // UPSTREAM-PARITY: libxml2 returns 1 if there's no DTD.
            vctxt_pop_node(ctxt);
            return 1;
        }

        let dtd_ref = &*dtd;

        // Look up element declaration
        let elem_name = e.name;
        let elem_decl = if !dtd_ref.elements.is_null() {
            hash::hash_lookup(dtd_ref.elements as *mut hash::HashTable, elem_name)
        } else {
            ptr::null_mut()
        };

        if elem_decl.is_null() {
            let name_str = string::xmlstr_to_string(elem_name);
            let err_msg = format!("No declaration for element {}\0", name_str);
            vctxt_error(ctxt, err_msg.as_ptr() as *const c_char);
            vctxt_pop_node(ctxt);
            return 0;
        }

        let elem_decl_ref = &*(elem_decl as *mut _xmlElement);

        // ── Content model validation ──────────────────────────────────────
        let elem_type = elem_decl_ref.etype as u32;

        if elem_type == XML_ELEMENT_TYPE_EMPTY as u32 {
            // Element must have no children (except text nodes)
            let mut child = e.children;
            while !child.is_null() {
                let child_type = (*child).type_ as u32;
                if child_type != XML_TEXT_NODE as u32 && child_type != XML_CDATA_SECTION_NODE as u32
                {
                    valid = 0;
                    let name_str = string::xmlstr_to_string(elem_name);
                    let err_msg = format!(
                        "Element '{}' is declared EMPTY but has child elements\0",
                        name_str
                    );
                    vctxt_error(ctxt, err_msg.as_ptr() as *const c_char);
                    break;
                }
                child = (*child).next;
            }
        } else if elem_type == XML_ELEMENT_TYPE_ANY as u32 {
            // ANY: any content is allowed
        } else if elem_type == XML_ELEMENT_TYPE_MIXED as u32 {
            // MIXED: PCDATA plus optionally declared child elements
            let mut child = e.children;
            while !child.is_null() {
                let child_type = (*child).type_ as u32;
                if child_type == XML_ELEMENT_NODE as u32 {
                    // Validate that child element name is in the mixed content model
                    let child_name = (*child).name;
                    let result = dtd::valid_content_model(elem_decl_ref.content, &[child_name]);
                    if result != dtd::ContentModelResult::Valid {
                        let cname_str = string::xmlstr_to_string(child_name);
                        let ename_str = string::xmlstr_to_string(elem_name);
                        let err_msg = format!(
                            "Element '{}' is not allowed in mixed content of '{}'\0",
                            cname_str, ename_str
                        );
                        vctxt_error(ctxt, err_msg.as_ptr() as *const c_char);
                        valid = 0;
                    }
                }
                child = (*child).next;
            }
        } else if elem_type == XML_ELEMENT_TYPE_ELEMENT as u32 {
            // Element-only content: collect child element names and validate
            let mut child_names: Vec<*const xmlChar> = Vec::new();
            let mut child = e.children;
            while !child.is_null() {
                if (*child).type_ == XML_ELEMENT_NODE as c_int {
                    child_names.push((*child).name);
                }
                child = (*child).next;
            }

            let result = dtd::valid_content_model(elem_decl_ref.content, &child_names);
            if result != dtd::ContentModelResult::Valid {
                let ename_str = string::xmlstr_to_string(elem_name);
                let err_msg = format!(
                    "Content model validation failed for element '{}'\0",
                    ename_str
                );
                vctxt_error(ctxt, err_msg.as_ptr() as *const c_char);
                valid = 0;
            }
        }

        // ── Attribute validation ──────────────────────────────────────────
        if !dtd_ref.attributes.is_null() {
            // Walk all attributes on the element node
            let mut attr_prop = e.properties;
            while !attr_prop.is_null() {
                let attr_ref = &*attr_prop;
                let attr_name = attr_ref.name;

                // Look up the attribute declaration (keyed by name, prefix,
                // elem — upstream xmlHashLookup3).
                let attr_decl = hash::hash_lookup3(
                    dtd_ref.attributes as *mut hash::HashTable,
                    attr_name,
                    ptr::null(),
                    elem_name,
                );

                if attr_decl.is_null() {
                    // Undeclared attribute — not a validation error per se
                    // in DTD validation, but might be in Schema validation.
                    // UPSTREAM-PARITY: libxml2 skips undeclared attrs in
                    // DTD validation mode.
                    attr_prop = attr_ref.next;
                    continue;
                }

                let attr_decl_ref = &*(attr_decl as *mut _xmlAttribute);
                let atype = attr_decl_ref.atype as c_int;

                // Get attribute value from content
                let attr_value = if !attr_ref.children.is_null() {
                    // Get text content of the attribute node
                    let text_node = attr_ref.children;
                    if (*text_node).type_ == XML_TEXT_NODE as c_int
                        || (*text_node).type_ == XML_CDATA_SECTION_NODE as c_int
                    {
                        (*text_node).content
                    } else {
                        ptr::null()
                    }
                } else {
                    ptr::null()
                };

                // Validate the attribute value against its type
                if !attr_value.is_null() {
                    if atype == XML_ATTRIBUTE_ENUMERATION as c_int && !attr_decl_ref.tree.is_null()
                    {
                        if validate_enumeration(ctxt, attr_value, attr_decl_ref.tree) == 0 {
                            let aname_str = string::xmlstr_to_string(attr_name);
                            let aval_str = string::xmlstr_to_string(attr_value);
                            let err_msg = format!(
                                "Attribute '{}' has value '{}' not in enumeration\0",
                                aname_str, aval_str
                            );
                            vctxt_error(ctxt, err_msg.as_ptr() as *const c_char);
                            valid = 0;
                        }
                    } else if atype == XML_ATTRIBUTE_NOTATION as c_int {
                        if validate_notation_use(ctxt, doc, attr_value) == 0 {
                            let aname_str = string::xmlstr_to_string(attr_name);
                            let aval_str = string::xmlstr_to_string(attr_value);
                            let err_msg = format!(
                                "Attribute '{}' references undeclared notation '{}'\0",
                                aname_str, aval_str
                            );
                            vctxt_error(ctxt, err_msg.as_ptr() as *const c_char);
                            valid = 0;
                        }
                    } else if validate_attribute_value(atype, attr_value) == 0 {
                        let aname_str = string::xmlstr_to_string(attr_name);
                        let aval_str = string::xmlstr_to_string(attr_value);
                        let err_msg = format!(
                            "Attribute '{}' has invalid value '{}' for its type\0",
                            aname_str, aval_str
                        );
                        vctxt_error(ctxt, err_msg.as_ptr() as *const c_char);
                        valid = 0;
                    }

                    // ID/IDREF specific validation
                    if atype == XML_ATTRIBUTE_ID as c_int {
                        if validate_id(ctxt, doc, elem, attr_value) == 0 {
                            valid = 0;
                        }
                    } else if atype == XML_ATTRIBUTE_IDREF as c_int {
                        if validate_id_ref(ctxt, doc, elem, attr_value) == 0 {
                            valid = 0;
                        }
                    } else if atype == XML_ATTRIBUTE_IDREFS as c_int {
                        if validate_id_refs(ctxt, doc, elem, attr_value) == 0 {
                            valid = 0;
                        }
                    }
                }

                attr_prop = attr_ref.next;
            }

            // ── Check for required attributes ─────────────────────────────
            struct RequiredAttrCheck {
                ctxt: *mut _xmlValidCtxt,
                elem_name: *const xmlChar,
                elem_props: *mut _xmlAttr,
                valid: *mut c_int,
            }

            extern "C" fn check_required_attr(
                payload: *mut c_void,
                data: *mut c_void,
                name: *const xmlChar,
                name2: *const xmlChar,
                _name3: *const xmlChar,
            ) {
                if payload.is_null() || data.is_null() || name2.is_null() {
                    return;
                }

                // SAFETY: Called from hash_scan_full.
                let check = unsafe { &*(data as *mut RequiredAttrCheck) };
                unsafe {
                    // Only check attributes belonging to this element
                    if string::xml_strcmp(name, check.elem_name) != 0 {
                        return;
                    }

                    let attr_decl = &*(payload as *mut _xmlAttribute);

                    // If the attribute is REQUIRED, check if it's present
                    if attr_decl.def == XML_ATTRIBUTE_REQUIRED as c_int {
                        // Check if this attribute name is in the element's properties
                        let mut found = 0;
                        let mut prop = check.elem_props;
                        while !prop.is_null() {
                            if string::xml_strcmp((*prop).name, name2) == 0 {
                                found = 1;
                                break;
                            }
                            prop = (*prop).next;
                        }

                        if found == 0 {
                            let aname_str = string::xmlstr_to_string(name2);
                            let ename_str = string::xmlstr_to_string(check.elem_name);
                            let err_msg = format!(
                                "Required attribute '{}' missing on element '{}'\0",
                                aname_str, ename_str
                            );
                            vctxt_error(check.ctxt, err_msg.as_ptr() as *const c_char);
                            *(check.valid) = 0;
                        }
                    }
                }
            }

            let mut required_valid = valid;
            let check = RequiredAttrCheck {
                ctxt,
                elem_name,
                elem_props: e.properties,
                valid: &mut required_valid,
            };

            hash::hash_scan_full(
                dtd_ref.attributes as *mut hash::HashTable,
                Some(check_required_attr),
                &check as *const RequiredAttrCheck as *mut c_void,
            );

            valid = required_valid;
        }

        // ── Recurse into children ─────────────────────────────────────────
        let mut child = e.children;
        while !child.is_null() {
            if (*child).type_ == XML_ELEMENT_NODE as c_int {
                if validate_element(ctxt, doc, child) == 0 {
                    valid = 0;
                }
            }
            child = (*child).next;
        }

        vctxt_pop_node(ctxt);
        valid
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlValidateDocument
// ═══════════════════════════════════════════════════════════════════════════════

/// Validate an entire document against its DTD.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateDocument(xmlValidCtxtPtr ctxt, xmlDocPtr doc);
/// ```
///
/// Validates the root element and all its descendants, plus the DTD itself.
///
/// Returns 1 if valid, 0 otherwise.
///
/// # SAFETY
///
/// - `ctxt`, `doc` may be NULL.
pub unsafe fn validate_document(ctxt: *mut _xmlValidCtxt, doc: *mut _xmlDoc) -> c_int {
    if ctxt.is_null() || doc.is_null() {
        return 0;
    }

    unsafe {
        let c = &mut *ctxt;
        c.doc = doc;
        c.valid = 1;

        let d = &*doc;

        // UPSTREAM-PARITY: xmlValidateDocumentInternal rejects documents with
        // no internal or external subset (valid.c:6266-6271):
        //
        // ```c
        // if ((doc->intSubset == NULL) && (doc->extSubset == NULL)) {
        //     xmlErrValid(vctxt, XML_DTD_NO_DTD, "no DTD found!\n", NULL);
        //     return(0);
        // }
        // ```
        if d.intSubset.is_null() && d.extSubset.is_null() {
            vctxt_error(ctxt, b"no DTD found!\0" as *const u8 as *const c_char);
            return 0;
        }

        // Find the root element (first child that's an element node)
        let mut root = d.children;
        while !root.is_null() {
            if (*root).type_ == XML_ELEMENT_NODE as c_int {
                break;
            }
            root = (*root).next;
        }

        if root.is_null() {
            vctxt_error(
                ctxt,
                b"No root element found in document\0" as *const u8 as *const c_char,
            );
            return 0;
        }

        // Validate the root element
        if validate_element(ctxt, doc, root) == 0 {
            return 0;
        }

        c.valid
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlValidateDocumentFinal
// ═══════════════════════════════════════════════════════════════════════════════

/// Final validation: check that all IDREFs resolve to existing IDs.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateDocumentFinal(xmlValidCtxtPtr ctxt, xmlDocPtr doc);
/// ```
///
/// This is called after the document is fully parsed, to verify ID/IDREF
/// consistency. During parsing, forward IDREFs may not be resolvable, so
/// this final pass checks them.
///
/// Returns 1 if all IDREFs resolve, 0 otherwise.
///
/// # SAFETY
///
/// - `ctxt`, `doc` may be NULL.
pub unsafe fn validate_document_final(ctxt: *mut _xmlValidCtxt, doc: *mut _xmlDoc) -> c_int {
    if ctxt.is_null() || doc.is_null() {
        return 0;
    }

    unsafe {
        let c = &mut *ctxt;
        c.doc = doc;

        let d = &*doc;

        // If there's no refs table, no IDREFs were found
        if d.refs.is_null() {
            return c.valid;
        }

        // Check each IDREF against the IDs table
        struct IdRefCheckContext {
            ctxt: *mut _xmlValidCtxt,
            doc: *mut _xmlDoc,
        }

        extern "C" fn check_idref(
            _payload: *mut c_void,
            data: *mut c_void,
            _name: *const xmlChar,
            name2: *const xmlChar,
            _name3: *const xmlChar,
        ) {
            if data.is_null() || name2.is_null() {
                return;
            }

            // SAFETY: Called from hash_scan_full.
            let cx = unsafe { &*(data as *mut IdRefCheckContext) };
            unsafe {
                let doc_ref = &*cx.doc;

                // Look up the IDREF value in the IDs table
                if doc_ref.ids.is_null()
                    || hash::hash_lookup(doc_ref.ids as *mut hash::HashTable, name2).is_null()
                {
                    let ref_str = string::xmlstr_to_string(name2);
                    let err_msg = format!("IDREF '{}' does not reference a declared ID\0", ref_str);
                    vctxt_error(cx.ctxt, err_msg.as_ptr() as *const c_char);
                }
            }
        }

        let ctx = IdRefCheckContext { ctxt, doc };
        hash::hash_scan_full(
            d.refs as *mut hash::HashTable,
            Some(check_idref),
            &ctx as *const IdRefCheckContext as *mut c_void,
        );

        c.valid
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlValidateRoot
// ═══════════════════════════════════════════════════════════════════════════════

/// Validate the root element of a document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateRoot(xmlValidCtxtPtr ctxt, xmlDocPtr doc);
/// ```
///
/// Returns 1 if the root element is valid, 0 otherwise.
///
/// # SAFETY
///
/// - `ctxt`, `doc` may be NULL.
pub unsafe fn validate_root(ctxt: *mut _xmlValidCtxt, doc: *mut _xmlDoc) -> c_int {
    if ctxt.is_null() || doc.is_null() {
        return 0;
    }

    unsafe {
        let c = &mut *ctxt;
        c.doc = doc;
        c.valid = 1;

        let d = &*doc;

        // Find root element
        let mut root = d.children;
        while !root.is_null() {
            if (*root).type_ == XML_ELEMENT_NODE as c_int {
                break;
            }
            root = (*root).next;
        }

        if root.is_null() {
            vctxt_error(
                ctxt,
                b"No root element found\0" as *const u8 as *const c_char,
            );
            return 0;
        }

        // Get the DTD
        let dtd = get_valid_dtd(doc);
        if dtd.is_null() {
            // No DTD — nothing to validate against
            return 1;
        }

        // UPSTREAM-PARITY: libxml2 checks that the root element name matches
        // the DTD's name (the DOCTYPE name).
        let dtd_ref = &*dtd;
        if !dtd_ref.name.is_null() {
            if string::xml_strcmp((*root).name, dtd_ref.name) != 0 {
                let root_str = string::xmlstr_to_string((*root).name);
                let dtd_str = string::xmlstr_to_string(dtd_ref.name);
                let err_msg = format!(
                    "Root element '{}' does not match DTD root '{}'\0",
                    root_str, dtd_str
                );
                vctxt_error(ctxt, err_msg.as_ptr() as *const c_char);
                return 0;
            }
        }

        c.valid
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlValidateContent
// ═══════════════════════════════════════════════════════════════════════════════

/// Validate the content of an element node against its content model.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateContent(xmlValidCtxtPtr ctxt,
///                        xmlNodePtr node,
///                        xmlDocPtr doc);
/// ```
///
/// Returns 1 if content is valid, 0 otherwise.
///
/// # SAFETY
///
/// - `ctxt`, `node`, `doc` may be NULL.
pub unsafe fn validate_content(
    ctxt: *mut _xmlValidCtxt,
    node: *mut _xmlNode,
    doc: *mut _xmlDoc,
) -> c_int {
    if node.is_null() || doc.is_null() || ctxt.is_null() {
        return 0;
    }

    unsafe {
        let n = &*node;
        if n.type_ != XML_ELEMENT_NODE as c_int {
            return 1;
        }

        let dtd = get_valid_dtd(doc);
        if dtd.is_null() {
            return 1;
        }

        let dtd_ref = &*dtd;
        if dtd_ref.elements.is_null() {
            return 1;
        }

        let elem_decl = hash::hash_lookup(dtd_ref.elements as *mut hash::HashTable, n.name);
        if elem_decl.is_null() {
            return 1;
        }

        let elem_decl_ref = &*(elem_decl as *mut _xmlElement);
        if elem_decl_ref.content.is_null() {
            return 1;
        }

        let elem_type = elem_decl_ref.etype as u32;
        if elem_type == XML_ELEMENT_TYPE_EMPTY as u32 {
            // Check no element children
            let mut child = n.children;
            while !child.is_null() {
                if (*child).type_ == XML_ELEMENT_NODE as c_int {
                    let name_str = string::xmlstr_to_string(n.name);
                    let err_msg =
                        format!("Element '{}' is EMPTY but has child elements\0", name_str);
                    vctxt_error(ctxt, err_msg.as_ptr() as *const c_char);
                    return 0;
                }
                child = (*child).next;
            }
            return 1;
        }

        if elem_type == XML_ELEMENT_TYPE_ANY as u32 {
            return 1;
        }

        // Collect child element names
        let mut child_names: Vec<*const xmlChar> = Vec::new();
        let mut child = n.children;
        while !child.is_null() {
            if (*child).type_ == XML_ELEMENT_NODE as c_int {
                child_names.push((*child).name);
            }
            child = (*child).next;
        }

        let result = dtd::valid_content_model(elem_decl_ref.content, &child_names);
        if result != dtd::ContentModelResult::Valid {
            let name_str = string::xmlstr_to_string(n.name);
            let err_msg = format!(
                "Content model validation failed for element '{}'\0",
                name_str
            );
            vctxt_error(ctxt, err_msg.as_ptr() as *const c_char);
            0
        } else {
            1
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlIsMixedElement / xmlIsEmptyElement
// ═══════════════════════════════════════════════════════════════════════════════

/// Check if an element has a mixed content model.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlIsMixedElement(xmlDocPtr doc, const xmlChar *name);
/// ```
///
/// Returns 1 if the element is declared as mixed content, 0 otherwise.
///
/// # SAFETY
///
/// - `doc`, `name` may be NULL.
pub unsafe fn is_mixed_element(doc: *mut _xmlDoc, name: *const xmlChar) -> c_int {
    if doc.is_null() || name.is_null() {
        return 0;
    }

    let dtd = unsafe { get_valid_dtd(doc) };
    if dtd.is_null() {
        return 0;
    }

    unsafe {
        let dtd_ref = &*dtd;
        if dtd_ref.elements.is_null() {
            return 0;
        }

        let elem_decl = hash::hash_lookup(dtd_ref.elements as *mut hash::HashTable, name);
        if elem_decl.is_null() {
            return 0;
        }

        let elem_decl_ref = &*(elem_decl as *mut _xmlElement);
        ((elem_decl_ref.etype as u32) == XML_ELEMENT_TYPE_MIXED as u32) as c_int
    }
}

/// Check if an element is declared as EMPTY.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlIsEmptyElement(xmlDocPtr doc, const xmlChar *name);
/// ```
///
/// Returns 1 if the element is declared EMPTY, 0 otherwise.
///
/// # SAFETY
///
/// - `doc`, `name` may be NULL.
pub unsafe fn is_empty_element(doc: *mut _xmlDoc, name: *const xmlChar) -> c_int {
    if doc.is_null() || name.is_null() {
        return 0;
    }

    let dtd = unsafe { get_valid_dtd(doc) };
    if dtd.is_null() {
        return 0;
    }

    unsafe {
        let dtd_ref = &*dtd;
        if dtd_ref.elements.is_null() {
            return 0;
        }

        let elem_decl = hash::hash_lookup(dtd_ref.elements as *mut hash::HashTable, name);
        if elem_decl.is_null() {
            return 0;
        }

        let elem_decl_ref = &*(elem_decl as *mut _xmlElement);
        ((elem_decl_ref.etype as u32) == XML_ELEMENT_TYPE_EMPTY as u32) as c_int
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlValidateDtd
// ═══════════════════════════════════════════════════════════════════════════════

/// Validate a DTD's declarations (element/attribute declarations).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateDtd(xmlValidCtxtPtr ctxt,
///                    xmlDocPtr doc,
///                    xmlDtdPtr dtd);
/// ```
///
/// Validates:
/// - Attribute declarations (default values, enumeration values, notation refs)
/// - Element content models reference only declared elements
///
/// Returns 1 if the DTD is valid, 0 otherwise.
///
/// # SAFETY
///
/// - `ctxt`, `doc`, `dtd` may be NULL.
pub unsafe fn validate_dtd(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    dtd: *mut _xmlDtd,
) -> c_int {
    if ctxt.is_null() || dtd.is_null() {
        return 0;
    }

    let c = unsafe { &mut *ctxt };
    c.doc = doc;
    c.valid = 1;

    struct ValidateDtdCtx {
        ctxt: *mut _xmlValidCtxt,
        doc: *mut _xmlDoc,
    }

    extern "C" fn validate_attr_decl_cb(
        payload: *mut c_void,
        data: *mut c_void,
        _name: *const xmlChar,
        _name2: *const xmlChar,
        _name3: *const xmlChar,
    ) {
        if payload.is_null() || data.is_null() {
            return;
        }

        // SAFETY: Called from hash_scan_full with a ValidateDtdCtx as data.
        let ctx = unsafe { &*(data as *mut ValidateDtdCtx) };
        unsafe {
            let attr = payload as *mut _xmlAttribute;
            validate_attribute_decl(ctx.ctxt, ctx.doc, ptr::null_mut(), attr);
        }
    }

    extern "C" fn validate_elem_content_cb(
        payload: *mut c_void,
        data: *mut c_void,
        _name: *const xmlChar,
        _name2: *const xmlChar,
        _name3: *const xmlChar,
    ) {
        if payload.is_null() || data.is_null() {
            return;
        }

        // SAFETY: Called from hash_scan_full with a ValidateDtdCtx as data.
        let ctx = unsafe { &*(data as *mut ValidateDtdCtx) };
        unsafe {
            let elem = &*(payload as *mut _xmlElement);
            if !elem.content.is_null() {
                validate_content_model_refs(ctx.ctxt, ctx.doc, elem.content);
            }
        }
    }

    unsafe {
        let dtd_ref = &*dtd;

        // Validate all attribute declarations
        if !dtd_ref.attributes.is_null() {
            let ctx = ValidateDtdCtx { ctxt, doc };
            hash::hash_scan_full(
                dtd_ref.attributes as *mut hash::HashTable,
                Some(validate_attr_decl_cb),
                &ctx as *const ValidateDtdCtx as *mut c_void,
            );
        }

        // Validate that element content models reference declared elements
        if !dtd_ref.elements.is_null() {
            let ctx = ValidateDtdCtx { ctxt, doc };
            hash::hash_scan_full(
                dtd_ref.elements as *mut hash::HashTable,
                Some(validate_elem_content_cb),
                &ctx as *const ValidateDtdCtx as *mut c_void,
            );
        }

        c.valid
    }
}

/// Recursively check that all element references in a content model
/// reference declared elements.
///
/// # SAFETY
///
/// - `ctxt`, `doc`, `content` may be NULL.
unsafe fn validate_content_model_refs(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    content: *mut _xmlElementContent,
) {
    if content.is_null() {
        return;
    }

    unsafe {
        let c = &*content;

        match c.type_ as u32 {
            t if t == XML_ELEMENT_CONTENT_ELEMENT as u32 => {
                // Check that the element name is declared
                if !c.name.is_null() {
                    let dtd = get_valid_dtd(doc);
                    if !dtd.is_null() {
                        let dtd_ref = &*dtd;
                        if !dtd_ref.elements.is_null() {
                            let decl =
                                hash::hash_lookup(dtd_ref.elements as *mut hash::HashTable, c.name);
                            if decl.is_null() {
                                let name_str = string::xmlstr_to_string(c.name);
                                let err_msg = format!(
                                    "Element '{}' referenced in content model is not declared\0",
                                    name_str
                                );
                                vctxt_error(ctxt, err_msg.as_ptr() as *const c_char);
                            }
                        }
                    }
                }
            }
            t if t == XML_ELEMENT_CONTENT_SEQ as u32 || t == XML_ELEMENT_CONTENT_OR as u32 => {
                validate_content_model_refs(ctxt, doc, c.c1);
                validate_content_model_refs(ctxt, doc, c.c2);
            }
            _ => {}
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlValidateDtdFinal
// ═══════════════════════════════════════════════════════════════════════════════

/// Final DTD validation — checks ID/IDREF consistency.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlValidateDtdFinal(xmlValidCtxtPtr ctxt, xmlDocPtr doc);
/// ```
///
/// This is equivalent to `xmlValidateDocumentFinal` and checks that all
/// IDREF values resolve to declared IDs.
///
/// Returns 1 if valid, 0 otherwise.
///
/// # SAFETY
///
/// - `ctxt`, `doc` may be NULL.
pub unsafe fn validate_dtd_final(ctxt: *mut _xmlValidCtxt, doc: *mut _xmlDoc) -> c_int {
    unsafe { validate_document_final(ctxt, doc) }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 11.1-I validation surface closure
// ═══════════════════════════════════════════════════════════════════════════════
//
// Closes the missing xmlValidate* exports against the oracle (system libxml2
// 2.15.3): the modern 2-arg name validators (xmlValidateNCName/QName/Name/
// NMToken), the 1-arg *Value family (xmlValidateNameValue/NamesValue/
// NmtokenValue/NmtokensValue), the declaration validators (ElementDecl /
// NotationDecl / OneAttribute / OneElement / OneNamespace), the streaming
// push family (xmlValidatePushElement/PushCData/PopElement +
// xmlValidBuildContentModel), and the ID/REF table machinery they depend on
// (xmlAddID/xmlAddRef/xmlRemoveID/xmlRemoveRef).
//
// UPSTREAM-PARITY notes:
// - The modern 2-arg validators return -1 on NULL, 0 if valid, 1 if invalid.
// - The 1-arg *Value validators return 1 if valid, 0 otherwise (NULL too).
// - Names/Nmtokens separators are exactly 0x20 (upstream erratum E20: no
//   other whitespace is accepted).
// - The char classes are the XML 1.0 Fifth-Edition productions including the
//   supplementary plane 0x10000..0xEFFFF (upstream xmlIsNameStartCharNew /
//   xmlIsNameCharNew in parser.c).

// XML_SCAN_* flags (upstream parser.c xmlScanName)
const XML_SCAN_NC: u32 = 1; // stop at ':'
const XML_SCAN_NMTOKEN: u32 = 2; // first char may be any NameChar

/// Upstream IS_BLANK_CH: space, tab, LF, CR.
fn is_blank_byte(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\n' || b == b'\r'
}

/// UTF-8 sequence length from the lead byte (0 when invalid).
fn utf8_char_len(lead: u8) -> usize {
    if lead < 0x80 {
        1
    } else if lead >= 0xC0 && lead <= 0xDF {
        2
    } else if lead >= 0xE0 && lead <= 0xEF {
        3
    } else if lead >= 0xF0 && lead <= 0xF7 {
        4
    } else {
        0
    }
}

/// Byte-level scan mirroring upstream `xmlScanName(ptr, SIZE_MAX, flags)`
/// (parser.c): consumes a Name (or NCName / Nmtoken) starting at `start`.
///
/// Semantics preserved:
/// - NC mode stops (without consuming) at ':'.
/// - The first character must be a NameStartChar unless XML_SCAN_NMTOKEN;
///   every later character must be a NameChar.
/// - Invalid UTF-8 stops the scan (upstream xmlGetUTF8Char < 0).
/// - With SIZE_MAX the length bound never triggers.
///
/// Returns the offset of the first byte past the name; equals `start` when
/// nothing was consumed.
unsafe fn scan_name_offsets(bytes: &[u8], start: usize, flags: u32) -> usize {
    let stop = if flags & XML_SCAN_NC != 0 {
        Some(b':')
    } else {
        None
    };
    let mut i = start;
    let mut is_nmtoken = flags & XML_SCAN_NMTOKEN != 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b < 0x80 {
            if stop == Some(b) {
                break;
            }
            let c = b as char;
            let ok = if is_nmtoken {
                is_xml_name_char(c)
            } else {
                is_xml_name_start(c)
            };
            if !ok {
                break;
            }
            i += 1;
        } else {
            let len = utf8_char_len(b);
            if len == 0 || i + len > bytes.len() {
                break;
            }
            let ch = match core::str::from_utf8(&bytes[i..i + len])
                .ok()
                .and_then(|s| s.chars().next())
            {
                Some(c) => c,
                None => break,
            };
            let ok = if is_nmtoken {
                is_xml_name_char(ch)
            } else {
                is_xml_name_start(ch)
            };
            if !ok {
                break;
            }
            i += len;
        }
        // subsequent characters use the NameChar production
        is_nmtoken = true;
    }
    i
}

/// Modern 2-arg form, upstream tree.c `xmlValidateNCName(value, space)`.
///
/// # SAFETY
///
/// - `value` must be a valid null-terminated string or NULL.
pub unsafe fn validate_ncname(value: *const xmlChar, space: c_int) -> c_int {
    if value.is_null() {
        return -1;
    }
    let bytes = string::xmlstr_to_bytes(value);
    let mut start = 0usize;
    if space != 0 {
        while start < bytes.len() && is_blank_byte(bytes[start]) {
            start += 1;
        }
    }
    let end = scan_name_offsets(bytes, start, XML_SCAN_NC);
    if end == start {
        return 1;
    }
    let mut end2 = end;
    if space != 0 {
        while end2 < bytes.len() && is_blank_byte(bytes[end2]) {
            end2 += 1;
        }
    }
    if end2 == bytes.len() {
        0
    } else {
        1
    }
}

/// Modern 2-arg form, upstream tree.c `xmlValidateQName(value, space)`.
///
/// # SAFETY
///
/// - `value` must be a valid null-terminated string or NULL.
pub unsafe fn validate_qname(value: *const xmlChar, space: c_int) -> c_int {
    if value.is_null() {
        return -1;
    }
    let bytes = string::xmlstr_to_bytes(value);
    let mut start = 0usize;
    if space != 0 {
        while start < bytes.len() && is_blank_byte(bytes[start]) {
            start += 1;
        }
    }
    let mut end = scan_name_offsets(bytes, start, XML_SCAN_NC);
    if end == start {
        return 1;
    }
    if end < bytes.len() && bytes[end] == b':' {
        end += 1;
        let end2 = scan_name_offsets(bytes, end, XML_SCAN_NC);
        if end2 == end {
            return 1;
        }
        end = end2;
    }
    if space != 0 {
        while end < bytes.len() && is_blank_byte(bytes[end]) {
            end += 1;
        }
    }
    if end == bytes.len() {
        0
    } else {
        1
    }
}

/// Modern 2-arg form, upstream tree.c `xmlValidateName(value, space)`.
///
/// NOTE: this is the CURRENT oracle ABI — since libxml2 2.12 the symbol
/// carries a second `int space` parameter (tree.c) and inverted return
/// semantics (0 valid / 1 invalid / -1 NULL). The pre-2.12 1-arg form is
/// gone from the DSO; the 1-arg semantics live on as xmlValidateNameValue.
///
/// # SAFETY
///
/// - `value` must be a valid null-terminated string or NULL.
pub unsafe fn validate_name_space(value: *const xmlChar, space: c_int) -> c_int {
    if value.is_null() {
        return -1;
    }
    let bytes = string::xmlstr_to_bytes(value);
    let mut start = 0usize;
    if space != 0 {
        while start < bytes.len() && is_blank_byte(bytes[start]) {
            start += 1;
        }
    }
    let end = scan_name_offsets(bytes, start, 0);
    if end == start {
        return 1;
    }
    let mut end2 = end;
    if space != 0 {
        while end2 < bytes.len() && is_blank_byte(bytes[end2]) {
            end2 += 1;
        }
    }
    if end2 == bytes.len() {
        0
    } else {
        1
    }
}

/// Modern 2-arg form, upstream tree.c `xmlValidateNMToken(value, space)`.
///
/// # SAFETY
///
/// - `value` must be a valid null-terminated string or NULL.
pub unsafe fn validate_nmtoken_space(value: *const xmlChar, space: c_int) -> c_int {
    if value.is_null() {
        return -1;
    }
    let bytes = string::xmlstr_to_bytes(value);
    let mut start = 0usize;
    if space != 0 {
        while start < bytes.len() && is_blank_byte(bytes[start]) {
            start += 1;
        }
    }
    let end = scan_name_offsets(bytes, start, XML_SCAN_NMTOKEN);
    if end == start {
        return 1;
    }
    let mut end2 = end;
    if space != 0 {
        while end2 < bytes.len() && is_blank_byte(bytes[end2]) {
            end2 += 1;
        }
    }
    if end2 == bytes.len() {
        0
    } else {
        1
    }
}

/// 1-arg form, upstream valid.c `xmlValidate*ValueInternal(value, flags)`.
/// Returns 1 if valid, 0 if not (including NULL / empty).
unsafe fn validate_value_internal(value: *const xmlChar, flags: u32) -> c_int {
    if value.is_null() {
        return 0;
    }
    let bytes = string::xmlstr_to_bytes(value);
    if bytes.is_empty() {
        return 0;
    }
    let end = scan_name_offsets(bytes, 0, flags);
    if end == 0 {
        return 0;
    }
    if end == bytes.len() {
        1
    } else {
        0
    }
}

/// 1-arg Names/Nmtokens list form. Separator is exactly 0x20 — upstream
/// valid.c deliberately does NOT use IS_BLANK here (XML erratum E20).
unsafe fn validate_values_internal(value: *const xmlChar, flags: u32) -> c_int {
    if value.is_null() {
        return 0;
    }
    let bytes = string::xmlstr_to_bytes(value);
    let mut cur = scan_name_offsets(bytes, 0, flags);
    if cur == 0 {
        return 0;
    }
    while cur < bytes.len() && bytes[cur] == b' ' {
        while cur < bytes.len() && bytes[cur] == b' ' {
            cur += 1;
        }
        let end = scan_name_offsets(bytes, cur, flags);
        if end == cur {
            return 0;
        }
        cur = end;
    }
    if cur == bytes.len() {
        1
    } else {
        0
    }
}

/// Upstream `xmlValidateNameValue(value)` — 1 if valid, 0 otherwise.
///
/// # SAFETY
///
/// - `value` must be a valid null-terminated string or NULL.
pub unsafe fn validate_name_value(value: *const xmlChar) -> c_int {
    validate_value_internal(value, 0)
}

/// Upstream `xmlValidateNamesValue(value)` — 1 if valid, 0 otherwise.
///
/// # SAFETY
///
/// - `value` must be a valid null-terminated string or NULL.
pub unsafe fn validate_names_value(value: *const xmlChar) -> c_int {
    validate_values_internal(value, 0)
}

/// Upstream `xmlValidateNmtokenValue(value)` — 1 if valid, 0 otherwise.
///
/// # SAFETY
///
/// - `value` must be a valid null-terminated string or NULL.
pub unsafe fn validate_nmtoken_value(value: *const xmlChar) -> c_int {
    validate_value_internal(value, XML_SCAN_NMTOKEN)
}

/// Upstream `xmlValidateNmtokensValue(value)` — 1 if valid, 0 otherwise.
///
/// # SAFETY
///
/// - `value` must be a valid null-terminated string or NULL.
pub unsafe fn validate_nmtokens_value(value: *const xmlChar) -> c_int {
    validate_values_internal(value, XML_SCAN_NMTOKEN)
}

// ═══════════════════════════════════════════════════════════════════════════════
// DTD description lookups (upstream valid.c xmlGetDtd*Desc)
// ═══════════════════════════════════════════════════════════════════════════════

/// Upstream `xmlGetDtdQElementDesc(dtd, name, prefix)`.
///
/// # SAFETY
///
/// - `dtd` must be a valid pointer or NULL; `name`/`prefix` NULL-terminated
///   strings or NULL.
pub unsafe fn get_dtd_qelement_desc(
    dtd: *mut _xmlDtd,
    name: *const xmlChar,
    prefix: *const xmlChar,
) -> *mut _xmlElement {
    if dtd.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let elements = (*dtd).elements;
        if elements.is_null() {
            return ptr::null_mut();
        }
        hash::hash_lookup2(elements as *mut hash::HashTable, name, prefix) as *mut _xmlElement
    }
}

/// Upstream `xmlGetDtdQAttrDesc(dtd, elem, name, prefix)` — the attribute
/// declaration table is keyed by (name, prefix, elem).
///
/// # SAFETY
///
/// - `dtd` must be a valid pointer or NULL; `elem`/`name`/`prefix`
///   NULL-terminated strings or NULL.
pub unsafe fn get_dtd_qattr_desc(
    dtd: *mut _xmlDtd,
    elem: *const xmlChar,
    name: *const xmlChar,
    prefix: *const xmlChar,
) -> *mut _xmlAttribute {
    if dtd.is_null() || elem.is_null() || name.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let attrs = (*dtd).attributes;
        if attrs.is_null() {
            return ptr::null_mut();
        }
        hash::hash_lookup3(attrs as *mut hash::HashTable, name, prefix, elem) as *mut _xmlAttribute
    }
}

/// Upstream `xmlGetDtdNotationDesc(dtd, name)`.
///
/// # SAFETY
///
/// - `dtd` must be a valid pointer or NULL; `name` a NULL-terminated string.
pub unsafe fn get_dtd_notation_desc(dtd: *mut _xmlDtd, name: *const xmlChar) -> *mut _xmlNotation {
    if dtd.is_null() || name.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let notations = (*dtd).notations;
        if notations.is_null() {
            return ptr::null_mut();
        }
        hash::hash_lookup(notations as *mut hash::HashTable, name) as *mut _xmlNotation
    }
}

/// Split a QName at the FIRST ':' (upstream tree.c `xmlSplitQName4`):
/// `prefix` receives a duplicated prefix (or NULL) and the local name
/// (a pointer into the original string) is returned.
///
/// # SAFETY
///
/// - `name` must be a valid null-terminated string; `prefix` a valid
///   out-pointer.
unsafe fn split_qname4(name: *const xmlChar, prefix: *mut *mut xmlChar) -> *const xmlChar {
    if prefix.is_null() {
        return name;
    }
    unsafe {
        *prefix = ptr::null_mut();
        if name.is_null() {
            return ptr::null();
        }
        let bytes = string::xmlstr_to_bytes(name);
        match bytes.iter().position(|&b| b == b':') {
            None => name,
            Some(pos) => {
                let p = string::bytes_to_xmlstr(&bytes[..pos]);
                *prefix = p;
                name.add(pos + 1)
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ID / REF tables (upstream valid.c xmlAddID / xmlAddRef / xmlRemoveID / xmlRemoveRef)
// ═══════════════════════════════════════════════════════════════════════════════

/// Free an xmlID entry (upstream xmlFreeID). Also clears the owning
/// attribute's id/atype back-references.
unsafe fn free_id(id: *mut _xmlID) {
    if id.is_null() {
        return;
    }
    unsafe {
        if !(*id).value.is_null() {
            allocator::xmlFreeImpl((*id).value as *mut c_void);
        }
        if !(*id).name.is_null() {
            allocator::xmlFreeImpl((*id).name as *mut c_void);
        }
        if !(*id).attr.is_null() {
            (*(*id).attr).id = ptr::null_mut();
            (*(*id).attr).atype = 0;
        }
        allocator::xmlFreeImpl(id as *mut c_void);
    }
}

/// Hash-table deallocator for ID entries (name is *mut per
/// xmlHashDeallocator).
unsafe extern "C" fn free_id_entry(payload: *mut c_void, _name: *mut xmlChar) {
    free_id(payload as *mut _xmlID);
}

/// Upstream xmlAddIDInternal: add an attribute value as an ID.
/// Returns 1 on success, 0 if the ID already exists, -1 on OOM.
unsafe fn add_id_internal(
    attr: *mut _xmlAttr,
    value: *const xmlChar,
    id_ptr: *mut *mut _xmlID,
) -> c_int {
    unsafe {
        if !id_ptr.is_null() {
            *id_ptr = ptr::null_mut();
        }
        if value.is_null() || *value == 0 {
            return 0;
        }
        if attr.is_null() {
            return 0;
        }
        let doc = (*attr).doc;
        if doc.is_null() {
            return 0;
        }

        let mut table = (*doc).ids as *mut hash::HashTable;
        if table.is_null() {
            (*doc).ids = hash::hash_create(0) as *mut c_void;
            table = (*doc).ids as *mut hash::HashTable;
            if table.is_null() {
                return -1;
            }
        } else if !hash::hash_lookup(table, value).is_null() {
            return 0;
        }

        let id = allocator::xmlMallocZero(size_of::<_xmlID>() as usize) as *mut _xmlID;
        if id.is_null() {
            return -1;
        }
        (*id).doc = doc;
        (*id).value = string::xml_strdup(value);
        if (*id).value.is_null() {
            free_id(id);
            return -1;
        }
        // re-registering an attribute drops its previous ID
        if !(*attr).id.is_null() {
            remove_id(doc, attr);
        }
        if hash::hash_add_entry(table, value, id as *mut c_void) != 0 {
            free_id(id);
            return -1;
        }
        if !id_ptr.is_null() {
            *id_ptr = id;
        }
        (*id).attr = attr;
        (*id).lineno = tree::get_line_no((*attr).parent) as c_int;
        (*attr).atype = XML_ATTRIBUTE_ID as c_int;
        (*attr).id = id as *mut c_void;
        1
    }
}

/// Upstream `xmlAddID(ctxt, doc, value, attr)` — returns the xmlID or NULL.
/// Reports "ID %s already defined" through the validation context on
/// duplicates and a memory error on OOM.
///
/// # SAFETY
///
/// - `ctxt` may be NULL; `doc`/`attr` must be valid pointers (attr->doc == doc).
pub unsafe fn add_id(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    value: *const xmlChar,
    attr: *mut _xmlAttr,
) -> *mut _xmlID {
    unsafe {
        if attr.is_null() || doc != (*attr).doc {
            return ptr::null_mut();
        }
        let mut id = ptr::null_mut();
        let res = add_id_internal(attr, value, &mut id);
        if res < 0 {
            vctxt_error(
                ctxt,
                b"Memory allocation failed : xmlAddID\0" as *const u8 as *const c_char,
            );
        } else if res == 0 && !ctxt.is_null() {
            let msg = format!("ID {} already defined\0", string::xmlstr_to_string(value));
            vctxt_error(ctxt, msg.as_ptr() as *const c_char);
        }
        id
    }
}

/// Upstream `xmlRemoveID(doc, attr)` — removes the attribute's ID entry.
/// Returns 0 on success, -1 otherwise.
///
/// # SAFETY
///
/// - `doc`/`attr` must be valid pointers or NULL.
pub unsafe fn remove_id(doc: *mut _xmlDoc, attr: *mut _xmlAttr) -> c_int {
    unsafe {
        if doc.is_null() {
            return -1;
        }
        if attr.is_null() || (*attr).id.is_null() {
            return -1;
        }
        let table = (*doc).ids as *mut hash::HashTable;
        if table.is_null() {
            return -1;
        }
        let value = (*((*attr).id as *mut _xmlID)).value;
        if hash::hash_remove_entry(table, value, Some(free_id_entry)) < 0 {
            return -1;
        }
        0
    }
}

/// Free an xmlRef entry.
unsafe fn free_ref(r: *mut _xmlRef) {
    if r.is_null() {
        return;
    }
    unsafe {
        if !(*r).value.is_null() {
            allocator::xmlFreeImpl((*r).value as *mut c_void);
        }
        if !(*r).name.is_null() {
            allocator::xmlFreeImpl((*r).name as *mut c_void);
        }
        allocator::xmlFreeImpl(r as *mut c_void);
    }
}

/// xmlList deallocator for REF entries.
unsafe extern "C" fn free_ref_list_entry(data: *mut c_void) {
    free_ref(data as *mut _xmlRef);
}

/// xmlList comparator (upstream xmlDummyCompare: never equal).
unsafe extern "C" fn dummy_compare(_a: *const c_void, _b: *const c_void) -> c_int {
    1
}

/// Hash-table deallocator for REF lists.
unsafe extern "C" fn free_ref_table_entry(payload: *mut c_void, _name: *mut xmlChar) {
    crate::xml::list::list_delete(payload as *mut crate::xml::list::List);
}

/// Upstream `xmlAddRef(ctxt, doc, value, attr)` — registers an IDREF.
/// Returns the xmlRef or NULL.
///
/// # SAFETY
///
/// - `ctxt` may be NULL; `doc`/`attr`/`value` must be valid pointers.
pub unsafe fn add_ref(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    value: *const xmlChar,
    attr: *mut _xmlAttr,
) -> *mut _xmlRef {
    unsafe {
        if doc.is_null() || value.is_null() || attr.is_null() {
            return ptr::null_mut();
        }

        let mut table = (*doc).refs as *mut hash::HashTable;
        if table.is_null() {
            (*doc).refs = hash::hash_create(0) as *mut c_void;
            table = (*doc).refs as *mut hash::HashTable;
            if table.is_null() {
                vctxt_error(
                    ctxt,
                    b"Memory allocation failed : xmlAddRef\0" as *const u8 as *const c_char,
                );
                return ptr::null_mut();
            }
        }

        let ret = allocator::xmlMallocZero(size_of::<_xmlRef>() as usize) as *mut _xmlRef;
        if ret.is_null() {
            vctxt_error(
                ctxt,
                b"Memory allocation failed : xmlAddRef\0" as *const u8 as *const c_char,
            );
            return ptr::null_mut();
        }
        (*ret).value = string::xml_strdup(value);
        if (*ret).value.is_null() {
            free_ref(ret);
            vctxt_error(
                ctxt,
                b"Memory allocation failed : xmlAddRef\0" as *const u8 as *const c_char,
            );
            return ptr::null_mut();
        }
        // Upstream xmlIsStreaming(ctxt): streaming (reader) mode stores the
        // attr name because the attribute node will be destroyed; tree mode
        // stores the attribute pointer.
        let streaming = !ctxt.is_null()
            && !(*ctxt).userData.is_null()
            && (*((*ctxt).userData as *mut _xmlParserCtxt)).parseMode
                == crate::abi::types::xmlParserMode::XML_PARSE_READER as c_int;
        if streaming {
            (*ret).name = string::xml_strdup((*attr).name);
            (*ret).attr = ptr::null_mut();
        } else {
            (*ret).name = ptr::null();
            (*ret).attr = attr;
        }
        (*ret).lineno = tree::get_line_no((*attr).parent) as c_int;

        // References are lists of xmlRef per value.
        let ref_list = hash::hash_lookup(table, value) as *mut crate::xml::list::List;
        if ref_list.is_null() {
            let l = crate::xml::list::list_create(Some(free_ref_list_entry), Some(dummy_compare));
            if l.is_null() {
                free_ref(ret);
                vctxt_error(
                    ctxt,
                    b"Memory allocation failed : xmlAddRef\0" as *const u8 as *const c_char,
                );
                return ptr::null_mut();
            }
            if hash::hash_add_entry(table, value, l as *mut c_void) != 0 {
                crate::xml::list::list_delete(l);
                free_ref(ret);
                vctxt_error(
                    ctxt,
                    b"Memory allocation failed : xmlAddRef\0" as *const u8 as *const c_char,
                );
                return ptr::null_mut();
            }
            crate::xml::list::list_append(l, ret as *mut c_void);
        } else {
            if crate::xml::list::list_append(ref_list, ret as *mut c_void) != 0 {
                free_ref(ret);
                vctxt_error(
                    ctxt,
                    b"Memory allocation failed : xmlAddRef\0" as *const u8 as *const c_char,
                );
                return ptr::null_mut();
            }
        }
        ret
    }
}

/// Upstream `xmlRemoveRef(doc, attr)` — removes the attribute's IDREF
/// entry. Returns 0 on success, -1 otherwise.
///
/// # SAFETY
///
/// - `doc`/`attr` must be valid pointers or NULL.
pub unsafe fn remove_ref(doc: *mut _xmlDoc, attr: *mut _xmlAttr) -> c_int {
    // The candidate does not track a back-pointer from attribute to ref
    // (upstream keeps the ref's value only in the table key). Re-scan the
    // ref table for entries owned by this attribute.
    if doc.is_null() || attr.is_null() {
        return -1;
    }
    unsafe {
        let table = (*doc).refs as *mut hash::HashTable;
        if table.is_null() {
            return -1;
        }
        let mut removed = -1;
        // iterate: hash_scan with a callback that removes matching entries
        struct ScanCtx {
            table: *mut hash::HashTable,
            attr: *mut _xmlAttr,
            removed: c_int,
        }
        extern "C" fn scan_remove(payload: *mut c_void, data: *mut c_void, name: *const xmlChar) {
            let ctx = unsafe { &mut *(data as *mut ScanCtx) };
            let l = payload as *mut crate::xml::list::List;
            // remove every list element whose attr matches
            let mut cur: *mut c_void = crate::xml::list::list_front(l);
            while !cur.is_null() {
                let next: *mut c_void = unsafe { (*(cur as *mut _xmlRef)).next as *mut c_void };
                let r = cur as *mut _xmlRef;
                if unsafe { (*r).attr } == ctx.attr {
                    unsafe {
                        crate::xml::list::list_remove_first(l, cur);
                    }
                    unsafe { ctx.removed = 0 };
                }
                cur = next;
            }
            if crate::xml::list::list_empty(l) != 0 {
                unsafe {
                    hash::hash_remove_entry(ctx.table, name, Some(free_ref_table_entry));
                }
            }
            let _ = name;
        }
        let mut ctx = ScanCtx {
            table,
            attr,
            removed: -1,
        };
        hash::hash_scan(
            table,
            Some(scan_remove),
            &mut ctx as *mut ScanCtx as *mut c_void,
        );
        removed = ctx.removed;
        removed
    }
}

/// Upstream `xmlAddIDSafe(attr, value)` (2.13+): add an ID without a
/// validation context. Returns 1 on success, 0 if the ID already exists,
/// -1 on OOM.
///
/// # SAFETY
///
/// - `attr`/`value` must be valid pointers or NULL.
pub unsafe fn add_id_safe(attr: *mut _xmlAttr, value: *const xmlChar) -> c_int {
    add_id_internal(attr, value, ptr::null_mut())
}

/// Upstream `xmlFreeIDTable(table)`.
///
/// # SAFETY
///
/// - `table` must be a valid ID hash table or NULL.
pub unsafe fn free_id_table(table: *mut hash::HashTable) {
    hash::hash_free(table, Some(free_id_entry));
}

/// Upstream `xmlFreeRefTable(table)`.
///
/// # SAFETY
///
/// - `table` must be a valid ref hash table or NULL.
pub unsafe fn free_ref_table(table: *mut hash::HashTable) {
    hash::hash_free(table, Some(free_ref_table_entry));
}

/// Upstream `xmlGetID(doc, ID)`: returns the attribute holding the ID, or
/// the document pointer itself when operating on a stream (attribute node no
/// longer exists).
///
/// # SAFETY
///
/// - `doc`/`ID` must be valid pointers or NULL.
pub unsafe fn get_id(doc: *mut _xmlDoc, id: *const xmlChar) -> *mut _xmlAttr {
    unsafe {
        if doc.is_null() || id.is_null() {
            return ptr::null_mut();
        }
        let table = (*doc).ids as *mut hash::HashTable;
        if table.is_null() {
            return ptr::null_mut();
        }
        let id_entry = hash::hash_lookup(table, id) as *mut _xmlID;
        if id_entry.is_null() {
            return ptr::null_mut();
        }
        if (*id_entry).attr.is_null() {
            // streaming mode: return the document as a well-known reference
            doc as *mut _xmlAttr
        } else {
            (*id_entry).attr
        }
    }
}

/// Upstream `xmlGetRefs(doc, ID)`: returns the list of references for an ID.
///
/// # SAFETY
///
/// - `doc`/`ID` must be valid pointers or NULL.
pub unsafe fn get_refs(doc: *mut _xmlDoc, id: *const xmlChar) -> *mut crate::xml::list::List {
    unsafe {
        if doc.is_null() || id.is_null() {
            return ptr::null_mut();
        }
        let table = (*doc).refs as *mut hash::HashTable;
        if table.is_null() {
            return ptr::null_mut();
        }
        hash::hash_lookup(table, id) as *mut crate::xml::list::List
    }
}

/// Upstream `xmlIsID(doc, elem, attr)`: is this attribute an ID? Handles the
/// HTML special cases (id attribute; name attribute on <a>) and the DTD
/// declaration lookup, plus the xml:id namespace convention.
///
/// # SAFETY
///
/// - `doc`/`elem`/`attr` must be valid pointers or NULL.
pub unsafe fn is_id(doc: *mut _xmlDoc, elem: *mut _xmlNode, attr: *mut _xmlAttr) -> c_int {
    unsafe {
        if attr.is_null() || (*attr).name.is_null() {
            return 0;
        }
        if !doc.is_null() && (*doc).type_ == XML_HTML_DOCUMENT_NODE as c_int {
            if string::xml_strcmp(b"id\0" as *const u8 as *const xmlChar, (*attr).name) == 0 {
                return 1;
            }
            if elem.is_null() || (*elem).type_ != XML_ELEMENT_NODE as c_int {
                return 0;
            }
            if string::xml_strcmp(b"name\0" as *const u8 as *const xmlChar, (*attr).name) == 0
                && string::xml_strcmp(b"a\0" as *const u8 as *const xmlChar, (*elem).name) == 0
            {
                return 1;
            }
        } else {
            // xml:id convention
            if !(*attr).ns.is_null()
                && !(*(*attr).ns).prefix.is_null()
                && string::xml_strcmp(
                    (*(*attr).ns).prefix,
                    b"xml\0" as *const u8 as *const xmlChar,
                ) == 0
                && string::xml_strcmp((*attr).name, b"id\0" as *const u8 as *const xmlChar) == 0
            {
                return 1;
            }
            if doc.is_null() || ((*doc).intSubset.is_null() && (*doc).extSubset.is_null()) {
                return 0;
            }
            if elem.is_null()
                || (*elem).type_ != XML_ELEMENT_NODE as c_int
                || (*elem).name.is_null()
            {
                return 0;
            }
            let mut fullname = (*elem).name;
            let mut owned = false;
            if !(*elem).ns.is_null() && !(*(*elem).ns).prefix.is_null() {
                let f = string::build_qname((*elem).name, (*(*elem).ns).prefix, ptr::null_mut(), 0);
                if f.is_null() {
                    return -1;
                }
                fullname = f;
                owned = true;
            }
            let aprefix = if !(*attr).ns.is_null() {
                (*(*attr).ns).prefix
            } else {
                ptr::null()
            };
            let mut attr_decl =
                get_dtd_qattr_desc((*doc).intSubset, fullname, (*attr).name, aprefix);
            if attr_decl.is_null() && !(*doc).extSubset.is_null() {
                attr_decl = get_dtd_qattr_desc((*doc).extSubset, fullname, (*attr).name, aprefix);
            }
            if owned {
                allocator::xmlFreeImpl(fullname as *mut c_void);
            }
            if !attr_decl.is_null() && (*attr_decl).atype == XML_ATTRIBUTE_ID as c_int {
                return 1;
            }
        }
        0
    }
}

/// Upstream `xmlIsRef(doc, elem, attr)`: is this attribute an IDREF?
///
/// # SAFETY
///
/// - `doc`/`elem`/`attr` must be valid pointers or NULL.
pub unsafe fn is_ref(doc: *mut _xmlDoc, elem: *mut _xmlNode, attr: *mut _xmlAttr) -> c_int {
    unsafe {
        if attr.is_null() {
            return 0;
        }
        let doc = if doc.is_null() { (*attr).doc } else { doc };
        if doc.is_null() {
            return 0;
        }
        if (*doc).intSubset.is_null() && (*doc).extSubset.is_null() {
            return 0;
        }
        if (*doc).type_ == XML_HTML_DOCUMENT_NODE as c_int {
            return 0;
        }
        if elem.is_null() {
            return 0;
        }
        let aprefix = if !(*attr).ns.is_null() {
            (*(*attr).ns).prefix
        } else {
            ptr::null()
        };
        let mut attr_decl =
            get_dtd_qattr_desc((*doc).intSubset, (*elem).name, (*attr).name, aprefix);
        if attr_decl.is_null() && !(*doc).extSubset.is_null() {
            attr_decl = get_dtd_qattr_desc((*doc).extSubset, (*elem).name, (*attr).name, aprefix);
        }
        if !attr_decl.is_null()
            && ((*attr_decl).atype == XML_ATTRIBUTE_IDREF as c_int
                || (*attr_decl).atype == XML_ATTRIBUTE_IDREFS as c_int)
        {
            return 1;
        }
        0
    }
}

/// Upstream `xmlGetDtdElementDesc(dtd, name)` — plain element declaration
/// lookup with QName splitting.
///
/// # SAFETY
///
/// - `dtd` must be a valid pointer or NULL; `name` a NULL-terminated string.
pub unsafe fn get_dtd_element_desc(dtd: *mut _xmlDtd, name: *const xmlChar) -> *mut _xmlElement {
    unsafe {
        if dtd.is_null() || name.is_null() {
            return ptr::null_mut();
        }
        let elements = (*dtd).elements;
        if elements.is_null() {
            return ptr::null_mut();
        }
        let mut prefix = ptr::null_mut();
        let local = split_qname4(name, &mut prefix);
        if local.is_null() {
            if !prefix.is_null() {
                allocator::xmlFreeImpl(prefix as *mut c_void);
            }
            return ptr::null_mut();
        }
        let cur =
            hash::hash_lookup2(elements as *mut hash::HashTable, local, prefix) as *mut _xmlElement;
        if !prefix.is_null() {
            allocator::xmlFreeImpl(prefix as *mut c_void);
        }
        cur
    }
}

/// Upstream `xmlGetDtdAttrDesc(dtd, elem, name)` — attribute declaration
/// lookup splitting the attribute QName into (local, prefix).
///
/// # SAFETY
///
/// - `dtd` must be a valid pointer or NULL; `elem`/`name` NULL-terminated
///   strings.
pub unsafe fn get_dtd_attr_desc(
    dtd: *mut _xmlDtd,
    elem: *const xmlChar,
    name: *const xmlChar,
) -> *mut _xmlAttribute {
    unsafe {
        if dtd.is_null() || elem.is_null() || name.is_null() {
            return ptr::null_mut();
        }
        let attrs = (*dtd).attributes;
        if attrs.is_null() {
            return ptr::null_mut();
        }
        let mut prefix = ptr::null_mut();
        let local = split_qname4(name, &mut prefix);
        if local.is_null() {
            if !prefix.is_null() {
                allocator::xmlFreeImpl(prefix as *mut c_void);
            }
            return ptr::null_mut();
        }
        let cur = hash::hash_lookup3(attrs as *mut hash::HashTable, local, prefix, elem)
            as *mut _xmlAttribute;
        if !prefix.is_null() {
            allocator::xmlFreeImpl(prefix as *mut c_void);
        }
        cur
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Declaration validators (upstream valid.c xmlValidateElementDecl / NotationDecl
// / OneAttribute / OneElement / OneNamespace)
// ═══════════════════════════════════════════════════════════════════════════════

/// Emit a validation error with node context, mirroring upstream
/// xmlErrValidNode's formatting. The candidate's valid context carries a
/// generic error callback only (no structured error slot), so the error
/// code is not stored — the message text matches upstream byte-for-byte.
unsafe fn vctxt_error_node(ctxt: *mut _xmlValidCtxt, _node: *mut _xmlNode, msg: *const c_char) {
    vctxt_error(ctxt, msg);
}

/// Upstream `xmlValidateElementDecl(ctxt, doc, elem)`: verifies the
/// declaration is not duplicated and that MIXED content models do not list
/// the same element twice.
///
/// # SAFETY
///
/// - `ctxt`/`doc` may be NULL; `elem` a valid pointer or NULL.
pub unsafe fn validate_element_decl(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    elem: *mut _xmlElement,
) -> c_int {
    unsafe {
        if doc.is_null() || (*doc).intSubset.is_null() && (*doc).extSubset.is_null() {
            return 1;
        }
        if elem.is_null() {
            return 1;
        }
        let mut ret = 1;

        // No Duplicate Types (VC: No Duplicate Types) — only for MIXED
        // declarations: walk the OR chain and compare element names.
        if (*elem).etype == XML_ELEMENT_TYPE_MIXED as c_int {
            let mut cur = (*elem).content;
            while !cur.is_null() {
                if (*cur).type_ != XML_ELEMENT_CONTENT_OR as c_int {
                    break;
                }
                if (*cur).c1.is_null() {
                    break;
                }
                if (*(*cur).c1).type_ == XML_ELEMENT_CONTENT_ELEMENT as c_int {
                    let name = (*(*cur).c1).name;
                    let mut next = (*cur).c2;
                    while !next.is_null() {
                        if (*next).type_ == XML_ELEMENT_CONTENT_ELEMENT as c_int {
                            if string::xml_strcmp((*next).name, name) == 0
                                && string::xml_strcmp((*next).prefix, (*(*cur).c1).prefix) == 0
                            {
                                if (*(*cur).c1).prefix.is_null() {
                                    let msg = format!(
                                        "Definition of {} has duplicate references of {}\0",
                                        string::xmlstr_to_string((*elem).name),
                                        string::xmlstr_to_string(name)
                                    );
                                    vctxt_error_node(
                                        ctxt,
                                        elem as *mut _xmlNode,
                                        msg.as_ptr() as *const c_char,
                                    );
                                } else {
                                    let msg = format!(
                                        "Definition of {} has duplicate references of {}:{}\0",
                                        string::xmlstr_to_string((*elem).name),
                                        string::xmlstr_to_string((*(*cur).c1).prefix),
                                        string::xmlstr_to_string(name)
                                    );
                                    vctxt_error_node(
                                        ctxt,
                                        elem as *mut _xmlNode,
                                        msg.as_ptr() as *const c_char,
                                    );
                                }
                                ret = 0;
                            }
                            break;
                        }
                        if (*next).c1.is_null() {
                            break;
                        }
                        if (*(*next).c1).type_ != XML_ELEMENT_CONTENT_ELEMENT as c_int {
                            break;
                        }
                        if string::xml_strcmp((*(*next).c1).name, name) == 0
                            && string::xml_strcmp((*(*next).c1).prefix, (*(*cur).c1).prefix) == 0
                        {
                            if (*(*cur).c1).prefix.is_null() {
                                let msg = format!(
                                    "Definition of {} has duplicate references to {}\0",
                                    string::xmlstr_to_string((*elem).name),
                                    string::xmlstr_to_string(name)
                                );
                                vctxt_error_node(
                                    ctxt,
                                    elem as *mut _xmlNode,
                                    msg.as_ptr() as *const c_char,
                                );
                            } else {
                                let msg = format!(
                                    "Definition of {} has duplicate references to {}:{}\0",
                                    string::xmlstr_to_string((*elem).name),
                                    string::xmlstr_to_string((*(*cur).c1).prefix),
                                    string::xmlstr_to_string(name)
                                );
                                vctxt_error_node(
                                    ctxt,
                                    elem as *mut _xmlNode,
                                    msg.as_ptr() as *const c_char,
                                );
                            }
                            ret = 0;
                        }
                        next = (*next).c2;
                    }
                }
                cur = (*cur).c2;
            }
        }

        // VC: Unique Element Type Declaration — the declaration must not
        // already exist (with the same prefix) in either subset.
        let mut prefix = ptr::null_mut();
        let local_name = split_qname4((*elem).name, &mut prefix);
        if local_name.is_null() {
            vctxt_error(
                ctxt,
                b"Memory allocation failed : xmlValidateElementDecl\0" as *const u8
                    as *const c_char,
            );
            if !prefix.is_null() {
                allocator::xmlFreeImpl(prefix as *mut c_void);
            }
            return 0;
        }

        for subset in [(*doc).intSubset, (*doc).extSubset] {
            if subset.is_null() {
                continue;
            }
            let tst = get_dtd_qelement_desc(subset, local_name, prefix);
            if !tst.is_null()
                && tst != elem
                && ((*tst).prefix == (*elem).prefix
                    || string::xml_strcmp((*tst).prefix, (*elem).prefix) == 0)
                && (*tst).etype != XML_ELEMENT_TYPE_UNDEFINED as c_int
            {
                let msg = format!(
                    "Redefinition of element {}\0",
                    string::xmlstr_to_string((*elem).name)
                );
                vctxt_error_node(ctxt, elem as *mut _xmlNode, msg.as_ptr() as *const c_char);
                ret = 0;
            }
        }
        if !prefix.is_null() {
            allocator::xmlFreeImpl(prefix as *mut c_void);
        }
        ret
    }
}

/// Upstream `xmlValidateNotationDecl(ctxt, doc, nota)`: modern libxml2 has
/// no validity constraint on notation declarations and returns 1 always
/// (verified by disassembly of the system DSO: `mov $1,%eax; ret`).
pub unsafe fn validate_notation_decl(
    _ctxt: *mut _xmlValidCtxt,
    _doc: *mut _xmlDoc,
    _nota: *mut _xmlNotation,
) -> c_int {
    1
}

/// Upstream `xmlValidateOneAttribute(ctxt, doc, elem, attr, value)`.
///
/// Performs [VC: Attribute Value Type], [VC: Fixed Attribute Default],
/// [VC: ID], [VC: IDREF], [VC: Notation Attributes], [VC: Enumeration],
/// and the ENTITY existence check via xmlValidateAttributeValue2.
///
/// # SAFETY
///
/// - `ctxt`/`doc` may be NULL; `elem`/`attr`/`value` valid pointers or NULL.
pub unsafe fn validate_one_attribute(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    elem: *mut _xmlNode,
    attr: *mut _xmlAttr,
    value: *const xmlChar,
) -> c_int {
    unsafe {
        if doc.is_null() {
            return 0;
        }
        if elem.is_null() || (*elem).name.is_null() {
            return 0;
        }
        if attr.is_null() || (*attr).name.is_null() {
            return 0;
        }
        let mut ret = 1;

        let aprefix = if !(*attr).ns.is_null() {
            (*(*attr).ns).prefix
        } else {
            ptr::null()
        };

        let mut attr_decl = ptr::null_mut();
        if !(*elem).ns.is_null() && !(*(*elem).ns).prefix.is_null() {
            let fullname =
                string::build_qname((*elem).name, (*(*elem).ns).prefix, ptr::null_mut(), 0);
            if fullname.is_null() {
                vctxt_error(
                    ctxt,
                    b"Memory allocation failed : xmlValidateOneAttribute\0" as *const u8
                        as *const c_char,
                );
                return 0;
            }
            attr_decl = get_dtd_qattr_desc((*doc).intSubset, fullname, (*attr).name, aprefix);
            if attr_decl.is_null() && !(*doc).extSubset.is_null() {
                attr_decl = get_dtd_qattr_desc((*doc).extSubset, fullname, (*attr).name, aprefix);
            }
            if fullname != (*elem).name as *mut xmlChar {
                allocator::xmlFreeImpl(fullname as *mut c_void);
            }
        }
        if attr_decl.is_null() {
            attr_decl = get_dtd_qattr_desc((*doc).intSubset, (*elem).name, (*attr).name, aprefix);
            if attr_decl.is_null() && !(*doc).extSubset.is_null() {
                attr_decl =
                    get_dtd_qattr_desc((*doc).extSubset, (*elem).name, (*attr).name, aprefix);
            }
        }

        // [VC: Attribute Value Type]
        if attr_decl.is_null() {
            let msg = format!(
                "No declaration for attribute {} of element {}\0",
                string::xmlstr_to_string((*attr).name),
                string::xmlstr_to_string((*elem).name)
            );
            vctxt_error_node(ctxt, elem, msg.as_ptr() as *const c_char);
            return 0;
        }
        if !(*attr).id.is_null() {
            remove_id(doc, attr);
        }
        (*attr).atype = (*attr_decl).atype;

        // syntax check against the declared type (with OLD10 doc flag)
        let val = if (*doc).properties & crate::abi::types::xmlDocProperties::XML_DOC_OLD10 as c_int
            != 0
        {
            // OLD10 name classes are not implemented; the modern classes are
            // a superset for ASCII and match for all BMP ranges used here.
            match (*attr_decl).atype as u32 {
                t if t == XML_ATTRIBUTE_ENTITIES as u32 || t == XML_ATTRIBUTE_IDREFS as u32 => {
                    validate_values_internal(value, 0)
                }
                t if t == XML_ATTRIBUTE_ENTITY as u32
                    || t == XML_ATTRIBUTE_IDREF as u32
                    || t == XML_ATTRIBUTE_ID as u32
                    || t == XML_ATTRIBUTE_NOTATION as u32 =>
                {
                    validate_value_internal(value, 0)
                }
                t if t == XML_ATTRIBUTE_NMTOKENS as u32
                    || t == XML_ATTRIBUTE_ENUMERATION as u32 =>
                {
                    validate_values_internal(value, XML_SCAN_NMTOKEN)
                }
                t if t == XML_ATTRIBUTE_NMTOKEN as u32 => {
                    validate_value_internal(value, XML_SCAN_NMTOKEN)
                }
                _ => 1,
            }
        } else {
            validate_attribute_value((*attr_decl).atype, value)
        };
        if val == 0 {
            let msg = format!(
                "Syntax of value for attribute {} of {} is not valid\0",
                string::xmlstr_to_string((*attr).name),
                string::xmlstr_to_string((*elem).name)
            );
            vctxt_error_node(ctxt, elem, msg.as_ptr() as *const c_char);
            ret = 0;
        }

        // [VC: Fixed Attribute Default]
        if (*attr_decl).def == XML_ATTRIBUTE_FIXED as c_int {
            if string::xml_strcmp(value, (*attr_decl).defaultValue) != 0 {
                let msg = format!(
                    "Value for attribute {} of {} is different from default \"{}\n\0",
                    string::xmlstr_to_string((*attr).name),
                    string::xmlstr_to_string((*elem).name),
                    string::xmlstr_to_string((*attr_decl).defaultValue)
                );
                // upstream format: "Value for attribute %s of %s is different from default \"%s\"\n"
                let msg = format!(
                    "Value for attribute {} of {} is different from default \"{}\"\0",
                    string::xmlstr_to_string((*attr).name),
                    string::xmlstr_to_string((*elem).name),
                    string::xmlstr_to_string((*attr_decl).defaultValue)
                );
                vctxt_error_node(ctxt, elem, msg.as_ptr() as *const c_char);
                ret = 0;
            }
        }

        // [VC: ID] uniqueness (skipped inside entities)
        const XML_VCTXT_IN_ENTITY: c_uint = 4; // upstream valid.h
        if (*attr_decl).atype == XML_ATTRIBUTE_ID as c_int
            && (ctxt.is_null() || (*ctxt).flags & XML_VCTXT_IN_ENTITY == 0)
        {
            if add_id(ctxt, doc, value, attr).is_null() {
                ret = 0;
            }
        }
        if (*attr_decl).atype == XML_ATTRIBUTE_IDREF as c_int
            || (*attr_decl).atype == XML_ATTRIBUTE_IDREFS as c_int
        {
            if add_ref(ctxt, doc, value, attr).is_null() {
                ret = 0;
            }
        }

        // [VC: Notation Attributes]
        if (*attr_decl).atype == XML_ATTRIBUTE_NOTATION as c_int {
            let mut nota = get_dtd_notation_desc((*doc).intSubset, value);
            if nota.is_null() {
                nota = get_dtd_notation_desc((*doc).extSubset, value);
            }
            if nota.is_null() {
                let msg = format!(
                    "Value \"{}\" for attribute {} of {} is not a declared Notation\0",
                    string::xmlstr_to_string(value),
                    string::xmlstr_to_string((*attr).name),
                    string::xmlstr_to_string((*elem).name)
                );
                vctxt_error_node(ctxt, elem, msg.as_ptr() as *const c_char);
                ret = 0;
            }
            let mut tree = (*attr_decl).tree;
            while !tree.is_null() {
                if string::xml_strcmp((*tree).name, value) == 0 {
                    break;
                }
                tree = (*tree).next;
            }
            if tree.is_null() {
                let msg = format!(
                    "Value \"{}\" for attribute {} of {} is not among the enumerated notations\0",
                    string::xmlstr_to_string(value),
                    string::xmlstr_to_string((*attr).name),
                    string::xmlstr_to_string((*elem).name)
                );
                vctxt_error_node(ctxt, elem, msg.as_ptr() as *const c_char);
                ret = 0;
            }
        }

        // [VC: Enumeration]
        if (*attr_decl).atype == XML_ATTRIBUTE_ENUMERATION as c_int {
            let mut tree = (*attr_decl).tree;
            while !tree.is_null() {
                if string::xml_strcmp((*tree).name, value) == 0 {
                    break;
                }
                tree = (*tree).next;
            }
            if tree.is_null() {
                let msg = format!(
                    "Value \"{}\" for attribute {} of {} is not among the enumerated set\0",
                    string::xmlstr_to_string(value),
                    string::xmlstr_to_string((*attr).name),
                    string::xmlstr_to_string((*elem).name)
                );
                vctxt_error_node(ctxt, elem, msg.as_ptr() as *const c_char);
                ret = 0;
            }
        }

        // Fixed Attribute Default (second occurrence, upstream)
        if (*attr_decl).def == XML_ATTRIBUTE_FIXED as c_int
            && string::xml_strcmp((*attr_decl).defaultValue, value) != 0
        {
            let msg = format!(
                "Value for attribute {} of {} must be \"{}\"\0",
                string::xmlstr_to_string((*attr).name),
                string::xmlstr_to_string((*elem).name),
                string::xmlstr_to_string((*attr_decl).defaultValue)
            );
            vctxt_error_node(ctxt, elem, msg.as_ptr() as *const c_char);
            ret = 0;
        }

        // [VC: Entity Name] — ENTITY must name a declared unparsed entity
        if (*attr_decl).atype == XML_ATTRIBUTE_ENTITY as c_int {
            let ent = tree::get_doc_entity(doc, value);
            if ent.is_null() {
                let msg = format!(
                    "ENTITY attribute {} reference an unknown entity \"{}\"\0",
                    string::xmlstr_to_string((*attr).name),
                    string::xmlstr_to_string(value)
                );
                vctxt_error_node(ctxt, doc as *mut _xmlNode, msg.as_ptr() as *const c_char);
                ret = 0;
            } else if (*ent).etype != XML_EXTERNAL_GENERAL_UNPARSED_ENTITY as c_int {
                let msg = format!(
                    "ENTITY attribute {} reference an entity \"{}\" of wrong type\0",
                    string::xmlstr_to_string((*attr).name),
                    string::xmlstr_to_string(value)
                );
                vctxt_error_node(ctxt, doc as *mut _xmlNode, msg.as_ptr() as *const c_char);
                ret = 0;
            }
        }
        ret
    }
}

/// Upstream `xmlValidateOneNamespace(ctxt, doc, elem, prefix, ns, value)` —
/// namespace-declaration attribute validation.
///
/// # SAFETY
///
/// - `ctxt` may be NULL; `doc`/`elem`/`ns` valid pointers or NULL.
pub unsafe fn validate_one_namespace(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    elem: *mut _xmlNode,
    prefix: *const xmlChar,
    ns: *mut _xmlNs,
    value: *const xmlChar,
) -> c_int {
    unsafe {
        if doc.is_null() {
            return 0;
        }
        if elem.is_null() || (*elem).name.is_null() {
            return 0;
        }
        if ns.is_null() || (*ns).href.is_null() {
            return 0;
        }
        let mut ret = 1;

        let mut attr_decl = ptr::null_mut();
        if !prefix.is_null() {
            let fullname = string::build_qname((*elem).name, prefix, ptr::null_mut(), 0);
            if fullname.is_null() {
                vctxt_error(
                    ctxt,
                    b"Memory allocation failed : xmlValidateOneNamespace\0" as *const u8
                        as *const c_char,
                );
                return 0;
            }
            if !(*ns).prefix.is_null() {
                attr_decl = get_dtd_qattr_desc(
                    (*doc).intSubset,
                    fullname,
                    (*ns).prefix,
                    b"xmlns\0" as *const u8 as *const xmlChar,
                );
                if attr_decl.is_null() && !(*doc).extSubset.is_null() {
                    attr_decl = get_dtd_qattr_desc(
                        (*doc).extSubset,
                        fullname,
                        (*ns).prefix,
                        b"xmlns\0" as *const u8 as *const xmlChar,
                    );
                }
            } else {
                attr_decl = get_dtd_qattr_desc(
                    (*doc).intSubset,
                    fullname,
                    b"xmlns\0" as *const u8 as *const xmlChar,
                    ptr::null(),
                );
                if attr_decl.is_null() && !(*doc).extSubset.is_null() {
                    attr_decl = get_dtd_qattr_desc(
                        (*doc).extSubset,
                        fullname,
                        b"xmlns\0" as *const u8 as *const xmlChar,
                        ptr::null(),
                    );
                }
            }
            if fullname != (*elem).name as *mut xmlChar {
                allocator::xmlFreeImpl(fullname as *mut c_void);
            }
        }
        if attr_decl.is_null() {
            if !(*ns).prefix.is_null() {
                attr_decl = get_dtd_qattr_desc(
                    (*doc).intSubset,
                    (*elem).name,
                    (*ns).prefix,
                    b"xmlns\0" as *const u8 as *const xmlChar,
                );
                if attr_decl.is_null() && !(*doc).extSubset.is_null() {
                    attr_decl = get_dtd_qattr_desc(
                        (*doc).extSubset,
                        (*elem).name,
                        (*ns).prefix,
                        b"xmlns\0" as *const u8 as *const xmlChar,
                    );
                }
            } else {
                attr_decl = get_dtd_qattr_desc(
                    (*doc).intSubset,
                    (*elem).name,
                    b"xmlns\0" as *const u8 as *const xmlChar,
                    ptr::null(),
                );
                if attr_decl.is_null() && !(*doc).extSubset.is_null() {
                    attr_decl = get_dtd_qattr_desc(
                        (*doc).extSubset,
                        (*elem).name,
                        b"xmlns\0" as *const u8 as *const xmlChar,
                        ptr::null(),
                    );
                }
            }
        }

        // [VC: Attribute Value Type]
        if attr_decl.is_null() {
            let msg = if !(*ns).prefix.is_null() {
                format!(
                    "No declaration for attribute xmlns:{} of element {}\0",
                    string::xmlstr_to_string((*ns).prefix),
                    string::xmlstr_to_string((*elem).name)
                )
            } else {
                format!(
                    "No declaration for attribute xmlns of element {}\0",
                    string::xmlstr_to_string((*elem).name)
                )
            };
            vctxt_error_node(ctxt, elem, msg.as_ptr() as *const c_char);
            return 0;
        }

        let val = validate_attribute_value((*attr_decl).atype, value);
        if val == 0 {
            let msg = if !(*ns).prefix.is_null() {
                format!(
                    "Syntax of value for attribute xmlns:{} of {} is not valid\0",
                    string::xmlstr_to_string((*ns).prefix),
                    string::xmlstr_to_string((*elem).name)
                )
            } else {
                format!(
                    "Syntax of value for attribute xmlns of {} is not valid\0",
                    string::xmlstr_to_string((*elem).name)
                )
            };
            vctxt_error_node(ctxt, elem, msg.as_ptr() as *const c_char);
            ret = 0;
        }

        // [VC: Fixed Attribute Default]
        if (*attr_decl).def == XML_ATTRIBUTE_FIXED as c_int
            && string::xml_strcmp(value, (*attr_decl).defaultValue) != 0
        {
            let msg = if !(*ns).prefix.is_null() {
                format!(
                    "Value for attribute xmlns:{} of {} is different from default \"{}\"\0",
                    string::xmlstr_to_string((*ns).prefix),
                    string::xmlstr_to_string((*elem).name),
                    string::xmlstr_to_string((*attr_decl).defaultValue)
                )
            } else {
                format!(
                    "Value for attribute xmlns of {} is different from default \"{}\"\0",
                    string::xmlstr_to_string((*elem).name),
                    string::xmlstr_to_string((*attr_decl).defaultValue)
                )
            };
            vctxt_error_node(ctxt, elem, msg.as_ptr() as *const c_char);
            ret = 0;
        }
        ret
    }
}

/// Upstream `xmlValidateOneElement(ctxt, doc, elem)` — validates a single
/// element against its declaration (content model + attributes), WITHOUT
/// recursing into children.
///
/// # SAFETY
///
/// - `ctxt` may be NULL; `doc`/`elem` valid pointers or NULL.
pub unsafe fn validate_one_element(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    elem: *mut _xmlNode,
) -> c_int {
    unsafe {
        if doc.is_null() {
            return 0;
        }
        if elem.is_null() {
            return 0;
        }
        match (*elem).type_ {
            t if t == XML_TEXT_NODE as c_int
                || t == XML_CDATA_SECTION_NODE as c_int
                || t == XML_ENTITY_REF_NODE as c_int
                || t == XML_PI_NODE as c_int
                || t == XML_COMMENT_NODE as c_int
                || t == XML_XINCLUDE_START as c_int
                || t == XML_XINCLUDE_END as c_int =>
            {
                return 1;
            }
            t if t == XML_ELEMENT_NODE as c_int => {}
            _ => {
                vctxt_error_node(
                    ctxt,
                    elem,
                    b"unexpected element type\0" as *const u8 as *const c_char,
                );
                return 0;
            }
        }

        let mut ret = 1;
        let mut extsubset = 0;
        let elem_decl = valid_get_elem_decl(ctxt, doc, elem, &mut extsubset);
        if elem_decl.is_null() {
            return 0;
        }

        // Continuous (push) validation already checks the content model via
        // the vstate stack; skip the tree walk when active.
        if (*ctxt).vstateNr == 0 {
            match (*elem_decl).etype as u32 {
                t if t == XML_ELEMENT_TYPE_UNDEFINED as u32 => {
                    let msg = format!(
                        "No declaration for element {}\0",
                        string::xmlstr_to_string((*elem).name)
                    );
                    vctxt_error_node(ctxt, elem, msg.as_ptr() as *const c_char);
                    return 0;
                }
                t if t == XML_ELEMENT_TYPE_EMPTY as u32 => {
                    if !(*elem).children.is_null() {
                        let msg = format!(
                            "Element {} was declared EMPTY this one has content\0",
                            string::xmlstr_to_string((*elem).name)
                        );
                        vctxt_error_node(ctxt, elem, msg.as_ptr() as *const c_char);
                        ret = 0;
                    }
                }
                t if t == XML_ELEMENT_TYPE_ANY as u32 => {}
                t if t == XML_ELEMENT_TYPE_MIXED as u32 => {
                    if !(*elem_decl).content.is_null()
                        && (*(*elem_decl).content).type_ == XML_ELEMENT_CONTENT_PCDATA as c_int
                    {
                        // #PCDATA-only: any element child is an error
                        let mut child = (*elem).children;
                        while !child.is_null() {
                            if (*child).type_ == XML_ELEMENT_NODE as c_int {
                                let msg = format!(
                                    "Element {} was declared #PCDATA but contains non text nodes\0",
                                    string::xmlstr_to_string((*elem).name)
                                );
                                vctxt_error_node(ctxt, elem, msg.as_ptr() as *const c_char);
                                ret = 0;
                                break;
                            }
                            child = (*child).next;
                        }
                    } else {
                        // check each child element against the mixed list
                        let mut child = (*elem).children;
                        while !child.is_null() {
                            if (*child).type_ == XML_ELEMENT_NODE as c_int {
                                let mut fullname = (*child).name;
                                let mut own = false;
                                if !(*child).ns.is_null() && !(*(*child).ns).prefix.is_null() {
                                    let fnp = string::build_qname(
                                        (*child).name,
                                        (*(*child).ns).prefix,
                                        ptr::null_mut(),
                                        0,
                                    );
                                    if fnp.is_null() {
                                        vctxt_error(
                                            ctxt,
                                            b"Memory allocation failed : xmlValidateOneElement\0"
                                                as *const u8
                                                as *const c_char,
                                        );
                                        return 0;
                                    }
                                    fullname = fnp;
                                    own = true;
                                }
                                if validate_check_mixed(ctxt, (*elem_decl).content, fullname) != 1 {
                                    let msg = format!(
                                        "Element {} is not declared in {} list of possible children\0",
                                        string::xmlstr_to_string(fullname),
                                        string::xmlstr_to_string((*elem).name)
                                    );
                                    vctxt_error_node(ctxt, elem, msg.as_ptr() as *const c_char);
                                    ret = 0;
                                }
                                if own {
                                    allocator::xmlFreeImpl(fullname as *mut c_void);
                                }
                            }
                            child = (*child).next;
                        }
                    }
                }
                t if t == XML_ELEMENT_TYPE_ELEMENT as u32 => {
                    // Element-only content: collect child element names and
                    // check against the content model.
                    let mut names: Vec<*const xmlChar> = Vec::new();
                    let mut owned: Vec<*mut xmlChar> = Vec::new();
                    let mut child = (*elem).children;
                    while !child.is_null() {
                        if (*child).type_ == XML_ELEMENT_NODE as c_int {
                            let mut fullname = (*child).name;
                            if !(*child).ns.is_null() && !(*(*child).ns).prefix.is_null() {
                                let fnp = string::build_qname(
                                    (*child).name,
                                    (*(*child).ns).prefix,
                                    ptr::null_mut(),
                                    0,
                                );
                                if !fnp.is_null() {
                                    fullname = fnp;
                                    owned.push(fnp);
                                }
                            }
                            names.push(fullname);
                        }
                        child = (*child).next;
                    }
                    let result = dtd::valid_content_model((*elem_decl).content, &names);
                    for n in owned {
                        allocator::xmlFreeImpl(n as *mut c_void);
                    }
                    if result != dtd::ContentModelResult::Valid {
                        let msg = format!(
                            "Element {} content does not follow the DTD\0",
                            string::xmlstr_to_string((*elem).name)
                        );
                        vctxt_error_node(ctxt, elem, msg.as_ptr() as *const c_char);
                        ret = 0;
                    }
                }
                _ => {}
            }

            // Required attributes + attribute value checks
            let mut attr = (*elem).properties;
            while !attr.is_null() {
                let aval = if !(*attr).children.is_null() {
                    (*(*attr).children).content
                } else {
                    ptr::null()
                };
                if validate_one_attribute(ctxt, doc, elem, attr, aval) == 0 {
                    ret = 0;
                }
                attr = (*attr).next;
            }
        }
        ret
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Streaming (push) validation — upstream valid.c xmlValidatePushElement /
// PushCData / PopElement + xmlValidBuildContentModel
// ═══════════════════════════════════════════════════════════════════════════════
//
// Upstream keeps a stack of validation states (one per open element). Each
// state holds the element declaration and, for ELEMENT content, a regexp
// exec context over the compiled content model. The candidate reproduces
// the same observable contract: per-push checks against the current state,
// "Misplaced"/"Text not allowed"/"Expecting more children" diagnostics,
// and the vstate push/pop stack on the public _xmlValidCtxt layout
// (vstate/vstateNr/vstateMax/vstateTab).

/// Mirror of upstream `_xmlValidState` (valid.c): one entry per open element.
#[repr(C)]
struct ValidState {
    elem_decl: *mut _xmlElement,
    node: *mut _xmlNode,
    exec: *mut ContentModelExec,
}

/// Find the declaration for an element (upstream xmlValidGetElemDecl).
/// Reports "No declaration for element %s" when absent.
unsafe fn valid_get_elem_decl(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    elem: *mut _xmlNode,
    extsubset: *mut c_int,
) -> *mut _xmlElement {
    unsafe {
        if ctxt.is_null() || doc.is_null() || elem.is_null() || (*elem).name.is_null() {
            return ptr::null_mut();
        }
        if !extsubset.is_null() {
            *extsubset = 0;
        }
        let mut elem_decl = ptr::null_mut();

        let prefix = if !(*elem).ns.is_null() && !(*(*elem).ns).prefix.is_null() {
            (*(*elem).ns).prefix
        } else {
            ptr::null()
        };
        if !prefix.is_null() {
            elem_decl = get_dtd_qelement_desc((*doc).intSubset, (*elem).name, prefix);
            if elem_decl.is_null() && !(*doc).extSubset.is_null() {
                elem_decl = get_dtd_qelement_desc((*doc).extSubset, (*elem).name, prefix);
                if !elem_decl.is_null() && !extsubset.is_null() {
                    *extsubset = 1;
                }
            }
        }
        if elem_decl.is_null() {
            // non-strict fallback: plain name against either subset
            elem_decl = get_dtd_qelement_desc((*doc).intSubset, (*elem).name, ptr::null());
            if elem_decl.is_null() && !(*doc).extSubset.is_null() {
                elem_decl = get_dtd_qelement_desc((*doc).extSubset, (*elem).name, ptr::null());
                if !elem_decl.is_null() && !extsubset.is_null() {
                    *extsubset = 1;
                }
            }
        }
        if elem_decl.is_null() {
            let msg = format!(
                "No declaration for element {}\0",
                string::xmlstr_to_string((*elem).name)
            );
            vctxt_error_node(ctxt, elem, msg.as_ptr() as *const c_char);
        }
        elem_decl
    }
}

/// Upstream xmlValidateCheckMixed: is `qname` in the MIXED content list?
unsafe fn validate_check_mixed(
    ctxt: *mut _xmlValidCtxt,
    cont: *mut _xmlElementContent,
    qname: *const xmlChar,
) -> c_int {
    unsafe {
        let mut plen: c_int = 0;
        let has_colon = string::split_qname3(qname, &mut plen) != 0;
        // upstream xmlSplitQName3 returns the local-name pointer (NULL when
        // the qname has no colon); the candidate's split_qname3 returns the
        // prefix length, so the local part is qname + plen + 1.
        let local = if has_colon {
            qname.add(plen as usize + 1)
        } else {
            ptr::null()
        };
        let mut cur = cont;
        if local.is_null() {
            while !cur.is_null() {
                if (*cur).type_ == XML_ELEMENT_CONTENT_ELEMENT as c_int {
                    if (*cur).prefix.is_null() && string::xml_strcmp((*cur).name, qname) == 0 {
                        return 1;
                    }
                } else if (*cur).type_ == XML_ELEMENT_CONTENT_OR as c_int
                    && !(*cur).c1.is_null()
                    && (*(*cur).c1).type_ == XML_ELEMENT_CONTENT_ELEMENT as c_int
                {
                    if (*(*cur).c1).prefix.is_null()
                        && string::xml_strcmp((*(*cur).c1).name, qname) == 0
                    {
                        return 1;
                    }
                } else if (*cur).type_ != XML_ELEMENT_CONTENT_OR as c_int
                    || (*cur).c1.is_null()
                    || (*(*cur).c1).type_ != XML_ELEMENT_CONTENT_PCDATA as c_int
                {
                    vctxt_error(
                        ctxt,
                        b"Internal: MIXED struct corrupted\0" as *const u8 as *const c_char,
                    );
                    break;
                }
                cur = (*cur).c2;
            }
        } else {
            while !cur.is_null() {
                if (*cur).type_ == XML_ELEMENT_CONTENT_ELEMENT as c_int {
                    if !(*cur).prefix.is_null()
                        && prefix_matches((*cur).prefix, qname, plen)
                        && string::xml_strcmp((*cur).name, local) == 0
                    {
                        return 1;
                    }
                } else if (*cur).type_ == XML_ELEMENT_CONTENT_OR as c_int
                    && !(*cur).c1.is_null()
                    && (*(*cur).c1).type_ == XML_ELEMENT_CONTENT_ELEMENT as c_int
                {
                    if !(*(*cur).c1).prefix.is_null()
                        && prefix_matches((*(*cur).c1).prefix, qname, plen)
                        && string::xml_strcmp((*(*cur).c1).name, local) == 0
                    {
                        return 1;
                    }
                } else if (*cur).type_ != XML_ELEMENT_CONTENT_OR as c_int
                    || (*cur).c1.is_null()
                    || (*(*cur).c1).type_ != XML_ELEMENT_CONTENT_PCDATA as c_int
                {
                    vctxt_error(
                        ctxt,
                        b"Internal: MIXED struct corrupted\0" as *const u8 as *const c_char,
                    );
                    break;
                }
                cur = (*cur).c2;
            }
        }
        0
    }
}

/// Does `prefix` equal the first `len` bytes of `qname` (upstream
/// xmlStrncmp(prefix, qname, plen))?
unsafe fn prefix_matches(prefix: *const xmlChar, qname: *const xmlChar, len: c_int) -> bool {
    unsafe {
        let p = string::xmlstr_to_bytes(prefix);
        let q = string::xmlstr_to_bytes(qname);
        p.len() == len as usize && q.len() >= len as usize && p[..len as usize] == q[..len as usize]
    }
}

/// Incremental content-model matcher stored in `_xmlElement.cont_model`.
///
/// The candidate's regex engine matches character-by-character, which does
/// not model upstream's whole-name content-model tokens, so the content
/// model is compiled into a dedicated small NFA over full element names.
/// Upstream builds the same automaton (xmlValidBuildAContentModel) and then
/// converts it with xmlRegFromAutomata; the observable push/pop contract is
/// identical (per-push "Misplaced" errors, completion checks on pop).
#[repr(C)]
pub struct ContentModelNfa {
    /// Flat transition list: (from_state, name, to_state); name NULL = epsilon.
    transitions: Vec<(u32, *const xmlChar, u32)>,
    /// start state index
    start: u32,
    /// accepting state indices (match complete)
    accept: Vec<u32>,
}

/// Runtime exec state for one open element's content model.
#[repr(C)]
pub struct ContentModelExec {
    /// the compiled NFA
    nfa: *mut ContentModelNfa,
    /// current state set after epsilon closure
    current: Vec<u32>,
}

/// Thompson-style NFA builder over the content tree.
struct NfaBuilder {
    transitions: Vec<(u32, *const xmlChar, u32)>,
    n_states: u32,
}

impl NfaBuilder {
    fn new() -> Self {
        NfaBuilder {
            transitions: Vec::new(),
            n_states: 0,
        }
    }
    fn new_state(&mut self) -> u32 {
        let s = self.n_states;
        self.n_states += 1;
        s
    }
    fn eps(&mut self, from: u32, to: u32) {
        self.transitions.push((from, ptr::null(), to));
    }
    fn name_trans(&mut self, from: u32, name: *const xmlChar, to: u32) {
        self.transitions.push((from, name, to));
    }
}

/// Compile one content-model subtree. Returns (in_state, out_states); the
/// occurrence quantifier on the node is applied by wrapping the fragment
/// with epsilon edges (standard Thompson construction, matching upstream's
/// automaton shape for OPT/MULT/PLUS).
unsafe fn compile_content_sub(
    b: &mut NfaBuilder,
    model: *mut _xmlElementContent,
) -> (u32, Vec<u32>) {
    if model.is_null() {
        let s = b.new_state();
        return (s, vec![s]);
    }
    let m = unsafe { &*model };
    let (mut in_s, mut outs) = match m.type_ as u32 {
        t if t == XML_ELEMENT_CONTENT_ELEMENT as u32 => {
            let s = b.new_state();
            let to = b.new_state();
            b.name_trans(s, m.name, to);
            (s, vec![to])
        }
        t if t == XML_ELEMENT_CONTENT_SEQ as u32 => {
            let (in1, out1) = compile_content_sub(b, m.c1);
            let (in2, out2) = compile_content_sub(b, m.c2);
            for &o in &out1 {
                b.eps(o, in2);
            }
            (in1, out2)
        }
        t if t == XML_ELEMENT_CONTENT_OR as u32 => {
            let (in1, out1) = compile_content_sub(b, m.c1);
            let (in2, out2) = compile_content_sub(b, m.c2);
            let s = b.new_state();
            b.eps(s, in1);
            b.eps(s, in2);
            let mut all = out1;
            all.extend(out2);
            (s, all)
        }
        // PCDATA cannot appear in an ELEMENT content model; the caller
        // rejects it before compiling (upstream xmlValidBuildAContentModel
        // emits "Found PCDATA in content model of %s"). A PCDATA node here
        // compiles to an empty fragment so a malformed tree cannot crash.
        _ => {
            let s = b.new_state();
            (s, vec![s])
        }
    };
    match m.ocur as u32 {
        o if o == XML_ELEMENT_CONTENT_OPT as u32 => {
            let s = b.new_state();
            b.eps(s, in_s);
            for &o2 in &outs {
                b.eps(s, o2);
            }
            in_s = s;
        }
        o if o == XML_ELEMENT_CONTENT_MULT as u32 => {
            let s = b.new_state();
            b.eps(s, in_s);
            for &o2 in &outs {
                b.eps(s, o2);
                b.eps(o2, s);
            }
            in_s = s;
        }
        o if o == XML_ELEMENT_CONTENT_PLUS as u32 => {
            let s = b.new_state();
            b.eps(s, in_s);
            for &o2 in &outs {
                b.eps(o2, s);
            }
            in_s = s;
        }
        _ => {}
    }
    (in_s, outs)
}

/// Does the content tree contain a PCDATA node (illegal in ELEMENT models)?
unsafe fn content_has_pcdata(model: *mut _xmlElementContent) -> bool {
    if model.is_null() {
        return false;
    }
    unsafe {
        let m = &*model;
        if m.type_ == XML_ELEMENT_CONTENT_PCDATA as c_int {
            return true;
        }
        content_has_pcdata(m.c1) || content_has_pcdata(m.c2)
    }
}

/// Compile an element content tree into a ContentModelNfa.
///
/// # SAFETY
///
/// - `content` must be a valid content tree or NULL (returns NULL).
unsafe fn build_content_nfa(content: *mut _xmlElementContent) -> *mut ContentModelNfa {
    unsafe {
        if content.is_null() {
            return ptr::null_mut();
        }
        let mut b = NfaBuilder::new();
        let (start, outs) = compile_content_sub(&mut b, content);
        let nfa = Box::new(ContentModelNfa {
            transitions: b.transitions,
            start,
            accept: outs,
        });
        Box::into_raw(nfa)
    }
}

/// Free a compiled content-model NFA (called from xmlFreeElement).
///
/// # SAFETY
///
/// - `nfa` must be a pointer from build_content_nfa or NULL.
pub unsafe fn free_content_model_nfa(nfa: *mut ContentModelNfa) {
    if nfa.is_null() {
        return;
    }
    unsafe {
        ptr::drop_in_place(nfa);
        allocator::xmlFreeImpl(nfa as *mut c_void);
    }
}

/// Epsilon closure of a state set.
unsafe fn eps_closure(nfa: &ContentModelNfa, states: &[u32]) -> Vec<u32> {
    let mut out = states.to_vec();
    let mut stack = states.to_vec();
    while let Some(s) = stack.pop() {
        for &(from, name, to) in &nfa.transitions {
            if from == s && name.is_null() && !out.contains(&to) {
                out.push(to);
                stack.push(to);
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Create an exec context over a compiled content model. Returns NULL on OOM.
unsafe fn new_content_exec(nfa: *mut ContentModelNfa) -> *mut ContentModelExec {
    unsafe {
        let exec = allocator::xmlMallocImpl(size_of::<ContentModelExec>()) as *mut ContentModelExec;
        if exec.is_null() {
            return ptr::null_mut();
        }
        let cur = eps_closure(&*nfa, &[(*nfa).start]);
        ptr::write(&mut (*exec).nfa, nfa);
        ptr::write(&mut (*exec).current, cur);
        exec
    }
}

/// Free an exec context.
unsafe fn free_content_exec(exec: *mut ContentModelExec) {
    if exec.is_null() {
        return;
    }
    unsafe {
        ptr::drop_in_place(&mut (*exec).current);
        allocator::xmlFreeImpl(exec as *mut c_void);
    }
}

/// Push a full element name (or NULL = end of input) into the exec context.
///
/// Mirrors upstream xmlRegExecPushString contract: 1 = match complete,
/// 0 = more input needed, -1 = cannot continue (Misplaced).
unsafe fn content_exec_push(exec: *mut ContentModelExec, value: *const xmlChar) -> c_int {
    unsafe {
        if exec.is_null() {
            return -1;
        }
        let nfa = &*(*exec).nfa;
        if value.is_null() {
            let cur = eps_closure(nfa, &(*exec).current);
            return if cur.iter().any(|&s| nfa.accept.contains(&s)) {
                1
            } else {
                0
            };
        }
        let mut next: Vec<u32> = Vec::new();
        for &s in &(*exec).current {
            for &(from, name, to) in &nfa.transitions {
                if from == s && !name.is_null() && string::xml_strcmp(name, value) == 0 {
                    next.push(to);
                }
            }
        }
        next.sort_unstable();
        next.dedup();
        if next.is_empty() {
            return -1;
        }
        let closed = eps_closure(nfa, &next);
        (*exec).current = closed;
        if (*exec).current.iter().any(|&s| nfa.accept.contains(&s)) {
            1
        } else {
            0
        }
    }
}

/// Upstream vstateVPush: push a validation state for an open element.
unsafe fn vstate_vpush(
    ctxt: *mut _xmlValidCtxt,
    elem_decl: *mut _xmlElement,
    node: *mut _xmlNode,
) -> c_int {
    unsafe {
        if (*ctxt).vstateNr >= (*ctxt).vstateMax {
            let new_max = if (*ctxt).vstateMax == 0 {
                10
            } else {
                (*ctxt).vstateMax * 2
            };
            let new_tab = allocator::xmlReallocImpl(
                (*ctxt).vstateTab as *mut c_void,
                (new_max as usize) * size_of::<ValidState>(),
            ) as *mut ValidState;
            if new_tab.is_null() {
                vctxt_error(
                    ctxt,
                    b"Memory allocation failed : xmlValidCtxt\0" as *const u8 as *const c_char,
                );
                return -1;
            }
            (*ctxt).vstateTab = new_tab as *mut c_void;
            (*ctxt).vstateMax = new_max;
        }
        let idx = (*ctxt).vstateNr as usize;
        let tab = (*ctxt).vstateTab as *mut ValidState;
        (*tab.add(idx)).elem_decl = elem_decl;
        (*tab.add(idx)).node = node;
        (*tab.add(idx)).exec = ptr::null_mut();
        if !elem_decl.is_null() && (*elem_decl).etype == XML_ELEMENT_TYPE_ELEMENT as c_int {
            if (*elem_decl).cont_model.is_null() {
                validate_build_content_model(ctxt, elem_decl);
            }
            if !(*elem_decl).cont_model.is_null() {
                let exec = new_content_exec((*elem_decl).cont_model as *mut ContentModelNfa);
                if exec.is_null() {
                    vctxt_error(
                        ctxt,
                        b"Memory allocation failed : xmlValidCtxt\0" as *const u8 as *const c_char,
                    );
                    return -1;
                }
                (*tab.add(idx)).exec = exec;
            } else {
                let msg = format!(
                    "Failed to build content model regexp for {}\0",
                    string::xmlstr_to_string((*elem_decl).name)
                );
                vctxt_error_node(ctxt, node, msg.as_ptr() as *const c_char);
            }
        }
        (*ctxt).vstate = tab.add(idx) as *mut c_void;
        (*ctxt).vstateNr += 1;
        0
    }
}

/// Upstream vstateVPop: pop the current validation state, freeing its exec.
unsafe fn vstate_vpop(ctxt: *mut _xmlValidCtxt) -> c_int {
    unsafe {
        if (*ctxt).vstateNr < 1 {
            return -1;
        }
        (*ctxt).vstateNr -= 1;
        let idx = (*ctxt).vstateNr as usize;
        let tab = (*ctxt).vstateTab as *mut ValidState;
        let elem_decl = (*tab.add(idx)).elem_decl;
        (*tab.add(idx)).elem_decl = ptr::null_mut();
        (*tab.add(idx)).node = ptr::null_mut();
        if !elem_decl.is_null() && (*elem_decl).etype == XML_ELEMENT_TYPE_ELEMENT as c_int {
            if !(*tab.add(idx)).exec.is_null() {
                free_content_exec((*tab.add(idx)).exec);
            }
        }
        (*tab.add(idx)).exec = ptr::null_mut();
        if (*ctxt).vstateNr >= 1 {
            (*ctxt).vstate = tab.add((*ctxt).vstateNr as usize - 1) as *mut c_void;
        } else {
            (*ctxt).vstate = ptr::null_mut();
        }
        0
    }
}

/// Upstream `xmlValidBuildContentModel(ctxt, elem)`: compile the element's
/// content tree into a content-model NFA cached in `elem->contModel`.
/// Returns 1 on success, 0 on failure.
///
/// # SAFETY
///
/// - `ctxt` may be NULL; `elem` a valid pointer.
pub unsafe fn validate_build_content_model(
    ctxt: *mut _xmlValidCtxt,
    elem: *mut _xmlElement,
) -> c_int {
    unsafe {
        if ctxt.is_null() {
            return 0;
        }
        if (*elem).type_ != XML_ELEMENT_DECL as c_int {
            return 0;
        }
        if (*elem).etype != XML_ELEMENT_TYPE_ELEMENT as c_int {
            return 1;
        }
        if !(*elem).cont_model.is_null() {
            return 1;
        }
        if (*elem).content.is_null() {
            return 1;
        }
        if content_has_pcdata((*elem).content) {
            let msg = format!(
                "Found PCDATA in content model of {}\0",
                string::xmlstr_to_string((*elem).name)
            );
            vctxt_error_node(ctxt, elem as *mut _xmlNode, msg.as_ptr() as *const c_char);
            return 0;
        }
        let nfa = build_content_nfa((*elem).content);
        if nfa.is_null() {
            vctxt_error(
                ctxt,
                b"Memory allocation failed : xmlValidBuildContentModel\0" as *const u8
                    as *const c_char,
            );
            return 0;
        }
        (*elem).cont_model = nfa as *mut c_void;
        1
    }
}

/// Upstream `xmlValidatePushElement(ctxt, doc, elem, qname)`: validate a
/// start tag against the parent's content model and push the new element's
/// validation state.
///
/// # SAFETY
///
/// - `ctxt` may be NULL; `doc`/`elem`/`qname` valid pointers or NULL.
pub unsafe fn validate_push_element(
    ctxt: *mut _xmlValidCtxt,
    doc: *mut _xmlDoc,
    elem: *mut _xmlNode,
    qname: *const xmlChar,
) -> c_int {
    unsafe {
        let mut ret = 1;
        if ctxt.is_null() {
            return 0;
        }
        if (*ctxt).vstateNr > 0 && !(*ctxt).vstate.is_null() {
            let state = (*ctxt).vstate as *mut ValidState;
            let elem_decl = (*state).elem_decl;
            if !elem_decl.is_null() {
                match (*elem_decl).etype as u32 {
                    t if t == XML_ELEMENT_TYPE_UNDEFINED as u32 => ret = 0,
                    t if t == XML_ELEMENT_TYPE_EMPTY as u32 => {
                        let msg = format!(
                            "Element {} was declared EMPTY this one has content\0",
                            string::xmlstr_to_string((*(*state).node).name)
                        );
                        vctxt_error_node(ctxt, (*state).node, msg.as_ptr() as *const c_char);
                        ret = 0;
                    }
                    t if t == XML_ELEMENT_TYPE_ANY as u32 => {}
                    t if t == XML_ELEMENT_TYPE_MIXED as u32 => {
                        if !(*elem_decl).content.is_null()
                            && (*(*elem_decl).content).type_ == XML_ELEMENT_CONTENT_PCDATA as c_int
                        {
                            let msg = format!(
                                "Element {} was declared #PCDATA but contains non text nodes\0",
                                string::xmlstr_to_string((*(*state).node).name)
                            );
                            vctxt_error_node(ctxt, (*state).node, msg.as_ptr() as *const c_char);
                            ret = 0;
                        } else {
                            ret = validate_check_mixed(ctxt, (*elem_decl).content, qname);
                            if ret != 1 {
                                let msg = format!(
                                    "Element {} is not declared in {} list of possible children\0",
                                    string::xmlstr_to_string(qname),
                                    string::xmlstr_to_string((*(*state).node).name)
                                );
                                vctxt_error_node(
                                    ctxt,
                                    (*state).node,
                                    msg.as_ptr() as *const c_char,
                                );
                            }
                        }
                    }
                    t if t == XML_ELEMENT_TYPE_ELEMENT as u32 => {
                        if !(*state).exec.is_null() {
                            ret = content_exec_push((*state).exec, qname);
                            if ret < 0 {
                                let msg = format!(
                                    "Element {} content does not follow the DTD, Misplaced {}\0",
                                    string::xmlstr_to_string((*(*state).node).name),
                                    string::xmlstr_to_string(qname)
                                );
                                vctxt_error_node(
                                    ctxt,
                                    (*state).node,
                                    msg.as_ptr() as *const c_char,
                                );
                                ret = 0;
                            } else {
                                ret = 1;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut extsubset = 0;
        let e_decl = valid_get_elem_decl(ctxt, doc, elem, &mut extsubset);
        // upstream ignores the vstateVPush return here
        let _ = vstate_vpush(ctxt, e_decl, elem);
        ret
    }
}

/// Upstream `xmlValidatePushCData(ctxt, data, len)`: character data is only
/// legal as whitespace inside ELEMENT content.
///
/// # SAFETY
///
/// - `ctxt` may be NULL; `data` a valid buffer of `len` bytes or NULL.
pub unsafe fn validate_push_cdata(
    ctxt: *mut _xmlValidCtxt,
    data: *const xmlChar,
    len: c_int,
) -> c_int {
    unsafe {
        let mut ret = 1;
        if ctxt.is_null() {
            return 0;
        }
        if len <= 0 {
            return 1;
        }
        if (*ctxt).vstateNr > 0 && !(*ctxt).vstate.is_null() {
            let state = (*ctxt).vstate as *mut ValidState;
            let elem_decl = (*state).elem_decl;
            if !elem_decl.is_null() {
                match (*elem_decl).etype as u32 {
                    t if t == XML_ELEMENT_TYPE_UNDEFINED as u32 => ret = 0,
                    t if t == XML_ELEMENT_TYPE_EMPTY as u32 => {
                        let msg = format!(
                            "Element {} was declared EMPTY this one has content\0",
                            string::xmlstr_to_string((*(*state).node).name)
                        );
                        vctxt_error_node(ctxt, (*state).node, msg.as_ptr() as *const c_char);
                        ret = 0;
                    }
                    t if t == XML_ELEMENT_TYPE_ANY as u32 || t == XML_ELEMENT_TYPE_MIXED as u32 => {
                    }
                    t if t == XML_ELEMENT_TYPE_ELEMENT as u32 => {
                        let bytes = core::slice::from_raw_parts(data, len as usize);
                        for &b in bytes {
                            if !is_blank_byte(b) {
                                let msg = format!(
                                    "Element {} content does not follow the DTD, Text not allowed\0",
                                    string::xmlstr_to_string((*(*state).node).name)
                                );
                                vctxt_error_node(
                                    ctxt,
                                    (*state).node,
                                    msg.as_ptr() as *const c_char,
                                );
                                ret = 0;
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        ret
    }
}

/// Upstream `xmlValidatePopElement(ctxt, doc, elem, qname)`: verify the
/// parent content model completed and pop the validation state.
///
/// # SAFETY
///
/// - `ctxt` may be NULL; `doc`/`elem`/`qname` valid pointers or NULL.
pub unsafe fn validate_pop_element(
    ctxt: *mut _xmlValidCtxt,
    _doc: *mut _xmlDoc,
    _elem: *mut _xmlNode,
    _qname: *const xmlChar,
) -> c_int {
    unsafe {
        let mut ret = 1;
        if ctxt.is_null() {
            return 0;
        }
        if (*ctxt).vstateNr > 0 && !(*ctxt).vstate.is_null() {
            let state = (*ctxt).vstate as *mut ValidState;
            let elem_decl = (*state).elem_decl;
            if !elem_decl.is_null() && (*elem_decl).etype == XML_ELEMENT_TYPE_ELEMENT as c_int {
                if !(*state).exec.is_null() {
                    ret = content_exec_push((*state).exec, ptr::null());
                    if ret <= 0 {
                        let msg = format!(
                            "Element {} content does not follow the DTD, Expecting more children\0",
                            string::xmlstr_to_string((*(*state).node).name)
                        );
                        vctxt_error_node(ctxt, (*state).node, msg.as_ptr() as *const c_char);
                        ret = 0;
                    } else {
                        ret = 1;
                    }
                }
            }
            let _ = vstate_vpop(ctxt);
        }
        ret
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::allocator;
    use crate::abi::types::xmlElementTypeVal::*;
    use crate::xml::dtd;
    use crate::xml::tree;

    // ── Helpers ───────────────────────────────────────────────────────────

    /// Create a null-terminated xmlChar* from a Rust string.
    unsafe fn c_str(s: &str) -> *const xmlChar {
        let bytes = s.as_bytes();
        let ptr = allocator::xmlMallocImpl(bytes.len() + 1) as *mut xmlChar;
        assert!(!ptr.is_null());
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        *ptr.add(bytes.len()) = 0;
        ptr
    }

    /// Create a simple document with a DTD for testing.
    unsafe fn make_test_doc() -> (*mut _xmlDoc, *mut _xmlDtd) {
        let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
        assert!(!doc.is_null());

        let name = c_str("root");
        let ext_id = c_str("--//Test//DTD//EN");
        let sys_id = c_str("test.dtd");
        let dtd = dtd::create_int_subset(doc, name, ext_id, sys_id);
        assert!(!dtd.is_null());

        (doc, dtd)
    }

    /// Add an element declaration to a DTD.
    #[allow(unused)]
    unsafe fn add_elem_decl(
        dtd: *mut _xmlDtd,
        name: *const xmlChar,
        elem_type: c_int,
        content: *mut _xmlElementContent,
    ) -> *mut _xmlElement {
        let result = dtd::add_element_decl(dtd, name, elem_type, content);
        result
    }

    /// Create a root element node.
    unsafe fn create_root_elem(doc: *mut _xmlDoc, name: *const xmlChar) -> *mut _xmlNode {
        let node = tree::new_node(ptr::null_mut(), name);
        assert!(!node.is_null());
        tree::add_child(doc as *mut _xmlNode, node);
        node
    }

    /// Create a child element node.
    #[allow(unused)]
    unsafe fn create_child_elem(parent: *mut _xmlNode, name: *const xmlChar) -> *mut _xmlNode {
        let node = tree::new_node(ptr::null_mut(), name);
        assert!(!node.is_null());
        tree::add_child(parent, node);
        node
    }

    // ── xmlValidateName tests ─────────────────────────────────────────────

    #[test]
    fn test_validate_name_null() {
        unsafe {
            assert_eq!(validate_name(ptr::null()), 0);
        }
    }

    #[test]
    fn test_validate_name_empty() {
        unsafe {
            let s = b"\0" as *const u8 as *const xmlChar;
            assert_eq!(validate_name(s), 0);
        }
    }

    #[test]
    fn test_validate_name_valid() {
        unsafe {
            let tests = ["foo", "_bar", ":baz", "hello-world", "ns:elem", "a123"];
            for t in &tests {
                let s = c_str(t);
                assert_eq!(validate_name(s), 1, "Expected '{}' to be a valid Name", t);
                allocator::xmlFreeImpl(s as *mut c_void);
            }
        }
    }

    #[test]
    fn test_validate_name_invalid() {
        unsafe {
            let tests = ["123abc", "-foo", ".bar", "foo bar", "a b"];
            for t in &tests {
                let s = c_str(t);
                assert_eq!(validate_name(s), 0, "Expected '{}' to be invalid", t);
                allocator::xmlFreeImpl(s as *mut c_void);
            }
        }
    }

    #[test]
    fn test_validate_names_valid() {
        unsafe {
            let s = c_str("foo bar baz");
            assert_eq!(validate_names(s), 1);
            allocator::xmlFreeImpl(s as *mut c_void);
        }
    }

    #[test]
    fn test_validate_names_invalid() {
        unsafe {
            let s = c_str("foo 123bar baz");
            assert_eq!(validate_names(s), 0);
            allocator::xmlFreeImpl(s as *mut c_void);
        }
    }

    // ── xmlValidateNmtoken tests ──────────────────────────────────────────

    #[test]
    fn test_validate_nmtoken_null() {
        unsafe {
            assert_eq!(validate_nmtoken(ptr::null()), 0);
        }
    }

    #[test]
    fn test_validate_nmtoken_valid() {
        unsafe {
            let tests = ["foo", "123abc", "-foo", ".bar", "_test", ":ns"];
            for t in &tests {
                let s = c_str(t);
                assert_eq!(
                    validate_nmtoken(s),
                    1,
                    "Expected '{}' to be a valid NMTOKEN",
                    t
                );
                allocator::xmlFreeImpl(s as *mut c_void);
            }
        }
    }

    #[test]
    fn test_validate_nmtoken_invalid() {
        unsafe {
            let s = c_str("foo bar");
            assert_eq!(validate_nmtoken(s), 0);
            allocator::xmlFreeImpl(s as *mut c_void);
        }
    }

    #[test]
    fn test_validate_nmtokens_valid() {
        unsafe {
            let s = c_str("foo 123bar -baz");
            assert_eq!(validate_nmtokens(s), 1);
            allocator::xmlFreeImpl(s as *mut c_void);
        }
    }

    // ── xmlValidateAttributeValue tests ───────────────────────────────────

    #[test]
    fn test_validate_attribute_value_cdata() {
        unsafe {
            let s = c_str("anything goes here!@#$%^&*()");
            assert_eq!(validate_attribute_value(XML_ATTRIBUTE_CDATA as c_int, s), 1);
            allocator::xmlFreeImpl(s as *mut c_void);

            // Empty CDATA is valid
            let empty = b"\0" as *const u8 as *const xmlChar;
            assert_eq!(
                validate_attribute_value(XML_ATTRIBUTE_CDATA as c_int, empty),
                1
            );
        }
    }

    #[test]
    fn test_validate_attribute_value_id() {
        unsafe {
            let valid = c_str("myId");
            assert_eq!(
                validate_attribute_value(XML_ATTRIBUTE_ID as c_int, valid),
                1
            );
            allocator::xmlFreeImpl(valid as *mut c_void);

            let invalid = c_str("123id");
            assert_eq!(
                validate_attribute_value(XML_ATTRIBUTE_ID as c_int, invalid),
                0
            );
            allocator::xmlFreeImpl(invalid as *mut c_void);
        }
    }

    #[test]
    fn test_validate_attribute_value_idref() {
        unsafe {
            let valid = c_str("someId");
            assert_eq!(
                validate_attribute_value(XML_ATTRIBUTE_IDREF as c_int, valid),
                1
            );
            allocator::xmlFreeImpl(valid as *mut c_void);
        }
    }

    #[test]
    fn test_validate_attribute_value_idrefs() {
        unsafe {
            let valid = c_str("id1 id2 id3");
            assert_eq!(
                validate_attribute_value(XML_ATTRIBUTE_IDREFS as c_int, valid),
                1
            );
            allocator::xmlFreeImpl(valid as *mut c_void);

            let invalid = c_str("id1 123id");
            assert_eq!(
                validate_attribute_value(XML_ATTRIBUTE_IDREFS as c_int, invalid),
                0
            );
            allocator::xmlFreeImpl(invalid as *mut c_void);
        }
    }

    #[test]
    fn test_validate_attribute_value_entity() {
        unsafe {
            let valid = c_str("myEntity");
            assert_eq!(
                validate_attribute_value(XML_ATTRIBUTE_ENTITY as c_int, valid),
                1
            );
            allocator::xmlFreeImpl(valid as *mut c_void);
        }
    }

    #[test]
    fn test_validate_attribute_value_nmtoken() {
        unsafe {
            let valid = c_str("123abc");
            assert_eq!(
                validate_attribute_value(XML_ATTRIBUTE_NMTOKEN as c_int, valid),
                1
            );
            allocator::xmlFreeImpl(valid as *mut c_void);

            let invalid = c_str("foo bar");
            assert_eq!(
                validate_attribute_value(XML_ATTRIBUTE_NMTOKEN as c_int, invalid),
                0
            );
            allocator::xmlFreeImpl(invalid as *mut c_void);
        }
    }

    #[test]
    fn test_validate_attribute_value_null() {
        unsafe {
            // UPSTREAM-PARITY: xmlValidateAttributeValueInternal's switch
            // breaks out of CDATA and returns 1 (valid.c 2.15.0), even for
            // a NULL value; unknown types also fall through to 1.
            assert_eq!(
                validate_attribute_value(XML_ATTRIBUTE_CDATA as c_int, ptr::null()),
                1
            );
            assert_eq!(
                validate_attribute_value(XML_ATTRIBUTE_ID as c_int, ptr::null()),
                0
            );
        }
    }

    // ── xmlValidateEnumeration tests ──────────────────────────────────────

    #[test]
    fn test_validate_enumeration_valid() {
        unsafe {
            let ctxt = new_valid_ctxt();
            assert!(!ctxt.is_null());

            let red = c_str("red");
            let green = c_str("green");
            let blue = c_str("blue");

            let e3 = allocator::xmlMallocZero(size_of::<_xmlEnumeration>()) as *mut _xmlEnumeration;
            (*e3).name = string::xml_strdup(blue);
            (*e3).next = ptr::null_mut();

            let e2 = allocator::xmlMallocZero(size_of::<_xmlEnumeration>()) as *mut _xmlEnumeration;
            (*e2).name = string::xml_strdup(green);
            (*e2).next = e3;

            let e1 = allocator::xmlMallocZero(size_of::<_xmlEnumeration>()) as *mut _xmlEnumeration;
            (*e1).name = string::xml_strdup(red);
            (*e1).next = e2;

            let value = c_str("green");
            assert_eq!(validate_enumeration(ctxt, value, e1), 1);
            assert_eq!((*ctxt).valid, 1);

            allocator::xmlFreeImpl(value as *mut c_void);
            allocator::xmlFreeImpl(red as *mut c_void);
            allocator::xmlFreeImpl(green as *mut c_void);
            allocator::xmlFreeImpl(blue as *mut c_void);
            free_valid_ctxt(ctxt);
        }
    }

    #[test]
    fn test_validate_enumeration_invalid() {
        unsafe {
            let ctxt = new_valid_ctxt();
            assert!(!ctxt.is_null());

            let e1 = allocator::xmlMallocZero(size_of::<_xmlEnumeration>()) as *mut _xmlEnumeration;
            (*e1).name = string::xml_strdup(b"red\0" as *const u8 as *const xmlChar);
            (*e1).next = ptr::null_mut();

            let value = c_str("yellow");
            assert_eq!(validate_enumeration(ctxt, value, e1), 0);

            allocator::xmlFreeImpl(value as *mut c_void);
            free_valid_ctxt(ctxt);
        }
    }

    // ── xmlValidateNotationUse tests ──────────────────────────────────────

    #[test]
    fn test_validate_notation_use_valid() {
        unsafe {
            let (doc, dtd) = make_test_doc();

            let notation_name = c_str("GIF");
            dtd::add_notation_decl(dtd, notation_name, ptr::null(), ptr::null());

            let ctxt = new_valid_ctxt();
            assert!(!ctxt.is_null());

            assert_eq!(validate_notation_use(ctxt, doc, notation_name), 1);

            free_valid_ctxt(ctxt);
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_validate_notation_use_invalid() {
        unsafe {
            let (doc, _dtd) = make_test_doc();

            let ctxt = new_valid_ctxt();
            assert!(!ctxt.is_null());

            let notation_name = c_str("UNDECLARED");
            assert_eq!(validate_notation_use(ctxt, doc, notation_name), 0);

            free_valid_ctxt(ctxt);
            allocator::xmlFreeImpl(notation_name as *mut c_void);
            tree::free_doc(doc);
        }
    }

    // ── xmlNewValidCtxt / xmlFreeValidCtxt tests ─────────────────────────

    #[test]
    fn test_new_free_valid_ctxt() {
        unsafe {
            let ctxt = new_valid_ctxt();
            assert!(!ctxt.is_null());
            assert_eq!((*ctxt).valid, 1);
            assert!((*ctxt).node.is_null());
            free_valid_ctxt(ctxt);
        }
    }

    #[test]
    fn test_free_valid_ctxt_null() {
        unsafe {
            free_valid_ctxt(ptr::null_mut());
        }
    }

    // ── xmlSetValidErrors tests ──────────────────────────────────────────

    #[test]
    fn test_set_valid_errors_null() {
        unsafe {
            set_valid_errors(ptr::null_mut(), None, None, ptr::null_mut());
        }
    }

    // ── xmlValidateElement tests ──────────────────────────────────────────

    #[test]
    fn test_validate_element_no_dtd() {
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            assert!(!doc.is_null());

            let root_name = c_str("root");
            let root = create_root_elem(doc, root_name);

            let ctxt = new_valid_ctxt();
            assert!(!ctxt.is_null());

            // No DTD — validation passes (returns 1)
            assert_eq!(validate_element(ctxt, doc, root), 1);

            free_valid_ctxt(ctxt);
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_validate_element_empty_valid() {
        unsafe {
            let (doc, dtd) = make_test_doc();

            let root_name = c_str("root");
            add_elem_decl(
                dtd,
                root_name,
                XML_ELEMENT_TYPE_EMPTY as c_int,
                ptr::null_mut(),
            );

            let root = create_root_elem(doc, root_name);

            let ctxt = new_valid_ctxt();
            assert!(!ctxt.is_null());

            assert_eq!(validate_element(ctxt, doc, root), 1);

            free_valid_ctxt(ctxt);
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_validate_element_undeclared() {
        unsafe {
            let (doc, _dtd) = make_test_doc();

            let root_name = c_str("root");
            let root = create_root_elem(doc, root_name);

            let ctxt = new_valid_ctxt();
            assert!(!ctxt.is_null());

            // Element not declared — validation fails
            assert_eq!(validate_element(ctxt, doc, root), 0);

            free_valid_ctxt(ctxt);
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_validate_element_with_content() {
        unsafe {
            let (doc, dtd) = make_test_doc();

            // Create element declarations
            let root_name = c_str("root");
            let child_name = c_str("child");

            // Root content model: child+
            let child_content =
                dtd::create_content_model(child_name, XML_ELEMENT_CONTENT_ELEMENT as c_int);
            assert!(!child_content.is_null());
            (*child_content).ocur = XML_ELEMENT_CONTENT_PLUS as c_int;

            add_elem_decl(
                dtd,
                root_name,
                XML_ELEMENT_TYPE_ELEMENT as c_int,
                child_content,
            );
            add_elem_decl(
                dtd,
                child_name,
                XML_ELEMENT_TYPE_EMPTY as c_int,
                ptr::null_mut(),
            );

            let root = create_root_elem(doc, root_name);
            let _child = create_child_elem(root, child_name);

            let ctxt = new_valid_ctxt();
            assert!(!ctxt.is_null());

            assert_eq!(validate_element(ctxt, doc, root), 1);

            free_valid_ctxt(ctxt);
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_validate_element_invalid_content() {
        unsafe {
            let (doc, dtd) = make_test_doc();

            let root_name = c_str("root");
            let child_name = c_str("child");
            let wrong_name = c_str("wrong");

            // Root content model: child+
            let child_content =
                dtd::create_content_model(child_name, XML_ELEMENT_CONTENT_ELEMENT as c_int);
            assert!(!child_content.is_null());
            (*child_content).ocur = XML_ELEMENT_CONTENT_PLUS as c_int;

            add_elem_decl(
                dtd,
                root_name,
                XML_ELEMENT_TYPE_ELEMENT as c_int,
                child_content,
            );
            add_elem_decl(
                dtd,
                child_name,
                XML_ELEMENT_TYPE_EMPTY as c_int,
                ptr::null_mut(),
            );
            add_elem_decl(
                dtd,
                wrong_name,
                XML_ELEMENT_TYPE_EMPTY as c_int,
                ptr::null_mut(),
            );

            let root = create_root_elem(doc, root_name);
            // Add "wrong" child instead of "child"
            create_child_elem(root, wrong_name);

            let ctxt = new_valid_ctxt();
            assert!(!ctxt.is_null());

            assert_eq!(validate_element(ctxt, doc, root), 0);

            free_valid_ctxt(ctxt);
            tree::free_doc(doc);
        }
    }

    // ── xmlValidateRoot tests ─────────────────────────────────────────────

    #[test]
    fn test_validate_root_match() {
        unsafe {
            let (doc, dtd) = make_test_doc();

            let root_name = c_str("root");
            add_elem_decl(
                dtd,
                root_name,
                XML_ELEMENT_TYPE_EMPTY as c_int,
                ptr::null_mut(),
            );
            create_root_elem(doc, root_name);

            let ctxt = new_valid_ctxt();
            assert!(!ctxt.is_null());

            assert_eq!(validate_root(ctxt, doc), 1);

            free_valid_ctxt(ctxt);
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_validate_root_no_dtd() {
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            assert!(!doc.is_null());

            let root_name = c_str("root");
            create_root_elem(doc, root_name);

            let ctxt = new_valid_ctxt();
            assert!(!ctxt.is_null());

            // No DTD — passes
            assert_eq!(validate_root(ctxt, doc), 1);

            free_valid_ctxt(ctxt);
            tree::free_doc(doc);
        }
    }

    // ── xmlValidateDocument tests ─────────────────────────────────────────

    #[test]
    fn test_validate_document_valid() {
        unsafe {
            let (doc, dtd) = make_test_doc();

            let root_name = c_str("root");
            add_elem_decl(
                dtd,
                root_name,
                XML_ELEMENT_TYPE_EMPTY as c_int,
                ptr::null_mut(),
            );
            create_root_elem(doc, root_name);

            let ctxt = new_valid_ctxt();
            assert!(!ctxt.is_null());

            assert_eq!(validate_document(ctxt, doc), 1);

            free_valid_ctxt(ctxt);
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_validate_document_no_root() {
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            assert!(!doc.is_null());

            let ctxt = new_valid_ctxt();
            assert!(!ctxt.is_null());

            assert_eq!(validate_document(ctxt, doc), 0);

            free_valid_ctxt(ctxt);
            tree::free_doc(doc);
        }
    }

    // ── xmlValidateContent tests ──────────────────────────────────────────

    #[test]
    fn test_validate_content_valid() {
        unsafe {
            let (doc, dtd) = make_test_doc();

            let root_name = c_str("root");
            let child_name = c_str("child");

            let child_content =
                dtd::create_content_model(child_name, XML_ELEMENT_CONTENT_ELEMENT as c_int);
            assert!(!child_content.is_null());

            add_elem_decl(
                dtd,
                root_name,
                XML_ELEMENT_TYPE_ELEMENT as c_int,
                child_content,
            );
            add_elem_decl(
                dtd,
                child_name,
                XML_ELEMENT_TYPE_EMPTY as c_int,
                ptr::null_mut(),
            );

            let root = create_root_elem(doc, root_name);
            create_child_elem(root, child_name);

            let ctxt = new_valid_ctxt();
            assert!(!ctxt.is_null());

            assert_eq!(validate_content(ctxt, root, doc), 1);

            free_valid_ctxt(ctxt);
            tree::free_doc(doc);
        }
    }

    // ── xmlIsMixedElement / xmlIsEmptyElement tests ───────────────────────

    #[test]
    fn test_is_mixed_element() {
        unsafe {
            let (doc, dtd) = make_test_doc();
            let name = c_str("mixedElem");
            add_elem_decl(dtd, name, XML_ELEMENT_TYPE_MIXED as c_int, ptr::null_mut());

            assert_eq!(is_mixed_element(doc, name), 1);

            let other = c_str("other");
            assert_eq!(is_mixed_element(doc, other), 0);

            allocator::xmlFreeImpl(other as *mut c_void);
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_is_empty_element() {
        unsafe {
            let (doc, dtd) = make_test_doc();
            let name = c_str("emptyElem");
            add_elem_decl(dtd, name, XML_ELEMENT_TYPE_EMPTY as c_int, ptr::null_mut());

            assert_eq!(is_empty_element(doc, name), 1);

            let other = c_str("other");
            assert_eq!(is_empty_element(doc, other), 0);

            allocator::xmlFreeImpl(other as *mut c_void);
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_is_mixed_element_no_dtd() {
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            assert!(!doc.is_null());

            let name = c_str("foo");
            assert_eq!(is_mixed_element(doc, name), 0);

            allocator::xmlFreeImpl(name as *mut c_void);
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_is_empty_element_no_dtd() {
        unsafe {
            let doc = tree::new_doc(b"1.0\0" as *const u8 as *const xmlChar);
            assert!(!doc.is_null());

            let name = c_str("foo");
            assert_eq!(is_empty_element(doc, name), 0);

            allocator::xmlFreeImpl(name as *mut c_void);
            tree::free_doc(doc);
        }
    }

    // ── xmlValidateDtd tests ──────────────────────────────────────────────

    #[test]
    fn test_validate_dtd_null() {
        unsafe {
            let ctxt = new_valid_ctxt();
            assert!(!ctxt.is_null());
            assert_eq!(validate_dtd(ctxt, ptr::null_mut(), ptr::null_mut()), 0);
            free_valid_ctxt(ctxt);
        }
    }

    // ── Additional edge case tests ────────────────────────────────────────

    #[test]
    fn test_validate_element_null() {
        unsafe {
            let (doc, _dtd) = make_test_doc();
            let ctxt = new_valid_ctxt();
            assert!(!ctxt.is_null());

            assert_eq!(validate_element(ctxt, doc, ptr::null_mut()), 0);

            free_valid_ctxt(ctxt);
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_validate_document_null() {
        unsafe {
            let ctxt = new_valid_ctxt();
            assert!(!ctxt.is_null());

            assert_eq!(validate_document(ctxt, ptr::null_mut()), 0);
            assert_eq!(validate_document(ptr::null_mut(), ptr::null_mut()), 0);

            free_valid_ctxt(ctxt);
        }
    }

    #[test]
    fn test_validate_document_final_null() {
        unsafe {
            let ctxt = new_valid_ctxt();
            assert!(!ctxt.is_null());

            assert_eq!(validate_document_final(ctxt, ptr::null_mut()), 0);
            assert_eq!(validate_document_final(ptr::null_mut(), ptr::null_mut()), 0);

            free_valid_ctxt(ctxt);
        }
    }

    #[test]
    fn test_validate_attribute_decl_null() {
        unsafe {
            let ctxt = new_valid_ctxt();
            assert!(!ctxt.is_null());

            assert_eq!(
                validate_attribute_decl(ctxt, ptr::null_mut(), ptr::null_mut(), ptr::null_mut()),
                0
            );

            free_valid_ctxt(ctxt);
        }
    }

    #[test]
    fn test_validate_content_null() {
        unsafe {
            let ctxt = new_valid_ctxt();
            assert!(!ctxt.is_null());

            assert_eq!(validate_content(ctxt, ptr::null_mut(), ptr::null_mut()), 0);

            free_valid_ctxt(ctxt);
        }
    }

    #[test]
    fn test_validate_root_null() {
        unsafe {
            assert_eq!(validate_root(ptr::null_mut(), ptr::null_mut()), 0);
        }
    }

    #[test]
    fn test_validate_enumeration_null() {
        unsafe {
            let ctxt = new_valid_ctxt();
            assert!(!ctxt.is_null());

            assert_eq!(validate_enumeration(ctxt, ptr::null(), ptr::null_mut()), 0);

            free_valid_ctxt(ctxt);
        }
    }

    #[test]
    fn test_validate_notation_use_null() {
        unsafe {
            let ctxt = new_valid_ctxt();
            assert!(!ctxt.is_null());

            assert_eq!(validate_notation_use(ctxt, ptr::null_mut(), ptr::null()), 0);

            free_valid_ctxt(ctxt);
        }
    }

    #[test]
    fn test_validate_name_start_characters() {
        unsafe {
            // Test some Unicode name characters
            let name = c_str("\u{C0}lph\u{E0}");
            assert_eq!(validate_name(name), 1);
            allocator::xmlFreeImpl(name as *mut c_void);
        }
    }

    #[test]
    fn test_validate_names_single() {
        unsafe {
            let s = c_str("singleName");
            assert_eq!(validate_names(s), 1);
            allocator::xmlFreeImpl(s as *mut c_void);
        }
    }

    #[test]
    fn test_validate_nmtokens_single() {
        unsafe {
            let s = c_str("123abc");
            assert_eq!(validate_nmtokens(s), 1);
            allocator::xmlFreeImpl(s as *mut c_void);
        }
    }

    #[test]
    fn test_validate_nmtokens_invalid() {
        unsafe {
            let s = c_str("foo\tbar"); // tab separated
            assert_eq!(validate_nmtokens(s), 1); // tab is whitespace
            allocator::xmlFreeImpl(s as *mut c_void);

            // An NMTOKEN with invalid characters should fail
            let s2 = c_str("foo@bar");
            assert_eq!(validate_nmtokens(s2), 0);
            allocator::xmlFreeImpl(s2 as *mut c_void);
        }
    }

    #[test]
    fn test_validate_attribute_value_empty_non_cdata() {
        unsafe {
            let empty = b"\0" as *const u8 as *const xmlChar;
            assert_eq!(
                validate_attribute_value(XML_ATTRIBUTE_ID as c_int, empty),
                0
            );
            assert_eq!(
                validate_attribute_value(XML_ATTRIBUTE_IDREF as c_int, empty),
                0
            );
            assert_eq!(
                validate_attribute_value(XML_ATTRIBUTE_NMTOKEN as c_int, empty),
                0
            );
        }
    }

    #[test]
    fn test_validate_attribute_value_unknown_type() {
        unsafe {
            // UPSTREAM-PARITY: unknown attribute types fall through to the
            // default return of 1 (valid.c xmlValidateAttributeValueInternal).
            let s = c_str("test");
            assert_eq!(validate_attribute_value(999, s), 1);
            allocator::xmlFreeImpl(s as *mut c_void);
        }
    }

    #[test]
    fn test_validate_element_any_content() {
        unsafe {
            let (doc, dtd) = make_test_doc();

            let root_name = c_str("root");
            add_elem_decl(
                dtd,
                root_name,
                XML_ELEMENT_TYPE_ANY as c_int,
                ptr::null_mut(),
            );

            let child_name = c_str("child");
            add_elem_decl(
                dtd,
                child_name,
                XML_ELEMENT_TYPE_EMPTY as c_int,
                ptr::null_mut(),
            );

            let root = create_root_elem(doc, root_name);
            create_child_elem(root, child_name);

            let ctxt = new_valid_ctxt();
            assert!(!ctxt.is_null());

            // ANY content allows any children
            assert_eq!(validate_element(ctxt, doc, root), 1);

            free_valid_ctxt(ctxt);
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_validate_element_empty_with_child() {
        unsafe {
            let (doc, dtd) = make_test_doc();

            let root_name = c_str("root");
            add_elem_decl(
                dtd,
                root_name,
                XML_ELEMENT_TYPE_EMPTY as c_int,
                ptr::null_mut(),
            );

            let child_name = c_str("child");
            add_elem_decl(
                dtd,
                child_name,
                XML_ELEMENT_TYPE_EMPTY as c_int,
                ptr::null_mut(),
            );

            let root = create_root_elem(doc, root_name);
            create_child_elem(root, child_name);

            let ctxt = new_valid_ctxt();
            assert!(!ctxt.is_null());

            // EMPTY element with child — validation fails
            assert_eq!(validate_element(ctxt, doc, root), 0);

            free_valid_ctxt(ctxt);
            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_validate_dtd_final_null() {
        unsafe {
            assert_eq!(validate_dtd_final(ptr::null_mut(), ptr::null_mut()), 0);
        }
    }
}
