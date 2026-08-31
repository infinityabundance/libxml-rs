//! C ABI type definitions — matching upstream libxml2/libxslt (§14).
//!
//! This module defines all public C-compatible types with exact layout,
//! size, and alignment matching the upstream headers.
//!
//! # Layout verification
//!
//! Every struct in this module must be verified against the oracle using
//! `offsetof` and `sizeof` probes compiled from the upstream headers.
//! See courts/abi-struct-*.json for verification receipts.
//!
//! # Phase 1 status
//!
//! All core types are defined. Verification against oracle is pending
//! Docker oracle build.
//!
//! # Source provenance
//!
//! All types derived from SRC-LIBXML2-2.15.3-TREE-H and related headers.
//! See atlas/SOURCES.md and atlas/api/ for the complete declaration inventory.

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_ulong, c_void};

// ── Fundamental libxml2 types ───────────────────────────────────────────

/// Basic type for all xmlChars (UTF-8 encoded bytes).
pub type xmlChar = u8;

/// Pointer to xmlChar.
pub type xmlCharPtr = *mut xmlChar;

/// Const pointer to xmlChar.
pub type xmlConstCharPtr = *const xmlChar;

// ── Character encoding (xmlCharEncoding) ──────────────────────────────
//
// Source: encoding.h lines 18-60

/// Character encoding identifiers.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum xmlCharEncoding {
    XML_CHAR_ENCODING_ERROR = -1,
    XML_CHAR_ENCODING_NONE = 0,
    XML_CHAR_ENCODING_UTF8 = 1,
    XML_CHAR_ENCODING_UTF16LE = 2,
    XML_CHAR_ENCODING_UTF16BE = 3,
    XML_CHAR_ENCODING_UCS4LE = 4,
    XML_CHAR_ENCODING_UCS4BE = 5,
    XML_CHAR_ENCODING_EBCDIC = 6,
    XML_CHAR_ENCODING_UCS4_2143 = 7,
    XML_CHAR_ENCODING_UCS4_3412 = 8,
    XML_CHAR_ENCODING_UCS2 = 9,
    XML_CHAR_ENCODING_8859_1 = 10,
    XML_CHAR_ENCODING_8859_2 = 11,
    XML_CHAR_ENCODING_8859_3 = 12,
    XML_CHAR_ENCODING_8859_4 = 13,
    XML_CHAR_ENCODING_8859_5 = 14,
    XML_CHAR_ENCODING_8859_6 = 15,
    XML_CHAR_ENCODING_8859_7 = 16,
    XML_CHAR_ENCODING_8859_8 = 17,
    XML_CHAR_ENCODING_8859_9 = 18,
    XML_CHAR_ENCODING_2022_JP = 19,
    XML_CHAR_ENCODING_SHIFT_JIS = 20,
    XML_CHAR_ENCODING_EUC_JP = 21,
    XML_CHAR_ENCODING_ASCII = 22,
}

// ── Node types (xmlElementType) ─────────────────────────────────────────
//
// Source: tree.h lines 162-184
// Provenance: SRC-LIBXML2-2.15.3-TREE-H

/// Type of an XML element or node.
///
/// These values are part of the ABI and MUST NOT be changed.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum xmlElementType {
    XML_ELEMENT_NODE = 1,
    XML_ATTRIBUTE_NODE = 2,
    XML_TEXT_NODE = 3,
    XML_CDATA_SECTION_NODE = 4,
    XML_ENTITY_REF_NODE = 5,
    XML_ENTITY_NODE = 6,
    XML_PI_NODE = 7,
    XML_COMMENT_NODE = 8,
    XML_DOCUMENT_NODE = 9,
    XML_DOCUMENT_TYPE_NODE = 10,
    XML_DOCUMENT_FRAG_NODE = 11,
    XML_NOTATION_NODE = 12,
    XML_HTML_DOCUMENT_NODE = 13,
    XML_DTD_NODE = 14,
    XML_ELEMENT_DECL = 15,
    XML_ATTRIBUTE_DECL = 16,
    XML_ENTITY_DECL = 17,
    XML_NAMESPACE_DECL = 18,
    XML_XINCLUDE_START = 19,
    XML_XINCLUDE_END = 20,
}

/// Namespace type is typedef'd to xmlElementType in upstream.
pub type xmlNsType = xmlElementType;

/// XML_LOCAL_NAMESPACE macro equals XML_NAMESPACE_DECL.
pub const XML_LOCAL_NAMESPACE: xmlElementType = xmlElementType::XML_NAMESPACE_DECL;

// ── Document properties ─────────────────────────────────────────────────
//
// Source: tree.h lines 762-770

/// Document properties flags.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum xmlDocProperties {
    XML_DOC_WELLFORMED = 1 << 0,
    XML_DOC_NSVALID = 1 << 1,
    XML_DOC_OLD10 = 1 << 2,
    XML_DOC_DTDVALID = 1 << 3,
    XML_DOC_XINCLUDE = 1 << 4,
    XML_DOC_USERBUILT = 1 << 5,
    XML_DOC_INTERNAL = 1 << 6,
    XML_DOC_HTML = 1 << 7,
}

// ── Buffer allocation scheme ────────────────────────────────────────────
//
// Source: tree.h lines 93-100

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum xmlBufferAllocationScheme {
    XML_BUFFER_ALLOC_DOUBLEIT = 0,
    XML_BUFFER_ALLOC_EXACT = 1,
    XML_BUFFER_ALLOC_IMMUTABLE = 2,
    XML_BUFFER_ALLOC_IO = 3,
    XML_BUFFER_ALLOC_HYBRID = 4,
    XML_BUFFER_ALLOC_BOUNDED = 5,
}

// ── Attribute types ─────────────────────────────────────────────────────
//
// Source: tree.h lines 309-319

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum xmlAttributeType {
    XML_ATTRIBUTE_CDATA = 1,
    XML_ATTRIBUTE_ID = 2,
    XML_ATTRIBUTE_IDREF = 3,
    XML_ATTRIBUTE_IDREFS = 4,
    XML_ATTRIBUTE_ENTITY = 5,
    XML_ATTRIBUTE_ENTITIES = 6,
    XML_ATTRIBUTE_NMTOKEN = 7,
    XML_ATTRIBUTE_NMTOKENS = 8,
    XML_ATTRIBUTE_ENUMERATION = 9,
    XML_ATTRIBUTE_NOTATION = 10,
}

// ── Attribute default modes ─────────────────────────────────────────────
//
// Source: tree.h lines 325-330

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum xmlAttributeDefault {
    XML_ATTRIBUTE_NONE = 1,
    XML_ATTRIBUTE_REQUIRED = 2,
    XML_ATTRIBUTE_IMPLIED = 3,
    XML_ATTRIBUTE_FIXED = 4,
}

// ── Entity types ────────────────────────────────────────────────────────
//
// Source: entities.h lines 28-35

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum xmlEntityType {
    XML_INTERNAL_GENERAL_ENTITY = 1,
    XML_EXTERNAL_GENERAL_PARSED_ENTITY = 2,
    XML_EXTERNAL_GENERAL_UNPARSED_ENTITY = 3,
    XML_INTERNAL_PARAMETER_ENTITY = 4,
    XML_EXTERNAL_PARAMETER_ENTITY = 5,
    XML_INTERNAL_PREDEFINED_ENTITY = 6,
}

// ── Element content types ───────────────────────────────────────────────
//
// Source: tree.h lines 396-401

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum xmlElementContentType {
    XML_ELEMENT_CONTENT_PCDATA = 1,
    XML_ELEMENT_CONTENT_ELEMENT = 2,
    XML_ELEMENT_CONTENT_SEQ = 3,
    XML_ELEMENT_CONTENT_OR = 4,
}

// ── Element content occurrence ──────────────────────────────────────────
//
// Source: tree.h lines 406-410

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum xmlElementContentOccur {
    XML_ELEMENT_CONTENT_ONCE = 1,
    XML_ELEMENT_CONTENT_OPT = 2,
    XML_ELEMENT_CONTENT_MULT = 3,
    XML_ELEMENT_CONTENT_PLUS = 4,
}

// ── Element type values ─────────────────────────────────────────────────
//
// Source: tree.h lines 443-450

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum xmlElementTypeVal {
    XML_ELEMENT_TYPE_UNDEFINED = 0,
    XML_ELEMENT_TYPE_EMPTY = 1,
    XML_ELEMENT_TYPE_ANY = 2,
    XML_ELEMENT_TYPE_MIXED = 3,
    XML_ELEMENT_TYPE_ELEMENT = 4,
}

// ── Error levels ────────────────────────────────────────────────────────
//
// Source: xmlerror.h

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum xmlErrorLevel {
    XML_ERR_NONE = 0,
    XML_ERR_WARNING = 1,
    XML_ERR_ERROR = 2,
    XML_ERR_FATAL = 3,
}

// ── Parser modes ────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum xmlParserMode {
    XML_PARSE_UNKNOWN = 0,
    XML_PARSE_DOM = 1,
    XML_PARSE_SAX = 2,
    XML_PARSE_PUSH_DOM = 3,
    XML_PARSE_PUSH_SAX = 4,
    XML_PARSE_READER = 5,
}

// ── Parser input state ──────────────────────────────────────────────────

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum xmlParserInputState {
    XML_PARSER_EOF = -1,
    XML_PARSER_START = 0,
    XML_PARSER_MISC = 1,
    XML_PARSER_DTD = 2,
    XML_PARSER_PROLOG = 3,
    XML_PARSER_CONTENT = 4,
    XML_PARSER_CDATA_SECTION = 5,
    XML_PARSER_ENTITY_REF = 6,
    XML_PARSER_ENTITY_VALUE = 7,
    XML_PARSER_ATTRIBUTE_VALUE = 8,
    XML_PARSER_SYSTEM_LITERAL = 9,
    XML_PARSER_EPILOG = 10,
    XML_PARSER_IGNORE = 11,
    XML_PARSER_PUBLIC_LITERAL = 12,
}

// ── XPath object types ──────────────────────────────────────────────────

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum xmlXPathObjectType {
    XPATH_UNDEFINED = 0,
    XPATH_NODESET = 1,
    XPATH_BOOLEAN = 2,
    XPATH_NUMBER = 3,
    XPATH_STRING = 4,
    XPATH_POINT = 5,
    XPATH_RANGE = 6,
    XPATH_LOCATIONSET = 7,
    XPATH_USERS = 8,
    XPATH_XSLT_TREE = 9,
}

// ── Parser option flags ─────────────────────────────────────────────────
//
// Source: parser.h

pub const XML_PARSE_RECOVER: c_int = 1 << 0;
pub const XML_PARSE_NOENT: c_int = 1 << 1;
pub const XML_PARSE_DTDLOAD: c_int = 1 << 2;
pub const XML_PARSE_DTDATTR: c_int = 1 << 3;
pub const XML_PARSE_DTDVALID: c_int = 1 << 4;
pub const XML_PARSE_NOERROR: c_int = 1 << 5;
pub const XML_PARSE_NOWARNING: c_int = 1 << 6;
pub const XML_PARSE_PEDANTIC: c_int = 1 << 7;
pub const XML_PARSE_NOBLANKS: c_int = 1 << 8;
pub const XML_PARSE_SAX1: c_int = 1 << 9;
pub const XML_PARSE_XINCLUDE: c_int = 1 << 10;
pub const XML_PARSE_NONET: c_int = 1 << 11;
pub const XML_PARSE_NODICT: c_int = 1 << 12;
pub const XML_PARSE_NSCLEAN: c_int = 1 << 13;
pub const XML_PARSE_NOCDATA: c_int = 1 << 14;
pub const XML_PARSE_NOXINCNODE: c_int = 1 << 15;
pub const XML_PARSE_COMPACT: c_int = 1 << 16;
pub const XML_PARSE_OLD10: c_int = 1 << 17;
pub const XML_PARSE_NOBASEFIX: c_int = 1 << 18;
pub const XML_PARSE_HUGE: c_int = 1 << 19;
pub const XML_PARSE_OLDSAX: c_int = 1 << 20;
pub const XML_PARSE_IGNORE_ENC: c_int = 1 << 21;
pub const XML_PARSE_BIG_LINES: c_int = 1 << 22;
pub const XML_PARSE_NO_XXE: c_int = 1 << 23;
pub const XML_PARSE_UNZIP: c_int = 1 << 24;
pub const XML_PARSE_NO_SYS_CATALOG: c_int = 1 << 25;
pub const XML_PARSE_CATALOG_PI: c_int = 1 << 26;
pub const XML_PARSE_SKIP_IDS: c_int = 1 << 27;

// ── Error domains ───────────────────────────────────────────────────────
//
// Source: xmlerror.h

pub const XML_FROM_NONE: c_int = 0;
pub const XML_FROM_PARSER: c_int = 1;
pub const XML_FROM_TREE: c_int = 2;
pub const XML_FROM_NAMESPACE: c_int = 3;
pub const XML_FROM_DTD: c_int = 4;
pub const XML_FROM_HTML: c_int = 5;
pub const XML_FROM_MEMORY: c_int = 6;
pub const XML_FROM_OUTPUT: c_int = 7;
pub const XML_FROM_IO: c_int = 8;
pub const XML_FROM_FTP: c_int = 9;
pub const XML_FROM_HTTP: c_int = 10;
pub const XML_FROM_XINCLUDE: c_int = 11;
pub const XML_FROM_XPATH: c_int = 12;
pub const XML_FROM_XPOINTER: c_int = 13;
pub const XML_FROM_REGEXP: c_int = 14;
pub const XML_FROM_DATATYPE: c_int = 15;
pub const XML_FROM_SCHEMASP: c_int = 16;
pub const XML_FROM_SCHEMASV: c_int = 17;
pub const XML_FROM_RELAXNGP: c_int = 18;
pub const XML_FROM_RELAXNGV: c_int = 19;
pub const XML_FROM_CATALOG: c_int = 20;
pub const XML_FROM_C14N: c_int = 21;
pub const XML_FROM_XSLT: c_int = 22;
pub const XML_FROM_VALID: c_int = 23;
pub const XML_FROM_CHECK: c_int = 24;
pub const XML_FROM_WRITER: c_int = 25;
pub const XML_FROM_MODULE: c_int = 26;
pub const XML_FROM_I18N: c_int = 27;
pub const XML_FROM_SCHEMATRONV: c_int = 28;
pub const XML_FROM_BUFFER: c_int = 29;
pub const XML_FROM_URI: c_int = 30;

// ── Parser error codes ──────────────────────────────────────────────────
//
// Source: xmlerror.h (xmlParserErrors enum)

pub const XML_ERR_OK: c_int = 0;
pub const XML_ERR_INTERNAL_ERROR: c_int = 1;
pub const XML_ERR_NO_MEMORY: c_int = 2;
pub const XML_ERR_DOCUMENT_START: c_int = 3;
pub const XML_ERR_DOCUMENT_EMPTY: c_int = 4;
pub const XML_ERR_DOCUMENT_END: c_int = 5;
pub const XML_ERR_INVALID_HEX_CHARREF: c_int = 6;
pub const XML_ERR_INVALID_DEC_CHARREF: c_int = 7;
pub const XML_ERR_INVALID_CHARREF: c_int = 8;
pub const XML_ERR_INVALID_CHAR: c_int = 9;
pub const XML_ERR_CHARREF_AT_EOF: c_int = 10;
pub const XML_ERR_CHARREF_IN_PROLOG: c_int = 11;
pub const XML_ERR_CHARREF_IN_EPILOG: c_int = 12;
pub const XML_ERR_CHARREF_IN_DTD: c_int = 13;
pub const XML_ERR_ENTITYREF_AT_EOF: c_int = 14;
pub const XML_ERR_ENTITYREF_IN_PROLOG: c_int = 15;
pub const XML_ERR_ENTITYREF_IN_EPILOG: c_int = 16;
pub const XML_ERR_ENTITYREF_IN_DTD: c_int = 17;
pub const XML_ERR_PEREF_AT_EOF: c_int = 18;
pub const XML_ERR_PEREF_IN_PROLOG: c_int = 19;
pub const XML_ERR_PEREF_IN_EPILOG: c_int = 20;
pub const XML_ERR_PEREF_IN_INT_SUBSET: c_int = 21;
pub const XML_ERR_ENTITYREF_NO_NAME: c_int = 22;
pub const XML_ERR_ENTITYREF_SEMICOL_MISSING: c_int = 23;
pub const XML_ERR_PEREF_NO_NAME: c_int = 24;
pub const XML_ERR_PEREF_SEMICOL_MISSING: c_int = 25;
pub const XML_ERR_UNDECLARED_ENTITY: c_int = 26;
pub const XML_WAR_UNDECLARED_ENTITY: c_int = 27;
pub const XML_ERR_UNPARSED_ENTITY: c_int = 28;
pub const XML_ERR_ENTITY_IS_EXTERNAL: c_int = 29;
pub const XML_ERR_ENTITY_IS_PARAMETER: c_int = 30;
pub const XML_ERR_UNKNOWN_ENCODING: c_int = 31;
pub const XML_ERR_UNSUPPORTED_ENCODING: c_int = 32;
pub const XML_ERR_STRING_NOT_STARTED: c_int = 33;
pub const XML_ERR_STRING_NOT_CLOSED: c_int = 34;
pub const XML_ERR_NS_DECL_ERROR: c_int = 35;
pub const XML_ERR_ENTITY_NOT_STARTED: c_int = 36;
pub const XML_ERR_ENTITY_NOT_FINISHED: c_int = 37;
pub const XML_ERR_LT_IN_ATTRIBUTE: c_int = 38;
pub const XML_ERR_ATTRIBUTE_NOT_STARTED: c_int = 39;
pub const XML_ERR_ATTRIBUTE_NOT_FINISHED: c_int = 40;
pub const XML_ERR_ATTRIBUTE_WITHOUT_VALUE: c_int = 41;
pub const XML_ERR_ATTRIBUTE_REDEFINED: c_int = 42;
pub const XML_ERR_LITERAL_NOT_STARTED: c_int = 43;
pub const XML_ERR_LITERAL_NOT_FINISHED: c_int = 44;
pub const XML_ERR_COMMENT_NOT_FINISHED: c_int = 45;
pub const XML_ERR_PI_NOT_STARTED: c_int = 46;
pub const XML_ERR_PI_NOT_FINISHED: c_int = 47;
pub const XML_ERR_NOTATION_NOT_STARTED: c_int = 48;
pub const XML_ERR_NOTATION_NOT_FINISHED: c_int = 49;
pub const XML_ERR_ATTLIST_NOT_STARTED: c_int = 50;
pub const XML_ERR_ATTLIST_NOT_FINISHED: c_int = 51;
pub const XML_ERR_MIXED_NOT_STARTED: c_int = 52;
pub const XML_ERR_MIXED_NOT_FINISHED: c_int = 53;
pub const XML_ERR_ELEMCONTENT_NOT_STARTED: c_int = 54;
pub const XML_ERR_ELEMCONTENT_NOT_FINISHED: c_int = 55;
pub const XML_ERR_XMLDECL_NOT_STARTED: c_int = 56;
pub const XML_ERR_XMLDECL_NOT_FINISHED: c_int = 57;
pub const XML_ERR_CONDSEC_NOT_STARTED: c_int = 58;
pub const XML_ERR_CONDSEC_NOT_FINISHED: c_int = 59;
pub const XML_ERR_EXT_SUBSET_NOT_FINISHED: c_int = 60;
pub const XML_ERR_DOCTYPE_NOT_FINISHED: c_int = 61;
pub const XML_ERR_MISPLACED_CDATA_END: c_int = 62;
pub const XML_ERR_CDATA_NOT_FINISHED: c_int = 63;
pub const XML_ERR_RESERVED_XML_NAME: c_int = 64;
pub const XML_ERR_SPACE_REQUIRED: c_int = 65;
pub const XML_ERR_SEPARATOR_REQUIRED: c_int = 66;
pub const XML_ERR_NMTOKEN_REQUIRED: c_int = 67;
pub const XML_ERR_NAME_REQUIRED: c_int = 68;
pub const XML_ERR_PCDATA_REQUIRED: c_int = 69;
pub const XML_ERR_URI_REQUIRED: c_int = 70;
pub const XML_ERR_PUBID_REQUIRED: c_int = 71;
pub const XML_ERR_LT_REQUIRED: c_int = 72;
pub const XML_ERR_GT_REQUIRED: c_int = 73;
pub const XML_ERR_LTSLASH_REQUIRED: c_int = 74;
pub const XML_ERR_EQUAL_REQUIRED: c_int = 75;
pub const XML_ERR_TAG_NAME_MISMATCH: c_int = 76;
pub const XML_ERR_TAG_NOT_FINISHED: c_int = 77;
pub const XML_ERR_STANDALONE_VALUE: c_int = 78;
pub const XML_ERR_ENCODING_NAME: c_int = 79;
pub const XML_ERR_HYPHEN_IN_COMMENT: c_int = 80;
pub const XML_ERR_INVALID_ENCODING: c_int = 81;
pub const XML_ERR_EXT_ENTITY_STANDALONE: c_int = 82;
pub const XML_ERR_CONDSEC_INVALID: c_int = 83;
pub const XML_ERR_VALUE_REQUIRED: c_int = 84;
pub const XML_ERR_NOT_WELL_BALANCED: c_int = 85;
pub const XML_ERR_EXTRA_CONTENT: c_int = 86;
pub const XML_ERR_ENTITY_CHAR_ERROR: c_int = 87;
pub const XML_ERR_ENTITY_PE_INTERNAL: c_int = 88;
pub const XML_ERR_ENTITY_LOOP: c_int = 89;
pub const XML_ERR_ENTITY_BOUNDARY: c_int = 90;
pub const XML_ERR_INVALID_URI: c_int = 91;
pub const XML_ERR_URI_FRAGMENT: c_int = 92;
pub const XML_WAR_CATALOG_PI: c_int = 93;
pub const XML_ERR_NO_DTD: c_int = 94;
pub const XML_ERR_RESOURCE_LIMIT: c_int = 114;
pub const XML_ERR_CONDSEC_INVALID_KEYWORD: c_int = 95;
pub const XML_ERR_VERSION_MISSING: c_int = 96;
pub const XML_ERR_ARGUMENT: c_int = 115;
pub const XML_IO_ENOENT: c_int = 1524;
// Removed: duplicate of XML_ERR_NAME_TOO_LONG at line 414

// ── Parser limits ──────────────────────────────────────────────────────
//
// Source: parserInternals.h

pub const XML_MAX_TEXT_LENGTH: c_int = 10_000_000;
pub const XML_MAX_NAME_LENGTH: c_int = 50_000;
pub const XML_MAX_DICTIONARY_LIMIT: c_int = 1_000_000;
pub const XML_MAX_LOOKUP_LIMIT: c_int = 1_000_000;
pub const XML_MAX_HUGE_LENGTH: c_int = 100_000_000;

// ── XPath parser-context error codes (upstream xmlXPathError) ──────────
//
// Source: xpath.h (typedef enum { ... } xmlXPathError). The numeric values
// are part of the observable ABI (ctxt->error after evaluation).

pub const XPATH_EXPRESSION_OK: c_int = 0;
pub const XPATH_NUMBER_ERROR: c_int = 1;
pub const XPATH_UNFINISHED_LITERAL_ERROR: c_int = 2;
pub const XPATH_START_LITERAL_ERROR: c_int = 3;
pub const XPATH_VARIABLE_REF_ERROR: c_int = 4;
pub const XPATH_UNDEF_VARIABLE_ERROR: c_int = 5;
pub const XPATH_INVALID_PREDICATE_ERROR: c_int = 6;
pub const XPATH_EXPR_ERROR: c_int = 7;
pub const XPATH_UNCLOSED_ERROR: c_int = 8;
pub const XPATH_UNKNOWN_FUNC_ERROR: c_int = 9;
pub const XPATH_INVALID_OPERAND: c_int = 10;
pub const XPATH_INVALID_TYPE: c_int = 11;
pub const XPATH_INVALID_ARITY: c_int = 12;
pub const XPATH_INVALID_CTXT_SIZE: c_int = 13;
pub const XPATH_INVALID_CTXT_POSITION: c_int = 14;
pub const XPATH_MEMORY_ERROR: c_int = 15;
pub const XPTR_SYNTAX_ERROR: c_int = 16;
pub const XPTR_RESOURCE_ERROR: c_int = 17;
pub const XPTR_SUB_RESOURCE_ERROR: c_int = 18;
pub const XPATH_UNDEF_PREFIX_ERROR: c_int = 19;
pub const XPATH_ENCODING_ERROR: c_int = 20;
pub const XPATH_INVALID_CHAR_ERROR: c_int = 21;
pub const XPATH_INVALID_CTXT: c_int = 22;
pub const XPATH_STACK_ERROR: c_int = 23;
pub const XPATH_FORBID_VARIABLE_ERROR: c_int = 24;
pub const XPATH_OP_LIMIT_EXCEEDED: c_int = 25;
pub const XPATH_RECURSION_LIMIT_EXCEEDED: c_int = 26;
pub const XML_MAX_NAMELEN: c_int = 100;
pub const XML_MAX_ATTRIBUTE_LENGTH: c_int = 500_000;

// ── Version constants ──────────────────────────────────────────────────

pub const LIBXML2_VERSION: &str = "2.15.3";
pub const LIBXML2_VERSION_MAJOR: c_int = 2;
pub const LIBXML2_VERSION_MINOR: c_int = 15;
pub const LIBXML2_VERSION_MICRO: c_int = 3;
pub const LIBXML2_VERSION_NUMBER: c_int = 21503;
pub const LIBXML2_VERSION_EXTRA: &str = "";

pub const LIBXSLT_VERSION: &str = "1.1.45";
pub const LIBXSLT_VERSION_MAJOR: c_int = 1;
pub const LIBXSLT_VERSION_MINOR: c_int = 1;
pub const LIBXSLT_VERSION_MICRO: c_int = 45;
pub const LIBXSLT_VERSION_NUMBER: c_int = 10145;
pub const LIBXSLT_VERSION_EXTRA: &str = "";

// ── SAX2 magic value ───────────────────────────────────────────────────

pub const XML_SAX2_MAGIC: c_int = 0xDEEDBEAFu32 as i32;
