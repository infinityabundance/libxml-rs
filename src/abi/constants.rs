//! C ABI constants — supplementary ABI constants not defined as enum variants or flag bits in `types.rs` (§14).
//!
//! This module contains additional constants that appear as `#define` macros in upstream headers
//! but are not naturally represented as Rust enum variants. The primary type/enum definitions
//! live in `types.rs` — this module holds the rest.
//!
//! # Phase 1 status
//!
//! Complete — all key upstream `#define` constants are present.
//!
//! # Upstream header mapping
//!
//! | Constant | Header | Purpose |
//! |---|---|---|
//! | `XML_DEFAULT_VERSION` | parser.h | Default XML version string |
//! | `XML_DETECT_IDS` | parser.h | Flag for xmlLoadExtDtdDefaultValue |
//! | `XML_COMPLETE_ATTRS` | parser.h | Flag for xmlLoadExtDtdDefaultValue |
//! | `XML_SKIP_IDS` | parser.h | Flag for xmlLoadExtDtdDefaultValue |
//! | `XML_SUBSTITUTE_*` | parserInternals.h | Entity substitution flags |
//! | `XML_DOCB_DOCUMENT_NODE` | tree.h | DocBook document node type value |
//! | `XML_XML_NAMESPACE` | tree.h | The XML namespace URI |
//! | `XML_XPATH_CHECKNS` | xpath.h | XPath evaluation flag |
//! | `XML_XPATH_NOVAR` | xpath.h | XPath evaluation flag |
//! | `XML_CATALOGS_NAMESPACE` | catalog.h | Catalog namespace URI |
//! | `XML_CATALOG_PI` | catalog.h | Catalog PI target |
//! | `XML_SAX2_MAGIC` | parser.h | SAX2 initialization marker |

#![allow(non_upper_case_globals)]

use std::os::raw::c_int;

// ═══════════════════════════════════════════════════════════════════════════════
// XML Version Constants (parser.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Default XML version string.
/// #define XML_DEFAULT_VERSION "1.0"
pub const XML_DEFAULT_VERSION: &[u8] = b"1.0\0";

// ═══════════════════════════════════════════════════════════════════════════════
// Parser Load Subset Flags (parser.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Flag for xmlLoadExtDtdDefaultValue: detect ID/IDREF attributes.
pub const XML_DETECT_IDS: c_int = 2;

/// Flag for xmlLoadExtDtdDefaultValue: complete attributes from DTD.
pub const XML_COMPLETE_ATTRS: c_int = 4;

/// Flag for xmlLoadExtDtdDefaultValue: skip ID processing.
pub const XML_SKIP_IDS: c_int = 8;

// ═══════════════════════════════════════════════════════════════════════════════
// Entity Substitution Flags (parserInternals.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Do not substitute entities.
pub const XML_SUBSTITUTE_NONE: c_int = 0;

/// Substitute general entities.
pub const XML_SUBSTITUTE_REF: c_int = 1;

/// Substitute parameter entities.
pub const XML_SUBSTITUTE_PEREF: c_int = 2;

/// Substitute both general and parameter entities.
pub const XML_SUBSTITUTE_BOTH: c_int = 3;

// ═══════════════════════════════════════════════════════════════════════════════
// Tree Constants (tree.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// DocBook document node type (value 21).
///
/// # UPSTREAM-PARITY
///
/// DocBook is largely deprecated in modern libxml2 but the constant value
/// is preserved for ABI compatibility.
pub const XML_DOCB_DOCUMENT_NODE: c_int = 21;

/// The XML namespace URI.
///
/// ```c
/// #define XML_XML_NAMESPACE \
///     (const xmlChar *) "http://www.w3.org/XML/1998/namespace"
/// ```
///
/// # UPSTREAM-PARITY
///
/// This is the standard `xml` namespace URI, hard-coded in upstream.
pub const XML_XML_NAMESPACE: &[u8] = b"http://www.w3.org/XML/1998/namespace\0";

// ═══════════════════════════════════════════════════════════════════════════════
// XPath Constants (xpath.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Check namespaces during XPath evaluation.
pub const XML_XPATH_CHECKNS: c_int = 1 << 0;

/// Do not use default variable resolution.
pub const XML_XPATH_NOVAR: c_int = 1 << 1;

// ═══════════════════════════════════════════════════════════════════════════════
// Catalog Constants (catalog.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// The XML Catalogs namespace URI.
///
/// ```c
/// #define XML_CATALOGS_NAMESPACE \
///     (const xmlChar *) "urn:oasis:names:tc:entity:xmlns:xml:catalog"
/// ```
pub const XML_CATALOGS_NAMESPACE: &[u8] = b"urn:oasis:names:tc:entity:xmlns:xml:catalog\0";

/// The XML Catalogs PI target string.
///
/// ```c
/// #define XML_CATALOG_PI \
///     (const xmlChar *) "oasis-xml-catalog"
/// ```
pub const XML_CATALOG_PI: &[u8] = b"oasis-xml-catalog\0";

// ═══════════════════════════════════════════════════════════════════════════════
// SAX2 Magic Marker (parser.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Magic value for SAX2 initialization in xmlSAXHandler.initialized.
///
/// ```c
/// #define XML_SAX2_MAGIC 0xDEEDBEAF
/// ```
///
/// # UPSTREAM-PARITY
///
/// When `_xmlSAXHandler.initialized` is set to `XML_SAX2_MAGIC`,
/// the handler uses SAX2 callbacks (startElementNs/endElementNs)
/// instead of SAX1 callbacks.
pub const XML_SAX2_MAGIC: c_int = 0xDEEDBEAFu32 as i32;

// ═══════════════════════════════════════════════════════════════════════════════
// Well-known Namespace URIs (tree.h, namespaces.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// The xmlns namespace URI.
///
/// ```c
/// #define XML_XMLNS_NAMESPACE \
///     (const xmlChar *) "http://www.w3.org/2000/xmlns/"
/// ```
///
/// # UPSTREAM-PARITY
///
/// This is the namespace URI for `xmlns` declarations.
pub const XML_XMLNS_NAMESPACE: &[u8] = b"http://www.w3.org/2000/xmlns/\0";

/// The xmlns prefix.
///
/// ```c
/// #define XML_XMLNS_PREFIX (const xmlChar *) "xmlns"
/// ```
pub const XML_XMLNS_PREFIX: &[u8] = b"xmlns\0";

/// The xml prefix.
///
/// ```c
/// #define XML_XML_PREFIX (const xmlChar *) "xml"
/// ```
pub const XML_XML_PREFIX: &[u8] = b"xml\0";

// ═══════════════════════════════════════════════════════════════════════════════
// Encoding Constants (encoding.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Maximum encoding name length.
pub const XML_MAX_ENCODING_NAME_LEN: c_int = 50;

/// Maximum conversion buffer length.
pub const XML_MAX_CONV_BUF_LEN: c_int = 4096;

// ═══════════════════════════════════════════════════════════════════════════════
// HTML Parser Constants (HTMLparser.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// HTML parser status: no problem.
pub const HTML_PARSE_NO_FLAGS: c_int = 0;

/// HTML parser option: suppress error reports.
pub const HTML_PARSE_NOERROR: c_int = 1 << 5;

/// HTML parser option: suppress warning reports.
pub const HTML_PARSE_NOWARNING: c_int = 1 << 6;

/// HTML parser option: pedantic error reporting.
pub const HTML_PARSE_PEDANTIC: c_int = 1 << 7;

/// HTML parser option: remove blank nodes.
pub const HTML_PARSE_NOBLANKS: c_int = 1 << 8;

/// HTML parser option: do not load external entities.
pub const HTML_PARSE_NONET: c_int = 1 << 11;

/// HTML parser option: do not generate XInclude nodes.
pub const HTML_PARSE_NOXINCNODE: c_int = 1 << 15;

/// HTML parser option: compact text nodes.
pub const HTML_PARSE_COMPACT: c_int = 1 << 16;

/// HTML parser option: ignore encoding declaration.
pub const HTML_PARSE_IGNORE_ENC: c_int = 1 << 21;

// ═══════════════════════════════════════════════════════════════════════════════
// Writer Constants (xmlwriter.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Writer type: no writer.
pub const XML_TEXTWRITER_NONE: c_int = 0;

/// Writer type: XML writer.
pub const XML_TEXTWRITER_NAME: c_int = 1;

/// Writer type: DOM writer (tree-based).
pub const XML_TEXTWRITER_ATTRIBUTE_NAME: c_int = 2;

/// Writer type: text content.
pub const XML_TEXTWRITER_TEXT: c_int = 3;

/// Writer type: CDATA content.
pub const XML_TEXTWRITER_CDATA: c_int = 4;

/// Writer type: entity reference.
pub const XML_TEXTWRITER_ENTITY_REF: c_int = 5;

/// Writer type: PI node.
pub const XML_TEXTWRITER_PI: c_int = 6;

/// Writer type: comment node.
pub const XML_TEXTWRITER_COMMENT: c_int = 7;

/// Writer type: DTD node.
pub const XML_TEXTWRITER_DTD: c_int = 8;

/// Writer type: DTD element.
pub const XML_TEXTWRITER_DTD_ELEM: c_int = 9;

/// Writer type: DTD attribute.
pub const XML_TEXTWRITER_DTD_ATTL: c_int = 10;

/// Writer type: DTD entity.
pub const XML_TEXTWRITER_DTD_ENTY: c_int = 11;

/// Writer type: DTD notation.
pub const XML_TEXTWRITER_DTD_NOTA: c_int = 12;

/// Writer type: namespace declaration.
pub const XML_TEXTWRITER_NAMESPACE: c_int = 13;

/// Writer type: element with no content.
pub const XML_TEXTWRITER_NO_CONTENT: c_int = 14;

/// Writer type: attribute value (content only).
pub const XML_TEXTWRITER_ATTRIBUTE_VALUE: c_int = 15;

/// Writer type: XML declaration.
pub const XML_TEXTWRITER_XML_DECLARATION: c_int = 16;
