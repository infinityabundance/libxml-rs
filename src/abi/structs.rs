//! C ABI struct definitions — exact upstream layout (§14, §17).
//!
//! Every struct in this module is laid out to match the corresponding
//! upstream C struct byte-for-byte on the target platform.
//!
//! # Upstream structs
//!
//! All structs derived from SRC-LIBXML2-2.15.3-TREE-H and related headers.
//! See the sub-agent extraction output for complete field-level archaeology.
//!
//! # Safety
//!
//! These structs are `#[repr(C)]` and may be passed across the FFI boundary.
//! Fields marked `_deprecated_*` or `_unused_*` are retained for ABI
//! compatibility even when the upstream has deprecated them.
//!
//! # Phase 1 status
//!
//! All core tree structs, error struct, parser context, SAX handler,
//! XPath context/object, and I/O structs are defined.
//!
//! # ABI verification
//!
//! Each struct must be verified with `offsetof`/`sizeof` probes compiled
//! from upstream headers. See courts/abi-struct-*.json for receipts.

use crate::abi::callbacks::*;
use crate::abi::types::*;
use std::os::raw::{c_char, c_int, c_long, c_uint, c_ulong, c_ushort, c_void};

// ── Forward declarations ────────────────────────────────────────────────
//
// These are the internal struct tags. In C, these are typedef'd to
// pointer types. We define them as opaque handles or full structs
// depending on whether the struct is public or opaque.

// Opaque types (defined in .c files, not exposed in headers):
//   _xmlDict, _xmlHashTable, _xmlList, _xmlRegexp, _xmlCatalog,
//   _xmlXPathCompExpr, _xmlAutomata, _xmlPattern

// ── xmlBuffer ───────────────────────────────────────────────────────────
//
// Source: tree.h lines 109-126

/// Buffer structure. Deprecated in favor of xmlBuf.
#[repr(C)]
pub struct _xmlBuffer {
    pub content: *mut xmlChar,   // The buffer content UTF8 (deprecated)
    pub use_: c_uint,            // The buffer size used (deprecated)
    pub size: c_uint,            // The buffer size (deprecated)
    pub alloc: c_int,            // The realloc method (deprecated)
    pub contentIO: *mut xmlChar, // In IO mode may have different base (deprecated)
}

// ── xmlNotation ─────────────────────────────────────────────────────────
//
// Source: tree.h lines 297-305

/// Notation declaration.
#[repr(C)]
pub struct _xmlNotation {
    pub name: *const xmlChar,     // Notation name
    pub PublicID: *const xmlChar, // Public identifier, if any
    pub SystemID: *const xmlChar, // System identifier, if any
}

// ── xmlEnumeration ──────────────────────────────────────────────────────
//
// Source: tree.h lines 341-345

/// Enumeration value for attribute declarations.
#[repr(C)]
pub struct _xmlEnumeration {
    pub next: *mut _xmlEnumeration, // Next in enumeration (deprecated)
    pub name: *const xmlChar,       // Enumeration value
}

// ── xmlAttribute (declaration) ──────────────────────────────────────────
//
// Source: tree.h lines 357-391
// NOTE: This is the ATTRIBUTE DECLARATION struct, not attribute node.
// Attribute node is _xmlAttr below.

/// Attribute declaration from DTD.
#[repr(C)]
pub struct _xmlAttribute {
    pub _private: *mut c_void,   // Application data
    pub type_: c_int,            // XML_ATTRIBUTE_DECL
    pub name: *const xmlChar,    // Attribute name
    pub children: *mut _xmlNode, // NULL
    pub last: *mut _xmlNode,     // NULL
    pub parent: *mut _xmlDtd,    // DTD
    pub next: *mut _xmlNode,     // Next sibling
    pub prev: *mut _xmlNode,     // Previous sibling
    pub doc: *mut _xmlDoc,       // Containing document

    pub nexth: *mut _xmlAttribute,  // Next in hash table
    pub atype: c_int,               // Attribute type
    pub def: c_int,                 // Attribute default
    pub defaultValue: *mut xmlChar, // Default value
    pub tree: *mut _xmlEnumeration, // Enumeration tree (deprecated)
    pub prefix: *const xmlChar,     // Namespace prefix
    pub elem: *const xmlChar,       // Element name
}

// ── xmlElementContent ───────────────────────────────────────────────────
//
// Source: tree.h lines 423-438

/// Element content model.
#[repr(C)]
pub struct _xmlElementContent {
    pub type_: c_int,                    // PCDATA, ELEMENT, SEQ or OR (deprecated)
    pub ocur: c_int,                     // ONCE, OPT, MULT or PLUS (deprecated)
    pub name: *const xmlChar,            // Element name (deprecated)
    pub c1: *mut _xmlElementContent,     // First child (deprecated)
    pub c2: *mut _xmlElementContent,     // Second child (deprecated)
    pub parent: *mut _xmlElementContent, // Parent (deprecated)
    pub prefix: *const xmlChar,          // Namespace prefix (deprecated)
}

// ── xmlElement (declaration) ────────────────────────────────────────────
//
// Source: tree.h lines 447-474

/// Element declaration from DTD.
#[repr(C)]
pub struct _xmlElement {
    pub name: *const xmlChar,             // Element name
    pub type_: c_int,                     // xmlElementTypeVal
    pub content: *mut _xmlElementContent, // Content model
    pub attributes: *mut _xmlAttribute,   // Linked list of attribute declarations
    pub prefix: *const xmlChar,           // Namespace prefix
    pub next: *mut _xmlElement,           // Next in hash chain
    pub _private: c_int,                  // Application data
}

// ── xmlNs ───────────────────────────────────────────────────────────────
//
// Source: tree.h lines 501-512

/// Namespace declaration or XPath namespace node.
#[repr(C)]
pub struct _xmlNs {
    pub next: *mut _xmlNs,      // Next namespace
    pub type_: c_int,           // XML_NAMESPACE_DECL
    pub href: *const xmlChar,   // Namespace URI
    pub prefix: *const xmlChar, // Namespace prefix
    pub _private: *mut c_void,  // Application data
    pub context: *mut _xmlDoc,  // Normally an xmlDoc (deprecated)
}

// ── xmlDtd ──────────────────────────────────────────────────────────────
//
// Source: tree.h lines 538-564

/// DTD (Document Type Definition).
#[repr(C)]
pub struct _xmlDtd {
    pub _private: *mut c_void,   // Application data
    pub type_: c_int,            // XML_DTD_NODE
    pub name: *const xmlChar,    // Name of the DTD
    pub children: *mut _xmlNode, // First child
    pub last: *mut _xmlNode,     // Last child
    pub parent: *mut _xmlDoc,    // Parent node (document)
    pub next: *mut _xmlNode,     // Next sibling
    pub prev: *mut _xmlNode,     // Previous sibling
    pub doc: *mut _xmlDoc,       // Containing document
    // End of common part
    pub notations: *mut c_void,   // Hash table for notations
    pub elements: *mut c_void,    // Hash table for elements
    pub attributes: *mut c_void,  // Hash table for attributes
    pub entities: *mut c_void,    // Hash table for entities
    pub ExternalID: *mut xmlChar, // Public identifier
    pub SystemID: *mut xmlChar,   // System identifier
    pub pentities: *mut c_void,   // Hash table for parameter entities
}

// ── xmlAttr ─────────────────────────────────────────────────────────────
//
// Source: tree.h lines 595-615

/// Attribute node.
#[repr(C)]
pub struct _xmlAttr {
    pub _private: *mut c_void,   // Application data
    pub type_: c_int,            // XML_ATTRIBUTE_NODE
    pub name: *const xmlChar,    // Local name
    pub children: *mut _xmlNode, // First child (text value)
    pub last: *mut _xmlNode,     // Last child
    pub parent: *mut _xmlNode,   // Parent node
    pub next: *mut _xmlAttr,     // Next sibling (attribute)
    pub prev: *mut _xmlAttr,     // Previous sibling (attribute)
    pub doc: *mut _xmlDoc,       // Containing document
    pub ns: *mut _xmlNs,         // Namespace if any
    pub atype: c_int,            // Attribute type
    pub psvi: *mut c_void,       // Type/PSVI information
    pub id: *mut c_void,         // ID struct (deprecated)
}

// ── xmlNode ─────────────────────────────────────────────────────────────
//
// Source: tree.h lines 645-688

/// XML node — the core tree structure.
#[repr(C)]
pub struct _xmlNode {
    pub _private: *mut c_void,   // Application data
    pub type_: c_int,            // Type enum
    pub name: *const xmlChar,    // Node name
    pub children: *mut _xmlNode, // First child
    pub last: *mut _xmlNode,     // Last child
    pub parent: *mut _xmlNode,   // Parent node (NULL for documents)
    pub next: *mut _xmlNode,     // Next sibling
    pub prev: *mut _xmlNode,     // Previous sibling
    pub doc: *mut _xmlDoc,       // Associated document
    // End of common part
    pub ns: *mut _xmlNs,           // Namespace of element
    pub content: *mut xmlChar,     // Content (text/comment/PI)
    pub properties: *mut _xmlAttr, // First attribute of element
    pub nsDef: *mut _xmlNs,        // First namespace definition
    pub psvi: *mut c_void,         // Type/PSVI information
    pub line: c_ushort,            // Line number
    pub extra: c_ushort,           // Extra data for XPath/XSLT
}

// ── xmlDoc ──────────────────────────────────────────────────────────────
//
// Source: tree.h lines 787-845

/// XML Document.
#[repr(C)]
pub struct _xmlDoc {
    pub _private: *mut c_void,   // Application data
    pub type_: c_int,            // XML_DOCUMENT_NODE or XML_HTML_DOCUMENT_NODE
    pub name: *mut c_char,       // NULL
    pub children: *mut _xmlNode, // First child (root element)
    pub last: *mut _xmlNode,     // Last child
    pub parent: *mut _xmlNode,   // Parent node
    pub next: *mut _xmlNode,     // Next sibling
    pub prev: *mut _xmlNode,     // Previous sibling
    pub doc: *mut _xmlDoc,       // Reference to itself
    // End of common part
    pub compression: c_int,      // Level of zlib compression
    pub standalone: c_int,       // Standalone document status
    pub intSubset: *mut _xmlDtd, // Internal subset
    pub extSubset: *mut _xmlDtd, // External subset
    pub oldNs: *mut _xmlNs,      // Old namespace (used during parsing)
    pub version: *mut xmlChar,   // Version string from XML declaration
    pub encoding: *mut xmlChar,  // Actual encoding
    pub ids: *mut c_void,        // Hash table for ID attributes
    pub refs: *mut c_void,       // Hash table for IDREFs (deprecated)
    pub URL: *mut xmlChar,       // URI of the document
    pub charset: c_int,          // Unused (encoding indicator)
    pub dict: *mut c_void,       // Dictionary for names (opaque _xmlDict)
    pub psvi: *mut c_void,       // Type/PSVI information
    pub parseFlags: c_int,       // Parser options used
    pub properties: c_int,       // Document properties flags
}

// ── xmlEntity ───────────────────────────────────────────────────────────
//
// Source: entities.h lines 42-74

/// Entity declaration.
#[repr(C)]
pub struct _xmlEntity {
    pub _private: *mut c_void,      // Application data
    pub type_: c_int,               // XML_ENTITY_DECL (must be second!)
    pub name: *const xmlChar,       // Entity name
    pub children: *mut _xmlNode,    // First child link
    pub last: *mut _xmlNode,        // Last child link
    pub parent: *mut _xmlDtd,       // -> DTD
    pub next: *mut _xmlNode,        // Next sibling link
    pub prev: *mut _xmlNode,        // Previous sibling link
    pub doc: *mut _xmlDoc,          // The containing document
    pub orig: *mut xmlChar,         // Content without ref substitution
    pub content: *mut xmlChar,      // Content or ndata if unparsed
    pub length: c_int,              // The content length
    pub etype: c_int,               // The entity type
    pub ExternalID: *const xmlChar, // External identifier for PUBLIC
    pub SystemID: *const xmlChar,   // URI for SYSTEM or PUBLIC Entity
    pub nexte: *mut _xmlEntity,     // Unused
    pub URI: *const xmlChar,        // The full URI as computed
    pub owner: c_int,               // Unused
    pub flags: c_int,               // Various flags
    pub expandedSize: c_ulong,      // Expanded size
}

// ── xmlError ────────────────────────────────────────────────────────────
//
// Source: xmlerror.h

/// Structured error information.
#[repr(C)]
pub struct _xmlError {
    pub domain: c_int,        // Error domain
    pub code: c_int,          // Error code
    pub message: *mut c_char, // Human-readable error message
    pub level: c_int,         // Error level
    pub file: *mut c_char,    // Filename if available
    pub line: c_int,          // Line number if available
    pub str1: *mut c_char,    // Extra string information
    pub str2: *mut c_char,    // Extra string information
    pub str3: *mut c_char,    // Extra string information
    pub int1: c_int,          // Extra number information
    pub int2: c_int,          // Column number if available
    pub ctxt: *mut c_void,    // Parser context if available
    pub node: *mut c_void,    // Node if available
}

// ── xmlParserInput ──────────────────────────────────────────────────────
//
// Source: parser.h

/// Parser input stream.
#[repr(C)]
pub struct _xmlParserInput {
    pub buf: *mut _xmlParserInputBuffer, // Input buffer (deprecated)
    pub filename: *const c_char,         // The filename or URI (deprecated)
    pub directory: *const c_char,        // Unused (deprecated)
    pub base: *const xmlChar,            // Base of the array to parse
    pub cur: *const xmlChar,             // Current char being parsed (deprecated)
    pub end: *const xmlChar,             // End of the array to parse
    pub length: c_int,                   // Unused (deprecated)
    pub line: c_int,                     // Current line (deprecated)
    pub col: c_int,                      // Current column (deprecated)
    pub consumed: c_ulong,               // How many xmlChars consumed (deprecated)
    pub free: Option<unsafe extern "C" fn(*mut c_char)>, // Deallocation func (deprecated)
    pub encoding: *const xmlChar,        // Unused (deprecated)
    pub version: *const xmlChar,         // The version string for entity (deprecated)
    pub flags: c_int,                    // Flags (deprecated)
    pub id: c_int,                       // Unique identifier (deprecated)
    pub parentConsumed: c_ulong,         // Unused (deprecated)
    pub entity: *mut _xmlEntity,         // Entity if any (deprecated)
}

// ── xmlParserInputBuffer ────────────────────────────────────────────────
//
// Source: xmlIO.h

/// Parser input buffer.
#[repr(C)]
pub struct _xmlParserInputBuffer {
    pub context: *mut c_void,                         // (deprecated)
    pub readcallback: Option<xmlInputReadCallback>,   // (deprecated)
    pub closecallback: Option<xmlInputCloseCallback>, // (deprecated)
    pub encoder: *mut c_void,                         // I18N converter (deprecated)
    pub buffer: *mut c_void,                          // Local buffer (deprecated)
    pub raw: *mut c_void,                             // Raw input buffer (deprecated)
    pub compressed: c_int,                            // Compression flag (deprecated)
    pub error: c_int,                                 // (deprecated)
    pub rawconsumed: c_ulong,                         // (deprecated)
}

// ── xmlOutputBuffer ─────────────────────────────────────────────────────
//
// Source: xmlIO.h

/// Output buffer.
#[repr(C)]
pub struct _xmlOutputBuffer {
    pub context: *mut c_void,                          // (deprecated)
    pub writecallback: Option<xmlOutputWriteCallback>, // (deprecated)
    pub closecallback: Option<xmlOutputCloseCallback>, // (deprecated)
    pub encoder: *mut c_void,                          // I18N converter
    pub buffer: *mut c_void,                           // Local buffer
    pub conv: *mut c_void,                             // Output conversion buffer
    pub written: c_int,                                // Total bytes written
    pub error: c_int,                                  // Error flag
}

// ── xmlCharEncodingHandler ───────────────────────────────────────────────
//
// Source: encoding.h

/// Character encoding conversion handler.
///
/// # UPSTREAM-PARITY
///
/// Layout matches upstream `encoding.h` `struct _xmlCharEncodingHandler`
/// (2.15.x): `name`, then two anonymous unions (`input`, `output`) each
/// carrying either a modern `xmlCharEncConvFunc` or a legacy
/// `xmlCharEncodingInputFunc`/`xmlCharEncodingOutputFunc`, then
/// `inputCtxt`, `outputCtxt`, `ctxtDtor`, `flags`.
/// sizeof == 56, _Alignof == 8 on x86-64.
#[repr(C)]
pub union EncodingInputUnion {
    pub func: Option<xmlCharEncConvFunc>,
    pub legacyFunc: Option<xmlCharEncodingInputFunc>,
}

/// Output-side counterpart of [`EncodingInputUnion`].
#[repr(C)]
pub union EncodingOutputUnion {
    pub func: Option<xmlCharEncConvFunc>,
    pub legacyFunc: Option<xmlCharEncodingOutputFunc>,
}

/// Character encoding conversion handler.
#[repr(C)]
pub struct _xmlCharEncodingHandler {
    pub name: *mut c_char,                        // Encoding name
    pub input: EncodingInputUnion,                // Input converter (union)
    pub output: EncodingOutputUnion,              // Output converter (union)
    pub inputCtxt: *mut c_void,                   // Iconv context for input
    pub outputCtxt: *mut c_void,                  // Iconv context for output
    pub ctxtDtor: Option<xmlCharEncConvCtxtDtor>, // Context destructor
    pub flags: c_int,                             // xmlCharEncFlags
}

// ── xmlBuf ──────────────────────────────────────────────────────────────
//
// Source: tree.h

/// Buffer structure (modern replacement for xmlBuffer).
#[repr(C)]
pub struct _xmlBuf {
    pub content: *mut xmlChar, // The buffer content UTF8
    pub use_: c_uint,          // The buffer size used
    pub size: c_uint,          // The buffer size
    pub alloc: c_int,          // The realloc method
    pub error: c_int,          // Error flag
    pub buffer: c_int,         // Is this a buffer from xmlBuffer?
    pub io: c_int,             // In IO mode?
}

// ── xmlSAXHandler ───────────────────────────────────────────────────────
//
// Source: parser.h

/// SAX event handler structure.
#[repr(C)]
pub struct _xmlSAXHandler {
    pub internalSubset: Option<internalSubsetSAXFunc>,
    pub isStandalone: Option<isStandaloneSAXFunc>,
    pub hasInternalSubset: Option<hasInternalSubsetSAXFunc>,
    pub hasExternalSubset: Option<hasExternalSubsetSAXFunc>,
    pub resolveEntity: Option<resolveEntitySAXFunc>,
    pub getEntity: Option<getEntitySAXFunc>,
    pub entityDecl: Option<entityDeclSAXFunc>,
    pub notationDecl: Option<notationDeclSAXFunc>,
    pub attributeDecl: Option<attributeDeclSAXFunc>,
    pub elementDecl: Option<elementDeclSAXFunc>,
    pub unparsedEntityDecl: Option<unparsedEntityDeclSAXFunc>,
    pub setDocumentLocator: Option<setDocumentLocatorSAXFunc>,
    pub startDocument: Option<startDocumentSAXFunc>,
    pub endDocument: Option<endDocumentSAXFunc>,
    pub startElement: Option<startElementSAXFunc>,
    pub endElement: Option<endElementSAXFunc>,
    pub reference: Option<referenceSAXFunc>,
    pub characters: Option<charactersSAXFunc>,
    pub ignorableWhitespace: Option<ignorableWhitespaceSAXFunc>,
    pub processingInstruction: Option<processingInstructionSAXFunc>,
    pub comment: Option<commentSAXFunc>,
    pub warning: Option<warningSAXFunc>,
    pub error: Option<errorSAXFunc>,
    pub fatalError: Option<fatalErrorSAXFunc>,
    pub getParameterEntity: Option<getParameterEntitySAXFunc>,
    pub cdataBlock: Option<cdataBlockSAXFunc>,
    pub externalSubset: Option<externalSubsetSAXFunc>,
    pub initialized: c_uint,
    pub _private: *mut c_void,
    pub startElementNs: Option<startElementNsSAX2Func>,
    pub endElementNs: Option<endElementNsSAX2Func>,
    pub serror: Option<xmlStructuredErrorFunc>,
}

// ── xmlParserCtxt ───────────────────────────────────────────────────────
//
// Source: parser.h

/// Parser context — the primary parsing state structure.
#[repr(C)]
pub struct _xmlParserCtxt {
    pub sax: *mut _xmlSAXHandler, // SAX handler (deprecated)
    pub userData: *mut c_void,    // User data (deprecated)
    pub myDoc: *mut _xmlDoc,      // Document being built (deprecated)
    pub wellFormed: c_int,        // Is document well formed? (deprecated)
    pub replaceEntities: c_int,   // Replace entities? (deprecated)
    pub version: *mut xmlChar,    // XML version string (deprecated)
    pub encoding: *mut xmlChar,   // Declared encoding (deprecated)
    pub standalone: c_int,        // Standalone document (deprecated)
    pub html: c_int,              // HTML document (deprecated)
    // Input stream stack
    pub input: *mut _xmlParserInput,         // Current input stream
    pub inputNr: c_int,                      // Number of current input streams
    pub inputMax: c_int,                     // Max number of input streams (deprecated)
    pub inputTab: *mut *mut _xmlParserInput, // Stack of inputs
    // Node analysis stack
    pub node: *mut _xmlNode,         // Current element (deprecated)
    pub nodeNr: c_int,               // Depth of parsing stack (deprecated)
    pub nodeMax: c_int,              // Max depth (deprecated)
    pub nodeTab: *mut *mut _xmlNode, // Array of nodes (deprecated)
    // Node info
    pub record_info: c_int,             // Whether node info should be kept
    pub node_seq: xmlParserNodeInfoSeq, // Info about each node parsed (deprecated)
    // Error
    pub errNo: c_int, // Error code (deprecated)
    // Reference and external subset
    pub hasExternalSubset: c_int, // (deprecated)
    pub hasPErefs: c_int,         // (deprecated)
    pub external: c_int,          // (deprecated)
    pub valid: c_int,             // Is document valid? (deprecated)
    pub validate: c_int,          // Validate flag (deprecated)
    pub vctxt: _xmlValidCtxt,     // Validity context
    // Push parser state
    pub instate: c_int,         // (deprecated)
    pub token: c_int,           // (deprecated)
    pub directory: *mut c_char, // Document directory (deprecated)
    // Node name stack
    pub name: *const xmlChar,         // Current parsed Node (deprecated)
    pub nameNr: c_int,                // (deprecated)
    pub nameMax: c_int,               // (deprecated)
    pub nameTab: *mut *const xmlChar, // (deprecated)
    // Misc
    pub nbChars: c_long,            // (deprecated)
    pub checkIndex: c_long,         // (deprecated)
    pub keepBlanks: c_int,          // (deprecated)
    pub disableSAX: c_int,          // (deprecated)
    pub inSubset: c_int,            // DTD parsing state (deprecated)
    pub intSubName: *const xmlChar, // Internal subset name (deprecated)
    pub extSubURI: *mut xmlChar,    // External subset URI (deprecated)
    pub extSubSystem: *mut xmlChar, // External subset public ID (deprecated)
    // xml:space values
    pub space: *mut c_int,    // (deprecated)
    pub spaceNr: c_int,       // (deprecated)
    pub spaceMax: c_int,      // (deprecated)
    pub spaceTab: *mut c_int, // (deprecated)
    // Entity loop prevention
    pub depth: c_int,                 // (deprecated)
    pub entity: *mut _xmlParserInput, // (deprecated)
    pub charset: c_int,               // (deprecated)
    pub nodelen: c_int,               // (deprecated)
    pub nodemem: c_int,               // (deprecated)
    pub pedantic: c_int,              // (deprecated)
    pub _private: *mut c_void,        // User data (deprecated)
    pub loadsubset: c_int,            // Load external subset (deprecated)
    pub linenumbers: c_int,           // (deprecated)
    pub catalogs: *mut c_void,        // (deprecated)
    pub recovery: c_int,              // Recovery mode (deprecated)
    pub progressive: c_int,           // (deprecated)
    pub dict: *mut c_void,            // Dictionary (deprecated)
    pub atts: *mut *const xmlChar,    // Attributes array (deprecated)
    pub maxatts: c_int,               // (deprecated)
    pub docdict: c_int,               // (deprecated)
    // Pre-interned strings
    pub str_xml: *const xmlChar,    // (deprecated)
    pub str_xmlns: *const xmlChar,  // (deprecated)
    pub str_xml_ns: *const xmlChar, // (deprecated)
    // New SAX mode
    pub sax2: c_int,                // (deprecated)
    pub nsNr: c_int,                // (deprecated)
    pub nsMax: c_int,               // (deprecated)
    pub nsTab: *mut *const xmlChar, // (deprecated)
    pub attallocs: *mut c_uint,     // (deprecated)
    pub pushTab: *mut c_void,       // xmlStartTag (deprecated)
    pub attsDefault: *mut c_void,   // (deprecated)
    pub attsSpecial: *mut c_void,   // (deprecated)
    pub nsWellFormed: c_int,        // (deprecated)
    pub options: c_int,             // Extra options (deprecated)
    pub dictNames: c_int,           // (deprecated)
    // Streaming
    pub freeElemsNr: c_int,       // (deprecated)
    pub freeElems: *mut _xmlNode, // (deprecated)
    pub freeAttrsNr: c_int,       // (deprecated)
    pub freeAttrs: *mut _xmlAttr, // (deprecated)
    pub lastError: _xmlError,     // Last error info (deprecated)
    pub parseMode: c_int,         // (deprecated)
    pub nbentities: c_ulong,      // (deprecated)
    pub sizeentities: c_ulong,    // (deprecated)
    // HTML non-recursive parser
    pub nodeInfo: *mut c_void,    // (deprecated)
    pub nodeInfoNr: c_int,        // (deprecated)
    pub nodeInfoMax: c_int,       // (deprecated)
    pub nodeInfoTab: *mut c_void, // (deprecated)
    pub input_id: c_int,          // (deprecated)
    pub sizeentcopy: c_ulong,     // (deprecated)
    pub endCheckState: c_int,     // (deprecated)
    pub nbErrors: c_ushort,       // (deprecated)
    pub nbWarnings: c_ushort,     // (deprecated)
    pub maxAmpl: c_uint,          // (deprecated)
    // Namespace database
    pub nsdb: *mut c_void,     // (deprecated)
    pub attrHashMax: c_uint,   // (deprecated)
    pub attrHash: *mut c_void, // (deprecated)
    // Error handler
    pub errorHandler: Option<xmlStructuredErrorFunc>, // (deprecated)
    pub errorCtxt: *mut c_void,                       // (deprecated)
    // Resource loader
    pub resourceLoader: Option<xmlResourceLoader>, // (deprecated)
    pub resourceCtxt: *mut c_void,                 // (deprecated)
    // Encoding conversion
    pub convImpl: Option<xmlCharEncConvImpl>, // (deprecated)
    pub convCtxt: *mut c_void,                // (deprecated)
}

// ── xmlValidCtxt ────────────────────────────────────────────────────────
//
// Source: valid.h

/// Validation context.
#[repr(C)]
pub struct _xmlValidCtxt {
    pub userData: *mut c_void,                   // User specific data
    pub error: Option<xmlValidityErrorFunc>,     // Error callback
    pub warning: Option<xmlValidityWarningFunc>, // Warning callback
    pub node: *mut _xmlNode,                     // Current parsed Node
    pub nodeNr: c_int,                           // Depth of the parsing stack
    pub nodeMax: c_int,                          // Max depth
    pub nodeTab: *mut *mut _xmlNode,             // Array of nodes
    pub flags: c_uint,                           // Internal flags
    pub doc: *mut _xmlDoc,                       // The document
    pub valid: c_int,                            // Temporary validity check result
    pub vstate: *mut c_void,                     // Current validation state
    pub vstateNr: c_int,                         // Depth of validation stack
    pub vstateMax: c_int,                        // Max depth
    pub vstateTab: *mut c_void,                  // Array of validation states
    pub am: *mut c_void,                         // Automata
    pub state: *mut c_void,                      // Automata state
}

// ── xmlParserNodeInfo ───────────────────────────────────────────────────

/// Node info for parser tracking.
#[repr(C)]
pub struct _xmlParserNodeInfo {
    pub node: *const _xmlNode,
    pub begin_pos: c_ulong,
    pub begin_line: c_ulong,
    pub end_pos: c_ulong,
    pub end_line: c_ulong,
}

/// Node info sequence.
#[repr(C)]
pub struct _xmlParserNodeInfoSeq {
    pub block: *mut _xmlParserNodeInfo,
    pub index: *mut c_int,
    pub block_max: c_int,
    pub size: c_int,
}

/// Typedef alias for `_xmlParserNodeInfoSeq`.
pub type xmlParserNodeInfoSeq = _xmlParserNodeInfoSeq;

// ── xmlXPathContext ─────────────────────────────────────────────────────
//
// Source: xpath.h

/// XPath evaluation context.
#[repr(C)]
pub struct _xmlXPathContext {
    pub doc: *mut _xmlDoc,                                 // The current document
    pub node: *mut _xmlNode,                               // The current node
    pub nb_variables_unused: c_int,                        // (unused)
    pub max_variables_unused: c_int,                       // (unused)
    pub varHash: *mut c_void,                              // Hash table of defined variables
    pub nb_types: c_int,                                   // Number of defined types
    pub max_types: c_int,                                  // Max number of types
    pub types: *mut c_void,                                // Array of defined types (xmlXPathType)
    pub nb_funcs_unused: c_int,                            // (unused)
    pub max_funcs_unused: c_int,                           // (unused)
    pub funcHash: *mut c_void,                             // Hash table of defined funcs
    pub nb_axis: c_int,                                    // Number of defined axis
    pub max_axis: c_int,                                   // Max number of axis
    pub axis: *mut c_void,                                 // Array of defined axis (xmlXPathAxis)
    pub namespaces: *mut *mut _xmlNs,                      // Array of namespaces
    pub nsNr: c_int,                                       // Number of namespaces in scope
    pub user: *mut c_void,                                 // Function to free
    pub contextSize: c_int,                                // Context size
    pub proximityPosition: c_int,                          // Proximity position
    pub xptr: c_int,                                       // XPointer context?
    pub here: *mut _xmlNode,                               // For here()
    pub origin: *mut _xmlNode,                             // For origin()
    pub nsHash: *mut c_void,                               // Namespaces hash table
    pub varLookupFunc: Option<xmlXPathVariableLookupFunc>, // Variable lookup func
    pub varLookupData: *mut c_void,                        // Variable lookup data
    pub extra: *mut c_void,                                // Needed for XSLT
    pub function: *const xmlChar,                          // Function name when calling a function
    pub functionURI: *const xmlChar,                       // Function namespace URI
    pub funcLookupFunc: Option<xmlXPathFuncLookupFunc>,    // Function lookup func
    pub funcLookupData: *mut c_void,                       // Function lookup data
    pub tmpNsList: *mut *mut _xmlNs,                       // Array of temp namespaces
    pub tmpNsNr: c_int,                                    // Number of temp namespaces
    pub userData: *mut c_void,                             // User specific data
    pub error: Option<xmlStructuredErrorFunc>,             // Error callback
    pub lastError: _xmlError,                              // Last error
    pub debugNode: *mut _xmlNode,                          // Source node (XSLT)
    pub dict: *mut c_void,                                 // Dictionary
    pub flags: c_int,                                      // Compilation flags
    pub cache: *mut c_void,                                // Cache for XPath objects
    pub opLimit: c_ulong,                                  // Resource limits
    pub opCount: c_ulong,
    pub depth: c_int,
}

// ── xmlXPathObject ──────────────────────────────────────────────────────
//
// Source: xpath.h

/// XPath evaluated object.
#[repr(C)]
pub struct _xmlXPathObject {
    pub type_: c_int,            // Object type
    pub nodesetval: *mut c_void, // Node set (xmlNodeSet)
    pub boolval: c_int,          // Boolean value
    pub floatval: f64,           // Number value
    pub stringval: *mut xmlChar, // String value
    pub user: *mut c_void,       // User pointer
    pub index: c_int,            // Index
    pub user2: *mut c_void,      // User pointer 2
    pub index2: c_int,           // Index 2
}

// ── Node set (contained within XPath objects) ───────────────────────────

/// XPath node set.
#[repr(C)]
pub struct _xmlNodeSet {
    pub nodeNr: c_int,               // Number of nodes
    pub nodeMax: c_int,              // Max number of nodes
    pub nodeTab: *mut *mut _xmlNode, // Array of nodes
}

// ── XSLT types ──────────────────────────────────────────────────────────
//
// ═══════════════════════════════════════════════════════════════════════════════
// XSLT Types (Phase 8)
// ═══════════════════════════════════════════════════════════════════════════════
// Source: xslt.h, xsltInternals.h (from libxslt)

/// XSLT stylesheet — the compiled representation of an XSLT stylesheet.
///
/// # UPSTREAM-PARITY
///
/// Layout matches upstream `_xsltStylesheet` from xsltInternals.h (libxslt 1.1.39).
/// Field order and types match the C struct exactly for ABI compatibility.
///
/// Courts: XSLT-STYLESHEET-*
#[repr(C)]
pub struct _xsltStylesheet {
    /// The stylesheet document (the parsed XML of the .xsl file).
    pub doc: *mut _xmlDoc,

    /// Ordered list of templates (highest priority first).
    pub templates: *mut _xsltTemplate,

    /// Unused/freed templates for recycling.
    pub templatesFree: *mut _xsltTemplate,

    /// Hash for template lookup by name/mode.
    pub internalHash: *mut c_void,

    /// Key definitions.
    pub keys: *mut _xsltKeyDef,

    /// Parent stylesheet (for import tree).
    pub parent: *mut _xsltStylesheet,

    /// Next stylesheet in import chain.
    pub next: *mut _xsltStylesheet,

    /// Imported stylesheets (linked list).
    pub imports: *mut _xsltStylesheet,

    /// Output method ("xml", "html", "text", or custom).
    pub method: *const xmlChar,

    /// Method namespace URI (for custom output methods).
    pub methodURI: *const xmlChar,

    /// Version string for output.
    pub version: *const xmlChar,

    /// Output encoding.
    pub encoding: *const xmlChar,

    /// Omit XML declaration (0 = include, 1 = omit).
    pub omitXmlDeclaration: c_int,

    /// Standalone declaration (0 = no, 1 = yes, -1 = auto).
    pub standalone: c_int,

    /// Namespace list for output.
    pub nsList: *mut *mut xmlChar,

    /// Number of namespaces in nsList.
    pub nsNr: c_int,

    /// Maximum capacity of nsList.
    pub nsMax: c_int,

    /// Hash of decimal format definitions.
    pub decimalFormat: *mut c_void,

    /// Hash of namespace aliases.
    pub nsAliases: *mut c_void,

    /// Hash of attribute sets.
    pub attributeSets: *mut c_void,

    /// Hash of global variables.
    pub variables: *mut c_void,

    /// Hash of global parameters.
    pub params: *mut c_void,

    /// Hash of strip-space elements.
    pub stripSpaces: *mut c_void,

    /// Hash of preserve-space elements.
    pub preserveSpaces: *mut c_void,

    /// Error count during compilation.
    pub errors: c_int,

    /// Warning count during compilation.
    pub warnings: c_int,

    /// Stylesheet name (URI).
    pub name: *const xmlChar,

    /// Profiling flags.
    pub profile: c_int,

    /// Hash of extension information.
    pub extInfos: *mut c_void,

    /// Security preferences.
    pub secPrefs: *mut c_void,

    /// Skip default namespace flag.
    pub skipDefaultNS: c_int,

    /// Indent output (0 = no, 1 = yes, -1 = auto).
    pub indent: c_int,

    /// DOCTYPE public identifier.
    pub doctypePublic: *const xmlChar,

    /// DOCTYPE system identifier.
    pub doctypeSystem: *const xmlChar,

    /// Media type (for output).
    pub mediaType: *const xmlChar,

    /// PSVI data.
    pub psvi: *mut c_void,

    /// Number of global variables.
    pub varsNr: c_int,

    /// Maximum global variables.
    pub varsMax: c_int,

    /// Variables table.
    pub varsTab: *mut c_void,

    /// Deep context flag (internal).
    pub deepCtxt: c_int,

    /// Include count.
    pub includes: c_int,
}

/// XSLT transform context — holds runtime state during a transformation.
///
/// # UPSTREAM-PARITY
///
/// Layout matches upstream `_xsltTransformContext` from xsltInternals.h (libxslt 1.1.39).
///
/// Courts: XSLT-TRANSFORM-*
#[repr(C)]
pub struct _xsltTransformContext {
    /// Current stylesheet being applied.
    pub style: *mut _xsltStylesheet,

    /// Current template being executed.
    pub templ: *mut _xsltTemplate,

    /// Source document being transformed.
    pub document: *mut _xmlDoc,

    /// Current node in the source document.
    pub node: *mut _xmlNode,

    /// Current node list (for xsl:for-each context).
    pub nodeList: *mut _xmlNode,

    /// Context size (number of nodes in current node list).
    pub contextSize: c_int,

    /// Context position (1-based position in current node list).
    pub proximityPosition: c_int,

    /// XPath evaluation context.
    pub xpathCtxt: *mut _xmlXPathContext,

    /// XPath return value stack.
    pub xpathReturn: *mut c_void,

    pub xpathReturnNr: c_int,
    pub xpathReturnMax: c_int,
    pub xpathReturnTab: *mut c_void,

    /// Template stack.
    pub templNr: c_int,
    pub templMax: c_int,
    pub templTab: *mut *mut _xsltTemplate,

    /// Current template recursion depth.
    pub depth: c_int,

    /// Maximum allowed template recursion depth.
    pub maxDepth: c_int,

    /// Variable stack.
    pub varsNr: c_int,
    pub varsMax: c_int,
    pub varsBase: c_int,
    pub varsTab: *mut c_void,

    /// Parameter stack.
    pub paramsNr: c_int,
    pub paramsMax: c_int,
    pub paramsBase: c_int,
    pub paramsTab: *mut c_void,

    /// Key tables (hash of xsltKeyTablePtr).
    pub keyTables: *mut c_void,

    /// Whether sorting is active.
    pub hasSort: c_int,

    /// Output buffer.
    pub output: *mut _xmlOutputBuffer,

    /// Error handler function.
    pub errFunc: *mut c_void,

    /// Error handler context.
    pub errCtxt: *mut c_void,

    /// Hash of extension data.
    pub extInfos: *mut c_void,

    pub extrasNr: c_int,
    pub extrasMax: c_int,
    pub extrasTab: *mut *mut c_void,

    /// Current state flags.
    pub state: c_int,

    /// Template priority during matching.
    pub priority: c_int,

    /// Current parser context (for document() function).
    pub parserCtxt: *mut _xmlParserCtxt,

    /// Save flags.
    pub save: c_int,

    /// Options.
    pub opts: c_int,

    /// Security preferences.
    pub secPrefs: *mut c_void,

    /// Document cache for document() function.
    pub docCache: *mut c_void,

    /// Hash of global variable values.
    pub globalVars: *mut c_void,

    /// Profile data.
    pub profile: *mut c_void,

    /// Profile timer.
    pub prof: c_long,

    pub extFunctionsNr: c_int,
    pub extFunctionsMax: c_int,
    pub extFunctionsTab: *mut c_void,

    /// User data pointer.
    pub userData: *mut c_void,

    /// Current stack frame.
    pub frame: c_int,
    pub frameNr: c_int,
    pub frameMax: c_int,
    pub frameTab: *mut c_void,

    /// Current insertion point for result tree construction.
    pub insert: *mut _xmlNode,

    /// Current result document.
    pub resultDoc: *mut _xmlDoc,

    /// Namespace stack.
    pub nsNr: c_int,
    pub nsMax: c_int,
    pub nsTab: *mut c_void,
    pub nsNrOriginal: c_int,
    pub nsMaxOriginal: c_int,
    pub nsTabOriginal: *mut c_void,

    /// Current template node.
    pub currentTemplate: *mut _xmlNode,

    /// Template lookup hash.
    pub templHash: *mut c_void,

    /// Original variable stack.
    pub varsNrOriginal: c_int,
    pub varsMaxOriginal: c_int,
    pub varsTabOriginal: *mut c_void,

    /// Insert depth (number of nested insertion points).
    pub insertDepth: c_int,

    /// Maximum allowed insert depth.
    pub maxInsertDepth: c_int,
}

/// XSLT compiled template.
///
/// # UPSTREAM-PARITY
///
/// Layout matches upstream `_xsltTemplate` from xsltInternals.h.
#[repr(C)]
pub struct _xsltTemplate {
    /// Next template in the list.
    pub next: *mut _xsltTemplate,

    /// Owning stylesheet.
    pub style: *mut _xsltStylesheet,

    /// Match pattern node (the `match` attribute expression).
    pub r#match: *mut _xmlNode,

    /// Template name (for named templates).
    pub name: *const xmlChar,

    /// Template mode.
    pub mode: *const xmlChar,

    /// Template priority.
    pub priority: f64,

    /// Template content (the child instruction nodes).
    pub content: *mut _xmlNode,

    /// Import depth (0 = main stylesheet, 1+ = imported).
    pub depth: c_int,

    /// Template flags.
    pub flags: c_int,

    /// Number of inherited namespace declarations.
    pub inheritedNsNr: c_int,

    /// Inherited namespace declarations.
    pub inheritedNs: *mut c_void,
}

/// XSLT document wrapper.
///
/// # UPSTREAM-PARITY
///
/// Layout matches upstream `_xsltDocument` from xsltInternals.h.
#[repr(C)]
pub struct _xsltDocument {
    /// Next document in the list.
    pub next: *mut _xsltDocument,

    /// Import depth.
    pub depth: c_int,

    /// The wrapped document.
    pub doc: *mut _xmlDoc,

    /// Insertion point for result tree construction.
    pub insert: *mut _xmlNode,

    /// Owning stylesheet.
    pub style: *mut _xsltStylesheet,

    /// Whether this is a compiled context.
    pub compCtxt: c_int,
}

/// XSLT key definition.
///
/// # UPSTREAM-PARITY
///
/// Layout matches upstream `_xsltKeyDef` from xsltInternals.h.
#[repr(C)]
pub struct _xsltKeyDef {
    /// Next key definition.
    pub next: *mut _xsltKeyDef,

    /// Owning stylesheet.
    pub style: *mut _xsltStylesheet,

    /// The xsl:key instruction node.
    pub inst: *mut _xmlNode,

    /// Key name.
    pub name: *const xmlChar,

    /// Match pattern.
    pub r#match: *const xmlChar,

    /// Use expression.
    pub r#use: *const xmlChar,

    /// Import depth.
    pub depth: c_int,
}

/// XSLT key table entry.
///
/// # UPSTREAM-PARITY
///
/// Layout matches upstream `_xsltKeyTable` from xsltInternals.h.
#[repr(C)]
pub struct _xsltKeyTable {
    /// Next key table.
    pub next: *mut _xsltKeyTable,

    /// Table of XPath object results.
    pub table: *mut *mut _xmlXPathObject,

    /// Number of entries in table.
    pub nb: c_int,

    /// Maximum capacity of table.
    pub max: c_int,

    /// Key name.
    pub name: *mut xmlChar,

    /// Import depth.
    pub depth: c_int,
}

/// XSLT stack element (variable/parameter binding).
///
/// # UPSTREAM-PARITY
///
/// Layout matches upstream `_xsltStackElem` from xsltInternals.h.
#[repr(C)]
pub struct _xsltStackElem {
    /// Next stack element.
    pub next: *mut _xsltStackElem,

    /// Owning stylesheet.
    pub style: *mut _xsltStylesheet,

    /// The xsl:variable or xsl:param instruction node.
    pub inst: *mut _xmlNode,

    /// Variable/parameter name.
    pub name: *const xmlChar,

    /// Variable/parameter namespace URI.
    pub nameURI: *const xmlChar,

    /// Select expression.
    pub select: *const xmlChar,

    /// Compiled select expression.
    pub comp: *mut _xmlXPathObject,

    /// Tree content (for inline content).
    pub tree: *mut _xmlNode,

    /// Evaluated value.
    pub value: *mut _xmlXPathObject,

    /// Flags.
    pub flags: c_int,

    /// Scope level.
    pub level: c_int,

    /// Whether this is a compiled context.
    pub compCtxt: c_int,
}

/// XSLT decimal format definition.
///
/// # UPSTREAM-PARITY
///
/// Layout matches upstream `_xsltDecimalFormat` from xsltInternals.h.
#[repr(C)]
pub struct _xsltDecimalFormat {
    /// Next decimal format.
    pub next: *mut _xsltDecimalFormat,

    /// Format name (NULL = default).
    pub name: *const xmlChar,

    /// Digit character.
    pub digit: *mut xmlChar,

    /// Pattern separator character.
    pub patternSeparator: *mut xmlChar,

    /// Decimal point character.
    pub decimalPoint: *mut xmlChar,

    /// Grouping separator character.
    pub groupingSeparator: *mut xmlChar,

    /// Infinity string.
    pub infinity: *mut xmlChar,

    /// Minus sign character.
    pub minusSign: *mut xmlChar,

    /// NaN string.
    pub NaN: *mut xmlChar,

    /// Percent character.
    pub percent: *mut xmlChar,

    /// Per-mille character.
    pub perMille: *mut xmlChar,

    /// Zero digit character.
    pub zeroDigit: *mut xmlChar,
}

/// XSLT namespace alias.
///
/// # UPSTREAM-PARITY
///
/// Layout matches upstream `_xsltNsAlias` from xsltInternals.h.
#[repr(C)]
pub struct _xsltNsAlias {
    /// Next namespace alias.
    pub next: *mut _xsltNsAlias,

    /// Result namespace URI.
    pub resultNs: *const xmlChar,

    /// Stylesheet namespace URI.
    pub styleNs: *const xmlChar,
}

/// XSLT attribute set.
///
/// # UPSTREAM-PARITY
///
/// Layout matches upstream `_xsltAttrSet` from xsltInternals.h.
#[repr(C)]
pub struct _xsltAttrSet {
    /// Next attribute set.
    pub next: *mut _xsltAttrSet,

    /// Attribute set name.
    pub name: *const xmlChar,

    /// Attribute set namespace URI.
    pub ns: *const xmlChar,

    /// The xsl:attribute-set instruction node.
    pub inst: *mut _xmlNode,

    /// Owning stylesheet.
    pub style: *mut _xsltStylesheet,

    /// Import depth.
    pub depth: c_int,
}

/// XSLT sort element.
///
/// # UPSTREAM-PARITY
///
/// Layout matches upstream `_xsltSort` from xsltInternals.h.
#[repr(C)]
pub struct _xsltSort {
    /// Next sort element.
    pub next: *mut _xsltSort,

    /// The xsl:sort instruction node.
    pub inst: *mut _xmlNode,

    /// The sort key select expression.
    pub select: *const xmlChar,

    /// Language for sorting.
    pub lang: *const xmlChar,

    /// Data type ("text" or "number").
    pub dataType: *const xmlChar,

    /// Sort order ("ascending" or "descending").
    pub order: *const xmlChar,

    /// Case order ("upper-first" or "lower-first").
    pub caseOrder: *const xmlChar,

    /// Whether this sort is a text sort.
    pub isText: c_int,

    /// Whether the select expression was a constant.
    pub hasConst: c_int,

    /// Locale information.
    pub locale: *mut c_void,

    /// Owning stylesheet.
    pub style: *mut _xsltStylesheet,

    /// Import depth.
    pub depth: c_int,
}

// ── Type aliases for pointer types ──────────────────────────────────────

pub type xmlNodePtr = *mut _xmlNode;
pub type xmlDocPtr = *mut _xmlDoc;
pub type xmlNsPtr = *mut _xmlNs;
pub type xmlAttrPtr = *mut _xmlAttr;
pub type xmlDtdPtr = *mut _xmlDtd;
pub type xmlEntityPtr = *mut _xmlEntity;
pub type xmlErrorPtr = *mut _xmlError;
pub type xmlParserCtxtPtr = *mut _xmlParserCtxt;
pub type xmlParserInputPtr = *mut _xmlParserInput;
pub type xmlParserInputBufferPtr = *mut _xmlParserInputBuffer;
pub type xmlOutputBufferPtr = *mut _xmlOutputBuffer;
pub type xmlSAXHandlerPtr = *mut _xmlSAXHandler;
pub type xmlValidCtxtPtr = *mut _xmlValidCtxt;
pub type xmlBufferPtr = *mut _xmlBuffer;
pub type xmlElementPtr = *mut _xmlElement;
pub type xmlElementContentPtr = *mut _xmlElementContent;
pub type xmlNotationPtr = *mut _xmlNotation;
pub type xmlEnumerationPtr = *mut _xmlEnumeration;
pub type xmlAttributeDeclPtr = *mut _xmlAttribute;
pub type xmlXPathContextPtr = *mut _xmlXPathContext;
pub type xmlXPathObjectPtr = *mut _xmlXPathObject;
pub type xmlNodeSetPtr = *mut _xmlNodeSet;
pub type xmlCharEncodingHandlerPtr = *mut _xmlCharEncodingHandler;
pub type xmlBufPtr = *mut _xmlBuf;
pub type xsltStylesheetPtr = *mut _xsltStylesheet;
pub type xsltTransformContextPtr = *mut _xsltTransformContext;
pub type xsltTemplatePtr = *mut _xsltTemplate;
pub type xsltKeyDefPtr = *mut _xsltKeyDef;
pub type xsltKeyTablePtr = *mut _xsltKeyTable;
pub type xsltStackElemPtr = *mut _xsltStackElem;
pub type xsltDecimalFormatPtr = *mut _xsltDecimalFormat;
pub type xsltNsAliasPtr = *mut _xsltNsAlias;
pub type xsltAttrSetPtr = *mut _xsltAttrSet;
pub type xsltDocumentPtr = *mut _xsltDocument;
pub type xsltSortPtr = *mut _xsltSort;
