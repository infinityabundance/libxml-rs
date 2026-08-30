//! exports_html — HTML C ABI family (HTMLparser.h, HTMLtree.h, SAX2.h).
//!
//! Implements the 57 `html*` entry points of the libxml2 HTML subsystem
//! (family closure 11.1-I) with exact upstream signatures:
//!
//! - Parser contexts: `htmlNewParserCtxt`, `htmlNewSAXParserCtxt`,
//!   `htmlCreateMemoryParserCtxt`, `htmlCreatePushParserCtxt`,
//!   `htmlCtxtReset`, `htmlCtxtUseOptions`, `htmlCtxtParseDocument`,
//!   `htmlParseDocument`, `htmlParseChunk`, `htmlCtxtReadMemory/File/Fd/IO/Doc`,
//!   `htmlReadMemory/File/Fd/IO/Doc`, `htmlSAXParseDoc`, `htmlSAXParseFile`.
//! - Entities/encoding: `htmlEncodeEntities`, `htmlDecodeEntities`,
//!   `htmlEntityLookup`, `htmlEntityValueLookup`, `htmlGetMetaEncoding`,
//!   `htmlSetMetaEncoding`, `htmlIsBooleanAttr`, `htmlIsScriptAttribute`.
//! - Tree: `htmlNewDoc`, `htmlNewDocNoDtD`, `htmlDocDump`,
//!   `htmlDocDumpMemory(Format)`, `htmlDocContentDumpOutput(FormatOutput)`,
//!   `htmlNodeDump(File/FileFormat/Output/FormatOutput)`,
//!   `htmlSaveFile(Enc/Format)`.
//! - Element rules: `htmlAutoCloseTag`, `htmlElementAllowedHere`,
//!   `htmlElementStatusHere`, `htmlNodeStatus`, `htmlIsAutoClosed`,
//!   `htmlHandleOmittedElem`, `htmlInitAutoClose`, `htmlTagLookup`,
//!   `htmlAttrAllowed`.
//! - Misc: `htmlDefaultSAXHandlerInit`, `htmlParseElement`,
//!   `htmlParseEntityRef`, `htmlParseCharRef`.
//!
//! Semantics follow archaeology/libxml2-git (HTMLparser.c, HTMLtree.c,
//! SAX2.c, legacy.c). The document-parsing entry points wrap the internal
//! module `src/xml/html/mod.rs` (`parse_memory` / `parse_file` / `parse_doc` /
//! `serialize_node` / `new_doc*`), which is the oracle-verified engine used by
//! `xmllint --html`.
//!
//! The `htmlParserCtxt` type is opaque at this boundary (`*mut c_void`):
//! structs.rs has no `_htmlParserCtxt` mirror. Contexts created here are
//! freed by the already-exported `htmlFreeParserCtxt`
//! (`crate::xml::html::free_parser_ctxt`), so the context struct mirrors the
//! internal `HtmlParserCtxt` field layout through `filename`/`encoding`
//! (the only fields that function touches) before appending ABI state.

#![allow(
    missing_docs,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals
)]
#![allow(unused_variables)]
#![allow(private_interfaces)]
#![allow(unused_assignments)]
#![allow(unused_unsafe)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::mem::size_of;
use std::os::raw::{c_char, c_int, c_uint};

use crate::abi::allocator::{xmlFreeImpl, xmlMallocImpl, xmlMallocZero, xmlReallocImpl};
use crate::abi::callbacks::{xmlInputCloseCallback, xmlInputReadCallback};
use crate::abi::structs::*;
use crate::abi::types::xmlChar;
use crate::abi::types::xmlCharEncoding;
use crate::abi::types::xmlElementType::*;
use crate::xml::html;
use crate::xml::io;
use crate::xml::string::{c_strdup, xml_strcmp, xml_strlen, xml_strndup, xmlstr_to_bytes};
use crate::xml::tree;

// ═══════════════════════════════════════════════════════════════════════════════
// HTML parser options / status / error constants (HTMLparser.h)
// ═══════════════════════════════════════════════════════════════════════════════

const HTML_PARSE_RECOVER: c_int = 1 << 0;
const HTML_PARSE_NODEFDTD: c_int = 1 << 2;
const HTML_PARSE_NOERROR: c_int = 1 << 5;
const HTML_PARSE_NOWARNING: c_int = 1 << 6;
const HTML_PARSE_PEDANTIC: c_int = 1 << 7;
const HTML_PARSE_NOBLANKS: c_int = 1 << 8;
const HTML_PARSE_NONET: c_int = 1 << 11;
const HTML_PARSE_NOIMPLIED: c_int = 1 << 13;
const HTML_PARSE_COMPACT: c_int = 1 << 16;
const HTML_PARSE_HUGE: c_int = 1 << 19;
const HTML_PARSE_IGNORE_ENC: c_int = 1 << 21;
const HTML_PARSE_BIG_LINES: c_int = 1 << 22;
const HTML_PARSE_HTML5: c_int = 1 << 26;

/// Options that `htmlCtxtUseOptions` can only enable, never clear.
const HTML_OPTIONS_KEEP_MASK: c_int = HTML_PARSE_NODEFDTD
    | HTML_PARSE_NOERROR
    | HTML_PARSE_NOWARNING
    | HTML_PARSE_NOIMPLIED
    | HTML_PARSE_COMPACT
    | HTML_PARSE_HUGE
    | HTML_PARSE_IGNORE_ENC
    | HTML_PARSE_BIG_LINES;

/// All options the HTML parser recognizes (upstream `htmlCtxtSetOptionsInternal`).
const HTML_OPTIONS_ALL_MASK: c_int = HTML_PARSE_RECOVER
    | HTML_PARSE_HTML5
    | HTML_PARSE_NODEFDTD
    | HTML_PARSE_NOERROR
    | HTML_PARSE_NOWARNING
    | HTML_PARSE_PEDANTIC
    | HTML_PARSE_NOBLANKS
    | HTML_PARSE_NONET
    | HTML_PARSE_NOIMPLIED
    | HTML_PARSE_COMPACT
    | HTML_PARSE_HUGE
    | HTML_PARSE_IGNORE_ENC
    | HTML_PARSE_BIG_LINES;

/// `xmlParserErrors` values used by the HTML parser ABI.
const XML_ERR_OK: c_int = 0;
const XML_ERR_NO_MEMORY: c_int = 2;
const XML_ERR_ARGUMENT: c_int = 115;

/// `htmlStatus` (HTMLparser.h, deprecated content model).
const HTML_VALID: c_int = 0x4;

/// HTML element data modes (include/private/html.h).
const DATA_NEUTRAL: c_int = 0;
const DATA_RCDATA: c_int = 1;
const DATA_RAWTEXT: c_int = 2;
const DATA_PLAINTEXT: c_int = 3;
const DATA_SCRIPT: c_int = 4;

/// HTML whitespace predicate `IS_WS_HTML` (include/private/html.h).
#[inline]
fn is_ws_html(c: u8) -> bool {
    c == 0x20 || (c >= 0x09 && c <= 0x0d && c != 0x0b)
}

// ═══════════════════════════════════════════════════════════════════════════════
// htmlElemDesc / htmlAttributeDesc mirrors (HTMLparser.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Rust mirror of `struct _htmlElemDesc` (HTMLparser.h) with the upstream
/// field layout. Only `name`, `endTag`, `empty`, `isinline` and `desc` are
/// read by the exported functions; the deprecated pointer fields are kept
/// NULL for layout parity.
#[repr(C)]
pub struct _htmlElemDesc {
    pub name: *const c_char,
    pub startTag: c_char,
    pub endTag: c_char,
    pub saveEndTag: c_char,
    pub empty: c_char,
    pub depr: c_char,
    pub dtd: c_char,
    pub isinline: c_char,
    pub desc: *const c_char,
    pub subelts: *const *const c_char,
    pub defaultsubelt: *const c_char,
    pub attrs_opt: *const *const c_char,
    pub attrs_depr: *const *const c_char,
    pub attrs_req: *const *const c_char,
    pub dataMode: c_int,
}

// SAFETY: the struct only contains pointers into static, immutable tables.
unsafe impl Sync for _htmlElemDesc {}
unsafe impl Send for _htmlElemDesc {}

/// Build a static `_htmlElemDesc` from the (name, startTag, endTag,
/// saveEndTag, empty, depr, dtd, isinline, desc, dataMode) tuple used by the
/// upstream `html40ElementTable`.
macro_rules! elem {
    ($name:literal, $startTag:expr, $endTag:expr, $saveEndTag:expr, $empty:expr, $depr:expr, $dtd:expr, $isinline:expr, $desc:literal, $dataMode:expr) => {
        _htmlElemDesc {
            name: concat!($name, "\0").as_ptr() as *const c_char,
            startTag: $startTag,
            endTag: $endTag,
            saveEndTag: $saveEndTag,
            empty: $empty,
            depr: $depr,
            dtd: $dtd,
            isinline: $isinline,
            desc: concat!($desc, "\0").as_ptr() as *const c_char,
            subelts: ptr::null(),
            defaultsubelt: ptr::null(),
            attrs_opt: ptr::null(),
            attrs_depr: ptr::null(),
            attrs_req: ptr::null(),
            dataMode: $dataMode,
        }
    };
}

/// The HTML 4.01 element table, faithfully ported from
/// archaeology/libxml2-git/HTMLparser.c `html40ElementTable`.
static HTML40_ELEMENTS: &[_htmlElemDesc] = &[
    elem!("a", 0, 0, 0, 0, 0, 0, 1, "anchor ", DATA_NEUTRAL),
    elem!(
        "abbr",
        0,
        0,
        0,
        0,
        0,
        0,
        1,
        "abbreviated form",
        DATA_NEUTRAL
    ),
    elem!("acronym", 0, 0, 0, 0, 0, 0, 1, "", DATA_NEUTRAL),
    elem!(
        "address",
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        "information on author ",
        DATA_NEUTRAL
    ),
    elem!("applet", 0, 0, 0, 0, 1, 1, 2, "java applet ", DATA_NEUTRAL),
    elem!(
        "area",
        0,
        2,
        2,
        1,
        0,
        0,
        0,
        "client-side image map area ",
        DATA_NEUTRAL
    ),
    elem!("b", 0, 3, 0, 0, 0, 0, 1, "bold text style", DATA_NEUTRAL),
    elem!(
        "base",
        0,
        2,
        2,
        1,
        0,
        0,
        0,
        "document base uri ",
        DATA_NEUTRAL
    ),
    elem!(
        "basefont",
        0,
        2,
        2,
        1,
        1,
        1,
        1,
        "base font size ",
        DATA_NEUTRAL
    ),
    elem!(
        "bdo",
        0,
        0,
        0,
        0,
        0,
        0,
        1,
        "i18n bidi over-ride ",
        DATA_NEUTRAL
    ),
    elem!("bgsound", 0, 0, 2, 1, 0, 0, 0, "", DATA_NEUTRAL),
    elem!("big", 0, 3, 0, 0, 0, 0, 1, "large text style", DATA_NEUTRAL),
    elem!(
        "blockquote",
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        "long quotation ",
        DATA_NEUTRAL
    ),
    elem!("body", 1, 1, 0, 0, 0, 0, 0, "document body ", DATA_NEUTRAL),
    elem!(
        "br",
        0,
        2,
        2,
        1,
        0,
        0,
        1,
        "forced line break ",
        DATA_NEUTRAL
    ),
    elem!("button", 0, 0, 0, 0, 0, 0, 2, "push button ", DATA_NEUTRAL),
    elem!(
        "caption",
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        "table caption ",
        DATA_NEUTRAL
    ),
    elem!(
        "center",
        0,
        3,
        0,
        0,
        1,
        1,
        0,
        "shorthand for div align=center ",
        DATA_NEUTRAL
    ),
    elem!("cite", 0, 0, 0, 0, 0, 0, 1, "citation", DATA_NEUTRAL),
    elem!(
        "code",
        0,
        0,
        0,
        0,
        0,
        0,
        1,
        "computer code fragment",
        DATA_NEUTRAL
    ),
    elem!("col", 0, 2, 2, 1, 0, 0, 0, "table column ", DATA_NEUTRAL),
    elem!(
        "colgroup",
        0,
        1,
        0,
        0,
        0,
        0,
        0,
        "table column group ",
        DATA_NEUTRAL
    ),
    elem!(
        "dd",
        0,
        1,
        0,
        0,
        0,
        0,
        0,
        "definition description ",
        DATA_NEUTRAL
    ),
    elem!("del", 0, 0, 0, 0, 0, 0, 2, "deleted text ", DATA_NEUTRAL),
    elem!(
        "dfn",
        0,
        0,
        0,
        0,
        0,
        0,
        1,
        "instance definition",
        DATA_NEUTRAL
    ),
    elem!("dir", 0, 0, 0, 0, 1, 1, 0, "directory list", DATA_NEUTRAL),
    elem!(
        "div",
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        "generic language/style container",
        DATA_NEUTRAL
    ),
    elem!("dl", 0, 0, 0, 0, 0, 0, 0, "definition list ", DATA_NEUTRAL),
    elem!("dt", 0, 1, 0, 0, 0, 0, 0, "definition term ", DATA_NEUTRAL),
    elem!("em", 0, 3, 0, 0, 0, 0, 1, "emphasis", DATA_NEUTRAL),
    elem!(
        "embed",
        0,
        1,
        2,
        1,
        1,
        1,
        1,
        "generic embedded object ",
        DATA_NEUTRAL
    ),
    elem!(
        "fieldset",
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        "form control group ",
        DATA_NEUTRAL
    ),
    elem!(
        "font",
        0,
        3,
        0,
        0,
        1,
        1,
        1,
        "local change to font ",
        DATA_NEUTRAL
    ),
    elem!(
        "form",
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        "interactive form ",
        DATA_NEUTRAL
    ),
    elem!("frame", 0, 2, 2, 1, 0, 2, 0, "subwindow ", DATA_NEUTRAL),
    elem!(
        "frameset",
        0,
        0,
        0,
        0,
        0,
        2,
        0,
        "window subdivision",
        DATA_NEUTRAL
    ),
    elem!("h1", 0, 0, 0, 0, 0, 0, 0, "heading ", DATA_NEUTRAL),
    elem!("h2", 0, 0, 0, 0, 0, 0, 0, "heading ", DATA_NEUTRAL),
    elem!("h3", 0, 0, 0, 0, 0, 0, 0, "heading ", DATA_NEUTRAL),
    elem!("h4", 0, 0, 0, 0, 0, 0, 0, "heading ", DATA_NEUTRAL),
    elem!("h5", 0, 0, 0, 0, 0, 0, 0, "heading ", DATA_NEUTRAL),
    elem!("h6", 0, 0, 0, 0, 0, 0, 0, "heading ", DATA_NEUTRAL),
    elem!("head", 1, 1, 0, 0, 0, 0, 0, "document head ", DATA_NEUTRAL),
    elem!("hr", 0, 2, 2, 1, 0, 0, 0, "horizontal rule ", DATA_NEUTRAL),
    elem!(
        "html",
        1,
        1,
        0,
        0,
        0,
        0,
        0,
        "document root element ",
        DATA_NEUTRAL
    ),
    elem!("i", 0, 3, 0, 0, 0, 0, 1, "italic text style", DATA_NEUTRAL),
    elem!(
        "iframe",
        0,
        0,
        0,
        0,
        0,
        1,
        2,
        "inline subwindow ",
        DATA_RAWTEXT
    ),
    elem!("img", 0, 2, 2, 1, 0, 0, 1, "embedded image ", DATA_NEUTRAL),
    elem!("input", 0, 2, 2, 1, 0, 0, 1, "form control ", DATA_NEUTRAL),
    elem!("ins", 0, 0, 0, 0, 0, 0, 2, "inserted text", DATA_NEUTRAL),
    elem!(
        "isindex",
        0,
        2,
        2,
        1,
        1,
        1,
        0,
        "single line prompt ",
        DATA_NEUTRAL
    ),
    elem!(
        "kbd",
        0,
        0,
        0,
        0,
        0,
        0,
        1,
        "text to be entered by the user",
        DATA_NEUTRAL
    ),
    elem!("keygen", 0, 0, 2, 1, 0, 0, 0, "", DATA_NEUTRAL),
    elem!(
        "label",
        0,
        0,
        0,
        0,
        0,
        0,
        1,
        "form field label text ",
        DATA_NEUTRAL
    ),
    elem!(
        "legend",
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        "fieldset legend ",
        DATA_NEUTRAL
    ),
    elem!("li", 0, 1, 1, 0, 0, 0, 0, "list item ", DATA_NEUTRAL),
    elem!(
        "link",
        0,
        2,
        2,
        1,
        0,
        0,
        0,
        "a media-independent link ",
        DATA_NEUTRAL
    ),
    elem!(
        "map",
        0,
        0,
        0,
        0,
        0,
        0,
        2,
        "client-side image map ",
        DATA_NEUTRAL
    ),
    elem!("menu", 0, 0, 0, 0, 1, 1, 0, "menu list ", DATA_NEUTRAL),
    elem!(
        "meta",
        0,
        2,
        2,
        1,
        0,
        0,
        0,
        "generic metainformation ",
        DATA_NEUTRAL
    ),
    elem!("noembed", 0, 0, 0, 0, 0, 0, 0, "", DATA_RAWTEXT),
    elem!(
        "noframes",
        0,
        0,
        0,
        0,
        0,
        2,
        0,
        "alternate content container for non frame-based rendering ",
        DATA_RAWTEXT
    ),
    elem!(
        "noscript",
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        "alternate content container for non script-based rendering ",
        DATA_NEUTRAL
    ),
    elem!(
        "object",
        0,
        0,
        0,
        0,
        0,
        0,
        2,
        "generic embedded object ",
        DATA_NEUTRAL
    ),
    elem!("ol", 0, 0, 0, 0, 0, 0, 0, "ordered list ", DATA_NEUTRAL),
    elem!(
        "optgroup",
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        "option group ",
        DATA_NEUTRAL
    ),
    elem!(
        "option",
        0,
        1,
        0,
        0,
        0,
        0,
        0,
        "selectable choice ",
        DATA_NEUTRAL
    ),
    elem!("p", 0, 1, 0, 0, 0, 0, 0, "paragraph ", DATA_NEUTRAL),
    elem!(
        "param",
        0,
        2,
        2,
        1,
        0,
        0,
        0,
        "named property value ",
        DATA_NEUTRAL
    ),
    elem!("plaintext", 0, 0, 0, 0, 0, 0, 0, "", DATA_PLAINTEXT),
    elem!(
        "pre",
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        "preformatted text ",
        DATA_NEUTRAL
    ),
    elem!(
        "q",
        0,
        0,
        0,
        0,
        0,
        0,
        1,
        "short inline quotation ",
        DATA_NEUTRAL
    ),
    elem!(
        "s",
        0,
        3,
        0,
        0,
        1,
        1,
        1,
        "strike-through text style",
        DATA_NEUTRAL
    ),
    elem!(
        "samp",
        0,
        0,
        0,
        0,
        0,
        0,
        1,
        "sample program output, scripts, etc.",
        DATA_NEUTRAL
    ),
    elem!(
        "script",
        0,
        0,
        0,
        0,
        0,
        0,
        2,
        "script statements ",
        DATA_SCRIPT
    ),
    elem!(
        "select",
        0,
        0,
        0,
        0,
        0,
        0,
        1,
        "option selector ",
        DATA_NEUTRAL
    ),
    elem!(
        "small",
        0,
        3,
        0,
        0,
        0,
        0,
        1,
        "small text style",
        DATA_NEUTRAL
    ),
    elem!("source", 0, 0, 2, 1, 0, 0, 0, "", DATA_NEUTRAL),
    elem!(
        "span",
        0,
        0,
        0,
        0,
        0,
        0,
        1,
        "generic language/style container ",
        DATA_NEUTRAL
    ),
    elem!(
        "strike",
        0,
        3,
        0,
        0,
        1,
        1,
        1,
        "strike-through text",
        DATA_NEUTRAL
    ),
    elem!(
        "strong",
        0,
        3,
        0,
        0,
        0,
        0,
        1,
        "strong emphasis",
        DATA_NEUTRAL
    ),
    elem!("style", 0, 0, 0, 0, 0, 0, 0, "style info ", DATA_RAWTEXT),
    elem!("sub", 0, 3, 0, 0, 0, 0, 1, "subscript", DATA_NEUTRAL),
    elem!("sup", 0, 3, 0, 0, 0, 0, 1, "superscript ", DATA_NEUTRAL),
    elem!("table", 0, 0, 0, 0, 0, 0, 0, "", DATA_NEUTRAL),
    elem!("tbody", 1, 0, 0, 0, 0, 0, 0, "table body ", DATA_NEUTRAL),
    elem!("td", 0, 0, 0, 0, 0, 0, 0, "table data cell", DATA_NEUTRAL),
    elem!(
        "textarea",
        0,
        0,
        0,
        0,
        0,
        0,
        1,
        "multi-line text field ",
        DATA_RCDATA
    ),
    elem!("tfoot", 0, 1, 0, 0, 0, 0, 0, "table footer ", DATA_NEUTRAL),
    elem!("th", 0, 1, 0, 0, 0, 0, 0, "table header cell", DATA_NEUTRAL),
    elem!("thead", 0, 1, 0, 0, 0, 0, 0, "table header ", DATA_NEUTRAL),
    elem!("title", 0, 0, 0, 0, 0, 0, 0, "document title ", DATA_RCDATA),
    elem!("tr", 0, 0, 0, 0, 0, 0, 0, "table row ", DATA_NEUTRAL),
    elem!("track", 0, 0, 2, 1, 0, 0, 0, "", DATA_NEUTRAL),
    elem!(
        "tt",
        0,
        3,
        0,
        0,
        0,
        0,
        1,
        "teletype or monospaced text style",
        DATA_NEUTRAL
    ),
    elem!(
        "u",
        0,
        3,
        0,
        0,
        1,
        1,
        1,
        "underlined text style",
        DATA_NEUTRAL
    ),
    elem!("ul", 0, 0, 0, 0, 0, 0, 0, "unordered list ", DATA_NEUTRAL),
    elem!(
        "var",
        0,
        0,
        0,
        0,
        0,
        0,
        1,
        "instance of a variable or program argument",
        DATA_NEUTRAL
    ),
    elem!("wbr", 0, 0, 2, 1, 0, 0, 0, "", DATA_NEUTRAL),
    elem!("xmp", 0, 0, 0, 0, 0, 0, 1, "", DATA_RAWTEXT),
];

/// Lookup the HTML tag in the element table.
///
/// Upstream `htmlTagLookup` binary-searches the sorted `html40ElementTable`
/// with `xmlStrcasecmp`; the same tag set is searched here case-insensitively
/// with a linear scan (same results, no sort order required).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const htmlElemDesc *htmlTagLookup(const xmlChar *tag);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlTagLookup(tag: *const xmlChar) -> *const _htmlElemDesc {
    if tag.is_null() {
        return ptr::null();
    }
    let bytes = unsafe { xmlstr_to_bytes(tag) };
    for e in HTML40_ELEMENTS {
        let name = unsafe { core::ffi::CStr::from_ptr(e.name) }.to_bytes();
        if bytes.eq_ignore_ascii_case(name) {
            return e as *const _htmlElemDesc;
        }
    }
    ptr::null()
}

// ═══════════════════════════════════════════════════════════════════════════════
// HTML entity table (HTMLparser.c html40EntitiesTable)
// ═══════════════════════════════════════════════════════════════════════════════

/// Rust mirror of `struct _htmlEntityDesc` (HTMLparser.h).
#[repr(C)]
pub struct _htmlEntityDesc {
    pub value: c_uint,
    pub name: *const c_char,
    pub desc: *const c_char,
}

// SAFETY: the struct only contains pointers into static, immutable tables.
unsafe impl Sync for _htmlEntityDesc {}
unsafe impl Send for _htmlEntityDesc {}

/// Build a static `_htmlEntityDesc`.
macro_rules! ent {
    ($value:expr, $name:literal, $desc:literal) => {
        _htmlEntityDesc {
            value: $value,
            name: concat!($name, "\0").as_ptr() as *const c_char,
            desc: concat!($desc, "\0").as_ptr() as *const c_char,
        }
    };
}

/// The HTML 4.0 predefined entities table, faithfully ported from
/// archaeology/libxml2-git/HTMLparser.c `html40EntitiesTable` (sorted by
/// value, matching upstream).
static HTML40_ENTITIES: &[_htmlEntityDesc] = &[
    ent!(34, "quot", "quotation mark = APL quote, U+0022 ISOnum"),
    ent!(38, "amp", "ampersand, U+0026 ISOnum"),
    ent!(39, "apos", "single quote"),
    ent!(60, "lt", "less-than sign, U+003C ISOnum"),
    ent!(62, "gt", "greater-than sign, U+003E ISOnum"),
    ent!(
        160,
        "nbsp",
        "no-break space = non-breaking space, U+00A0 ISOnum"
    ),
    ent!(161, "iexcl", "inverted exclamation mark, U+00A1 ISOnum"),
    ent!(162, "cent", "cent sign, U+00A2 ISOnum"),
    ent!(163, "pound", "pound sign, U+00A3 ISOnum"),
    ent!(164, "curren", "currency sign, U+00A4 ISOnum"),
    ent!(165, "yen", "yen sign = yuan sign, U+00A5 ISOnum"),
    ent!(
        166,
        "brvbar",
        "broken bar = broken vertical bar, U+00A6 ISOnum"
    ),
    ent!(167, "sect", "section sign, U+00A7 ISOnum"),
    ent!(168, "uml", "diaeresis = spacing diaeresis, U+00A8 ISOdia"),
    ent!(169, "copy", "copyright sign, U+00A9 ISOnum"),
    ent!(170, "ordf", "feminine ordinal indicator, U+00AA ISOnum"),
    ent!(
        171,
        "laquo",
        "left-pointing double angle quotation mark = left pointing guillemet, U+00AB ISOnum"
    ),
    ent!(172, "not", "not sign, U+00AC ISOnum"),
    ent!(
        173,
        "shy",
        "soft hyphen = discretionary hyphen, U+00AD ISOnum"
    ),
    ent!(
        174,
        "reg",
        "registered sign = registered trade mark sign, U+00AE ISOnum"
    ),
    ent!(
        175,
        "macr",
        "macron = spacing macron = overline = APL overbar, U+00AF ISOdia"
    ),
    ent!(176, "deg", "degree sign, U+00B0 ISOnum"),
    ent!(
        177,
        "plusmn",
        "plus-minus sign = plus-or-minus sign, U+00B1 ISOnum"
    ),
    ent!(
        178,
        "sup2",
        "superscript two = superscript digit two = squared, U+00B2 ISOnum"
    ),
    ent!(
        179,
        "sup3",
        "superscript three = superscript digit three = cubed, U+00B3 ISOnum"
    ),
    ent!(180, "acute", "acute accent = spacing acute, U+00B4 ISOdia"),
    ent!(181, "micro", "micro sign, U+00B5 ISOnum"),
    ent!(182, "para", "pilcrow sign = paragraph sign, U+00B6 ISOnum"),
    ent!(
        183,
        "middot",
        "middle dot = Georgian comma Greek middle dot, U+00B7 ISOnum"
    ),
    ent!(184, "cedil", "cedilla = spacing cedilla, U+00B8 ISOdia"),
    ent!(
        185,
        "sup1",
        "superscript one = superscript digit one, U+00B9 ISOnum"
    ),
    ent!(186, "ordm", "masculine ordinal indicator, U+00BA ISOnum"),
    ent!(
        187,
        "raquo",
        "right-pointing double angle quotation mark right pointing guillemet, U+00BB ISOnum"
    ),
    ent!(
        188,
        "frac14",
        "vulgar fraction one quarter = fraction one quarter, U+00BC ISOnum"
    ),
    ent!(
        189,
        "frac12",
        "vulgar fraction one half = fraction one half, U+00BD ISOnum"
    ),
    ent!(
        190,
        "frac34",
        "vulgar fraction three quarters = fraction three quarters, U+00BE ISOnum"
    ),
    ent!(
        191,
        "iquest",
        "inverted question mark = turned question mark, U+00BF ISOnum"
    ),
    ent!(
        192,
        "Agrave",
        "latin capital letter A with grave = latin capital letter A grave, U+00C0 ISOlat1"
    ),
    ent!(
        193,
        "Aacute",
        "latin capital letter A with acute, U+00C1 ISOlat1"
    ),
    ent!(
        194,
        "Acirc",
        "latin capital letter A with circumflex, U+00C2 ISOlat1"
    ),
    ent!(
        195,
        "Atilde",
        "latin capital letter A with tilde, U+00C3 ISOlat1"
    ),
    ent!(
        196,
        "Auml",
        "latin capital letter A with diaeresis, U+00C4 ISOlat1"
    ),
    ent!(
        197,
        "Aring",
        "latin capital letter A with ring above = latin capital letter A ring, U+00C5 ISOlat1"
    ),
    ent!(
        198,
        "AElig",
        "latin capital letter AE = latin capital ligature AE, U+00C6 ISOlat1"
    ),
    ent!(
        199,
        "Ccedil",
        "latin capital letter C with cedilla, U+00C7 ISOlat1"
    ),
    ent!(
        200,
        "Egrave",
        "latin capital letter E with grave, U+00C8 ISOlat1"
    ),
    ent!(
        201,
        "Eacute",
        "latin capital letter E with acute, U+00C9 ISOlat1"
    ),
    ent!(
        202,
        "Ecirc",
        "latin capital letter E with circumflex, U+00CA ISOlat1"
    ),
    ent!(
        203,
        "Euml",
        "latin capital letter E with diaeresis, U+00CB ISOlat1"
    ),
    ent!(
        204,
        "Igrave",
        "latin capital letter I with grave, U+00CC ISOlat1"
    ),
    ent!(
        205,
        "Iacute",
        "latin capital letter I with acute, U+00CD ISOlat1"
    ),
    ent!(
        206,
        "Icirc",
        "latin capital letter I with circumflex, U+00CE ISOlat1"
    ),
    ent!(
        207,
        "Iuml",
        "latin capital letter I with diaeresis, U+00CF ISOlat1"
    ),
    ent!(208, "ETH", "latin capital letter ETH, U+00D0 ISOlat1"),
    ent!(
        209,
        "Ntilde",
        "latin capital letter N with tilde, U+00D1 ISOlat1"
    ),
    ent!(
        210,
        "Ograve",
        "latin capital letter O with grave, U+00D2 ISOlat1"
    ),
    ent!(
        211,
        "Oacute",
        "latin capital letter O with acute, U+00D3 ISOlat1"
    ),
    ent!(
        212,
        "Ocirc",
        "latin capital letter O with circumflex, U+00D4 ISOlat1"
    ),
    ent!(
        213,
        "Otilde",
        "latin capital letter O with tilde, U+00D5 ISOlat1"
    ),
    ent!(
        214,
        "Ouml",
        "latin capital letter O with diaeresis, U+00D6 ISOlat1"
    ),
    ent!(215, "times", "multiplication sign, U+00D7 ISOnum"),
    ent!(
        216,
        "Oslash",
        "latin capital letter O with stroke latin capital letter O slash, U+00D8 ISOlat1"
    ),
    ent!(
        217,
        "Ugrave",
        "latin capital letter U with grave, U+00D9 ISOlat1"
    ),
    ent!(
        218,
        "Uacute",
        "latin capital letter U with acute, U+00DA ISOlat1"
    ),
    ent!(
        219,
        "Ucirc",
        "latin capital letter U with circumflex, U+00DB ISOlat1"
    ),
    ent!(
        220,
        "Uuml",
        "latin capital letter U with diaeresis, U+00DC ISOlat1"
    ),
    ent!(
        221,
        "Yacute",
        "latin capital letter Y with acute, U+00DD ISOlat1"
    ),
    ent!(222, "THORN", "latin capital letter THORN, U+00DE ISOlat1"),
    ent!(
        223,
        "szlig",
        "latin small letter sharp s = ess-zed, U+00DF ISOlat1"
    ),
    ent!(
        224,
        "agrave",
        "latin small letter a with grave = latin small letter a grave, U+00E0 ISOlat1"
    ),
    ent!(
        225,
        "aacute",
        "latin small letter a with acute, U+00E1 ISOlat1"
    ),
    ent!(
        226,
        "acirc",
        "latin small letter a with circumflex, U+00E2 ISOlat1"
    ),
    ent!(
        227,
        "atilde",
        "latin small letter a with tilde, U+00E3 ISOlat1"
    ),
    ent!(
        228,
        "auml",
        "latin small letter a with diaeresis, U+00E4 ISOlat1"
    ),
    ent!(
        229,
        "aring",
        "latin small letter a with ring above = latin small letter a ring, U+00E5 ISOlat1"
    ),
    ent!(
        230,
        "aelig",
        "latin small letter ae = latin small ligature ae, U+00E6 ISOlat1"
    ),
    ent!(
        231,
        "ccedil",
        "latin small letter c with cedilla, U+00E7 ISOlat1"
    ),
    ent!(
        232,
        "egrave",
        "latin small letter e with grave, U+00E8 ISOlat1"
    ),
    ent!(
        233,
        "eacute",
        "latin small letter e with acute, U+00E9 ISOlat1"
    ),
    ent!(
        234,
        "ecirc",
        "latin small letter e with circumflex, U+00EA ISOlat1"
    ),
    ent!(
        235,
        "euml",
        "latin small letter e with diaeresis, U+00EB ISOlat1"
    ),
    ent!(
        236,
        "igrave",
        "latin small letter i with grave, U+00EC ISOlat1"
    ),
    ent!(
        237,
        "iacute",
        "latin small letter i with acute, U+00ED ISOlat1"
    ),
    ent!(
        238,
        "icirc",
        "latin small letter i with circumflex, U+00EE ISOlat1"
    ),
    ent!(
        239,
        "iuml",
        "latin small letter i with diaeresis, U+00EF ISOlat1"
    ),
    ent!(240, "eth", "latin small letter eth, U+00F0 ISOlat1"),
    ent!(
        241,
        "ntilde",
        "latin small letter n with tilde, U+00F1 ISOlat1"
    ),
    ent!(
        242,
        "ograve",
        "latin small letter o with grave, U+00F2 ISOlat1"
    ),
    ent!(
        243,
        "oacute",
        "latin small letter o with acute, U+00F3 ISOlat1"
    ),
    ent!(
        244,
        "ocirc",
        "latin small letter o with circumflex, U+00F4 ISOlat1"
    ),
    ent!(
        245,
        "otilde",
        "latin small letter o with tilde, U+00F5 ISOlat1"
    ),
    ent!(
        246,
        "ouml",
        "latin small letter o with diaeresis, U+00F6 ISOlat1"
    ),
    ent!(247, "divide", "division sign, U+00F7 ISOnum"),
    ent!(
        248,
        "oslash",
        "latin small letter o with stroke, = latin small letter o slash, U+00F8 ISOlat1"
    ),
    ent!(
        249,
        "ugrave",
        "latin small letter u with grave, U+00F9 ISOlat1"
    ),
    ent!(
        250,
        "uacute",
        "latin small letter u with acute, U+00FA ISOlat1"
    ),
    ent!(
        251,
        "ucirc",
        "latin small letter u with circumflex, U+00FB ISOlat1"
    ),
    ent!(
        252,
        "uuml",
        "latin small letter u with diaeresis, U+00FC ISOlat1"
    ),
    ent!(
        253,
        "yacute",
        "latin small letter y with acute, U+00FD ISOlat1"
    ),
    ent!(
        254,
        "thorn",
        "latin small letter thorn with, U+00FE ISOlat1"
    ),
    ent!(
        255,
        "yuml",
        "latin small letter y with diaeresis, U+00FF ISOlat1"
    ),
    ent!(338, "OElig", "latin capital ligature OE, U+0152 ISOlat2"),
    ent!(339, "oelig", "latin small ligature oe, U+0153 ISOlat2"),
    ent!(
        352,
        "Scaron",
        "latin capital letter S with caron, U+0160 ISOlat2"
    ),
    ent!(
        353,
        "scaron",
        "latin small letter s with caron, U+0161 ISOlat2"
    ),
    ent!(
        376,
        "Yuml",
        "latin capital letter Y with diaeresis, U+0178 ISOlat2"
    ),
    ent!(
        402,
        "fnof",
        "latin small f with hook = function = florin, U+0192 ISOtech"
    ),
    ent!(
        710,
        "circ",
        "modifier letter circumflex accent, U+02C6 ISOpub"
    ),
    ent!(732, "tilde", "small tilde, U+02DC ISOdia"),
    ent!(913, "Alpha", "greek capital letter alpha, U+0391"),
    ent!(914, "Beta", "greek capital letter beta, U+0392"),
    ent!(915, "Gamma", "greek capital letter gamma, U+0393 ISOgrk3"),
    ent!(916, "Delta", "greek capital letter delta, U+0394 ISOgrk3"),
    ent!(917, "Epsilon", "greek capital letter epsilon, U+0395"),
    ent!(918, "Zeta", "greek capital letter zeta, U+0396"),
    ent!(919, "Eta", "greek capital letter eta, U+0397"),
    ent!(920, "Theta", "greek capital letter theta, U+0398 ISOgrk3"),
    ent!(921, "Iota", "greek capital letter iota, U+0399"),
    ent!(922, "Kappa", "greek capital letter kappa, U+039A"),
    ent!(923, "Lambda", "greek capital letter lambda, U+039B ISOgrk3"),
    ent!(924, "Mu", "greek capital letter mu, U+039C"),
    ent!(925, "Nu", "greek capital letter nu, U+039D"),
    ent!(926, "Xi", "greek capital letter xi, U+039E ISOgrk3"),
    ent!(927, "Omicron", "greek capital letter omicron, U+039F"),
    ent!(928, "Pi", "greek capital letter pi, U+03A0 ISOgrk3"),
    ent!(929, "Rho", "greek capital letter rho, U+03A1"),
    ent!(931, "Sigma", "greek capital letter sigma, U+03A3 ISOgrk3"),
    ent!(932, "Tau", "greek capital letter tau, U+03A4"),
    ent!(
        933,
        "Upsilon",
        "greek capital letter upsilon, U+03A5 ISOgrk3"
    ),
    ent!(934, "Phi", "greek capital letter phi, U+03A6 ISOgrk3"),
    ent!(935, "Chi", "greek capital letter chi, U+03A7"),
    ent!(936, "Psi", "greek capital letter psi, U+03A8 ISOgrk3"),
    ent!(937, "Omega", "greek capital letter omega, U+03A9 ISOgrk3"),
    ent!(945, "alpha", "greek small letter alpha, U+03B1 ISOgrk3"),
    ent!(946, "beta", "greek small letter beta, U+03B2 ISOgrk3"),
    ent!(947, "gamma", "greek small letter gamma, U+03B3 ISOgrk3"),
    ent!(948, "delta", "greek small letter delta, U+03B4 ISOgrk3"),
    ent!(949, "epsilon", "greek small letter epsilon, U+03B5 ISOgrk3"),
    ent!(950, "zeta", "greek small letter zeta, U+03B6 ISOgrk3"),
    ent!(951, "eta", "greek small letter eta, U+03B7 ISOgrk3"),
    ent!(952, "theta", "greek small letter theta, U+03B8 ISOgrk3"),
    ent!(953, "iota", "greek small letter iota, U+03B9 ISOgrk3"),
    ent!(954, "kappa", "greek small letter kappa, U+03BA ISOgrk3"),
    ent!(955, "lambda", "greek small letter lambda, U+03BB ISOgrk3"),
    ent!(956, "mu", "greek small letter mu, U+03BC ISOgrk3"),
    ent!(957, "nu", "greek small letter nu, U+03BD ISOgrk3"),
    ent!(958, "xi", "greek small letter xi, U+03BE ISOgrk3"),
    ent!(959, "omicron", "greek small letter omicron, U+03BF NEW"),
    ent!(960, "pi", "greek small letter pi, U+03C0 ISOgrk3"),
    ent!(961, "rho", "greek small letter rho, U+03C1 ISOgrk3"),
    ent!(
        962,
        "sigmaf",
        "greek small letter final sigma, U+03C2 ISOgrk3"
    ),
    ent!(963, "sigma", "greek small letter sigma, U+03C3 ISOgrk3"),
    ent!(964, "tau", "greek small letter tau, U+03C4 ISOgrk3"),
    ent!(965, "upsilon", "greek small letter upsilon, U+03C5 ISOgrk3"),
    ent!(966, "phi", "greek small letter phi, U+03C6 ISOgrk3"),
    ent!(967, "chi", "greek small letter chi, U+03C7 ISOgrk3"),
    ent!(968, "psi", "greek small letter psi, U+03C8 ISOgrk3"),
    ent!(969, "omega", "greek small letter omega, U+03C9 ISOgrk3"),
    ent!(
        977,
        "thetasym",
        "greek small letter theta symbol, U+03D1 NEW"
    ),
    ent!(978, "upsih", "greek upsilon with hook symbol, U+03D2 NEW"),
    ent!(982, "piv", "greek pi symbol, U+03D6 ISOgrk3"),
    ent!(8194, "ensp", "en space, U+2002 ISOpub"),
    ent!(8195, "emsp", "em space, U+2003 ISOpub"),
    ent!(8201, "thinsp", "thin space, U+2009 ISOpub"),
    ent!(8204, "zwnj", "zero width non-joiner, U+200C NEW RFC 2070"),
    ent!(8205, "zwj", "zero width joiner, U+200D NEW RFC 2070"),
    ent!(8206, "lrm", "left-to-right mark, U+200E NEW RFC 2070"),
    ent!(8207, "rlm", "right-to-left mark, U+200F NEW RFC 2070"),
    ent!(8211, "ndash", "en dash, U+2013 ISOpub"),
    ent!(8212, "mdash", "em dash, U+2014 ISOpub"),
    ent!(8216, "lsquo", "left single quotation mark, U+2018 ISOnum"),
    ent!(8217, "rsquo", "right single quotation mark, U+2019 ISOnum"),
    ent!(8218, "sbquo", "single low-9 quotation mark, U+201A NEW"),
    ent!(8220, "ldquo", "left double quotation mark, U+201C ISOnum"),
    ent!(8221, "rdquo", "right double quotation mark, U+201D ISOnum"),
    ent!(8222, "bdquo", "double low-9 quotation mark, U+201E NEW"),
    ent!(8224, "dagger", "dagger, U+2020 ISOpub"),
    ent!(8225, "Dagger", "double dagger, U+2021 ISOpub"),
    ent!(8226, "bull", "bullet = black small circle, U+2022 ISOpub"),
    ent!(
        8230,
        "hellip",
        "horizontal ellipsis = three dot leader, U+2026 ISOpub"
    ),
    ent!(8240, "permil", "per mille sign, U+2030 ISOtech"),
    ent!(8242, "prime", "prime = minutes = feet, U+2032 ISOtech"),
    ent!(
        8243,
        "Prime",
        "double prime = seconds = inches, U+2033 ISOtech"
    ),
    ent!(
        8249,
        "lsaquo",
        "single left-pointing angle quotation mark, U+2039 ISO proposed"
    ),
    ent!(
        8250,
        "rsaquo",
        "single right-pointing angle quotation mark, U+203A ISO proposed"
    ),
    ent!(8254, "oline", "overline = spacing overscore, U+203E NEW"),
    ent!(8260, "frasl", "fraction slash, U+2044 NEW"),
    ent!(8364, "euro", "euro sign, U+20AC NEW"),
    ent!(
        8465,
        "image",
        "blackletter capital I = imaginary part, U+2111 ISOamso"
    ),
    ent!(
        8472,
        "weierp",
        "script capital P = power set = Weierstrass p, U+2118 ISOamso"
    ),
    ent!(
        8476,
        "real",
        "blackletter capital R = real part symbol, U+211C ISOamso"
    ),
    ent!(8482, "trade", "trade mark sign, U+2122 ISOnum"),
    ent!(
        8501,
        "alefsym",
        "alef symbol = first transfinite cardinal, U+2135 NEW"
    ),
    ent!(8592, "larr", "leftwards arrow, U+2190 ISOnum"),
    ent!(8593, "uarr", "upwards arrow, U+2191 ISOnum"),
    ent!(8594, "rarr", "rightwards arrow, U+2192 ISOnum"),
    ent!(8595, "darr", "downwards arrow, U+2193 ISOnum"),
    ent!(8596, "harr", "left right arrow, U+2194 ISOamsa"),
    ent!(
        8629,
        "crarr",
        "downwards arrow with corner leftwards = carriage return, U+21B5 NEW"
    ),
    ent!(8656, "lArr", "leftwards double arrow, U+21D0 ISOtech"),
    ent!(8657, "uArr", "upwards double arrow, U+21D1 ISOamsa"),
    ent!(8658, "rArr", "rightwards double arrow, U+21D2 ISOtech"),
    ent!(8659, "dArr", "downwards double arrow, U+21D3 ISOamsa"),
    ent!(8660, "hArr", "left right double arrow, U+21D4 ISOamsa"),
    ent!(8704, "forall", "for all, U+2200 ISOtech"),
    ent!(8706, "part", "partial differential, U+2202 ISOtech"),
    ent!(8707, "exist", "there exists, U+2203 ISOtech"),
    ent!(
        8709,
        "empty",
        "empty set = null set = diameter, U+2205 ISOamso"
    ),
    ent!(8711, "nabla", "nabla = backward difference, U+2207 ISOtech"),
    ent!(8712, "isin", "element of, U+2208 ISOtech"),
    ent!(8713, "notin", "not an element of, U+2209 ISOtech"),
    ent!(8715, "ni", "contains as member, U+220B ISOtech"),
    ent!(8719, "prod", "n-ary product = product sign, U+220F ISOamsb"),
    ent!(8721, "sum", "n-ary summation, U+2211 ISOamsb"),
    ent!(8722, "minus", "minus sign, U+2212 ISOtech"),
    ent!(8727, "lowast", "asterisk operator, U+2217 ISOtech"),
    ent!(8730, "radic", "square root = radical sign, U+221A ISOtech"),
    ent!(8733, "prop", "proportional to, U+221D ISOtech"),
    ent!(8734, "infin", "infinity, U+221E ISOtech"),
    ent!(8736, "ang", "angle, U+2220 ISOamso"),
    ent!(8743, "and", "logical and = wedge, U+2227 ISOtech"),
    ent!(8744, "or", "logical or = vee, U+2228 ISOtech"),
    ent!(8745, "cap", "intersection = cap, U+2229 ISOtech"),
    ent!(8746, "cup", "union = cup, U+222A ISOtech"),
    ent!(8747, "int", "integral, U+222B ISOtech"),
    ent!(8756, "there4", "therefore, U+2234 ISOtech"),
    ent!(
        8764,
        "sim",
        "tilde operator = varies with = similar to, U+223C ISOtech"
    ),
    ent!(8773, "cong", "approximately equal to, U+2245 ISOtech"),
    ent!(
        8776,
        "asymp",
        "almost equal to = asymptotic to, U+2248 ISOamsr"
    ),
    ent!(8800, "ne", "not equal to, U+2260 ISOtech"),
    ent!(8801, "equiv", "identical to, U+2261 ISOtech"),
    ent!(8804, "le", "less-than or equal to, U+2264 ISOtech"),
    ent!(8805, "ge", "greater-than or equal to, U+2265 ISOtech"),
    ent!(8834, "sub", "subset of, U+2282 ISOtech"),
    ent!(8835, "sup", "superset of, U+2283 ISOtech"),
    ent!(8836, "nsub", "not a subset of, U+2284 ISOamsn"),
    ent!(8838, "sube", "subset of or equal to, U+2286 ISOtech"),
    ent!(8839, "supe", "superset of or equal to, U+2287 ISOtech"),
    ent!(8853, "oplus", "circled plus = direct sum, U+2295 ISOamsb"),
    ent!(
        8855,
        "otimes",
        "circled times = vector product, U+2297 ISOamsb"
    ),
    ent!(
        8869,
        "perp",
        "up tack = orthogonal to = perpendicular, U+22A5 ISOtech"
    ),
    ent!(8901, "sdot", "dot operator, U+22C5 ISOamsb"),
    ent!(8968, "lceil", "left ceiling = apl upstile, U+2308 ISOamsc"),
    ent!(8969, "rceil", "right ceiling, U+2309 ISOamsc"),
    ent!(8970, "lfloor", "left floor = apl downstile, U+230A ISOamsc"),
    ent!(8971, "rfloor", "right floor, U+230B ISOamsc"),
    ent!(
        9001,
        "lang",
        "left-pointing angle bracket = bra, U+2329 ISOtech"
    ),
    ent!(
        9002,
        "rang",
        "right-pointing angle bracket = ket, U+232A ISOtech"
    ),
    ent!(9674, "loz", "lozenge, U+25CA ISOpub"),
    ent!(9824, "spades", "black spade suit, U+2660 ISOpub"),
    ent!(9827, "clubs", "black club suit = shamrock, U+2663 ISOpub"),
    ent!(
        9829,
        "hearts",
        "black heart suit = valentine, U+2665 ISOpub"
    ),
    ent!(9830, "diams", "black diamond suit, U+2666 ISOpub"),
];

/// Lookup the given entity in the entities table.
///
/// Upstream scans linearly with `xmlStrEqual`; the same table is scanned
/// here.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const htmlEntityDesc *htmlEntityLookup(const xmlChar *name);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlEntityLookup(name: *const xmlChar) -> *const _htmlEntityDesc {
    if name.is_null() {
        return ptr::null();
    }
    let bytes = unsafe { xmlstr_to_bytes(name) };
    for e in HTML40_ENTITIES {
        let ename = unsafe { core::ffi::CStr::from_ptr(e.name) }.to_bytes();
        if bytes == ename {
            return e as *const _htmlEntityDesc;
        }
    }
    ptr::null()
}

/// Lookup the given entity by unicode value.
///
/// Upstream binary-searches the value-sorted table; the same (sorted) table
/// is scanned linearly here.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const htmlEntityDesc *htmlEntityValueLookup(unsigned int value);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlEntityValueLookup(value: c_uint) -> *const _htmlEntityDesc {
    for e in HTML40_ENTITIES {
        if e.value == value {
            return e as *const _htmlEntityDesc;
        }
    }
    ptr::null()
}

/// Lookup helper used by `htmlEncodeEntities` (avoids the extern entry point
/// in the hot path; identical semantics).
#[inline]
unsafe fn html_entity_value_lookup_static(value: c_uint) -> *const _htmlEntityDesc {
    for e in HTML40_ENTITIES {
        if e.value == value {
            return e as *const _htmlEntityDesc;
        }
    }
    ptr::null()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Auto-close element rules (HTMLparser.c htmlStartClose / htmlScriptAttributes)
// ═══════════════════════════════════════════════════════════════════════════════

/// Start tags that imply the end of the current element
/// (archaeology/libxml2-git/HTMLparser.c `htmlStartClose`). Each pair
/// `(old, new)` means: starting element `new` implicitly closes `old`.
static HTML_START_CLOSE: &[(&str, &str)] = &[
    ("a", "a"),
    ("a", "fieldset"),
    ("a", "table"),
    ("a", "td"),
    ("a", "th"),
    ("address", "dd"),
    ("address", "dl"),
    ("address", "dt"),
    ("address", "form"),
    ("address", "li"),
    ("address", "ul"),
    ("b", "center"),
    ("b", "p"),
    ("b", "td"),
    ("b", "th"),
    ("big", "p"),
    ("caption", "col"),
    ("caption", "colgroup"),
    ("caption", "tbody"),
    ("caption", "tfoot"),
    ("caption", "thead"),
    ("caption", "tr"),
    ("col", "col"),
    ("col", "colgroup"),
    ("col", "tbody"),
    ("col", "tfoot"),
    ("col", "thead"),
    ("col", "tr"),
    ("colgroup", "colgroup"),
    ("colgroup", "tbody"),
    ("colgroup", "tfoot"),
    ("colgroup", "thead"),
    ("colgroup", "tr"),
    ("dd", "dt"),
    ("dir", "dd"),
    ("dir", "dl"),
    ("dir", "dt"),
    ("dir", "form"),
    ("dir", "ul"),
    ("dl", "form"),
    ("dl", "li"),
    ("dt", "dd"),
    ("dt", "dl"),
    ("font", "center"),
    ("font", "td"),
    ("font", "th"),
    ("form", "form"),
    ("h1", "fieldset"),
    ("h1", "form"),
    ("h1", "li"),
    ("h1", "p"),
    ("h1", "table"),
    ("h2", "fieldset"),
    ("h2", "form"),
    ("h2", "li"),
    ("h2", "p"),
    ("h2", "table"),
    ("h3", "fieldset"),
    ("h3", "form"),
    ("h3", "li"),
    ("h3", "p"),
    ("h3", "table"),
    ("h4", "fieldset"),
    ("h4", "form"),
    ("h4", "li"),
    ("h4", "p"),
    ("h4", "table"),
    ("h5", "fieldset"),
    ("h5", "form"),
    ("h5", "li"),
    ("h5", "p"),
    ("h5", "table"),
    ("h6", "fieldset"),
    ("h6", "form"),
    ("h6", "li"),
    ("h6", "p"),
    ("h6", "table"),
    ("head", "a"),
    ("head", "abbr"),
    ("head", "acronym"),
    ("head", "address"),
    ("head", "b"),
    ("head", "bdo"),
    ("head", "big"),
    ("head", "blockquote"),
    ("head", "body"),
    ("head", "br"),
    ("head", "center"),
    ("head", "cite"),
    ("head", "code"),
    ("head", "dd"),
    ("head", "dfn"),
    ("head", "dir"),
    ("head", "div"),
    ("head", "dl"),
    ("head", "dt"),
    ("head", "em"),
    ("head", "fieldset"),
    ("head", "font"),
    ("head", "form"),
    ("head", "frameset"),
    ("head", "h1"),
    ("head", "h2"),
    ("head", "h3"),
    ("head", "h4"),
    ("head", "h5"),
    ("head", "h6"),
    ("head", "hr"),
    ("head", "i"),
    ("head", "iframe"),
    ("head", "img"),
    ("head", "kbd"),
    ("head", "li"),
    ("head", "listing"),
    ("head", "map"),
    ("head", "menu"),
    ("head", "ol"),
    ("head", "p"),
    ("head", "pre"),
    ("head", "q"),
    ("head", "s"),
    ("head", "samp"),
    ("head", "small"),
    ("head", "span"),
    ("head", "strike"),
    ("head", "strong"),
    ("head", "sub"),
    ("head", "sup"),
    ("head", "table"),
    ("head", "tt"),
    ("head", "u"),
    ("head", "ul"),
    ("head", "var"),
    ("head", "xmp"),
    ("hr", "form"),
    ("i", "center"),
    ("i", "p"),
    ("i", "td"),
    ("i", "th"),
    ("legend", "fieldset"),
    ("li", "li"),
    ("link", "body"),
    ("link", "frameset"),
    ("listing", "dd"),
    ("listing", "dl"),
    ("listing", "dt"),
    ("listing", "fieldset"),
    ("listing", "form"),
    ("listing", "li"),
    ("listing", "table"),
    ("listing", "ul"),
    ("menu", "dd"),
    ("menu", "dl"),
    ("menu", "dt"),
    ("menu", "form"),
    ("menu", "ul"),
    ("ol", "form"),
    ("option", "optgroup"),
    ("option", "option"),
    ("p", "address"),
    ("p", "blockquote"),
    ("p", "body"),
    ("p", "caption"),
    ("p", "center"),
    ("p", "col"),
    ("p", "colgroup"),
    ("p", "dd"),
    ("p", "dir"),
    ("p", "div"),
    ("p", "dl"),
    ("p", "dt"),
    ("p", "fieldset"),
    ("p", "form"),
    ("p", "frameset"),
    ("p", "h1"),
    ("p", "h2"),
    ("p", "h3"),
    ("p", "h4"),
    ("p", "h5"),
    ("p", "h6"),
    ("p", "head"),
    ("p", "hr"),
    ("p", "li"),
    ("p", "listing"),
    ("p", "menu"),
    ("p", "ol"),
    ("p", "p"),
    ("p", "pre"),
    ("p", "table"),
    ("p", "tbody"),
    ("p", "td"),
    ("p", "tfoot"),
    ("p", "th"),
    ("p", "title"),
    ("p", "tr"),
    ("p", "ul"),
    ("p", "xmp"),
    ("pre", "dd"),
    ("pre", "dl"),
    ("pre", "dt"),
    ("pre", "fieldset"),
    ("pre", "form"),
    ("pre", "li"),
    ("pre", "table"),
    ("pre", "ul"),
    ("s", "p"),
    ("script", "noscript"),
    ("small", "p"),
    ("span", "td"),
    ("span", "th"),
    ("strike", "p"),
    ("style", "body"),
    ("style", "frameset"),
    ("tbody", "tbody"),
    ("tbody", "tfoot"),
    ("td", "tbody"),
    ("td", "td"),
    ("td", "tfoot"),
    ("td", "th"),
    ("td", "tr"),
    ("tfoot", "tbody"),
    ("th", "tbody"),
    ("th", "td"),
    ("th", "tfoot"),
    ("th", "th"),
    ("th", "tr"),
    ("thead", "tbody"),
    ("thead", "tfoot"),
    ("title", "body"),
    ("title", "frameset"),
    ("tr", "tbody"),
    ("tr", "tfoot"),
    ("tr", "tr"),
    ("tt", "p"),
    ("u", "p"),
    ("u", "td"),
    ("u", "th"),
    ("ul", "address"),
    ("ul", "form"),
    ("ul", "menu"),
    ("ul", "pre"),
    ("xmp", "dd"),
    ("xmp", "dl"),
    ("xmp", "dt"),
    ("xmp", "fieldset"),
    ("xmp", "form"),
    ("xmp", "li"),
    ("xmp", "table"),
    ("xmp", "ul"),
];

/// Checks whether the new tag is one of the registered valid tags for
/// closing old (upstream `htmlCheckAutoClose`). Exact, case-sensitive
/// byte comparison, like the upstream `strcmp`-based binary search.
unsafe fn html_check_auto_close(newtag: *const xmlChar, oldtag: *const xmlChar) -> bool {
    if newtag.is_null() || oldtag.is_null() {
        return false;
    }
    let new_bytes = unsafe { xmlstr_to_bytes(newtag) };
    let old_bytes = unsafe { xmlstr_to_bytes(oldtag) };
    HTML_START_CLOSE
        .iter()
        .any(|(old, new)| old.as_bytes() == old_bytes && new.as_bytes() == new_bytes)
}

/// The HTML DTD allows a tag to implicitly close other tags. This function
/// checks if the element or one of its children would auto-close the given
/// tag.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int htmlAutoCloseTag(xmlDoc *doc, const xmlChar *name, xmlNode *elem);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlAutoCloseTag(
    _doc: *mut _xmlDoc,
    name: *const xmlChar,
    elem: *mut _xmlNode,
) -> c_int {
    if elem.is_null() {
        return 1;
    }
    let n = unsafe { &*elem };
    if n.name.is_null() {
        // Upstream compares against elem->name; a nameless node cannot
        // match, fall through to the children scan.
    } else if unsafe { xml_strcmp(name, n.name) } == 0 {
        return 0;
    }
    if unsafe { html_check_auto_close(n.name, name) } {
        return 1;
    }
    let mut child = n.children;
    while !child.is_null() {
        if unsafe { htmlAutoCloseTag(_doc, name, child) } != 0 {
            return 1;
        }
        child = unsafe { (*child).next };
    }
    0
}

/// The HTML DTD allows a tag to implicitly close other tags. This function
/// checks if a tag is auto-closed by one of its children.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int htmlIsAutoClosed(xmlDoc *doc, xmlNode *elem);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlIsAutoClosed(doc: *mut _xmlDoc, elem: *mut _xmlNode) -> c_int {
    if elem.is_null() {
        return 1;
    }
    let n = unsafe { &*elem };
    let mut child = n.children;
    while !child.is_null() {
        if unsafe { htmlAutoCloseTag(doc, n.name, child) } != 0 {
            return 1;
        }
        child = unsafe { (*child).next };
    }
    0
}

/// The list of HTML attributes which are of content type %Script;
/// (HTMLparser.c `htmlScriptAttributes`).
static HTML_SCRIPT_ATTRIBUTES: &[&str] = &[
    "onclick",
    "ondblclick",
    "onmousedown",
    "onmouseup",
    "onmouseover",
    "onmousemove",
    "onmouseout",
    "onkeypress",
    "onkeydown",
    "onkeyup",
    "onload",
    "onunload",
    "onfocus",
    "onblur",
    "onsubmit",
    "onreset",
    "onchange",
    "onselect",
];

/// Check if an attribute is of content type Script. All script attributes
/// start with 'on'.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int htmlIsScriptAttribute(const xmlChar *name);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlIsScriptAttribute(name: *const xmlChar) -> c_int {
    if name.is_null() {
        return 0;
    }
    let bytes = unsafe { xmlstr_to_bytes(name) };
    if bytes.len() < 3 || bytes[0] != b'o' || bytes[1] != b'n' {
        return 0;
    }
    for cand in HTML_SCRIPT_ATTRIBUTES {
        if bytes == cand.as_bytes() {
            return 1;
        }
    }
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// Element-rule stubs (upstream 2.14+ deprecated content-model functions)
// ═══════════════════════════════════════════════════════════════════════════════

/// # UPSTREAM-PARITY
///
/// ```c
/// int htmlElementAllowedHere(const htmlElemDesc *parent, const xmlChar *elt);
/// ```
///
/// Upstream is a deprecated stub returning 1 unconditionally.
#[no_mangle]
pub unsafe extern "C" fn htmlElementAllowedHere(
    _parent: *const _htmlElemDesc,
    _elt: *const xmlChar,
) -> c_int {
    1
}

/// # UPSTREAM-PARITY
///
/// ```c
/// htmlStatus htmlElementStatusHere(const htmlElemDesc *parent, const htmlElemDesc *elt);
/// ```
///
/// Upstream is a deprecated stub returning HTML_VALID unconditionally.
#[no_mangle]
pub unsafe extern "C" fn htmlElementStatusHere(
    _parent: *const _htmlElemDesc,
    _elt: *const _htmlElemDesc,
) -> c_int {
    HTML_VALID
}

/// # UPSTREAM-PARITY
///
/// ```c
/// htmlStatus htmlAttrAllowed(const htmlElemDesc *elt, const xmlChar *attr, int legacy);
/// ```
///
/// Upstream is a deprecated stub returning HTML_VALID unconditionally.
#[no_mangle]
pub unsafe extern "C" fn htmlAttrAllowed(
    _elt: *const _htmlElemDesc,
    _attr: *const xmlChar,
    _legacy: c_int,
) -> c_int {
    HTML_VALID
}

/// # UPSTREAM-PARITY
///
/// ```c
/// htmlStatus htmlNodeStatus(xmlNode *node, int legacy);
/// ```
///
/// Upstream is a deprecated stub returning HTML_VALID unconditionally.
#[no_mangle]
pub unsafe extern "C" fn htmlNodeStatus(_node: *mut _xmlNode, _legacy: c_int) -> c_int {
    HTML_VALID
}

// ═══════════════════════════════════════════════════════════════════════════════
// Encoding helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Take a block of UTF-8 chars in and try to convert it to an ASCII plus
/// HTML entities block of chars out. Ported from
/// archaeology/libxml2-git/HTMLparser.c `htmlEncodeEntities`.
///
/// Returns 0 if success, -2 if the transcoding fails, or -1 otherwise.
/// `inlen` after return is the number of octets consumed; `outlen` the
/// number of octets produced.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int htmlEncodeEntities(unsigned char *out, int *outlen,
///                        const unsigned char *in, int *inlen, int quoteChar);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlEncodeEntities(
    out: *mut u8,
    outlen: *mut c_int,
    input: *const u8,
    inlen: *mut c_int,
    quoteChar: c_int,
) -> c_int {
    if out.is_null() || outlen.is_null() || inlen.is_null() || input.is_null() {
        return -1;
    }
    let outend = (out as usize).wrapping_add((*outlen).max(0) as usize);
    let inend = (input as usize).wrapping_add((*inlen).max(0) as usize);
    let mut in_ptr = input as usize;
    let mut out_ptr = out as usize;
    let mut processed = in_ptr;

    while in_ptr < inend {
        let mut c: c_uint;
        let d: c_uint;
        let mut trailing: c_int;

        d = unsafe { *(in_ptr as *const u8) as c_uint };
        in_ptr += 1;
        if d < 0x80 {
            c = d;
            trailing = 0;
        } else if d < 0xC0 {
            // trailing byte in leading position
            *outlen = (out_ptr - out as usize) as c_int;
            *inlen = (processed - input as usize) as c_int;
            return -2;
        } else if d < 0xE0 {
            c = d & 0x1F;
            trailing = 1;
        } else if d < 0xF0 {
            c = d & 0x0F;
            trailing = 2;
        } else if d < 0xF8 {
            c = d & 0x07;
            trailing = 3;
        } else {
            // no chance for this in Ascii
            *outlen = (out_ptr - out as usize) as c_int;
            *inlen = (processed - input as usize) as c_int;
            return -2;
        }

        if inend - in_ptr < trailing as usize {
            break;
        }

        while trailing > 0 {
            let t = unsafe { *(in_ptr as *const u8) as c_uint };
            in_ptr += 1;
            if (t & 0xC0) != 0x80 {
                *outlen = (out_ptr - out as usize) as c_int;
                *inlen = (processed - input as usize) as c_int;
                return -2;
            }
            c = (c << 6) | (t & 0x3F);
            trailing -= 1;
        }

        // assertion: c is a single UTF-4 value
        if (c < 0x80)
            && (c != quoteChar as c_uint)
            && (c != b'&' as c_uint)
            && (c != b'<' as c_uint)
            && (c != b'>' as c_uint)
        {
            if out_ptr >= outend {
                break;
            }
            unsafe { *(out_ptr as *mut u8) = c as u8 };
            out_ptr += 1;
        } else {
            let ent = unsafe { html_entity_value_lookup_static(c) };
            let mut nbuf = [0u8; 16];
            let (cp, len): (*const u8, usize) = if ent.is_null() {
                // snprintf(nbuf, sizeof(nbuf), "#%u", c)
                nbuf[0] = b'#';
                let mut i = 1usize;
                let mut digits = [0u8; 10];
                let mut nd = 0usize;
                let mut v = c;
                if v == 0 {
                    digits[0] = b'0';
                    nd = 1;
                }
                while v > 0 {
                    digits[nd] = b'0' + (v % 10) as u8;
                    nd += 1;
                    v /= 10;
                }
                while nd > 0 {
                    nd -= 1;
                    nbuf[i] = digits[nd];
                    i += 1;
                }
                (nbuf.as_ptr(), i)
            } else {
                (unsafe { (*ent).name } as *const u8, unsafe {
                    xml_strlen((*ent).name as *const xmlChar)
                })
            };
            if outend - out_ptr < len + 2 {
                break;
            }
            unsafe {
                *(out_ptr as *mut u8) = b'&';
                ptr::copy_nonoverlapping(cp, (out_ptr + 1) as *mut u8, len);
                *((out_ptr + 1 + len) as *mut u8) = b';';
            }
            out_ptr += len + 2;
        }
        processed = in_ptr;
    }

    *outlen = (out_ptr - out as usize) as c_int;
    *inlen = (processed - input as usize) as c_int;
    0
}

/// Substitute the HTML entities by their value.
///
/// DEPRECATED in upstream: since 2.13.0 the function lives in legacy.c and
/// emits a one-time diagnostic before returning NULL (oracle-verified
/// behavior).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *htmlDecodeEntities(htmlParserCtxtPtr ctxt, int len,
///                             xmlChar end, xmlChar end2, xmlChar end3);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlDecodeEntities(
    _ctxt: *mut c_void,
    _len: c_int,
    _end: xmlChar,
    _end2: xmlChar,
    _end3: xmlChar,
) -> *mut xmlChar {
    static DEPRECATED: AtomicBool = AtomicBool::new(false);
    if !DEPRECATED.swap(true, Ordering::Relaxed) {
        // Match the oracle: one-time "deprecated" diagnostic on stderr.
        let msg = b"htmlDecodeEntities() deprecated function reached\n";
        unsafe {
            libc::fwrite(
                msg.as_ptr() as *const c_void,
                1,
                msg.len(),
                libc::fdopen(2, b"w\0" as *const u8 as *const c_char) as *mut libc::FILE,
            );
        }
    }
    ptr::null_mut()
}

/// Determine if a given attribute is a boolean attribute (HTMLtree.c
/// `htmlIsBooleanAttr`): ported decision tree over the XSLT 1.0 16.2
/// minimized-form attributes.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int htmlIsBooleanAttr(const xmlChar *name);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlIsBooleanAttr(name: *const xmlChar) -> c_int {
    if name.is_null() {
        return 0;
    }
    let b = unsafe { xmlstr_to_bytes(name) };
    if b.is_empty() {
        return 0;
    }
    let mut i = 0usize;
    let mut suffix: Option<&'static [u8]> = None;
    match b[i].to_ascii_lowercase() {
        b'c' => {
            i += 1;
            match b.get(i).map(|&x| x.to_ascii_lowercase()) {
                Some(b'h') => suffix = Some(b"ecked"),
                Some(b'o') => suffix = Some(b"mpact"),
                _ => {}
            }
        }
        b'd' => {
            i += 1;
            match b.get(i).map(|&x| x.to_ascii_lowercase()) {
                Some(b'e') => {
                    i += 1;
                    match b.get(i).map(|&x| x.to_ascii_lowercase()) {
                        Some(b'c') => suffix = Some(b"lare"),
                        Some(b'f') => suffix = Some(b"er"),
                        _ => {}
                    }
                }
                Some(b'i') => suffix = Some(b"sabled"),
                _ => {}
            }
        }
        b'i' => suffix = Some(b"smap"),
        b'm' => suffix = Some(b"ultiple"),
        b'n' => {
            i += 1;
            if b.get(i).map(|&x| x.to_ascii_lowercase()) == Some(b'o') {
                i += 1;
                match b.get(i).map(|&x| x.to_ascii_lowercase()) {
                    Some(b'h') => suffix = Some(b"ref"),
                    Some(b'r') => suffix = Some(b"esize"),
                    Some(b's') => suffix = Some(b"hade"),
                    Some(b'w') => suffix = Some(b"rap"),
                    _ => {}
                }
            }
        }
        b'r' => suffix = Some(b"eadonly"),
        b's' => suffix = Some(b"elected"),
        _ => {}
    }
    let Some(suffix) = suffix else {
        return 0;
    };
    if b.len() == i + 1 + suffix.len() && b[i + 1..].eq_ignore_ascii_case(suffix) {
        1
    } else {
        0
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Global switches / initializers
// ═══════════════════════════════════════════════════════════════════════════════

/// Global `htmlOmittedDefaultValue` mirroring the upstream static in
/// HTMLparser.c (initialized to 1).
static HTML_OMITTED_DEFAULT_VALUE: AtomicI32 = AtomicI32::new(1);

/// Set and return the previous value for handling HTML omitted tags.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int htmlHandleOmittedElem(int val);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlHandleOmittedElem(val: c_int) -> c_int {
    HTML_OMITTED_DEFAULT_VALUE.swap(val, Ordering::Relaxed)
}

/// Upstream `htmlInitAutoClose` is a deprecated no-op.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void htmlInitAutoClose(void);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlInitAutoClose() {
    // Deprecated no-op (HTMLparser.c).
}

/// Initialize the htmlDefaultSAXHandler global (upstream SAX2.c
/// `htmlDefaultSAXHandlerInit`). The candidate's `htmlDefaultSAXHandler`
/// data symbol (data_globals.rs) is initialized statically, so this is a
/// no-op for ABI compatibility — same convention as `xmlDefaultSAXHandlerInit`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void htmlDefaultSAXHandlerInit(void);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlDefaultSAXHandlerInit() {
    // The exported htmlDefaultSAXHandler global is statically initialized.
}

// ═══════════════════════════════════════════════════════════════════════════════
// Deprecated parser entry points (upstream stubs)
// ═══════════════════════════════════════════════════════════════════════════════

/// Upstream `htmlParseEntityRef` is a deprecated stub returning NULL.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const htmlEntityDesc *htmlParseEntityRef(htmlParserCtxt *ctxt, const xmlChar **str);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlParseEntityRef(
    _ctxt: *mut c_void,
    _str: *mut *const xmlChar,
) -> *const _htmlEntityDesc {
    ptr::null()
}

/// Upstream `htmlParseCharRef` is a deprecated stub returning 0.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int htmlParseCharRef(htmlParserCtxt *ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlParseCharRef(_ctxt: *mut c_void) -> c_int {
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// HTML parser contexts
// ═══════════════════════════════════════════════════════════════════════════════

/// Opaque HTML parser context.
///
/// # Layout contract
///
/// The exported `htmlFreeParserCtxt` (src/abi/exports_xml2.rs) routes to
/// `crate::xml::html::free_parser_ctxt`, which interprets the pointer as the
/// internal `HtmlParserCtxt` and frees its `filename`/`encoding` fields and
/// the block itself. The field prefix below therefore mirrors
/// `HtmlParserCtxt`'s declaration **exactly** (same field types, same order,
/// same default Rust representation — NOT `repr(C)`), which places
/// `filename`/`encoding` at the same byte offsets as the internal struct
/// (64 / 72 on 64-bit; verified empirically). The trailing fields are ABI
/// state that the internal module never touches.
struct HtmlOpaqueCtxt {
    // ── prefix mirroring xml::html::HtmlParserCtxt ──────────────────────
    doc: *mut _xmlDoc,
    current: *mut _xmlNode,
    html: *mut _xmlNode,
    head: *mut _xmlNode,
    body: *mut _xmlNode,
    in_head: bool,
    in_body: bool,
    html_created: bool,
    head_created: bool,
    body_created: bool,
    seen_body_content: bool,
    /// Accumulated push/input buffer (owned by the context).
    input: *mut u8,
    input_pos: usize,
    input_len: usize,
    line: c_int,
    err: bool,
    filename: *mut c_char,
    encoding: *mut c_char,
    // ── ABI state (not touched by the internal module) ──────────────────
    options: c_int,
    sax: *mut _xmlSAXHandler,
    user_data: *mut c_void,
}

/// Allocate a zero-initialized HTML parser context with the internal
/// `HtmlParserCtxt`-compatible prefix. Freed by `htmlFreeParserCtxt`.
unsafe fn html_ctxt_alloc() -> *mut HtmlOpaqueCtxt {
    let mem = xmlMallocZero(size_of::<HtmlOpaqueCtxt>()) as *mut HtmlOpaqueCtxt;
    if mem.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::write(
            mem,
            HtmlOpaqueCtxt {
                doc: ptr::null_mut(),
                current: ptr::null_mut(),
                html: ptr::null_mut(),
                head: ptr::null_mut(),
                body: ptr::null_mut(),
                in_head: false,
                in_body: false,
                html_created: false,
                head_created: false,
                body_created: false,
                seen_body_content: false,
                input: ptr::null_mut(),
                input_pos: 0,
                input_len: 0,
                line: 1,
                err: false,
                filename: ptr::null_mut(),
                encoding: ptr::null_mut(),
                options: 0,
                sax: ptr::null_mut(),
                user_data: ptr::null_mut(),
            },
        );
    }
    mem
}

/// Store a copy of `buffer` (len bytes) as the context's input buffer.
unsafe fn html_ctxt_set_input(ctxt: *mut HtmlOpaqueCtxt, buffer: *const c_char, size: c_int) {
    if buffer.is_null() || size <= 0 {
        return;
    }
    let len = size as usize;
    let nb = xmlMallocImpl(len) as *mut u8;
    if nb.is_null() {
        return;
    }
    unsafe {
        ptr::copy_nonoverlapping(buffer as *const u8, nb, len);
        (*ctxt).input = nb;
        (*ctxt).input_len = len;
        (*ctxt).input_pos = 0;
    }
}

/// Allocate and initialize a new HTML SAX parser context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// htmlParserCtxt *htmlNewSAXParserCtxt(const htmlSAXHandler *sax, void *userData);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlNewSAXParserCtxt(
    sax: *const _xmlSAXHandler,
    userData: *mut c_void,
) -> *mut c_void {
    let ctxt = unsafe { html_ctxt_alloc() };
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        (*ctxt).sax = sax as *mut _xmlSAXHandler;
        (*ctxt).user_data = userData;
    }
    ctxt as *mut c_void
}

/// Allocate and initialize a new HTML parser context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// htmlParserCtxt *htmlNewParserCtxt(void);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlNewParserCtxt() -> *mut c_void {
    unsafe { htmlNewSAXParserCtxt(ptr::null(), ptr::null_mut()) }
}

/// Create a parser context for an HTML in-memory document. The input buffer
/// must not contain any terminating null bytes.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// htmlParserCtxt *htmlCreateMemoryParserCtxt(const char *buffer, int size);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlCreateMemoryParserCtxt(
    buffer: *const c_char,
    size: c_int,
) -> *mut c_void {
    if buffer.is_null() || size <= 0 {
        return ptr::null_mut();
    }
    let ctxt = unsafe { html_ctxt_alloc() };
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    unsafe { html_ctxt_set_input(ctxt, buffer, size) };
    if unsafe { (*ctxt).input.is_null() } {
        unsafe { crate::xml::html::free_parser_ctxt(ctxt as *mut c_void) };
        return ptr::null_mut();
    }
    ctxt as *mut c_void
}

/// Create a parser context for using the HTML parser in push mode.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// htmlParserCtxt *htmlCreatePushParserCtxt(htmlSAXHandler *sax, void *user_data,
///                                          const char *chunk, int size,
///                                          const char *filename, xmlCharEncoding enc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlCreatePushParserCtxt(
    sax: *mut _xmlSAXHandler,
    user_data: *mut c_void,
    chunk: *const c_char,
    size: c_int,
    filename: *const c_char,
    _enc: xmlCharEncoding,
) -> *mut c_void {
    let ctxt = unsafe { html_ctxt_alloc() };
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        (*ctxt).sax = sax;
        (*ctxt).user_data = user_data;
        if !filename.is_null() {
            (*ctxt).filename = c_strdup(filename);
        }
        if size > 0 && !chunk.is_null() {
            html_ctxt_set_input(ctxt, chunk, size);
        } else {
            // Upstream always creates a push input; allocate an (empty)
            // buffer so htmlParseChunk sees a valid input.
            let nb = xmlMallocImpl(1) as *mut u8;
            if !nb.is_null() {
                (*ctxt).input = nb;
                (*ctxt).input_len = 0;
                (*ctxt).input_pos = 0;
            }
        }
    }
    ctxt as *mut c_void
}

/// Reset a parser context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void htmlCtxtReset(htmlParserCtxt *ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlCtxtReset(ctxt: *mut c_void) {
    if ctxt.is_null() {
        return;
    }
    let c = ctxt as *mut HtmlOpaqueCtxt;
    unsafe {
        if !(*c).input.is_null() {
            xmlFreeImpl((*c).input as *mut c_void);
        }
        (*c).input = ptr::null_mut();
        (*c).input_len = 0;
        (*c).input_pos = 0;
        (*c).doc = ptr::null_mut();
        (*c).options = 0;
        (*c).line = 1;
        (*c).err = false;
    }
}

/// Applies the options to the parser context (upstream `htmlCtxtUseOptions`):
/// returns 0 when all options are known, else the set of unknown or
/// unimplemented options. The internal parser engine does not implement
/// option-driven behavior, so only the return value is observable.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int htmlCtxtUseOptions(htmlParserCtxt *ctxt, int options);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlCtxtUseOptions(ctxt: *mut c_void, options: c_int) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    let c = ctxt as *mut HtmlOpaqueCtxt;
    // Historic storage rule: some options can only be enabled.
    unsafe {
        (*c).options = ((*c).options & HTML_OPTIONS_KEEP_MASK) | (options & HTML_OPTIONS_ALL_MASK);
    }
    // Return the set of unknown/unimplemented options (XML_PARSE_NOENT is
    // accepted and ignored, matching upstream).
    options & !HTML_OPTIONS_ALL_MASK & !crate::abi::types::XML_PARSE_NOENT
}

/// Parse an HTML document from the context's stored input and invoke the
/// SAX handlers.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int htmlParseDocument(htmlParserCtxt *ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlParseDocument(ctxt: *mut c_void) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    let c = ctxt as *mut HtmlOpaqueCtxt;
    if unsafe { (*c).input.is_null() } {
        return -1;
    }
    let doc = unsafe { html::parse_memory((*c).input as *const c_char, (*c).input_len as c_int) };
    unsafe { (*c).doc = doc };
    if doc.is_null() {
        -1
    } else {
        0
    }
}

/// Parse a chunk of data. The last chunk must be marked with `terminate`; the
/// resulting document is stored in the context (opaque here) and returned by
/// `htmlCtxtParseDocument`-style entry points. The internal engine parses
/// incrementally only at the terminating chunk.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int htmlParseChunk(htmlParserCtxt *ctxt, const char *chunk, int size, int terminate);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlParseChunk(
    ctxt: *mut c_void,
    chunk: *const c_char,
    size: c_int,
    terminate: c_int,
) -> c_int {
    if ctxt.is_null() || size < 0 || (size > 0 && chunk.is_null()) {
        return XML_ERR_ARGUMENT;
    }
    let c = ctxt as *mut HtmlOpaqueCtxt;
    if unsafe { (*c).input.is_null() } {
        return XML_ERR_ARGUMENT;
    }

    if size > 0 {
        let new_len = unsafe { (*c).input_len }.wrapping_add(size as usize);
        let nb = unsafe { xmlReallocImpl((*c).input as *mut c_void, new_len) } as *mut u8;
        if nb.is_null() {
            return XML_ERR_NO_MEMORY;
        }
        unsafe {
            ptr::copy_nonoverlapping(chunk as *const u8, nb.add((*c).input_len), size as usize);
            (*c).input = nb;
            (*c).input_len = new_len;
        }
    }

    if terminate != 0 {
        let doc =
            unsafe { html::parse_memory((*c).input as *const c_char, (*c).input_len as c_int) };
        unsafe {
            (*c).doc = doc;
            // The accumulated input is no longer needed.
            xmlFreeImpl((*c).input as *mut c_void);
            (*c).input = ptr::null_mut();
            (*c).input_len = 0;
        }
    }
    XML_ERR_OK
}

/// Parse an HTML document and return the resulting document tree.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDoc *htmlCtxtParseDocument(htmlParserCtxt *ctxt, xmlParserInput *input);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlCtxtParseDocument(
    ctxt: *mut c_void,
    input: *mut _xmlParserInput,
) -> *mut _xmlDoc {
    if ctxt.is_null() || input.is_null() {
        return ptr::null_mut();
    }
    let cur = unsafe { (*input).cur };
    let end = unsafe { (*input).end };
    if cur.is_null() {
        return ptr::null_mut();
    }
    let len = (end as usize).wrapping_sub(cur as usize) as c_int;
    if len <= 0 {
        return ptr::null_mut();
    }
    let doc = unsafe { html::parse_memory(cur as *const c_char, len) };
    let c = ctxt as *mut HtmlOpaqueCtxt;
    unsafe {
        (*c).doc = doc;
    }
    doc
}

// ═══════════════════════════════════════════════════════════════════════════════
// Convenience read APIs (htmlCtxtRead* / htmlRead* / htmlSAXParse*)
// ═══════════════════════════════════════════════════════════════════════════════

/// Shared tail of the `htmlCtxtRead*` family: stash the parsed document in
/// the context and attach the URL.
unsafe fn html_ctxt_finish_read(
    ctxt: *mut c_void,
    doc: *mut _xmlDoc,
    url: *const c_char,
) -> *mut _xmlDoc {
    if ctxt.is_null() {
        return doc;
    }
    let c = ctxt as *mut HtmlOpaqueCtxt;
    unsafe {
        (*c).doc = doc;
        if !doc.is_null() && !url.is_null() {
            (*doc).URL = c_strdup(url) as *mut xmlChar;
        }
    }
    doc
}

/// Parse an HTML in-memory document and build a tree.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDoc *htmlCtxtReadMemory(xmlParserCtxt *ctxt, const char *buffer, int size,
///                            const char *URL, const char *encoding, int options);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlCtxtReadMemory(
    ctxt: *mut c_void,
    buffer: *const c_char,
    size: c_int,
    URL: *const c_char,
    _encoding: *const c_char,
    options: c_int,
) -> *mut _xmlDoc {
    if ctxt.is_null() || size < 0 {
        return ptr::null_mut();
    }
    unsafe { htmlCtxtReset(ctxt) };
    unsafe { htmlCtxtUseOptions(ctxt, options) };
    let doc = unsafe { html::parse_memory(buffer, size) };
    unsafe { html_ctxt_finish_read(ctxt, doc, URL) }
}

/// Parse an HTML in-memory document and build a tree.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDoc *htmlCtxtReadDoc(xmlParserCtxt *ctxt, const xmlChar *str,
///                         const char *URL, const char *encoding, int options);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlCtxtReadDoc(
    ctxt: *mut c_void,
    str: *const xmlChar,
    URL: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> *mut _xmlDoc {
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    unsafe { htmlCtxtReset(ctxt) };
    unsafe { htmlCtxtUseOptions(ctxt, options) };
    let doc = unsafe { html::parse_doc(str, encoding) };
    unsafe { html_ctxt_finish_read(ctxt, doc, URL) }
}

/// Parse an HTML file from the filesystem, the network or a user-defined
/// resource loader and build a tree.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDoc *htmlCtxtReadFile(xmlParserCtxt *ctxt, const char *filename,
///                          const char *encoding, int options);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlCtxtReadFile(
    ctxt: *mut c_void,
    filename: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> *mut _xmlDoc {
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    unsafe { htmlCtxtReset(ctxt) };
    unsafe { htmlCtxtUseOptions(ctxt, options) };
    let doc = unsafe { html::parse_file(filename, encoding) };
    unsafe { html_ctxt_finish_read(ctxt, doc, filename) }
}

/// Read all data from an open file descriptor.
unsafe fn html_read_fd(fd: c_int) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let n = libc::read(fd, tmp.as_mut_ptr() as *mut c_void, tmp.len());
        if n <= 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n as usize]);
    }
    buf
}

/// Read all data through an input callback.
unsafe fn html_read_io(ioread: Option<xmlInputReadCallback>, ioctx: *mut c_void) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    if let Some(read) = ioread {
        loop {
            let n = unsafe { read(ioctx, tmp.as_mut_ptr() as *mut c_char, tmp.len() as c_int) };
            if n <= 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n as usize]);
        }
    }
    buf
}

/// Parse an HTML document from a file descriptor and build a tree.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDoc *htmlCtxtReadFd(xmlParserCtxt *ctxt, int fd,
///                        const char *URL, const char *encoding, int options);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlCtxtReadFd(
    ctxt: *mut c_void,
    fd: c_int,
    URL: *const c_char,
    _encoding: *const c_char,
    options: c_int,
) -> *mut _xmlDoc {
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    unsafe { htmlCtxtReset(ctxt) };
    unsafe { htmlCtxtUseOptions(ctxt, options) };
    let data = unsafe { html_read_fd(fd) };
    let doc = unsafe { html::parse_memory(data.as_ptr() as *const c_char, data.len() as c_int) };
    unsafe { html_ctxt_finish_read(ctxt, doc, URL) }
}

/// Parse an HTML document from I/O functions and source and build a tree.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDoc *htmlCtxtReadIO(xmlParserCtxt *ctxt, xmlInputReadCallback ioread,
///                        xmlInputCloseCallback ioclose, void *ioctx,
///                        const char *URL, const char *encoding, int options);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlCtxtReadIO(
    ctxt: *mut c_void,
    ioread: Option<xmlInputReadCallback>,
    _ioclose: Option<xmlInputCloseCallback>,
    ioctx: *mut c_void,
    URL: *const c_char,
    _encoding: *const c_char,
    options: c_int,
) -> *mut _xmlDoc {
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    unsafe { htmlCtxtReset(ctxt) };
    unsafe { htmlCtxtUseOptions(ctxt, options) };
    let data = unsafe { html_read_io(ioread, ioctx) };
    let doc = unsafe { html::parse_memory(data.as_ptr() as *const c_char, data.len() as c_int) };
    unsafe { html_ctxt_finish_read(ctxt, doc, URL) }
}

/// Convenience function to parse an HTML document from memory.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDoc *htmlReadMemory(const char *buffer, int size, const char *url,
///                        const char *encoding, int options);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlReadMemory(
    buffer: *const c_char,
    size: c_int,
    url: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> *mut _xmlDoc {
    if size < 0 {
        return ptr::null_mut();
    }
    let ctxt = unsafe { htmlNewParserCtxt() };
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    let doc = unsafe { htmlCtxtReadMemory(ctxt, buffer, size, url, encoding, options) };
    unsafe { crate::xml::html::free_parser_ctxt(ctxt) };
    doc
}

/// Convenience function to parse an HTML document from a zero-terminated
/// string.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDoc *htmlReadDoc(const xmlChar *str, const char *url,
///                     const char *encoding, int options);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlReadDoc(
    str: *const xmlChar,
    url: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> *mut _xmlDoc {
    let ctxt = unsafe { htmlNewParserCtxt() };
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    let doc = unsafe { htmlCtxtReadDoc(ctxt, str, url, encoding, options) };
    unsafe { crate::xml::html::free_parser_ctxt(ctxt) };
    doc
}

/// Convenience function to parse an HTML file from the filesystem, the
/// network or a global user-defined resource loader.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDoc *htmlReadFile(const char *filename, const char *encoding, int options);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlReadFile(
    filename: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> *mut _xmlDoc {
    let ctxt = unsafe { htmlNewParserCtxt() };
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    let doc = unsafe { htmlCtxtReadFile(ctxt, filename, encoding, options) };
    unsafe { crate::xml::html::free_parser_ctxt(ctxt) };
    doc
}

/// Convenience function to parse an HTML document from a file descriptor.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDoc *htmlReadFd(int fd, const char *url, const char *encoding, int options);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlReadFd(
    fd: c_int,
    url: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> *mut _xmlDoc {
    let ctxt = unsafe { htmlNewParserCtxt() };
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    let doc = unsafe { htmlCtxtReadFd(ctxt, fd, url, encoding, options) };
    unsafe { crate::xml::html::free_parser_ctxt(ctxt) };
    doc
}

/// Convenience function to parse an HTML document from I/O functions and
/// context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDoc *htmlReadIO(xmlInputReadCallback ioread, xmlInputCloseCallback ioclose,
///                    void *ioctx, const char *url, const char *encoding, int options);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlReadIO(
    ioread: Option<xmlInputReadCallback>,
    ioclose: Option<xmlInputCloseCallback>,
    ioctx: *mut c_void,
    url: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> *mut _xmlDoc {
    let ctxt = unsafe { htmlNewParserCtxt() };
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    let doc = unsafe { htmlCtxtReadIO(ctxt, ioread, ioclose, ioctx, url, encoding, options) };
    unsafe { crate::xml::html::free_parser_ctxt(ctxt) };
    doc
}

/// Parse an HTML in-memory document. If sax is not NULL, use the SAX
/// callbacks to handle parse events; the internal engine is DOM-based, so a
/// non-NULL sax is accepted and ignored (documented divergence).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDoc *htmlSAXParseDoc(const xmlChar *cur, const char *encoding,
///                         htmlSAXHandler *sax, void *userData);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlSAXParseDoc(
    cur: *const xmlChar,
    encoding: *const c_char,
    sax: *mut _xmlSAXHandler,
    userData: *mut c_void,
) -> *mut _xmlDoc {
    if cur.is_null() {
        return ptr::null_mut();
    }
    let ctxt = unsafe { htmlNewSAXParserCtxt(sax, userData) };
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    let doc = unsafe { htmlCtxtReadDoc(ctxt, cur, ptr::null(), encoding, 0) };
    unsafe { crate::xml::html::free_parser_ctxt(ctxt) };
    doc
}

/// Parse an HTML file and build a tree. If sax is not NULL, use the SAX
/// callbacks to handle parse events; the internal engine is DOM-based, so a
/// non-NULL sax is accepted and ignored (documented divergence).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDoc *htmlSAXParseFile(const char *filename, const char *encoding,
///                          htmlSAXHandler *sax, void *userData);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlSAXParseFile(
    filename: *const c_char,
    encoding: *const c_char,
    sax: *mut _xmlSAXHandler,
    userData: *mut c_void,
) -> *mut _xmlDoc {
    let ctxt = unsafe { htmlNewSAXParserCtxt(sax, userData) };
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    let doc = unsafe { htmlCtxtReadFile(ctxt, filename, encoding, 0) };
    unsafe { crate::xml::html::free_parser_ctxt(ctxt) };
    doc
}

/// Upstream `htmlParseElement` parses one element from the context's input
/// stream. The internal engine exposes whole-document parsing only, so this
/// best-effort implementation parses the context's stored input and stashes
/// the document (deprecated internal function; no-op when no input is set).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void htmlParseElement(htmlParserCtxt *ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlParseElement(ctxt: *mut c_void) {
    if ctxt.is_null() {
        return;
    }
    let c = ctxt as *mut HtmlOpaqueCtxt;
    if unsafe { (*c).input.is_null() } {
        return;
    }
    let doc = unsafe { html::parse_memory((*c).input as *const c_char, (*c).input_len as c_int) };
    unsafe {
        (*c).doc = doc;
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Document creation
// ═══════════════════════════════════════════════════════════════════════════════

/// Creates a new HTML document without a DTD node if `URI` and `publicId`
/// are NULL.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDoc *htmlNewDocNoDtD(const xmlChar *URI, const xmlChar *ExternalID);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlNewDocNoDtD(
    URI: *const xmlChar,
    publicId: *const xmlChar,
) -> *mut _xmlDoc {
    let doc = unsafe { html::new_doc_no_dtd(ptr::null()) };
    if doc.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        // UPSTREAM-PARITY (HTMLparser.c htmlNewDocNoDtD): standalone=1,
        // charset=UTF-8, properties = XML_DOC_HTML | XML_DOC_USERBUILT.
        (*doc).standalone = 1;
        (*doc).charset = crate::abi::types::xmlCharEncoding::XML_CHAR_ENCODING_UTF8 as c_int;
        (*doc).properties = crate::abi::types::xmlDocProperties::XML_DOC_HTML as c_int
            | crate::abi::types::xmlDocProperties::XML_DOC_USERBUILT as c_int;
        if !publicId.is_null() || !URI.is_null() {
            let dtd = crate::xml::dtd::create_int_subset(
                doc,
                b"html\0" as *const u8 as *const xmlChar,
                publicId,
                URI,
            );
            if dtd.is_null() {
                tree::free_doc(doc);
                return ptr::null_mut();
            }
        }
    }
    doc
}

/// Creates a new HTML document.
///
/// The document comes from the internal module's `html::new_doc` (the
/// crate's oracle-verified HTML module auto-creates the implicit
/// html/head/body skeleton, matching the crate's `htmlNewDoc` semantics);
/// the internal subset is attached exactly like upstream — the default HTML
/// 4.0 Transitional DTD when neither URI nor publicId is given, otherwise a
/// DTD with the supplied identifiers.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDoc *htmlNewDoc(const xmlChar *URI, const xmlChar *ExternalID);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlNewDoc(
    URI: *const xmlChar,
    ExternalID: *const xmlChar,
) -> *mut _xmlDoc {
    let doc = unsafe { html::new_doc(ptr::null()) };
    if doc.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        // UPSTREAM-PARITY (HTMLparser.c htmlNewDocNoDtD): standalone=1,
        // charset=UTF-8, properties = XML_DOC_HTML | XML_DOC_USERBUILT.
        (*doc).standalone = 1;
        (*doc).charset = crate::abi::types::xmlCharEncoding::XML_CHAR_ENCODING_UTF8 as c_int;
        (*doc).properties = crate::abi::types::xmlDocProperties::XML_DOC_HTML as c_int
            | crate::abi::types::xmlDocProperties::XML_DOC_USERBUILT as c_int;
        if URI.is_null() && ExternalID.is_null() {
            let dtd = crate::xml::dtd::create_int_subset(
                doc,
                b"html\0" as *const u8 as *const xmlChar,
                b"-//W3C//DTD HTML 4.0 Transitional//EN\0" as *const u8 as *const xmlChar,
                b"http://www.w3.org/TR/REC-html40/loose.dtd\0" as *const u8 as *const xmlChar,
            );
            if dtd.is_null() {
                tree::free_doc(doc);
                return ptr::null_mut();
            }
        } else if !ExternalID.is_null() || !URI.is_null() {
            let dtd = crate::xml::dtd::create_int_subset(
                doc,
                b"html\0" as *const u8 as *const xmlChar,
                ExternalID,
                URI,
            );
            if dtd.is_null() {
                tree::free_doc(doc);
                return ptr::null_mut();
            }
        }
    }
    doc
}

// ═══════════════════════════════════════════════════════════════════════════════
// Meta encoding (HTMLtree.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Find the first child of `node` whose name matches (case-insensitive).
unsafe fn html_find_first_child(node: *mut _xmlNode, name: &[u8]) -> *mut _xmlNode {
    let mut c = unsafe { (*node).children };
    while !c.is_null() {
        let n = unsafe { &*c };
        if n.type_ == XML_ELEMENT_NODE as c_int
            && !n.name.is_null()
            && unsafe { xmlstr_to_bytes(n.name) }.eq_ignore_ascii_case(name)
        {
            return c;
        }
        c = unsafe { (*c).next };
    }
    ptr::null_mut()
}

/// Locate the `<head>` element (upstream `htmlFindHead`): first `html` child
/// of the document, then first `head` child.
unsafe fn html_find_head(doc: *mut _xmlDoc) -> *mut _xmlNode {
    if doc.is_null() {
        return ptr::null_mut();
    }
    let html = unsafe { html_find_first_child(doc as *mut _xmlNode, b"html") };
    if html.is_null() {
        return ptr::null_mut();
    }
    unsafe { html_find_first_child(html, b"head") }
}

/// Find the encoding-declaring attribute of a `meta` element
/// (upstream `htmlFindMetaEncodingAttr`). Returns `(attr, is_content_type)`.
unsafe fn html_find_meta_encoding_attr(elem: *mut _xmlNode) -> (*mut _xmlAttr, bool) {
    let n = unsafe { &*elem };
    if n.type_ != XML_ELEMENT_NODE as c_int || n.name.is_null() {
        return (ptr::null_mut(), false);
    }
    if !unsafe { xmlstr_to_bytes(n.name) }.eq_ignore_ascii_case(b"meta") {
        return (ptr::null_mut(), false);
    }

    let mut content_attr: *mut _xmlAttr = ptr::null_mut();
    let mut is_content_type = false;
    let mut attr = n.properties;
    while !attr.is_null() {
        let a = unsafe { &*attr };
        if a.ns.is_null() && !a.name.is_null() {
            let nm = unsafe { xmlstr_to_bytes(a.name) };
            if nm.eq_ignore_ascii_case(b"charset") {
                return (attr, false);
            }
            if nm.eq_ignore_ascii_case(b"content") {
                content_attr = attr;
            }
            if nm.eq_ignore_ascii_case(b"http-equiv")
                && !a.children.is_null()
                && unsafe { (*(a.children)).type_ } == XML_TEXT_NODE as c_int
                && unsafe { (*(a.children)).next }.is_null()
                && !unsafe { (*(a.children)).content }.is_null()
                && unsafe { xmlstr_to_bytes((*(a.children)).content) }
                    .eq_ignore_ascii_case(b"Content-Type")
            {
                is_content_type = true;
            }
        }
        attr = unsafe { (*attr).next };
    }
    if is_content_type && !content_attr.is_null() {
        (content_attr, true)
    } else {
        (ptr::null_mut(), false)
    }
}

/// Parse `charset=` out of a `content` attribute value (upstream
/// `htmlParseContentType`). Returns `(start, end, size)` offsets.
unsafe fn html_parse_content_type(val: *const xmlChar) -> Option<(usize, usize, usize)> {
    let bytes = unsafe { xmlstr_to_bytes(val) };
    let n = bytes.len();
    let at = |i: usize| -> u8 {
        if i < n {
            bytes[i]
        } else {
            0
        }
    };

    let mut p = 0usize;
    loop {
        // Find 'c' or 'C'
        loop {
            let ch = at(p);
            if ch == b'c' || ch == b'C' {
                break;
            }
            if ch == 0 {
                return None;
            }
            p += 1;
        }
        p += 1;

        // "harset" must follow (6 bytes, case-insensitive)
        let mut ok = true;
        for (k, want) in b"harset".iter().enumerate() {
            if at(p + k).to_ascii_lowercase() != *want {
                ok = false;
                break;
            }
        }
        if !ok {
            continue;
        }
        p += 6;
        while is_ws_html(at(p)) {
            p += 1;
        }
        if at(p) != b'=' {
            continue;
        }
        p += 1;
        while is_ws_html(at(p)) {
            p += 1;
        }
        if at(p) == 0 {
            return None;
        }

        let (start, mut end): (usize, usize);
        if at(p) == b'"' || at(p) == b'\'' {
            let quote = at(p);
            p += 1;
            while is_ws_html(at(p)) {
                p += 1;
            }
            start = p;
            end = start;
            loop {
                if at(p) == 0 {
                    return None;
                }
                if !is_ws_html(at(p)) {
                    end = p + 1;
                }
                if at(p) == quote {
                    break;
                }
                p += 1;
            }
        } else {
            start = p;
            while at(p) != 0 && at(p) != b';' && !is_ws_html(at(p)) {
                p += 1;
            }
            end = p;
        }
        let size = n;
        return Some((start, end, size));
    }
}

/// Look up an encoding declaration in the meta tags of the document.
///
/// The returned string points into attribute content (may contain trailing
/// garbage); it should be copied before modifying or freeing nodes —
/// upstream contract.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const xmlChar *htmlGetMetaEncoding(xmlDoc *doc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlGetMetaEncoding(doc: *mut _xmlDoc) -> *const xmlChar {
    let head = unsafe { html_find_head(doc) };
    if head.is_null() {
        return ptr::null();
    }
    let mut node = unsafe { (*head).children };
    while !node.is_null() {
        let (attr, is_content_type) = unsafe { html_find_meta_encoding_attr(node) };
        if !attr.is_null() {
            let a = unsafe { &*attr };
            let val = if !a.children.is_null()
                && unsafe { (*(a.children)).type_ } == XML_TEXT_NODE as c_int
                && unsafe { (*(a.children)).next }.is_null()
                && !unsafe { (*(a.children)).content }.is_null()
            {
                unsafe { (*(a.children)).content }
            } else {
                b"\0" as *const u8 as *const xmlChar
            };
            if !is_content_type {
                let bytes = unsafe { xmlstr_to_bytes(val) };
                let mut start = 0usize;
                while start < bytes.len() && is_ws_html(bytes[start]) {
                    start += 1;
                }
                return unsafe { val.add(start) };
            } else if let Some((start, _, _)) = unsafe { html_parse_content_type(val) } {
                return unsafe { val.add(start) };
            }
        }
        node = unsafe { (*node).next };
    }
    ptr::null()
}

/// Build the updated charset value for an existing meta tag
/// (upstream `htmlUpdateMetaEncoding`).
unsafe fn html_update_meta_encoding(
    attr_value: *const xmlChar,
    start: usize,
    end: usize,
    size: usize,
    encoding: &[u8],
) -> *mut xmlChar {
    // The pseudo "HTML" encoding only produces ASCII.
    let enc: &[u8] = if encoding.eq_ignore_ascii_case(b"HTML") {
        b"ASCII"
    } else {
        encoding
    };
    let bytes = unsafe { xmlstr_to_bytes(attr_value) };
    let e = end.min(bytes.len()).min(size);
    let s = start.min(e);
    let total = size - (e - s) + enc.len();
    let new_val = xmlMallocImpl(total + 1) as *mut xmlChar;
    if new_val.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        let mut p = new_val;
        ptr::copy_nonoverlapping(bytes.as_ptr(), p, s);
        p = p.add(s);
        ptr::copy_nonoverlapping(enc.as_ptr(), p, enc.len());
        p = p.add(enc.len());
        ptr::copy_nonoverlapping(bytes.as_ptr().add(e), p, size - e);
        *new_val.add(total) = 0;
    }
    new_val
}

/// Replace the content of an attribute's single text child
/// (upstream `xmlNodeSetContent` on the attribute node).
unsafe fn html_set_attr_content(attr: *mut _xmlAttr, content: *const xmlChar) -> c_int {
    if attr.is_null() {
        return -1;
    }
    unsafe {
        if !(*attr).children.is_null() {
            tree::free_node_list((*attr).children);
            (*attr).children = ptr::null_mut();
            (*attr).last = ptr::null_mut();
        }
        let text = tree::new_text(content);
        if text.is_null() {
            return -1;
        }
        (*text).parent = attr as *mut _xmlNode;
        (*text).doc = (*attr).doc;
        (*attr).children = text;
        (*attr).last = text;
    }
    0
}

/// Creates or updates a meta tag with an encoding declaration.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int htmlSetMetaEncoding(xmlDoc *doc, const xmlChar *encoding);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlSetMetaEncoding(doc: *mut _xmlDoc, encoding: *const xmlChar) -> c_int {
    if encoding.is_null() {
        return 1;
    }
    let head = unsafe { html_find_head(doc) };
    if head.is_null() {
        return 1;
    }
    let enc_bytes = unsafe { xmlstr_to_bytes(encoding) }.to_vec();

    let mut found = 0;
    let mut meta = unsafe { (*head).children };
    while !meta.is_null() {
        let (attr, is_content_type) = unsafe { html_find_meta_encoding_attr(meta) };
        if !attr.is_null() {
            let a = unsafe { &*attr };
            let val = if !a.children.is_null()
                && unsafe { (*(a.children)).type_ } == XML_TEXT_NODE as c_int
                && unsafe { (*(a.children)).next }.is_null()
                && !unsafe { (*(a.children)).content }.is_null()
            {
                unsafe { (*(a.children)).content }
            } else {
                b"\0" as *const u8 as *const xmlChar
            };
            found = 1;
            let off = if is_content_type {
                unsafe { html_parse_content_type(val) }
            } else {
                let bytes = unsafe { xmlstr_to_bytes(val) };
                let mut start = 0usize;
                let mut end = bytes.len();
                while start < end && is_ws_html(bytes[start]) {
                    start += 1;
                }
                while end > start && is_ws_html(bytes[end - 1]) {
                    end -= 1;
                }
                Some((start, end, bytes.len()))
            };
            if let Some((start, end, size)) = off {
                let new_val =
                    unsafe { html_update_meta_encoding(val, start, end, size, &enc_bytes) };
                if new_val.is_null() {
                    return -1;
                }
                let ret = unsafe { html_set_attr_content(attr, new_val) };
                unsafe { xmlFreeImpl(new_val as *mut c_void) };
                if ret < 0 {
                    return -1;
                }
            } else {
                return -1;
            }
        }
        meta = unsafe { (*meta).next };
    }

    if found != 0 {
        return 0;
    }

    // No meta found: create one and insert it as the first child of head.
    let meta_node =
        unsafe { tree::new_node(ptr::null_mut(), b"meta\0" as *const u8 as *const xmlChar) };
    if meta_node.is_null() {
        return -1;
    }
    unsafe {
        (*meta_node).doc = (*head).doc;
    }
    let prop = unsafe {
        tree::set_prop(
            meta_node,
            b"charset\0" as *const u8 as *const xmlChar,
            encoding,
        )
    };
    if prop.is_null() {
        unsafe { tree::free_node(meta_node) };
        return -1;
    }
    if unsafe { (*head).children }.is_null() {
        unsafe { tree::add_child(head, meta_node) };
    } else {
        unsafe { tree::add_sibling_before((*head).children, meta_node) };
    }
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// Serialization (HTMLtree.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Serialize `node` into a fresh `_xmlBuffer` via the internal HTML
/// serializer. Returns the buffer (caller frees with `io::buf_free`) or NULL.
unsafe fn html_serialize_to_buffer(node: *mut _xmlNode, format: c_int) -> *mut _xmlBuffer {
    let buf = io::buf_create(0);
    if buf.is_null() {
        return ptr::null_mut();
    }
    unsafe { html::serialize_node(node, buf, format, 0) };
    buf
}

/// Serialize `node` into an `_xmlOutputBuffer` (writes the serialized bytes
/// through the output buffer's I/O channel).
unsafe fn html_serialize_to_obuf(obuf: *mut _xmlOutputBuffer, node: *mut _xmlNode, format: c_int) {
    if obuf.is_null() || node.is_null() {
        return;
    }
    let buf = unsafe { html_serialize_to_buffer(node, format) };
    if buf.is_null() {
        return;
    }
    let len = io::buf_length(buf);
    if len > 0 {
        let content = io::buf_content(buf);
        unsafe {
            io::output_buffer_write(obuf, len, content as *const c_char);
        }
    }
    io::buf_free(buf);
}

/// Serialize an HTML node to an xmlBuffer. Always uses UTF-8.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int htmlNodeDump(xmlBuffer *buf, xmlDoc *doc, xmlNode *cur);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlNodeDump(
    buf: *mut _xmlBuffer,
    _doc: *mut _xmlDoc,
    cur: *mut _xmlNode,
) -> c_int {
    if buf.is_null() || cur.is_null() {
        return -1;
    }
    let before = io::buf_length(buf);
    unsafe { html::serialize_node(cur, buf, 1, 0) };
    let after = io::buf_length(buf);
    if after < 0 || before < 0 {
        return -1;
    }
    after - before
}

/// Serialize an HTML node to a FILE.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void htmlNodeDumpFile(FILE *out, xmlDoc *doc, xmlNode *cur);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlNodeDumpFile(out: *mut c_void, doc: *mut _xmlDoc, cur: *mut _xmlNode) {
    unsafe { htmlNodeDumpFileFormat(out, doc, cur, ptr::null(), 1) };
}

/// Serialize an HTML node to a FILE with encoding and format.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int htmlNodeDumpFileFormat(FILE *out, xmlDoc *doc, xmlNode *cur,
///                            const char *encoding, int format);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlNodeDumpFileFormat(
    out: *mut c_void,
    _doc: *mut _xmlDoc,
    cur: *mut _xmlNode,
    _encoding: *const c_char,
    format: c_int,
) -> c_int {
    let obuf = io::output_buffer_create_file(out as *mut libc::FILE, ptr::null_mut());
    if obuf.is_null() {
        return -1;
    }
    unsafe { html_serialize_to_obuf(obuf, cur, format) };
    io::output_buffer_close(obuf)
}

/// Serialize an HTML node to an output buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void htmlNodeDumpOutput(xmlOutputBuffer *buf, xmlDoc *doc, xmlNode *cur,
///                         const char *encoding);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlNodeDumpOutput(
    buf: *mut _xmlOutputBuffer,
    _doc: *mut _xmlDoc,
    cur: *mut _xmlNode,
    _encoding: *const c_char,
) {
    unsafe { html_serialize_to_obuf(buf, cur, 1) };
}

/// Serialize an HTML node to an output buffer with format.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void htmlNodeDumpFormatOutput(xmlOutputBuffer *buf, xmlDoc *doc, xmlNode *cur,
///                               const char *encoding, int format);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlNodeDumpFormatOutput(
    buf: *mut _xmlOutputBuffer,
    _doc: *mut _xmlDoc,
    cur: *mut _xmlNode,
    _encoding: *const c_char,
    format: c_int,
) {
    unsafe { html_serialize_to_obuf(buf, cur, format) };
}

/// Serialize an HTML document to an output buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void htmlDocContentDumpOutput(xmlOutputBuffer *buf, xmlDoc *cur,
///                               const char *encoding);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlDocContentDumpOutput(
    buf: *mut _xmlOutputBuffer,
    cur: *mut _xmlDoc,
    _encoding: *const c_char,
) {
    unsafe { html_serialize_to_obuf(buf, cur as *mut _xmlNode, 1) };
}

/// Serialize an HTML document to an output buffer with format.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void htmlDocContentDumpFormatOutput(xmlOutputBuffer *buf, xmlDoc *cur,
///                                     const char *encoding, int format);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlDocContentDumpFormatOutput(
    buf: *mut _xmlOutputBuffer,
    cur: *mut _xmlDoc,
    _encoding: *const c_char,
    format: c_int,
) {
    unsafe { html_serialize_to_obuf(buf, cur as *mut _xmlNode, format) };
}

/// Serialize an HTML document to memory, also returning the size of the
/// result. The caller frees `mem` with `xmlFree`. The output is UTF-8
/// (upstream converts to the document encoding).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void htmlDocDumpMemoryFormat(xmlDoc *cur, xmlChar **mem, int *size, int format);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlDocDumpMemoryFormat(
    cur: *mut _xmlDoc,
    mem: *mut *mut xmlChar,
    size: *mut c_int,
    format: c_int,
) {
    if mem.is_null() || size.is_null() {
        return;
    }
    unsafe {
        *mem = ptr::null_mut();
        *size = 0;
    }
    if cur.is_null() {
        return;
    }
    let buf = unsafe { html_serialize_to_buffer(cur as *mut _xmlNode, format) };
    if buf.is_null() {
        return;
    }
    let len = io::buf_length(buf);
    if len > 0 {
        let content = io::buf_content(buf);
        unsafe {
            *mem = xml_strndup(content, len as usize);
            if !(*mem).is_null() {
                *size = len;
            }
        }
    }
    io::buf_free(buf);
}

/// Same as `htmlDocDumpMemoryFormat` with `format` set to 1.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void htmlDocDumpMemory(xmlDoc *cur, xmlChar **mem, int *size);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlDocDumpMemory(
    cur: *mut _xmlDoc,
    mem: *mut *mut xmlChar,
    size: *mut c_int,
) {
    unsafe { htmlDocDumpMemoryFormat(cur, mem, size, 1) };
}

/// Serialize an HTML document to an open FILE.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int htmlDocDump(FILE *f, xmlDoc *cur);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlDocDump(f: *mut c_void, cur: *mut _xmlDoc) -> c_int {
    if f.is_null() || cur.is_null() {
        return -1;
    }
    let obuf = io::output_buffer_create_file(f as *mut libc::FILE, ptr::null_mut());
    if obuf.is_null() {
        return -1;
    }
    unsafe { html_serialize_to_obuf(obuf, cur as *mut _xmlNode, 1) };
    io::output_buffer_close(obuf)
}

/// Serialize an HTML document to a file using a given encoding and format.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int htmlSaveFileFormat(const char *filename, xmlDoc *cur,
///                        const char *encoding, int format);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlSaveFileFormat(
    filename: *const c_char,
    cur: *mut _xmlDoc,
    _encoding: *const c_char,
    format: c_int,
) -> c_int {
    if cur.is_null() || filename.is_null() {
        return -1;
    }
    let obuf = io::output_buffer_create_filename(filename, ptr::null_mut(), 0);
    if obuf.is_null() {
        // UPSTREAM-PARITY: a failed output buffer yields 0, not -1.
        return 0;
    }
    unsafe { html_serialize_to_obuf(obuf, cur as *mut _xmlNode, format) };
    io::output_buffer_close(obuf)
}

/// Same as `htmlSaveFileFormat` with `encoding` set to NULL and `format` set
/// to 1.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int htmlSaveFile(const char *filename, xmlDoc *cur);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlSaveFile(filename: *const c_char, cur: *mut _xmlDoc) -> c_int {
    unsafe { htmlSaveFileFormat(filename, cur, ptr::null(), 1) }
}

/// Same as `htmlSaveFileFormat` with `format` set to 1.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int htmlSaveFileEnc(const char *filename, xmlDoc *cur, const char *encoding);
/// ```
#[no_mangle]
pub unsafe extern "C" fn htmlSaveFileEnc(
    filename: *const c_char,
    cur: *mut _xmlDoc,
    encoding: *const c_char,
) -> c_int {
    unsafe { htmlSaveFileFormat(filename, cur, encoding, 1) }
}
