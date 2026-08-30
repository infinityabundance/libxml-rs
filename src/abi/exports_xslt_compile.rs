//! C ABI exports for libxslt.so.1 — the "compile" family (§16, Phase 8).
//!
//! This module implements the stylesheet-compilation entry points of the
//! libxslt 1.1.45 C ABI:
//!
//! - Stylesheet creation: `xsltNewStylesheet`, `xsltParseStylesheetProcess`,
//!   `xsltParseStylesheetUser`, `xsltParseStylesheetImportedDoc`
//! - Imports/includes: `xsltParseStylesheetImport`, `xsltParseStylesheetInclude`
//! - Top-level constructs: `xsltParseStylesheetOutput`,
//!   `xsltParseStylesheetAttributeSet`, `xsltParseGlobalVariable`,
//!   `xsltParseGlobalParam`
//! - Content preprocessing: `xsltParseTemplateContent`, `xsltCompileAttr`
//! - Precomputed instructions: `xsltDocumentComp`, `xsltStylePreCompute`,
//!   `xsltPreComputeExtModuleElement`, `xsltNormalizeCompSteps`,
//!   `xsltFreeStylePreComps`
//! - Style documents: `xsltNewStyleDocument`, `xsltLoadStyleDocument`,
//!   `xsltFreeStyleDocuments`
//! - Global state: `xsltInitGlobals`, `xsltUninit`, `xsltFreeExts`,
//!   `xsltShutdownExts`, `xsltDebugDumpExtensions`
//!
//! # UPSTREAM-PARITY
//!
//! Every function is a faithful port of the upstream libxslt 1.1.45 sources
//! in `archaeology/libxslt-git/libxslt/` (xslt.c, imports.c, preproc.c,
//! attributes.c, attrvt.c, documents.c, variables.c, extensions.c, pattern.c).
//! The oracle build has `XSLT_REFACTORED` disabled, so the *old* (non
//! refactored) code paths are the authoritative semantics.
//!
//! # Engine wiring
//!
//! The native-Rust engine in `src/xslt/compiler` compiles stylesheets
//! eagerly (top-level constructs) but compiles *instructions* lazily: at
//! transform time the runtime dispatches on the raw instruction node
//! (`src/xslt/transform`, `xsltProcessInstruction`) and never consults
//! `node->psvi`. Consequently the upstream per-instruction compilers
//! (`xsltApplyTemplatesComp` et al.) have no data to store; the ABI
//! functions below keep their *observable* semantics (the grammar checks
//! that bump `style->errors` / `style->warnings`, the return values, the
//! `style->preComps` chain for the structures that genuinely exist) and
//! skip the dead precomp allocation. Each such divergence is documented
//! at the function.

#![allow(non_snake_case)]
#![allow(unused_variables)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use core::ffi::c_void;
use core::ptr;
use std::os::raw::{c_char, c_int};

use crate::abi::allocator::{xmlFree, xmlMalloc};
use crate::abi::exports_hash::xmlDictReference;
use crate::abi::exports_string::xmlStrstr;
use crate::abi::exports_tree::xmlNodeGetBase;
use crate::abi::exports_uri::xmlBuildURI;
use crate::abi::exports_xml2::*;
use crate::abi::structs::*;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::*;
use crate::xml::xpath::exports::xmlXPathNewString;

/// The XSLT namespace URI (upstream `XSLT_NAMESPACE`, xslt.h).
const XSLT_NAMESPACE: &[u8] = b"http://www.w3.org/1999/XSL/Transform";

/// `XSLT_PARSE_OPTIONS` (xslt.h): NOENT | DTDLOAD | DTDATTR | NOCDATA.
const XSLT_PARSE_OPTIONS: c_int = (1 << 1) | (1 << 2) | (1 << 3) | (1 << 4);

/// `XSLT_LOAD_STYLESHEET` (documents.h).
const XSLT_LOAD_STYLESHEET: c_int = 1;

/// `xsltStyleType` values (xsltInternals.h, non-refactored enum).
const XSLT_FUNC_DOCUMENT: c_int = 17;
const XSLT_FUNC_EXTENSION: c_int = 22;

/// `XSLT_VAR_PARAM` (variables.c): stack-elem PARAM flag used to
/// distinguish global variables from parameters in `_xsltStackElem.flags`.
const XSLT_VAR_PARAM: c_int = 1 << 1;

/// `XSLT_SECPREF_READ_FILE` / `XSLT_SECPREF_READ_NETWORK` (security.c).
const XSLT_SECPREF_READ_FILE: c_int = 1;
const XSLT_SECPREF_READ_NETWORK: c_int = 4;

/// `XSLT_MAX_NESTING` (imports.c).
const XSLT_MAX_NESTING: c_int = 40;

/// `xsltExtMarker` (preproc.c) — the sentinel stored in `inst->psvi` for
/// extension elements with no registered precomputation.
///
/// # UPSTREAM-PARITY
///
/// Upstream exports `xsltExtMarker` as a variable; the candidate engine
/// never reads `psvi`, so the marker is carried as a private static (the
/// ext-family ABI exports own the exported variable).
static XSLT_EXT_MARKER: [u8; 18] = *b"Extension Element\0";

// ═══════════════════════════════════════════════════════════════════════════════
// Types & structures
// ═══════════════════════════════════════════════════════════════════════════════

/// `xsltTransformFunction` (xsltInternals.h): the handling function of a
/// compiled instruction/extension element.
pub type xsltTransformFunction = unsafe extern "C" fn(
    ctxt: *mut _xsltTransformContext,
    node: *mut _xmlNode,
    inst: *mut _xmlNode,
    comp: *mut c_void,
);

/// `xsltElemPreCompDeallocator` (xsltInternals.h): deallocates a precomp.
pub type xsltElemPreCompDeallocator = unsafe extern "C" fn(comp: *mut c_void);

/// `xsltPreComputeFunction` (extensions.h): precomputation callback of an
/// extension element.
pub type xsltPreComputeFunction = unsafe extern "C" fn(
    style: *mut _xsltStylesheet,
    inst: *mut _xmlNode,
    function: Option<xsltTransformFunction>,
) -> *mut c_void;

/// `_xsltElemPreComp` (xsltInternals.h, non-refactored layout).
///
/// ```c
/// struct _xsltElemPreComp {
///     xsltElemPreCompPtr next;    /* next item in the global chained list
///                                    held by xsltStylesheet. */
///     xsltStyleType type;         /* type of the element */
///     xsltTransformFunction func; /* handling function */
///     xmlNodePtr inst;            /* the node in the stylesheet's tree
///                                    corresponding to this item */
///     /* end of common part */
///     xsltElemPreCompDeallocator free; /* the deallocator */
/// };
/// ```
#[repr(C)]
pub struct _xsltElemPreComp {
    pub next: *mut _xsltElemPreComp,
    pub type_: c_int, // xsltStyleType
    pub func: Option<xsltTransformFunction>,
    pub inst: *mut _xmlNode,
    pub free: Option<xsltElemPreCompDeallocator>,
}

/// The old (non-refactored) `_xsltStylePreComp` (xsltInternals.h) extends
/// `_xsltElemPreComp` with per-instruction precomputed values. The
/// candidate engine compiles instructions lazily, so only the fields that
/// the compile-family itself writes (`ver11`, `filename`, `has_filename`,
/// used by `xsltDocumentComp`) are carried; the remaining upstream fields
/// (sort/name/select/numdata/comp/nsList…) hold nothing in this engine and
/// are omitted (documented divergence — nothing reads them).
#[repr(C)]
struct _xsltStylePreComp {
    pub base: _xsltElemPreComp,
    pub ver11: c_int,
    pub filename: *const xmlChar,
    pub has_filename: c_int,
}

/// Extension-element registry entry (upstream `xsltElementsHash` payload
/// `_xsltExtElement { precomp, transform }`, extended with the lookup key).
#[repr(C)]
struct _xsltExtElementEntry {
    pub next: *mut _xsltExtElementEntry,
    pub name: *mut xmlChar,
    pub URI: *mut xmlChar,
    pub precomp: Option<xsltPreComputeFunction>,
    pub transform: Option<xsltTransformFunction>,
}

/// Global registry of registered extension elements, keyed by
/// `(name, namespace-URI)` — the candidate mirror of upstream's global
/// `xsltElementsHash`. Upstream guards it with `xsltExtMutex`; the
/// candidate build is single-threaded for the compile phase, matching the
/// rest of the crate's registry handling.
static mut XSLT_ELEMENTS_REGISTRY: *mut _xsltExtElementEntry = ptr::null_mut();

/// Global registry of registered extension *modules* — the candidate
/// mirror of upstream's `xsltExtensionsHash` (used by
/// `xsltDebugDumpExtensions` and `xsltShutdownExts`).
#[repr(C)]
struct _xsltExtModuleEntry {
    pub next: *mut _xsltExtModuleEntry,
    pub URI: *mut xmlChar,
    pub shutdownFunc: Option<unsafe extern "C" fn(*mut c_void, *const xmlChar, *mut c_void)>,
}

static mut XSLT_MODULES_REGISTRY: *mut _xsltExtModuleEntry = ptr::null_mut();

/// Whether `xsltInitGlobals` has run (mirrors upstream `xsltExtMutex !=
/// NULL`).
static mut XSLT_GLOBALS_INITIALIZED: c_int = 0;

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// IS_XSLT_ELEM (xsltutils.h).
unsafe fn is_xslt_elem(n: *mut _xmlNode) -> bool {
    if n.is_null() || (*n).type_ != XML_ELEMENT_NODE as c_int || (*n).ns.is_null() {
        return false;
    }
    xmlStrEqual((*(*n).ns).href, XSLT_NAMESPACE.as_ptr() as *const xmlChar) != 0
}

/// IS_XSLT_NAME (xsltutils.h).
unsafe fn is_xslt_name(n: *mut _xmlNode, val: &[u8]) -> bool {
    if n.is_null() || (*n).name.is_null() {
        return false;
    }
    let len = libc::strlen((*n).name as *const libc::c_char) as usize;
    len == val.len() && core::slice::from_raw_parts((*n).name, len) == val
}

/// IS_BLANK (xsltutils.h): a string made only of XML whitespace.
unsafe fn is_blank_str(str: *const xmlChar) -> bool {
    if str.is_null() {
        return true;
    }
    let mut cur = str;
    while *cur != 0 {
        if *cur != b' ' && *cur != b'\t' && *cur != b'\n' && *cur != b'\r' {
            return false;
        }
        cur = cur.add(1);
    }
    true
}

/// Report a compile-time error (xsltTransformError; the candidate records
/// the literal message, matching the crate's non-variadic convention).
/// Render a NUL-terminated C string as a byte slice for `report_error`.
unsafe fn cbytes(p: *const u8) -> &'static [u8] {
    if p.is_null() {
        return b"";
    }
    core::ffi::CStr::from_ptr(p as *const c_char).to_bytes()
}

unsafe fn report_error(style: *mut _xsltStylesheet, inst: *mut _xmlNode, msg: &[u8]) {
    let mut m = msg.to_vec();
    m.push(0);
    crate::xslt::errors::xsltTransformError(
        ptr::null_mut(),
        style,
        inst,
        m.as_ptr() as *const c_char,
    );
}

/// `xsltFreeExtDef` (extensions.c): free one extension-prefix def.
///
/// Not used by the compile family itself (see `xsltFreeExts`), but kept
/// for symmetry with the upstream def-list handling.
#[allow(dead_code)]
unsafe fn xslt_free_ext_def(entry: *mut c_void) {
    // The candidate never allocates xsltExtDef lists; this is unreachable
    // and kept only to document the upstream shape.
    let _ = entry;
}

/// Look up a registered extension element by (name, namespace-URI).
unsafe fn ext_element_lookup(
    name: *const xmlChar,
    uri: *const xmlChar,
) -> *mut _xsltExtElementEntry {
    if name.is_null() || uri.is_null() {
        return ptr::null_mut();
    }
    let mut cur = XSLT_ELEMENTS_REGISTRY;
    while !cur.is_null() {
        if !(*cur).name.is_null()
            && !(*cur).URI.is_null()
            && xmlStrEqual((*cur).name, name) != 0
            && xmlStrEqual((*cur).URI, uri) != 0
        {
            return cur;
        }
        cur = (*cur).next;
    }
    ptr::null_mut()
}

/// Duplicate a NUL-terminated string with the xml allocator.
unsafe fn dup_str(s: *const xmlChar) -> *mut xmlChar {
    if s.is_null() {
        return ptr::null_mut();
    }
    let len = libc::strlen(s as *const libc::c_char);
    let copy = xmlMalloc(len + 1) as *mut xmlChar;
    if copy.is_null() {
        return ptr::null_mut();
    }
    core::ptr::copy_nonoverlapping(s, copy, len);
    *copy.add(len) = 0;
    copy
}

/// `xsltCheckRead` (security.c) for the file/network split. The candidate
/// security module exposes only the check-fn registry; the URI-scheme
/// analysis is reduced to upstream's first-order file-vs-network test
/// (a `://` in the value selects the network check).
///
/// Returns 1 if read is allowed, 0 if denied, -1 on error.
unsafe fn xslt_check_read(
    sec: *mut c_void,
    ctxt: *mut _xsltTransformContext,
    url: *const xmlChar,
) -> c_int {
    if sec.is_null() {
        return 1;
    }
    let is_network = !xmlStrstr(url, b"://\0".as_ptr() as *const xmlChar).is_null();
    let option = if is_network {
        XSLT_SECPREF_READ_NETWORK
    } else {
        XSLT_SECPREF_READ_FILE
    };
    let check = crate::xslt::security::xsltGetSecurityPrefs(sec, option);
    if let Some(check_fn) = check {
        let ret = check_fn(sec, ctxt as *mut c_void, url as *const c_char);
        if ret == 0 {
            if is_network {
                report_error(ptr::null_mut(), ptr::null_mut(), b"Network access for ");
            } else {
                report_error(ptr::null_mut(), ptr::null_mut(), b"Local file read for ");
            }
            report_error(ptr::null_mut(), ptr::null_mut(), cbytes(url as *const u8));
            report_error(ptr::null_mut(), ptr::null_mut(), b" refused\n");
            return 0;
        }
        return ret;
    }
    1
}

/// `xsltDocDefaultLoader` (documents.c) for the candidate engine: if a
/// global loader function is registered it is invoked (the returned
/// parser input cannot be fed into a document by this engine and is
/// freed, matching `src/xslt/documents` `load_via_loader`); otherwise the
/// URI is parsed as a file. Returns a parsed document or NULL.
unsafe fn xslt_doc_default_loader(
    uri: *const xmlChar,
    _dict: *mut c_void,
    options: c_int,
    ctxt: *mut c_void,
    _type: c_int,
) -> *mut _xmlDoc {
    let loader = crate::xslt::documents::xsltGetLoaderFunc();
    if let Some(loader_fn) = loader {
        let input = loader_fn(
            ctxt,
            ptr::null(), // base URL
            uri as *const c_char,
            ptr::null(), // ns
            0,           // secondary
        );
        if !input.is_null() {
            crate::xml::parser::helpers::free_parser_input(input);
        }
    }
    xmlReadFile(uri as *const c_char, ptr::null(), options)
}

/// `xsltNewDecimalFormat` (xslt.c): create a decimal format with the
/// default values. `name`/`nsUri` are borrowed (not owned).
unsafe fn xslt_new_decimal_format(
    nsUri: *const xmlChar,
    name: *mut xmlChar,
) -> *mut _xsltDecimalFormat {
    let self_ = xmlMalloc(core::mem::size_of::<_xsltDecimalFormat>()) as *mut _xsltDecimalFormat;
    if !self_.is_null() {
        ptr::write_bytes(
            self_ as *mut u8,
            0,
            core::mem::size_of::<_xsltDecimalFormat>(),
        );
        (*self_).nsUri = nsUri;
        (*self_).name = name;
        // Default values (xslt.c, UTF-8 for U+2030 PER MILLE SIGN).
        (*self_).digit = xmlStrdup(b"#\0".as_ptr() as *const xmlChar);
        (*self_).patternSeparator = xmlStrdup(b";\0".as_ptr() as *const xmlChar);
        (*self_).decimalPoint = xmlStrdup(b".\0".as_ptr() as *const xmlChar);
        (*self_).grouping = xmlStrdup(b",\0".as_ptr() as *const xmlChar);
        (*self_).percent = xmlStrdup(b"%\0".as_ptr() as *const xmlChar);
        (*self_).permille = xmlStrdup("\u{2030}\0".as_ptr() as *const xmlChar);
        (*self_).zeroDigit = xmlStrdup(b"0\0".as_ptr() as *const xmlChar);
        (*self_).minusSign = xmlStrdup(b"-\0".as_ptr() as *const xmlChar);
        (*self_).infinity = xmlStrdup(b"Infinity\0".as_ptr() as *const xmlChar);
        (*self_).noNumber = xmlStrdup(b"NaN\0".as_ptr() as *const xmlChar);
    }
    self_
}

/// `xsltNewStylesheetInternal` (xslt.c).
unsafe fn xslt_new_stylesheet_internal(parent: *mut _xsltStylesheet) -> *mut _xsltStylesheet {
    let ret = xmlMalloc(core::mem::size_of::<_xsltStylesheet>()) as *mut _xsltStylesheet;
    if ret.is_null() {
        report_error(
            ptr::null_mut(),
            ptr::null_mut(),
            b"xsltNewStylesheet : malloc failed\n",
        );
        return ptr::null_mut();
    }
    ptr::write_bytes(ret as *mut u8, 0, core::mem::size_of::<_xsltStylesheet>());

    (*ret).parent = parent;
    (*ret).omitXmlDeclaration = -1;
    (*ret).standalone = -1;
    (*ret).decimalFormat = xslt_new_decimal_format(ptr::null(), ptr::null_mut());
    (*ret).indent = -1;
    (*ret).errors = 0;
    (*ret).warnings = 0;
    (*ret).exclPrefixNr = 0;
    (*ret).exclPrefixMax = 0;
    (*ret).exclPrefixTab = ptr::null_mut();
    (*ret).extInfos = ptr::null_mut();
    (*ret).extrasNr = 0;
    (*ret).internalized = 1;
    (*ret).literal_result = 0;
    (*ret).forwards_compatible = 0;
    (*ret).dict = xmlDictCreate();

    if parent.is_null() {
        (*ret).principal = ret;
        (*ret).xpathCtxt = xmlXPathNewContext(ptr::null_mut());
        if (*ret).xpathCtxt.is_null() {
            report_error(
                ptr::null_mut(),
                ptr::null_mut(),
                b"xsltNewStylesheet: xmlXPathNewContext failed\n",
            );
            crate::xslt::stylesheet::xsltFreeStylesheet(ret);
            return ptr::null_mut();
        }
        if crate::xml::xpath::exports::xmlXPathContextSetCache((*ret).xpathCtxt, 1, -1, 0) == -1 {
            crate::xslt::stylesheet::xsltFreeStylesheet(ret);
            return ptr::null_mut();
        }
    } else {
        (*ret).principal = (*parent).principal;
    }

    // Upstream calls xsltInit() (registers built-in extras, sets the
    // initialized flag). The candidate has no built-in extras; xsltInit
    // only marks the library initialized.
    crate::abi::exports_xslt::xsltInit();

    ret
}

// ═══════════════════════════════════════════════════════════════════════════════
// 1. Stylesheet creation & parsing (xslt.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Create a new XSLT stylesheet.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltStylesheetPtr
/// xsltNewStylesheet(void) {
///     return xsltNewStylesheetInternal(NULL);
/// }
/// ```
///
/// See `xsltNewStylesheetInternal` (xslt.c 1.1.45): xmlMalloc + memset, a
/// default decimal format, `dict = xmlDictCreate()`, `internalized = 1`,
/// and for the principal stylesheet an XPath context with its cache
/// enabled. `version`/`method`/`encoding` are left NULL (they are set
/// later by `xsltParseStylesheetOutput`/version processing).
///
/// # SAFETY
///
/// The caller owns the returned stylesheet and must free it with
/// `xsltFreeStylesheet`.
#[no_mangle]
pub unsafe extern "C" fn xsltNewStylesheet() -> *mut _xsltStylesheet {
    xslt_new_stylesheet_internal(ptr::null_mut())
}

/// Parse an XSLT stylesheet, adding the associated structures.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltStylesheetPtr
/// xsltParseStylesheetProcess(xsltStylesheetPtr ret, xmlDocPtr doc) {
///     xsltInitGlobals();
///     if (doc == NULL) return(NULL);
///     if (ret == NULL) return(ret);
///     cur = xmlDocGetRootElement(doc);
///     if (cur == NULL) { ... "empty stylesheet" ... return(NULL); }
///     ...
/// }
/// ```
///
/// # ENGINE-WIRING
///
/// The heavy lifting (tree preprocessing, top-level compilation, or the
/// simplified-stylesheet implicit template) is performed by the engine's
/// `crate::xslt::compiler::compile`, which returns 0 on success.
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`, or NULL.
/// - `doc` must be a valid parsed document, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltParseStylesheetProcess(
    style: *mut _xsltStylesheet,
    doc: *mut _xmlDoc,
) -> *mut _xsltStylesheet {
    xsltInitGlobals();

    if doc.is_null() {
        return ptr::null_mut();
    }
    if style.is_null() {
        return style;
    }

    let root = crate::xml::tree::doc_get_root_element(doc);
    if root.is_null() {
        report_error(
            style,
            doc as *mut _xmlNode,
            b"xsltParseStylesheetProcess : empty stylesheet\n",
        );
        return ptr::null_mut();
    }

    let ret = crate::xslt::compiler::compile(style, doc);
    if ret != 0 {
        return ptr::null_mut();
    }
    style
}

/// Parse an XSLT stylesheet with a user-provided stylesheet struct.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int
/// xsltParseStylesheetUser(xsltStylesheetPtr style, xmlDocPtr doc) {
///     if ((style == NULL) || (doc == NULL)) return(-1);
///     if (doc->dict != NULL) {
///         xmlDictFree(style->dict);
///         style->dict = doc->dict;
///         xmlDictReference(style->dict);
///     }
///     xsltGatherNamespaces(style);
///     style->doc = doc;
///     if (xsltParseStylesheetProcess(style, doc) == NULL) {
///         style->doc = NULL;
///         return(-1);
///     }
///     if (style->parent == NULL)
///         xsltResolveStylesheetAttributeSet(style);
///     if (style->errors != 0) {
///         style->doc = NULL;
///         ... cleanup ...
///         return(-1);
///     }
///     return(0);
/// }
/// ```
///
/// # ENGINE-WIRING
///
/// `xsltGatherNamespaces` (namespaces.c) builds `style->nsHash` for the
/// upstream engine; the candidate resolves namespaces at runtime from the
/// node tree, so the call has no candidate equivalent (documented
/// divergence — `nsHash` is unused by the engine).
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`, or NULL.
/// - `doc` must be a valid parsed document, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltParseStylesheetUser(
    style: *mut _xsltStylesheet,
    doc: *mut _xmlDoc,
) -> c_int {
    if style.is_null() || doc.is_null() {
        return -1;
    }

    // Adjust the string dict (xslt.c 1.1.45).
    if !(*doc).dict.is_null() {
        xmlDictFree((*style).dict);
        (*style).dict = (*doc).dict;
        xmlDictReference((*style).dict);
    }

    // xsltGatherNamespaces(style) — no-op in the candidate engine, see
    // module docs.

    (*style).doc = doc;
    if xsltParseStylesheetProcess(style, doc).is_null() {
        (*style).doc = ptr::null_mut();
        return -1;
    }

    if (*style).parent.is_null() {
        crate::abi::exports_xslt_apply::xsltResolveStylesheetAttributeSet(style);
    }

    if (*style).errors != 0 {
        // Detach the doc from the stylesheet; otherwise the doc would be
        // freed by xsltFreeStylesheet(). The caller keeps ownership.
        (*style).doc = ptr::null_mut();
        return -1;
    }

    0
}

/// Parse an XSLT stylesheet from a document, with a parent stylesheet
/// context (used for `xsl:import`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltStylesheetPtr
/// xsltParseStylesheetImportedDoc(xmlDocPtr doc,
///                                xsltStylesheetPtr parentStyle) {
///     if (doc == NULL) return(NULL);
///     retStyle = xsltNewStylesheetInternal(parentStyle);
///     if (retStyle == NULL) return(NULL);
///     if (xsltParseStylesheetUser(retStyle, doc) != 0) {
///         xsltFreeStylesheet(retStyle);
///         return(NULL);
///     }
///     return(retStyle);
/// }
/// ```
///
/// # SAFETY
///
/// - `doc` must be a valid parsed document, or NULL. On failure the
///   document is detached from the stylesheet and remains owned by the
///   caller.
#[no_mangle]
pub unsafe extern "C" fn xsltParseStylesheetImportedDoc(
    doc: *mut _xmlDoc,
    parentStyle: *mut _xsltStylesheet,
) -> *mut _xsltStylesheet {
    if doc.is_null() {
        return ptr::null_mut();
    }

    let retStyle = xslt_new_stylesheet_internal(parentStyle);
    if retStyle.is_null() {
        return ptr::null_mut();
    }

    if xsltParseStylesheetUser(retStyle, doc) != 0 {
        crate::xslt::stylesheet::xsltFreeStylesheet(retStyle);
        return ptr::null_mut();
    }

    retStyle
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. Imports & includes (imports.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// `xsltFixImportedCompSteps` (imports.c): normalize the compiled steps of
/// an imported stylesheet against the master's extra slots.
///
/// # ENGINE-WIRING
///
/// Upstream scans the imported templates hash with `xsltNormalizeCompSteps`
/// (which re-bases step extra indices). The candidate's compiled patterns
/// carry no step-extra state (`xsltNormalizeCompSteps` is a no-op), so
/// only the `extrasNr` accumulation is observable.
unsafe fn xslt_fix_imported_comp_steps(master: *mut _xsltStylesheet, style: *mut _xsltStylesheet) {
    (*master).extrasNr += (*style).extrasNr;
    let mut res = (*style).imports;
    while !res.is_null() {
        xslt_fix_imported_comp_steps(master, res);
        res = (*res).next;
    }
}

/// `xsltCheckCycle` (imports.c): detect import/include recursion.
unsafe fn xslt_check_cycle(
    style: *mut _xsltStylesheet,
    cur: *mut _xmlNode,
    uri: *const xmlChar,
) -> c_int {
    let mut depth: c_int = 0;
    let mut ancestor = style;
    while !ancestor.is_null() {
        depth += 1;
        if depth >= XSLT_MAX_NESTING {
            report_error(style, cur, b"maximum nesting depth exceeded: ");
            report_error(style, cur, cbytes(uri as *const u8));
            report_error(style, cur, b"\n");
            return -1;
        }
        if !(*ancestor).doc.is_null()
            && !(*(*ancestor).doc).URL.is_null()
            && xmlStrEqual((*(*ancestor).doc).URL, uri) != 0
        {
            report_error(style, cur, b"recursion detected on imported URL ");
            report_error(style, cur, cbytes(uri as *const u8));
            report_error(style, cur, b"\n");
            return -1;
        }

        // Check included stylesheets.
        let mut docptr = (*ancestor).includes;
        while !docptr.is_null() {
            depth += 1;
            if depth >= XSLT_MAX_NESTING {
                report_error(style, cur, b"maximum nesting depth exceeded: ");
                report_error(style, cur, cbytes(uri as *const u8));
                report_error(style, cur, b"\n");
                return -1;
            }
            if !(*docptr).doc.is_null()
                && !(*(*docptr).doc).URL.is_null()
                && xmlStrEqual((*(*docptr).doc).URL, uri) != 0
            {
                report_error(style, cur, b"recursion detected on included URL ");
                report_error(style, cur, cbytes(uri as *const u8));
                report_error(style, cur, b"\n");
                return -1;
            }
            docptr = (*docptr).includes;
        }

        ancestor = (*ancestor).parent;
    }

    0
}

/// Parse an XSLT stylesheet import element.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int
/// xsltParseStylesheetImport(xsltStylesheetPtr style, xmlNodePtr cur) {
///     ... href/base/URI resolution, cycle + security checks,
///         xsltDocDefaultLoader(...), xsltParseStylesheetImportedDoc(),
///         res->next = style->imports; style->imports = res;
///         xsltFixImportedCompSteps(style, res) when style->parent == NULL
/// }
/// ```
///
/// Returns 0 on success, -1 on failure.
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`, or NULL.
/// - `cur` must be a valid `xsl:import` element node, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltParseStylesheetImport(
    style: *mut _xsltStylesheet,
    cur: *mut _xmlNode,
) -> c_int {
    let mut ret: c_int = -1;
    let mut uriRef: *mut xmlChar = ptr::null_mut();
    let mut base: *mut xmlChar = ptr::null_mut();
    let mut uri: *mut xmlChar = ptr::null_mut();

    if cur.is_null() || style.is_null() {
        return ret;
    }

    uriRef = xmlGetNsProp(cur, b"href\0".as_ptr() as *const xmlChar, ptr::null());
    if uriRef.is_null() {
        report_error(style, cur, b"xsl:import : missing href attribute\n");
        if !uriRef.is_null() {
            xmlFree(uriRef as *mut c_void);
        }
        if !base.is_null() {
            xmlFree(base as *mut c_void);
        }
        if !uri.is_null() {
            xmlFree(uri as *mut c_void);
        }
        return ret;
    }

    base = xmlNodeGetBase((*style).doc, cur);
    uri = xmlBuildURI(uriRef as *const c_char, base as *const c_char);
    if uri.is_null() {
        report_error(style, cur, b"xsl:import : invalid URI reference ");
        report_error(style, cur, cbytes(uriRef as *const u8));
        report_error(style, cur, b"\n");
        if !uriRef.is_null() {
            xmlFree(uriRef as *mut c_void);
        }
        if !base.is_null() {
            xmlFree(base as *mut c_void);
        }
        if !uri.is_null() {
            xmlFree(uri as *mut c_void);
        }
        return ret;
    }

    if xslt_check_cycle(style, cur, uri) < 0 {
        if !uriRef.is_null() {
            xmlFree(uriRef as *mut c_void);
        }
        if !base.is_null() {
            xmlFree(base as *mut c_void);
        }
        if !uri.is_null() {
            xmlFree(uri as *mut c_void);
        }
        return ret;
    }

    // Security framework check.
    let sec = crate::xslt::security::xsltGetDefaultSecurityPrefs();
    if !sec.is_null() {
        let secres = xslt_check_read(sec, ptr::null_mut(), uri);
        if secres <= 0 {
            if secres == 0 {
                report_error(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    b"xsl:import: read rights for ",
                );
                report_error(ptr::null_mut(), ptr::null_mut(), cbytes(uri as *const u8));
                report_error(ptr::null_mut(), ptr::null_mut(), b" denied\n");
            }
            if !uriRef.is_null() {
                xmlFree(uriRef as *mut c_void);
            }
            if !base.is_null() {
                xmlFree(base as *mut c_void);
            }
            if !uri.is_null() {
                xmlFree(uri as *mut c_void);
            }
            return ret;
        }
    }

    let import = xslt_doc_default_loader(
        uri,
        (*style).dict,
        XSLT_PARSE_OPTIONS,
        style as *mut c_void,
        XSLT_LOAD_STYLESHEET,
    );
    if import.is_null() {
        report_error(style, cur, b"xsl:import : unable to load ");
        report_error(style, cur, cbytes(uri as *const u8));
        report_error(style, cur, b"\n");
        if !uriRef.is_null() {
            xmlFree(uriRef as *mut c_void);
        }
        if !base.is_null() {
            xmlFree(base as *mut c_void);
        }
        if !uri.is_null() {
            xmlFree(uri as *mut c_void);
        }
        return ret;
    }

    let res = xsltParseStylesheetImportedDoc(import, style);
    if !res.is_null() {
        (*res).next = (*style).imports;
        (*style).imports = res;
        if (*style).parent.is_null() {
            xslt_fix_imported_comp_steps(style, res);
        }
        ret = 0;
    } else {
        crate::xml::tree::free_doc(import);
    }

    if !uriRef.is_null() {
        xmlFree(uriRef as *mut c_void);
    }
    if !base.is_null() {
        xmlFree(base as *mut c_void);
    }
    if !uri.is_null() {
        xmlFree(uri as *mut c_void);
    }

    ret
}

/// Parse an XSLT stylesheet include element.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int
/// xsltParseStylesheetInclude(xsltStylesheetPtr style, xmlNodePtr cur) {
///     ... href/base/URI resolution, cycle check,
///         include = xsltLoadStyleDocument(style, URI);
///         oldDoc = style->doc; style->doc = include->doc;
///         include->includes = style->includes; style->includes = include;
///         oldNopreproc = style->nopreproc;
///         style->nopreproc = include->preproc;
///         result = xsltParseStylesheetProcess(style, include->doc);
///         style->nopreproc = oldNopreproc;
///         include->preproc = 1;
///         style->includes = include->includes;
///         style->doc = oldDoc;
///         if (result == NULL) { ret = -1; goto error; }
///         ret = 0;
/// }
/// ```
///
/// Returns 0 on success, -1 on failure.
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`, or NULL.
/// - `cur` must be a valid `xsl:include` element node, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltParseStylesheetInclude(
    style: *mut _xsltStylesheet,
    cur: *mut _xmlNode,
) -> c_int {
    let mut ret: c_int = -1;
    let mut uriRef: *mut xmlChar = ptr::null_mut();
    let mut base: *mut xmlChar = ptr::null_mut();
    let mut uri: *mut xmlChar = ptr::null_mut();

    if cur.is_null() || style.is_null() {
        return ret;
    }

    uriRef = xmlGetNsProp(cur, b"href\0".as_ptr() as *const xmlChar, ptr::null());
    if uriRef.is_null() {
        report_error(style, cur, b"xsl:include : missing href attribute\n");
        if !uriRef.is_null() {
            xmlFree(uriRef as *mut c_void);
        }
        if !base.is_null() {
            xmlFree(base as *mut c_void);
        }
        if !uri.is_null() {
            xmlFree(uri as *mut c_void);
        }
        return ret;
    }

    base = xmlNodeGetBase((*style).doc, cur);
    uri = xmlBuildURI(uriRef as *const c_char, base as *const c_char);
    if uri.is_null() {
        report_error(style, cur, b"xsl:include : invalid URI reference ");
        report_error(style, cur, cbytes(uriRef as *const u8));
        report_error(style, cur, b"\n");
        if !uriRef.is_null() {
            xmlFree(uriRef as *mut c_void);
        }
        if !base.is_null() {
            xmlFree(base as *mut c_void);
        }
        if !uri.is_null() {
            xmlFree(uri as *mut c_void);
        }
        return ret;
    }

    if xslt_check_cycle(style, cur, uri) < 0 {
        if !uriRef.is_null() {
            xmlFree(uriRef as *mut c_void);
        }
        if !base.is_null() {
            xmlFree(base as *mut c_void);
        }
        if !uri.is_null() {
            xmlFree(uri as *mut c_void);
        }
        return ret;
    }

    let include = xsltLoadStyleDocument(style, uri);
    if include.is_null() {
        report_error(style, cur, b"xsl:include : unable to load ");
        report_error(style, cur, cbytes(uri as *const u8));
        report_error(style, cur, b"\n");
        if !uriRef.is_null() {
            xmlFree(uriRef as *mut c_void);
        }
        if !base.is_null() {
            xmlFree(base as *mut c_void);
        }
        if !uri.is_null() {
            xmlFree(uri as *mut c_void);
        }
        return ret;
    }

    let oldDoc = (*style).doc;
    (*style).doc = (*include).doc;
    // Chain to the stylesheet for recursion checking.
    (*include).includes = (*style).includes;
    (*style).includes = include;
    let oldNopreproc = (*style).nopreproc;
    (*style).nopreproc = (*include).preproc;
    // ENGINE-WIRING: upstream skips the whole-tree preprocessing when the
    // include was already preprocessed (`include->preproc`); the candidate
    // compiler's preprocessing is idempotent (blank-stripping and text
    // merging), so re-running it is safe. The `nopreproc` flag is restored
    // exactly like upstream.
    let result = xsltParseStylesheetProcess(style, (*include).doc);
    (*style).nopreproc = oldNopreproc;
    (*include).preproc = 1;
    (*style).includes = (*include).includes;
    (*style).doc = oldDoc;
    if result.is_null() {
        ret = -1;
        if !uriRef.is_null() {
            xmlFree(uriRef as *mut c_void);
        }
        if !base.is_null() {
            xmlFree(base as *mut c_void);
        }
        if !uri.is_null() {
            xmlFree(uri as *mut c_void);
        }
        return ret;
    }
    ret = 0;

    if !uriRef.is_null() {
        xmlFree(uriRef as *mut c_void);
    }
    if !base.is_null() {
        xmlFree(base as *mut c_void);
    }
    if !uri.is_null() {
        xmlFree(uri as *mut c_void);
    }
    return ret;
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. Top-level constructs (xslt.c, attributes.c, variables.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// `xsltParseContentError` (xslt.c): report a misplaced child node.
unsafe fn xslt_parse_content_error(style: *mut _xsltStylesheet, node: *mut _xmlNode) {
    if style.is_null() || node.is_null() {
        return;
    }
    if is_xslt_elem(node) {
        report_error(
            style,
            node,
            b"The XSLT-element is not allowed at this position.\n",
        );
    } else {
        report_error(
            style,
            node,
            b"The element is not allowed at this position.\n",
        );
    }
    (*style).errors += 1;
}

/// Parse an XSLT stylesheet output element and record the output settings.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltParseStylesheetOutput(xsltStylesheetPtr style, xmlNodePtr cur);
/// ```
///
/// Ported from xslt.c 1.1.45: version/encoding/method (with QName
/// resolution via `xsltGetQNameURI`), doctype-system/public, standalone,
/// indent, omit-xml-declaration, cdata-section-elements (a
/// `{name, ns-URI}` hash holding the sentinel "cdata"), media-type, and
/// the content-error check for children. Invalid enum values bump
/// `style->errors`; an invalid method bumps `style->warnings`.
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`, or NULL.
/// - `cur` must be a valid `xsl:output` element node, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltParseStylesheetOutput(
    style: *mut _xsltStylesheet,
    cur: *mut _xmlNode,
) {
    if cur.is_null() || style.is_null() || (*cur).type_ != XML_ELEMENT_NODE as c_int {
        return;
    }

    // version
    let mut prop = xmlGetNsProp(cur, b"version\0".as_ptr() as *const xmlChar, ptr::null());
    if !prop.is_null() {
        if !(*style).version.is_null() {
            xmlFree((*style).version as *mut c_void);
        }
        (*style).version = prop;
        prop = ptr::null_mut();
    }

    // encoding
    prop = xmlGetNsProp(cur, b"encoding\0".as_ptr() as *const xmlChar, ptr::null());
    if !prop.is_null() {
        if !(*style).encoding.is_null() {
            xmlFree((*style).encoding as *mut c_void);
        }
        (*style).encoding = prop;
        prop = ptr::null_mut();
    }

    // method (relaxed to support xt:document)
    prop = xmlGetNsProp(cur, b"method\0".as_ptr() as *const xmlChar, ptr::null());
    if !prop.is_null() {
        if !(*style).method.is_null() {
            xmlFree((*style).method as *mut c_void);
        }
        (*style).method = ptr::null_mut();
        if !(*style).methodURI.is_null() {
            xmlFree((*style).methodURI as *mut c_void);
        }
        (*style).methodURI = ptr::null_mut();

        let mut method = prop;
        let uri = crate::abi::exports_xslt_avt::xsltGetQNameURI(cur, &mut method);
        if method.is_null() {
            if !style.is_null() {
                (*style).errors += 1;
            }
        } else if uri.is_null() {
            if xmlStrEqual(method, b"xml\0".as_ptr() as *const xmlChar) != 0
                || xmlStrEqual(method, b"html\0".as_ptr() as *const xmlChar) != 0
                || xmlStrEqual(method, b"text\0".as_ptr() as *const xmlChar) != 0
            {
                (*style).method = method;
            } else {
                report_error(style, cur, b"invalid value for method: ");
                report_error(style, cur, cbytes(method as *const u8));
                report_error(style, cur, b"\n");
                if !style.is_null() {
                    (*style).warnings += 1;
                }
                xmlFree(method as *mut c_void);
            }
        } else {
            (*style).method = method;
            (*style).methodURI = xmlStrdup(uri);
        }
        prop = ptr::null_mut();
    }

    // doctype-system
    prop = xmlGetNsProp(
        cur,
        b"doctype-system\0".as_ptr() as *const xmlChar,
        ptr::null(),
    );
    if !prop.is_null() {
        if !(*style).doctypeSystem.is_null() {
            xmlFree((*style).doctypeSystem as *mut c_void);
        }
        (*style).doctypeSystem = prop;
        prop = ptr::null_mut();
    }

    // doctype-public
    prop = xmlGetNsProp(
        cur,
        b"doctype-public\0".as_ptr() as *const xmlChar,
        ptr::null(),
    );
    if !prop.is_null() {
        if !(*style).doctypePublic.is_null() {
            xmlFree((*style).doctypePublic as *mut c_void);
        }
        (*style).doctypePublic = prop;
        prop = ptr::null_mut();
    }

    // standalone
    prop = xmlGetNsProp(cur, b"standalone\0".as_ptr() as *const xmlChar, ptr::null());
    if !prop.is_null() {
        if xmlStrEqual(prop, b"yes\0".as_ptr() as *const xmlChar) != 0 {
            (*style).standalone = 1;
        } else if xmlStrEqual(prop, b"no\0".as_ptr() as *const xmlChar) != 0 {
            (*style).standalone = 0;
        } else {
            report_error(style, cur, b"invalid value for standalone\n");
            (*style).errors += 1;
        }
        xmlFree(prop as *mut c_void);
    }

    // indent
    prop = xmlGetNsProp(cur, b"indent\0".as_ptr() as *const xmlChar, ptr::null());
    if !prop.is_null() {
        if xmlStrEqual(prop, b"yes\0".as_ptr() as *const xmlChar) != 0 {
            (*style).indent = 1;
        } else if xmlStrEqual(prop, b"no\0".as_ptr() as *const xmlChar) != 0 {
            (*style).indent = 0;
        } else {
            report_error(style, cur, b"invalid value for indent\n");
            (*style).errors += 1;
        }
        xmlFree(prop as *mut c_void);
    }

    // omit-xml-declaration
    prop = xmlGetNsProp(
        cur,
        b"omit-xml-declaration\0".as_ptr() as *const xmlChar,
        ptr::null(),
    );
    if !prop.is_null() {
        if xmlStrEqual(prop, b"yes\0".as_ptr() as *const xmlChar) != 0 {
            (*style).omitXmlDeclaration = 1;
        } else if xmlStrEqual(prop, b"no\0".as_ptr() as *const xmlChar) != 0 {
            (*style).omitXmlDeclaration = 0;
        } else {
            report_error(style, cur, b"invalid value for omit-xml-declaration\n");
            (*style).errors += 1;
        }
        xmlFree(prop as *mut c_void);
    }

    // cdata-section-elements
    let elements = xmlGetNsProp(
        cur,
        b"cdata-section-elements\0".as_ptr() as *const xmlChar,
        ptr::null(),
    );
    if !elements.is_null() {
        if (*style).cdataSection.is_null() {
            (*style).cdataSection = crate::xml::hash::hash_create(10) as *mut c_void;
        }
        if (*style).cdataSection.is_null() {
            xmlFree(elements as *mut c_void);
            return;
        }

        let mut element: *mut xmlChar = elements;
        while *element != 0 {
            while matches!(*element, b' ' | b'\t' | b'\n' | b'\r') {
                element = element.add(1);
            }
            if *element == 0 {
                break;
            }
            let mut end = element;
            while *end != 0 && !matches!(*end, b' ' | b'\t' | b'\n' | b'\r') {
                end = end.add(1);
            }
            let len = end.offset_from(element) as usize;
            let token = xmlMalloc(len + 1) as *mut xmlChar;
            if !token.is_null() {
                core::ptr::copy_nonoverlapping(element, token, len);
                *token.add(len) = 0;
                if xmlValidateQName(token, 0) != 0 {
                    report_error(
                        style,
                        cur,
                        b"Attribute 'cdata-section-elements': The value is not a valid QName.\n",
                    );
                    xmlFree(token as *mut c_void);
                    (*style).errors += 1;
                } else {
                    let mut qname = token;
                    let quri = crate::abi::exports_xslt_avt::xsltGetQNameURI(cur, &mut qname);
                    if qname.is_null() {
                        report_error(
                            style,
                            cur,
                            b"Attribute 'cdata-section-elements': Not a valid QName.\n",
                        );
                        (*style).errors += 1;
                    } else {
                        let mut uri = quri;
                        // XSLT-1.0: QNames without a prefix use the default
                        // namespace in effect on xsl:output (bug #339570).
                        if uri.is_null() {
                            let ns = xmlSearchNs((*style).doc, cur, ptr::null());
                            if !ns.is_null() {
                                uri = (*ns).href;
                            }
                        }
                        crate::xml::hash::hash_add_entry2(
                            (*style).cdataSection as *mut crate::xml::hash::HashTable,
                            qname,
                            uri,
                            b"cdata\0".as_ptr() as *const c_void as *mut c_void,
                        );
                        xmlFree(qname as *mut c_void);
                    }
                }
            }
            element = end;
        }
        xmlFree(elements as *mut c_void);
    }

    // media-type
    prop = xmlGetNsProp(cur, b"media-type\0".as_ptr() as *const xmlChar, ptr::null());
    if !prop.is_null() {
        if !(*style).mediaType.is_null() {
            xmlFree((*style).mediaType as *mut c_void);
        }
        (*style).mediaType = prop;
        prop = ptr::null_mut();
    }

    // Content of xsl:output must be empty (upstream checks the first
    // child only).
    if !(*cur).children.is_null() {
        xslt_parse_content_error(style, (*cur).children);
    }
}

/// Parse an XSLT stylesheet attribute-set element.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltParseStylesheetAttributeSet(xsltStylesheetPtr style, xmlNodePtr cur);
/// ```
///
/// # ENGINE-WIRING
///
/// Wired to `crate::xslt::attributes::xsltCompileAttrSet`, which records
/// the set (name/instruction/stylesheet) on `style->attributeSets`. The
/// upstream QName validation and `use-attribute-sets` processing are
/// subsumed: the engine resolves referenced sets by name at apply time
/// (`xsltApplyAttrSets`). The QName check is kept for parity.
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`, or NULL.
/// - `cur` must be a valid `xsl:attribute-set` element node, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltParseStylesheetAttributeSet(
    style: *mut _xsltStylesheet,
    cur: *mut _xmlNode,
) {
    if cur.is_null() || style.is_null() || (*cur).type_ != XML_ELEMENT_NODE as c_int {
        return;
    }

    let value = xmlGetNsProp(cur, b"name\0".as_ptr() as *const xmlChar, ptr::null());
    if value.is_null() || *value == 0 {
        if !value.is_null() {
            xmlFree(value as *mut c_void);
        }
        return;
    }
    if xmlValidateQName(value, 0) != 0 {
        report_error(
            style,
            cur,
            b"xsl:attribute-set : The name is not a valid QName.\n",
        );
        (*style).errors += 1;
        xmlFree(value as *mut c_void);
        return;
    }
    xmlFree(value as *mut c_void);

    crate::xslt::attributes::xsltCompileAttrSet(style, cur);
}

/// Parse a global XSLT `variable` declaration at compilation time and
/// register it.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltParseGlobalVariable(xsltStylesheetPtr style, xmlNodePtr cur);
/// ```
///
/// # ENGINE-WIRING
///
/// Wired to the engine's `crate::xslt::compiler::compile_variable`
/// (is_param = 0), which allocates the `_xsltStackElem`, copies
/// name/select, records the content tree and prepends it to
/// `style->variables`. The upstream "missing name" and "redefinition of
/// global variable" diagnostics are reproduced here (the redefinition
/// check compares the local name only — the candidate stores no nameURI
/// for globals, documented divergence).
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`, or NULL.
/// - `cur` must be a valid `xsl:variable` element node, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltParseGlobalVariable(style: *mut _xsltStylesheet, cur: *mut _xmlNode) {
    if cur.is_null() || style.is_null() || (*cur).type_ != XML_ELEMENT_NODE as c_int {
        return;
    }

    let name = xmlGetNsProp(cur, b"name\0".as_ptr() as *const xmlChar, ptr::null());
    if name.is_null() {
        report_error(style, cur, b"xsl:variable : missing name attribute\n");
        return;
    }

    // Upstream reports a redefinition error for duplicate global
    // variables (not params).
    let mut tmp = (*style).variables;
    while !tmp.is_null() {
        if ((*tmp).flags & XSLT_VAR_PARAM) == 0
            && !(*tmp).name.is_null()
            && xmlStrEqual((*tmp).name, name) != 0
        {
            report_error(style, cur, b"redefinition of global variable ");
            report_error(style, cur, cbytes(name as *const u8));
            report_error(style, cur, b"\n");
            (*style).errors += 1;
            break;
        }
        tmp = (*tmp).next;
    }
    xmlFree(name as *mut c_void);

    // Parse the content (a sequence constructor).
    if !(*cur).children.is_null() {
        xsltParseTemplateContent(style, cur);
    }

    crate::xslt::compiler::compile_variable(style, cur, 0, 0);
}

/// Parse a global XSLT `param` declaration at compilation time and
/// register it.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltParseGlobalParam(xsltStylesheetPtr style, xmlNodePtr cur);
/// ```
///
/// # ENGINE-WIRING
///
/// Same as `xsltParseGlobalVariable` with is_param = 1 (the engine marks
/// the stack element with the `XSLT_VAR_PARAM` flag).
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`, or NULL.
/// - `cur` must be a valid `xsl:param` element node, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltParseGlobalParam(style: *mut _xsltStylesheet, cur: *mut _xmlNode) {
    if cur.is_null() || style.is_null() || (*cur).type_ != XML_ELEMENT_NODE as c_int {
        return;
    }

    let name = xmlGetNsProp(cur, b"name\0".as_ptr() as *const xmlChar, ptr::null());
    if name.is_null() {
        report_error(style, cur, b"xsl:param : missing name attribute\n");
        return;
    }
    xmlFree(name as *mut c_void);

    // Parse the content (a sequence constructor).
    if !(*cur).children.is_null() {
        xsltParseTemplateContent(style, cur);
    }

    crate::xslt::compiler::compile_variable(style, cur, 0, 1);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. Template content & attribute compilation (xslt.c, attrvt.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Parse a template content-model: precompute each XSLT instruction and
/// the AVTs of literal result elements.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltParseTemplateContent(xsltStylesheetPtr style, xmlNodePtr templ);
/// ```
///
/// Ported from xslt.c 1.1.45 (old behaviour): walk the subtree, run
/// `xsltStylePreCompute` on XSLT and extension elements, `xsltCompileAttr`
/// on literal-result-element attributes, and remove misplaced `xsl:param`
/// elements (with a warning).
///
/// # ENGINE-WIRING
///
/// Upstream *replaces* `xsl:text` with its children during this pass and
/// deletes the instruction node. The candidate engine evaluates `xsl:text`
/// directly at runtime (`xsltProcessInstruction` → `process_text`), so the
/// unwrap/delete is intentionally skipped — the tree stays intact
/// (documented divergence; observable output is identical).
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`, or NULL.
/// - `templ` must be a valid node whose children form the content, or
///   NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltParseTemplateContent(
    style: *mut _xsltStylesheet,
    templ: *mut _xmlNode,
) {
    if style.is_null() || templ.is_null() || (*templ).type_ == XML_NAMESPACE_DECL as c_int {
        return;
    }

    let mut cur = (*templ).children;
    while !cur.is_null() {
        if !(*style).principal.is_null() {
            (*(*style).principal).opCount += 1;
        }

        if is_xslt_elem(cur) {
            xsltStylePreCompute(style, cur);
            // xsl:text is evaluated at runtime by the engine; upstream's
            // unwrap + node deletion is not performed (see module docs).
        } else if !(*cur).ns.is_null() && !ext_ns_registered((*(*cur).ns).href) {
            // Not an XSLT element and not a registered extension element:
            // falls through to the literal-result-element branch below.
        } else if !(*cur).ns.is_null() && ext_ns_registered((*(*cur).ns).href) {
            // Extension element: compile it too.
            xsltStylePreCompute(style, cur);
        } else if (*cur).type_ == XML_ELEMENT_NODE as c_int {
            // A literal result element: precompile the AVTs of its
            // attributes.
            if (*cur).ns.is_null() && !(*style).defaultAlias.is_null() {
                (*cur).ns = xmlSearchNsByHref((*cur).doc, cur, (*style).defaultAlias);
            }
            if !(*cur).properties.is_null() {
                let mut attr = (*cur).properties;
                while !attr.is_null() {
                    xsltCompileAttr(style, attr);
                    attr = (*attr).next;
                }
            }
        }

        // Descend into children, else next sibling, else pop up to
        // `templ`.
        if !(*cur).children.is_null() {
            if (*(*cur).children).type_ != XML_ENTITY_DECL as c_int {
                cur = (*cur).children;
                continue;
            }
        }
        if !(*cur).next.is_null() {
            cur = (*cur).next;
            continue;
        }
        loop {
            cur = (*cur).parent;
            if cur.is_null() {
                break;
            }
            if cur == templ {
                cur = ptr::null_mut();
                break;
            }
            if !(*cur).next.is_null() {
                cur = (*cur).next;
                break;
            }
        }
    }

    // Skip the first params.
    let mut cur = (*templ).children;
    while !cur.is_null() {
        if is_xslt_elem(cur) && !is_xslt_name(cur, b"param") {
            break;
        }
        cur = (*cur).next;
    }

    // Browse the remainder of the template, removing misplaced params.
    while !cur.is_null() {
        if is_xslt_elem(cur) && is_xslt_name(cur, b"param") {
            let param = cur;
            report_error(
                style,
                cur,
                b"xsltParseTemplateContent: ignoring misplaced param element\n",
            );
            if !style.is_null() {
                (*style).warnings += 1;
            }
            cur = (*cur).next;
            crate::xml::tree::unlink_node(param);
            crate::xml::tree::free_node(param);
        } else {
            break;
        }
    }
}

/// Whether a namespace URI is registered as an extension namespace (used
/// to distinguish extension elements from literal result elements).
unsafe fn ext_ns_registered(uri: *const xmlChar) -> bool {
    if uri.is_null() {
        return false;
    }
    let mut cur = XSLT_ELEMENTS_REGISTRY;
    while !cur.is_null() {
        if !(*cur).URI.is_null() && xmlStrEqual((*cur).URI, uri) != 0 {
            return true;
        }
        cur = (*cur).next;
    }
    false
}

/// Precompile an attribute in a stylesheet: check whether it is an
/// attribute value template and validate its structure.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltCompileAttr(xsltStylesheetPtr style, xmlAttrPtr attr);
/// ```
///
/// # ENGINE-WIRING
///
/// Upstream parses the AVT into a segment list (`xsltAttrVT`) and stores
/// it in `attr->psvi` / `style->attVTs`. The candidate engine evaluates
/// AVTs lazily at transform time from the raw attribute string
/// (`crate::xslt::transform::eval_avt`), so no AVT object is allocated;
/// the compile-time *diagnostics* are kept for parity: a multi-node or
/// non-text attribute content, and unmatched `{`/`}` (an unmatched `}` is
/// reported without bumping the error counter, exactly like upstream).
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`, or NULL.
/// - `attr` must be a valid attribute of the stylesheet tree, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltCompileAttr(style: *mut _xsltStylesheet, attr: *mut _xmlAttr) {
    if style.is_null() || attr.is_null() || (*attr).children.is_null() {
        return;
    }
    if (*(*attr).children).type_ != XML_TEXT_NODE as c_int || !(*(*attr).children).next.is_null() {
        report_error(
            style,
            (*attr).parent,
            b"Attribute ': The content is expected to be a single text node when compiling an AVT.\n",
        );
        (*style).errors += 1;
        return;
    }

    let str_ = (*(*attr).children).content;
    if xmlStrchr(str_, b'{' as xmlChar).is_null() && xmlStrchr(str_, b'}' as xmlChar).is_null() {
        return;
    }
    if !(*attr).psvi.is_null() {
        // Already compiled.
        return;
    }

    // Validate the AVT structure (no object is built — the engine
    // evaluates the raw string lazily).
    let mut cur = str_;
    while *cur != 0 {
        if *cur == b'{' {
            if !cur.add(1).is_null() && *cur.add(1) == b'{' {
                // Escaped '{'.
                cur = cur.add(2);
                continue;
            }
            if !cur.add(1).is_null() && *cur.add(1) == b'}' {
                // Empty AVT.
                cur = cur.add(2);
                continue;
            }
            // Scan to the closing '}', honouring quoted literals
            // (bug539741).
            let mut p = cur.add(1);
            while *p != 0 && *p != b'}' {
                if *p == b'\'' || *p == b'"' {
                    let delim = *p;
                    p = p.add(1);
                    while *p != 0 && *p != delim {
                        p = p.add(1);
                    }
                    if *p != 0 {
                        p = p.add(1);
                    }
                } else {
                    p = p.add(1);
                }
            }
            if *p == 0 {
                report_error(
                    style,
                    (*attr).parent,
                    b"Attribute ': The AVT has an unmatched '{'.\n",
                );
                (*style).errors += 1;
                return;
            }
            cur = p.add(1);
        } else if *cur == b'}' {
            if !cur.add(1).is_null() && *cur.add(1) == b'}' {
                // Escaped '}'.
                cur = cur.add(2);
                continue;
            }
            report_error(
                style,
                (*attr).parent,
                b"Attribute ': The AVT has an unmatched '}'.\n",
            );
            return;
        } else {
            cur = cur.add(1);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. Precomputed instructions (preproc.c, extensions.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Free a (non-extension) style precomp: upstream also releases the
/// compiled XPath expression, number patterns and ns-list; the candidate
/// engine compiles lazily, so none of those exist and only the struct is
/// freed.
unsafe fn xslt_free_style_pre_comp(comp: *mut c_void) {
    if comp.is_null() {
        return;
    }
    xmlFree(comp);
}

/// `xsltFreeElemPreComp` (extensions.c).
unsafe extern "C" fn xslt_free_elem_pre_comp(comp: *mut c_void) {
    xmlFree(comp);
}

/// `xsltNewStylePreComp` (preproc.c) for the non-refactored engine: build
/// an old-style precomp of the requested type and chain it onto
/// `style->preComps`.
///
/// # ENGINE-WIRING
///
/// The upstream per-type transform-function assignment (xsltCopy, xsltIf,
/// …) is omitted: the candidate dispatches instructions by node name at
/// runtime, so `func` is only meaningful to external readers of the
/// structure.
unsafe fn xslt_new_style_pre_comp(
    style: *mut _xsltStylesheet,
    type_: c_int,
) -> *mut _xsltStylePreComp {
    if style.is_null() {
        return ptr::null_mut();
    }
    let cur = xmlMalloc(core::mem::size_of::<_xsltStylePreComp>()) as *mut _xsltStylePreComp;
    if cur.is_null() {
        report_error(
            style,
            ptr::null_mut(),
            b"xsltNewStylePreComp : malloc failed\n",
        );
        (*style).errors += 1;
        return ptr::null_mut();
    }
    ptr::write_bytes(cur as *mut u8, 0, core::mem::size_of::<_xsltStylePreComp>());

    (*cur).base.type_ = type_;
    (*cur).base.next = (*style).preComps as *mut _xsltElemPreComp;
    (*style).preComps = cur as *mut c_void;

    cur
}

/// Preprocess an XSLT-1.1 `document` (and the saxon/xalan/xt/exslt
/// document-like extension) element.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltElemPreCompPtr
/// xsltDocumentComp(xsltStylesheetPtr style, xmlNodePtr inst,
///                  xsltTransformFunction function ATTRIBUTE_UNUSED);
/// ```
///
/// Allocates an old-style precomp of type `XSLT_FUNC_DOCUMENT`, evaluates
/// the static `file`/`href` attribute template (`has_filename`), and marks
/// `ver11` when the element is `xsl:document` in the XSLT namespace.
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`.
/// - `inst` must be a valid instruction element node.
#[no_mangle]
pub unsafe extern "C" fn xsltDocumentComp(
    style: *mut _xsltStylesheet,
    inst: *mut _xmlNode,
    _function: Option<xsltTransformFunction>,
) -> *mut _xsltElemPreComp {
    if style.is_null() || inst.is_null() || (*inst).type_ != XML_ELEMENT_NODE as c_int {
        return ptr::null_mut();
    }

    let comp = xslt_new_style_pre_comp(style, XSLT_FUNC_DOCUMENT);
    if comp.is_null() {
        return ptr::null_mut();
    }
    (*comp).base.inst = inst;
    (*comp).ver11 = 0;
    let mut filename: *const xmlChar = ptr::null();

    if is_xslt_name(inst, b"output") {
        // saxon:output — @file is an AVT.
        filename = crate::abi::exports_xslt_avt::xsltEvalStaticAttrValueTemplate(
            style,
            inst,
            b"file\0".as_ptr() as *const xmlChar,
            ptr::null(),
            &mut (*comp).has_filename,
        );
    } else if is_xslt_name(inst, b"write") {
        // xalan:write — the filename is interpreted at run time.
    } else if is_xslt_name(inst, b"document") {
        if !(*inst).ns.is_null() {
            if xmlStrEqual(
                (*(*inst).ns).href,
                XSLT_NAMESPACE.as_ptr() as *const xmlChar,
            ) != 0
            {
                // xsl:document from the abandoned XSLT 1.1 draft.
                (*comp).ver11 = 1;
            }
            // exslt:document / xt:document need no extra marking.
        }
        filename = crate::abi::exports_xslt_avt::xsltEvalStaticAttrValueTemplate(
            style,
            inst,
            b"href\0".as_ptr() as *const xmlChar,
            ptr::null(),
            &mut (*comp).has_filename,
        );
    }
    if (*comp).has_filename != 0 {
        (*comp).filename = filename;
    }

    &mut (*comp).base as *mut _xsltElemPreComp
}

/// `xsltInitElemPreComp` (extensions.c): initialize an existing precomp
/// and chain it onto the stylesheet's precomp list.
///
/// This helper is intentionally not exported (the ext-family ABI exports
/// own the public symbol); it is used by the compile family to initialize
/// extension-element precomps.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn xslt_init_elem_pre_comp(
    comp: *mut _xsltElemPreComp,
    style: *mut _xsltStylesheet,
    inst: *mut _xmlNode,
    function: Option<xsltTransformFunction>,
    free_func: Option<xsltElemPreCompDeallocator>,
) {
    (*comp).type_ = XSLT_FUNC_EXTENSION;
    (*comp).func = function;
    (*comp).inst = inst;
    (*comp).free = free_func;

    (*comp).next = (*style).preComps as *mut _xsltElemPreComp;
    (*style).preComps = comp as *mut c_void;
}

/// `xsltNewElemPreComp` (extensions.c): allocate and initialize an
/// `_xsltElemPreComp`.
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`.
/// - `inst` must be a valid element node.
unsafe fn xslt_new_elem_pre_comp(
    style: *mut _xsltStylesheet,
    inst: *mut _xmlNode,
    function: Option<xsltTransformFunction>,
) -> *mut _xsltElemPreComp {
    let cur = xmlMalloc(core::mem::size_of::<_xsltElemPreComp>()) as *mut _xsltElemPreComp;
    if cur.is_null() {
        report_error(
            style,
            ptr::null_mut(),
            b"xsltNewExtElement : malloc failed\n",
        );
        return ptr::null_mut();
    }
    ptr::write_bytes(cur as *mut u8, 0, core::mem::size_of::<_xsltElemPreComp>());

    xslt_init_elem_pre_comp(cur, style, inst, function, Some(xslt_free_elem_pre_comp));

    cur
}

/// Precompute an extension module element.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltElemPreCompPtr
/// xsltPreComputeExtModuleElement(xsltStylesheetPtr style, xmlNodePtr inst);
/// ```
///
/// Looks the element up in the extension-element registry by
/// `(inst->name, inst->ns->href)`; if the registered module provides a
/// precomputation callback it is used, otherwise a default
/// `_xsltElemPreComp` is created with the registered transform function.
///
/// # ENGINE-WIRING
///
/// The candidate mirror of upstream's global `xsltElementsHash` is the
/// private registry in this module (`XSLT_ELEMENTS_REGISTRY`). The
/// ext-family ABI (exports_xslt_ext.rs) owns the public registration
/// functions; this module's registry is populated through them by
/// whichever agent wires the module-level registration (documented
/// cross-family wire-up point). With an empty registry the function
/// returns NULL exactly like upstream with no registered elements.
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`, or NULL.
/// - `inst` must be a valid element node, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltPreComputeExtModuleElement(
    style: *mut _xsltStylesheet,
    inst: *mut _xmlNode,
) -> *mut _xsltElemPreComp {
    if style.is_null()
        || inst.is_null()
        || (*inst).type_ != XML_ELEMENT_NODE as c_int
        || (*inst).ns.is_null()
    {
        return ptr::null_mut();
    }

    let ext = ext_element_lookup((*inst).name, (*(*inst).ns).href);
    if ext.is_null() {
        return ptr::null_mut();
    }

    let mut comp: *mut _xsltElemPreComp = ptr::null_mut();
    if let Some(precomp) = (*ext).precomp {
        comp = precomp(style, inst, (*ext).transform) as *mut _xsltElemPreComp;
    }
    if comp.is_null() {
        // Default creation of an _xsltElemPreComp.
        comp = xslt_new_elem_pre_comp(style, inst, (*ext).transform);
    }

    comp
}

/// Free all precomputed blocks of a stylesheet.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltFreeStylePreComps(xsltStylesheetPtr style);
/// ```
///
/// Walks `style->preComps`; extension-typed precomps are released through
/// their registered deallocator, all others through `xsltFreeStylePreComp`
/// (which, in this engine, only frees the struct — no compiled
/// expressions or pattern lists exist).
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltFreeStylePreComps(style: *mut _xsltStylesheet) {
    if style.is_null() {
        return;
    }

    let mut cur = (*style).preComps as *mut _xsltElemPreComp;
    (*style).preComps = ptr::null_mut();
    while !cur.is_null() {
        let next = (*cur).next;
        if (*cur).type_ == XSLT_FUNC_EXTENSION {
            if let Some(free_func) = (*cur).free {
                free_func(cur as *mut c_void);
            } else {
                xslt_free_style_pre_comp(cur as *mut c_void);
            }
        } else {
            xslt_free_style_pre_comp(cur as *mut c_void);
        }
        cur = next;
    }
}

/// Precompute an XSLT stylesheet element.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltStylePreCompute(xsltStylesheetPtr style, xmlNodePtr inst);
/// ```
///
/// Ported from preproc.c 1.1.45 (old behaviour): the grammar checks
/// (`xsltCheckTopLevelElement` / `xsltCheckInstructionElement` /
/// `xsltCheckParentElement`) and the per-instruction dispatch, including
/// the `xsl:document` precomp and the extension-element fallback
/// (`xsltPreComputeExtModuleElement`, else the `xsltExtMarker` sentinel).
///
/// # ENGINE-WIRING
///
/// The candidate engine compiles instructions lazily at transform time
/// from the raw node (it never reads `inst->psvi`), so the per-instruction
/// compilers allocate nothing; their observable effect — the error and
/// warning counters and the `style->preComps` chain for `xsl:document` and
/// extension elements — is preserved.
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`, or NULL.
/// - `inst` must be a valid element node, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltStylePreCompute(style: *mut _xsltStylesheet, inst: *mut _xmlNode) {
    if inst.is_null() || (*inst).type_ != XML_ELEMENT_NODE as c_int || !(*inst).psvi.is_null() {
        return;
    }

    if is_xslt_elem(inst) {
        if is_xslt_name(inst, b"apply-templates") {
            xslt_check_instruction_element(style, inst);
            // xsltApplyTemplatesComp — lazy in this engine.
        } else if is_xslt_name(inst, b"with-param") {
            xslt_check_parent_element(style, inst, b"apply-templates", b"call-template\0".as_ptr() as *const u8);
            // xsltWithParamComp — lazy.
        } else if is_xslt_name(inst, b"value-of") {
            xslt_check_instruction_element(style, inst);
        } else if is_xslt_name(inst, b"copy") {
            xslt_check_instruction_element(style, inst);
        } else if is_xslt_name(inst, b"copy-of") {
            xslt_check_instruction_element(style, inst);
        } else if is_xslt_name(inst, b"if") {
            xslt_check_instruction_element(style, inst);
        } else if is_xslt_name(inst, b"when") {
            xslt_check_parent_element(style, inst, b"choose", ptr::null());
        } else if is_xslt_name(inst, b"choose") {
            xslt_check_instruction_element(style, inst);
        } else if is_xslt_name(inst, b"for-each") {
            xslt_check_instruction_element(style, inst);
        } else if is_xslt_name(inst, b"apply-imports") {
            xslt_check_instruction_element(style, inst);
        } else if is_xslt_name(inst, b"attribute") {
            let parent = (*inst).parent;
            let is_in_attr_set = !parent.is_null()
                && (*parent).type_ == XML_ELEMENT_NODE as c_int
                && !(*parent).ns.is_null()
                && xmlStrEqual(
                    (*(*parent).ns).href,
                    XSLT_NAMESPACE.as_ptr() as *const xmlChar,
                ) != 0
                && is_xslt_name(parent, b"attribute-set");
            if !is_in_attr_set {
                xslt_check_instruction_element(style, inst);
            }
            // xsltAttributeComp — lazy.
        } else if is_xslt_name(inst, b"element") {
            xslt_check_instruction_element(style, inst);
        } else if is_xslt_name(inst, b"text") {
            xslt_check_instruction_element(style, inst);
        } else if is_xslt_name(inst, b"sort") {
            xslt_check_parent_element(style, inst, b"apply-templates", b"for-each\0".as_ptr() as *const u8);
        } else if is_xslt_name(inst, b"comment") {
            xslt_check_instruction_element(style, inst);
        } else if is_xslt_name(inst, b"number") {
            xslt_check_instruction_element(style, inst);
        } else if is_xslt_name(inst, b"processing-instruction") {
            xslt_check_instruction_element(style, inst);
        } else if is_xslt_name(inst, b"call-template") {
            xslt_check_instruction_element(style, inst);
        } else if is_xslt_name(inst, b"param") {
            if xslt_check_top_level_element(style, inst, 0) == 0 {
                xslt_check_instruction_element(style, inst);
            }
            // xsltParamComp — lazy.
        } else if is_xslt_name(inst, b"variable") {
            if xslt_check_top_level_element(style, inst, 0) == 0 {
                xslt_check_instruction_element(style, inst);
            }
            // xsltVariableComp — lazy.
        } else if is_xslt_name(inst, b"otherwise") {
            xslt_check_parent_element(style, inst, b"choose", ptr::null());
            xslt_check_instruction_element(style, inst);
            return;
        } else if is_xslt_name(inst, b"template") {
            xslt_check_top_level_element(style, inst, 1);
            return;
        } else if is_xslt_name(inst, b"output") {
            xslt_check_top_level_element(style, inst, 1);
            return;
        } else if is_xslt_name(inst, b"preserve-space") {
            xslt_check_top_level_element(style, inst, 1);
            return;
        } else if is_xslt_name(inst, b"strip-space") {
            xslt_check_top_level_element(style, inst, 1);
            return;
        } else if is_xslt_name(inst, b"stylesheet") || is_xslt_name(inst, b"transform") {
            let parent = (*inst).parent;
            if parent.is_null() || (*parent).type_ != XML_DOCUMENT_NODE as c_int {
                report_error(style, inst, b"element only allowed only as root element\n");
                (*style).errors += 1;
            }
            return;
        } else if is_xslt_name(inst, b"key") {
            xslt_check_top_level_element(style, inst, 1);
            return;
        } else if is_xslt_name(inst, b"message") {
            xslt_check_instruction_element(style, inst);
            return;
        } else if is_xslt_name(inst, b"attribute-set") {
            xslt_check_top_level_element(style, inst, 1);
            return;
        } else if is_xslt_name(inst, b"namespace-alias") {
            xslt_check_top_level_element(style, inst, 1);
            return;
        } else if is_xslt_name(inst, b"include") {
            xslt_check_top_level_element(style, inst, 1);
            return;
        } else if is_xslt_name(inst, b"import") {
            xslt_check_top_level_element(style, inst, 1);
            return;
        } else if is_xslt_name(inst, b"decimal-format") {
            xslt_check_top_level_element(style, inst, 1);
            return;
        } else if is_xslt_name(inst, b"fallback") {
            xslt_check_instruction_element(style, inst);
            return;
        } else if is_xslt_name(inst, b"document") {
            xslt_check_instruction_element(style, inst);
            (*inst).psvi = xsltDocumentComp(style, inst, None) as *mut c_void;
        } else if style.is_null() || (*style).forwards_compatible == 0 {
            report_error(
                style,
                inst,
                b"xsltStylePreCompute: unknown xsl: instruction\n",
            );
            if !style.is_null() {
                (*style).warnings += 1;
            }
        }
    } else {
        // Unknown element: maybe an extension element registered at the
        // module level.
        (*inst).psvi = xsltPreComputeExtModuleElement(style, inst) as *mut c_void;
        if (*inst).psvi.is_null() {
            (*inst).psvi = XSLT_EXT_MARKER.as_ptr() as *mut c_void;
        }
    }
}

/// `xsltCheckTopLevelElement` (preproc.c): check that the instruction is
/// instantiated as a top-level element.
///
/// Returns -1 on invalid args, 0 if the check failed, 1 on success.
unsafe fn xslt_check_top_level_element(
    style: *mut _xsltStylesheet,
    inst: *mut _xmlNode,
    err: c_int,
) -> c_int {
    if style.is_null() || inst.is_null() || (*inst).ns.is_null() {
        return -1;
    }

    let parent = (*inst).parent;
    if parent.is_null() {
        if err != 0 {
            report_error(style, inst, b"internal problem: element has no parent\n");
            (*style).errors += 1;
        }
        return 0;
    }
    if (*parent).ns.is_null()
        || (*parent).type_ != XML_ELEMENT_NODE as c_int
        || (xmlStrEqual((*(*parent).ns).href, (*(*inst).ns).href) == 0)
        || (!is_xslt_name(parent, b"stylesheet") && !is_xslt_name(parent, b"transform"))
    {
        if err != 0 {
            report_error(
                style,
                inst,
                b"element only allowed as child of stylesheet\n",
            );
            (*style).errors += 1;
        }
        return 0;
    }
    1
}

/// `xsltCheckInstructionElement` (preproc.c): check that the instruction
/// is instantiated as an instruction element.
unsafe fn xslt_check_instruction_element(style: *mut _xsltStylesheet, inst: *mut _xmlNode) {
    if style.is_null() || inst.is_null() || (*inst).ns.is_null() || (*style).literal_result != 0 {
        return;
    }

    let has_ext = !(*style).extInfos.is_null() || !ext_ns_registered(ptr::null());

    let mut parent = (*inst).parent;
    if parent.is_null() {
        report_error(style, inst, b"internal problem: element has no parent\n");
        (*style).errors += 1;
        return;
    }
    while !parent.is_null() && (*parent).type_ != XML_DOCUMENT_NODE as c_int {
        if ((*parent).ns == (*inst).ns
            || (!(*parent).ns.is_null()
                && xmlStrEqual((*(*parent).ns).href, (*(*inst).ns).href) != 0))
            && (is_xslt_name(parent, b"template")
                || is_xslt_name(parent, b"param")
                || is_xslt_name(parent, b"attribute")
                || is_xslt_name(parent, b"variable"))
        {
            return;
        }

        // If we are within an extension element all bets are off about the
        // semantics there (e.g. xsl:param within func:function).
        if has_ext && !(*parent).ns.is_null() && ext_ns_registered((*(*parent).ns).href) {
            return;
        }

        parent = (*parent).parent;
    }
    report_error(
        style,
        inst,
        b"element only allowed within a template, variable or param\n",
    );
    (*style).errors += 1;
}

/// `xsltCheckParentElement` (preproc.c): check that the instruction is a
/// child of one of the possible parents.
unsafe fn xslt_check_parent_element(
    style: *mut _xsltStylesheet,
    inst: *mut _xmlNode,
    allow1: &[u8],
    allow2: *const u8,
) {
    if style.is_null() || inst.is_null() || (*inst).ns.is_null() || (*style).literal_result != 0 {
        return;
    }

    let parent = (*inst).parent;
    if parent.is_null() {
        report_error(style, inst, b"internal problem: element has no parent\n");
        (*style).errors += 1;
        return;
    }
    let allow2_bytes: &[u8] = if allow2.is_null() {
        b""
    } else {
        core::slice::from_raw_parts(allow2, libc::strlen(allow2 as *const libc::c_char) as usize)
    };
    if ((*parent).ns == (*inst).ns
        || (!(*parent).ns.is_null() && xmlStrEqual((*(*parent).ns).href, (*(*inst).ns).href) != 0))
        && (is_xslt_name(parent, allow1)
            || (!allow2_bytes.is_empty() && is_xslt_name(parent, allow2_bytes)))
    {
        return;
    }

    if !ext_ns_registered(ptr::null()) {
        let mut p = parent;
        while !p.is_null() && (*p).type_ != XML_DOCUMENT_NODE as c_int {
            if !(*p).ns.is_null() && ext_ns_registered((*(*p).ns).href) {
                return;
            }
            p = (*p).parent;
        }
    }
    report_error(style, inst, b"element is not allowed within that context\n");
    (*style).errors += 1;
}

/// Normalize the compiled steps of an imported stylesheet (hash scanner
/// callback).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltNormalizeCompSteps(void *payload,
///         void *data, const xmlChar *name ATTRIBUTE_UNUSED) {
///     xsltCompMatchPtr comp = payload;
///     xsltStylesheetPtr style = data;
///     for (ix = 0; ix < comp->nbStep; ix++) {
///         comp->steps[ix].previousExtra += style->extrasNr;
///         comp->steps[ix].indexExtra += style->extrasNr;
///         comp->steps[ix].lenExtra += style->extrasNr;
///     }
/// }
/// ```
///
/// # ENGINE-WIRING
///
/// Upstream's `xsltCompMatch` carries a step array with extra-slot
/// indices; the candidate's compiled pattern (`_xsltCompMatch` in
/// `exports_xslt_apply.rs`) is an opaque pointer with no step array, so
/// there is nothing to re-base — the function is a faithful no-op for the
/// candidate representation (documented divergence).
///
/// # SAFETY
///
/// - `payload` and `data` are only passed through (never dereferenced).
#[no_mangle]
pub unsafe extern "C" fn xsltNormalizeCompSteps(
    payload: *mut c_void,
    data: *mut c_void,
    _name: *const xmlChar,
) {
    let _ = (payload, data);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 6. Style documents (documents.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Register a new stylesheet document (wrap it in an `_xsltDocument`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltDocumentPtr
/// xsltNewStyleDocument(xsltStylesheetPtr style, xmlDocPtr doc) {
///     cur = xmlMalloc(sizeof(xsltDocument));
///     if (cur == NULL) { ... return(NULL); }
///     memset(cur, 0, sizeof(xsltDocument));
///     cur->doc = doc;
///     if (style != NULL) {
///         cur->next = style->docList;
///         style->docList = cur;
///     }
///     return(cur);
/// }
/// ```
///
/// The wrapper does NOT own `doc` (ownership stays with the caller or the
/// stylesheet's `doc` field).
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`, or NULL.
/// - `doc` must be a valid parsed document.
#[no_mangle]
pub unsafe extern "C" fn xsltNewStyleDocument(
    style: *mut _xsltStylesheet,
    doc: *mut _xmlDoc,
) -> *mut _xsltDocument {
    let cur = xmlMalloc(core::mem::size_of::<_xsltDocument>()) as *mut _xsltDocument;
    if cur.is_null() {
        report_error(
            style,
            doc as *mut _xmlNode,
            b"xsltNewStyleDocument : malloc failed\n",
        );
        return ptr::null_mut();
    }
    ptr::write_bytes(cur as *mut u8, 0, core::mem::size_of::<_xsltDocument>());
    (*cur).doc = doc;
    if !style.is_null() {
        (*cur).next = (*style).docList;
        (*style).docList = cur;
    }
    cur
}

/// Load a stylesheet document by URI, reusing an already-loaded document
/// from the stylesheet's doc list when possible.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltDocumentPtr
/// xsltLoadStyleDocument(xsltStylesheetPtr style, const xmlChar *URI);
/// ```
///
/// # ENGINE-WIRING
///
/// The default loader (documents.c `xsltDocDefaultLoaderFunc`) parses the
/// URI with `XSLT_PARSE_OPTIONS`; the candidate's loader mirrors
/// `src/xslt/documents` `load_via_loader` (registered loader first, else
/// `xmlReadFile`). On failure the freshly parsed document is freed.
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`, or NULL.
/// - `URI` must be a valid NUL-terminated string, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltLoadStyleDocument(
    style: *mut _xsltStylesheet,
    uri: *const xmlChar,
) -> *mut _xsltDocument {
    if style.is_null() || uri.is_null() {
        return ptr::null_mut();
    }

    // Security framework check.
    let sec = crate::xslt::security::xsltGetDefaultSecurityPrefs();
    if !sec.is_null() {
        let res = xslt_check_read(sec, ptr::null_mut(), uri);
        if res <= 0 {
            if res == 0 {
                report_error(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    b"xsltLoadStyleDocument: read rights for ",
                );
                report_error(ptr::null_mut(), ptr::null_mut(), cbytes(uri as *const u8));
                report_error(ptr::null_mut(), ptr::null_mut(), b" denied\n");
            }
            return ptr::null_mut();
        }
    }

    // Walk the style's document list for a preparsed match.
    let mut ret = (*style).docList;
    while !ret.is_null() {
        if !(*ret).doc.is_null()
            && !(*(*ret).doc).URL.is_null()
            && xmlStrEqual((*(*ret).doc).URL, uri) != 0
        {
            return ret;
        }
        ret = (*ret).next;
    }

    let doc = xslt_doc_default_loader(
        uri,
        (*style).dict,
        XSLT_PARSE_OPTIONS,
        style as *mut c_void,
        XSLT_LOAD_STYLESHEET,
    );
    if doc.is_null() {
        return ptr::null_mut();
    }

    let ret = xsltNewStyleDocument(style, doc);
    if ret.is_null() {
        crate::xml::tree::free_doc(doc);
    }
    ret
}

/// Free the node-trees (and `_xsltDocument` structures) of all
/// stylesheet-modules of the stylesheet-level represented by `style`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltFreeStyleDocuments(xsltStylesheetPtr style) {
///     if (style == NULL) return;
///     cur = style->docList;
///     while (cur != NULL) {
///         doc = cur; cur = cur->next;
///         xsltFreeDocumentKeys(doc);
///         if (!doc->main) xmlFreeDoc(doc->doc);
///         xmlFree(doc);
///     }
/// }
/// ```
///
/// # ENGINE-WIRING
///
/// `xsltFreeDocumentKeys` (keys.c) frees the key tables cached on the
/// wrapper; the candidate computes keys on demand under the transform
/// context and caches them on the context's document wrapper, so nothing
/// is cached on style documents (documented divergence).
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltFreeStyleDocuments(style: *mut _xsltStylesheet) {
    if style.is_null() {
        return;
    }

    let mut cur = (*style).docList;
    (*style).docList = ptr::null_mut();
    while !cur.is_null() {
        let doc = cur;
        cur = (*cur).next;
        if (*doc).main == 0 && !(*doc).doc.is_null() {
            crate::xml::tree::free_doc((*doc).doc);
        }
        xmlFree(doc as *mut c_void);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 7. Global state & extensions (extensions.c, xslt.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Initialize the global variables for extensions.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltInitGlobals(void) {
///     if (xsltExtMutex == NULL) {
///         xsltExtMutex = xmlNewMutex();
///     }
/// }
/// ```
///
/// # ENGINE-WIRING
///
/// The candidate's global extension registries are plain statics that
/// require no lazy initialization; the call is kept as the idempotent
/// initialization marker (documented no-op mirroring upstream's
/// mutex-creation).
#[no_mangle]
pub unsafe extern "C" fn xsltInitGlobals() {
    if XSLT_GLOBALS_INITIALIZED == 0 {
        XSLT_GLOBALS_INITIALIZED = 1;
    }
}

/// Uninitialize the processor.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltUninit (void) {
/// #ifdef XSLT_LOCALE_WINAPI
///     xmlFreeRMutex(xsltLocaleMutex);
///     xsltLocaleMutex = NULL;
/// #endif
///     initialized = 0;
/// }
/// ```
///
/// # ENGINE-WIRING
///
/// On the oracle (non-Win32) build this only clears the global
/// initialized flag; the candidate keeps process-lifetime statics, so the
/// observable behaviour is a no-op. The marker is reset for symmetry.
#[no_mangle]
pub unsafe extern "C" fn xsltUninit() {
    XSLT_GLOBALS_INITIALIZED = 0;
}

/// Free the memory used by XSLT extensions in a stylesheet.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltFreeExts(xsltStylesheetPtr style) {
///     if (style->nsDefs != NULL)
///         xsltFreeExtDefList((xsltExtDefPtr) style->nsDefs);
/// }
/// ```
///
/// # ENGINE-WIRING
///
/// Upstream keeps the stylesheet's extension-prefix definitions in
/// `style->nsDefs`; the candidate *repurposes* `style->nsDefs` for the
/// preserve-space rule list (see `src/xslt/compiler`
/// `compile_space_rules`, documented divergence) and frees it as such in
/// `xsltFreeStylesheet`. No extension-prefix def list exists, so there is
/// nothing to free here — the function is an intentionally empty port
/// (freeing `nsDefs` again would double-free the preserve-space list).
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltFreeExts(style: *mut _xsltStylesheet) {
    if style.is_null() {
        return;
    }
    // See ENGINE-WIRING above: nothing to free in the candidate engine.
}

/// Shut down the set of extension modules loaded for a stylesheet.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltShutdownExts(xsltStylesheetPtr style) {
///     if (style == NULL) return;
///     if (style->extInfos == NULL) return;
///     xmlHashScan(style->extInfos, xsltShutdownExt, style);
///     xmlHashFree(style->extInfos, xsltFreeExtDataEntry);
///     style->extInfos = NULL;
/// }
/// ```
///
/// # ENGINE-WIRING
///
/// Upstream populates `style->extInfos` per stylesheet when a registered
/// module provides a style-init function; the candidate has no such
/// registration path, so `style->extInfos` is NULL for every stylesheet
/// and the function returns immediately — the exact upstream behaviour for
/// that state. If a hash were ever attached by an external writer, it is
/// released without invoking shutdown callbacks (no module metadata is
/// available to the compile family; documented divergence).
///
/// # SAFETY
///
/// - `style` must be a valid `_xsltStylesheet`, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltShutdownExts(style: *mut _xsltStylesheet) {
    if style.is_null() {
        return;
    }
    if (*style).extInfos.is_null() {
        return;
    }
    // Unreachable in the candidate engine (extInfos is never populated);
    // free the table so the stylesheet teardown stays leak-free for
    // external writers.
    crate::xml::hash::hash_free((*style).extInfos as *mut crate::xml::hash::HashTable, None);
    (*style).extInfos = ptr::null_mut();
}

/// Dump a list of the registered XSLT extension functions and elements.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void
/// xsltDebugDumpExtensions(FILE * output);
/// ```
///
/// Prints the same headings as extensions.c 1.1.45 to the given `FILE*`
/// (stdout when NULL): the extension-function and top-level registries do
/// not exist in the candidate (always "No registered …"), while the
/// instruction-element and module registries print their entries
/// (`{URI}name` and `URI` lines respectively).
///
/// # SAFETY
///
/// - `output` must be a valid `FILE*`, or NULL (stdout).
#[no_mangle]
pub unsafe extern "C" fn xsltDebugDumpExtensions(output: *mut libc::FILE) {
    let out = if output.is_null() {
        libc::fdopen(1, b"w\0".as_ptr() as *const c_char)
    } else {
        output
    };

    if out.is_null() {
        return;
    }

    libc::fprintf(
        out,
        b"Registered XSLT Extensions\n--------------------------\n\0".as_ptr() as *const c_char,
    );
    libc::fprintf(
        out,
        b"No registered extension functions\n\0".as_ptr() as *const c_char,
    );
    libc::fprintf(
        out,
        b"\nNo registered top-level extension elements\n\0".as_ptr() as *const c_char,
    );

    if XSLT_ELEMENTS_REGISTRY.is_null() {
        libc::fprintf(
            out,
            b"\nNo registered instruction extension elements\n\0".as_ptr() as *const c_char,
        );
    } else {
        libc::fprintf(
            out,
            b"\nRegistered instruction extension elements:\n\0".as_ptr() as *const c_char,
        );
        let mut cur = XSLT_ELEMENTS_REGISTRY;
        while !cur.is_null() {
            if !(*cur).URI.is_null() && !(*cur).name.is_null() {
                libc::fprintf(
                    out,
                    b"{%s}%s\n\0".as_ptr() as *const c_char,
                    (*cur).URI as *const c_char,
                    (*cur).name as *const c_char,
                );
            }
            cur = (*cur).next;
        }
    }

    if XSLT_MODULES_REGISTRY.is_null() {
        libc::fprintf(
            out,
            b"\nNo registered extension modules\n\0".as_ptr() as *const c_char,
        );
    } else {
        libc::fprintf(
            out,
            b"\nRegistered extension modules:\n\0".as_ptr() as *const c_char,
        );
        let mut cur = XSLT_MODULES_REGISTRY;
        while !cur.is_null() {
            if !(*cur).URI.is_null() {
                libc::fprintf(
                    out,
                    b"%s\n\0".as_ptr() as *const c_char,
                    (*cur).URI as *const c_char,
                );
            }
            cur = (*cur).next;
        }
    }
}
