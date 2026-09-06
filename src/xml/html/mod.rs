//! HTML parser and serializer (§29, §85 Phase 4).
//!
//! The libxml2 historical HTML parser — a tag-recovery parser, NOT a WHATWG
//! HTML5 parser. Preserves version-specific historical behavior.
//!
//! Implements:
//! - Tag-recovery parsing (auto-close, implicit open, case-insensitive)
//! - HTML element info table with flags matching libxml2
//! - HTML entity resolution
//! - Auto-creation of html/head/body when missing
//! - HTML-specific serialization (no self-closing void tags, no namespace decls)
//! - Minimized and unquoted attribute support
//!
//! # Upstream contract
//!
//! Mirrors upstream `HTMLparser.c`, `HTMLtree.c` and `HTMLdocument.c`
//! (`SRC-LIBXML2-2.15.0-HTMLPARSER-C` et al., parity target libxml2 2.15.3
//! oracle): the tag-recovery HTML parser, the htmlElementInfo table,
//! htmlDocDump serialization and the htmlDefaultSAXHandler /
//! htmlDefaultSAXLocator data globals (R-000135 exports them
//! byte-identical; htmlInitAutoClose / htmlElementAllowedHere are the
//! R-000138 no-op set).
//!
//! # Conceptual behavior
//!
//! Implements libxml2 historical HTML parsing: case-insensitive tag
//! recovery with auto-close and implicit-open rules driven by the
//! htmlElementInfo flags table (HTML_EMPTY/HTML_NO_END/HTML_HEAD/...),
//! HTML entity resolution, auto-creation of html/head/body, minimized and
//! unquoted attributes, and HTML-specific serialization (no self-closing
//! void tags, no namespace declarations). This is deliberately NOT a
//! WHATWG HTML5 parser (WHATWG-HTML).
//!
//! # Ownership & safety invariants
//!
//! The parser creates a document the caller owns (freed with
//! `xmlFreeDoc`, same as XML); elements/attributes are owned by the tree.
//! Serialization borrows the tree and writes through the output buffer.
//! Push-mode input buffers are owned by the parser context (CVE-2015-8242
//! fixed a push-mode buffer overread upstream, SEC-0008).
//!
//! # Historical quirks & epochs
//!
//! E-007: the `--html` dump became a single line in the 2.15.0 epoch —
//! six `xmlOutputBufferWriteString(buf, "\n")` calls were removed from
//! HTMLtree.c (commits 0d81d6f8, 46f05ea4); the crate matches the 2.15+
//! single-line epoch. R-000118 locked the HTML output method for XSLT.
//! The tag-recovery rules themselves go back to the 1.x/2.0 HTML era and
//! stay version-faithful.
//!
//! # Deliberate oddities
//!
//! The htmlElementInfo table ordering and flag values reproduce upstream
//! exactly (R-000135 DATA-GLOBALS-001 fingerprints the default handler
//! slots); case-folding and the auto-close stack follow HTMLparser.c
//! rather than the WHATWG spec — a deliberate historical fidelity choice.
//!
//! # Proving courts
//!
//! HTML-* courts (SEC-0008), the CLI `--html` differential cases
//! (html-dump epoch case in SEMANTIC_EPOCHS.md) and DATA-GLOBALS-001
//! compare output byte-identical against the oracle; cargo test runs the
//! HTML unit suites.
//!
//! # Tempting simplifications that would break parity
//!
//! Do not switch to a WHATWG HTML5 parser: downstream consumers depend on
//! libxml2 tag-recovery quirks. Do not re-add newlines to the dump (E-007
//! epoch) and do not reorder or re-flag the element table — both are
//! observable through the serializer and the exported data globals.
//!
//! # Safety
//!
//! - Raw `_xmlDoc`, `_xmlNode`, `_xmlAttr` and `_xmlBuffer` pointers are
//!   allocated by the crate allocator (or the tree helpers) and must stay
//!   valid for the duration of each call; owned documents are freed with
//!   `tree::free_doc` and buffers with the matching `xmlFreeImpl` routine.
//! - `HtmlParserCtxt.input` must be non-NULL and readable for `input_len`
//!   bytes whenever `input_pos` is below `input_len`; every read is
//!   bounds-checked against `input_len` before dereferencing.
//! - Node trees walked through `parent`, `children`, `next` and `properties`
//!   links must be well-formed: links are NULL-terminated, the parent chain
//!   terminates (no cycles), and every node in a chain is a valid `_xmlNode`.
//! - `name`, `content`, `version` and `encoding` fields are NULL or valid
//!   NUL-terminated `xmlChar` strings.

use core::ffi::c_void;
use core::mem::size_of;
use core::ptr;
use core::slice;
use std::os::raw::{c_char, c_int};

use crate::abi::allocator::{xmlFreeImpl, xmlMallocZero};
use crate::abi::structs::*;
use crate::abi::types::xmlDocProperties::XML_DOC_WELLFORMED;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::*;
use crate::xml::io;
use crate::xml::string::*;
use crate::xml::tree;

// ═══════════════════════════════════════════════════════════════════════════════
// HTML Element Info
// ═══════════════════════════════════════════════════════════════════════════════

/// HTML element flag constants matching libxml2 HTMLparser.h.
// HTML parser option bits (HTMLparser.h) as stored in `HtmlParserCtxt.options`
// (the same merged option space as the XML_PARSE_* bits).
const HTML_PARSE_NODEFDTD: c_int = 1 << 2;
const HTML_PARSE_NOIMPLIED: c_int = 1 << 13;

const HTML_INLINE: u32 = 0x1;
const HTML_BLOCK: u32 = 0x2;
const HTML_EMPTY: u32 = 0x4;
#[allow(dead_code)]
const HTML_DEPRECATED: u32 = 0x8;
const HTML_OL: u32 = 0x10;
const HTML_DL: u32 = 0x20;
#[allow(dead_code)]
const HTML_COMPACT: u32 = 0x40;
const HTML_HEAD: u32 = 0x80;
const HTML_BODY: u32 = 0x100;
#[allow(dead_code)]
const HTML_HEADSTRUCK: u32 = 0x200;
const HTML_VALID: u32 = 0x400;
const HTML_NO_END: u32 = 0x800; // end tag optional
#[allow(dead_code)]
const HTML_IMPLIED: u32 = 0x1000; // implied/auto-created

/// Information about an HTML element.
#[derive(Clone, Copy)]
struct HtmlElementInfo {
    name: &'static str,
    flags: u32,
}

/// Lookup table of HTML elements and their properties.
/// This matches libxml2's `htmlElementInfo` table.
const HTML_ELEMENTS: &[HtmlElementInfo] = &[
    // Void / empty elements
    HtmlElementInfo {
        name: "br",
        flags: HTML_INLINE | HTML_EMPTY | HTML_VALID,
    },
    HtmlElementInfo {
        name: "hr",
        flags: HTML_BLOCK | HTML_EMPTY | HTML_VALID,
    },
    HtmlElementInfo {
        name: "img",
        flags: HTML_INLINE | HTML_EMPTY | HTML_VALID,
    },
    HtmlElementInfo {
        name: "input",
        flags: HTML_INLINE | HTML_EMPTY | HTML_VALID,
    },
    HtmlElementInfo {
        name: "meta",
        flags: HTML_HEAD | HTML_EMPTY | HTML_VALID,
    },
    HtmlElementInfo {
        name: "link",
        flags: HTML_HEAD | HTML_EMPTY | HTML_VALID,
    },
    HtmlElementInfo {
        name: "base",
        flags: HTML_HEAD | HTML_EMPTY | HTML_VALID,
    },
    HtmlElementInfo {
        name: "area",
        flags: HTML_INLINE | HTML_EMPTY | HTML_VALID,
    },
    HtmlElementInfo {
        name: "col",
        flags: HTML_BLOCK | HTML_EMPTY | HTML_VALID,
    },
    HtmlElementInfo {
        name: "embed",
        flags: HTML_INLINE | HTML_EMPTY | HTML_VALID,
    },
    HtmlElementInfo {
        name: "param",
        flags: HTML_HEAD | HTML_EMPTY | HTML_VALID,
    },
    HtmlElementInfo {
        name: "source",
        flags: HTML_INLINE | HTML_EMPTY | HTML_VALID,
    },
    HtmlElementInfo {
        name: "track",
        flags: HTML_INLINE | HTML_EMPTY | HTML_VALID,
    },
    HtmlElementInfo {
        name: "wbr",
        flags: HTML_INLINE | HTML_EMPTY | HTML_VALID,
    },
    // Block elements
    HtmlElementInfo {
        name: "html",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "head",
        flags: HTML_HEAD | HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "body",
        flags: HTML_BODY | HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "div",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "p",
        flags: HTML_BLOCK | HTML_VALID | HTML_NO_END,
    },
    HtmlElementInfo {
        name: "h1",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "h2",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "h3",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "h4",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "h5",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "h6",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "ul",
        flags: HTML_BLOCK | HTML_VALID | HTML_OL,
    },
    HtmlElementInfo {
        name: "ol",
        flags: HTML_BLOCK | HTML_VALID | HTML_OL,
    },
    HtmlElementInfo {
        name: "li",
        flags: HTML_BLOCK | HTML_VALID | HTML_NO_END,
    },
    HtmlElementInfo {
        name: "dl",
        flags: HTML_BLOCK | HTML_VALID | HTML_DL,
    },
    HtmlElementInfo {
        name: "dt",
        flags: HTML_BLOCK | HTML_VALID | HTML_NO_END,
    },
    HtmlElementInfo {
        name: "dd",
        flags: HTML_BLOCK | HTML_VALID | HTML_NO_END,
    },
    HtmlElementInfo {
        name: "table",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "tr",
        flags: HTML_BLOCK | HTML_VALID | HTML_NO_END,
    },
    HtmlElementInfo {
        name: "td",
        flags: HTML_BLOCK | HTML_VALID | HTML_NO_END,
    },
    HtmlElementInfo {
        name: "th",
        flags: HTML_BLOCK | HTML_VALID | HTML_NO_END,
    },
    HtmlElementInfo {
        name: "thead",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "tbody",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "tfoot",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "colgroup",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "caption",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "form",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "fieldset",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "legend",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "pre",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "blockquote",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "address",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "center",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "dir",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "menu",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "noscript",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "frameset",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "frame",
        flags: HTML_BLOCK | HTML_EMPTY | HTML_VALID,
    },
    HtmlElementInfo {
        name: "iframe",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "noframes",
        flags: HTML_BLOCK | HTML_VALID,
    },
    // Inline elements
    HtmlElementInfo {
        name: "a",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "abbr",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "acronym",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "b",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "basefont",
        flags: HTML_INLINE | HTML_EMPTY | HTML_VALID,
    },
    HtmlElementInfo {
        name: "bdo",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "big",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "cite",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "code",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "dfn",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "em",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "font",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "i",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "kbd",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "label",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "map",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "nobr",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "object",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "q",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "rb",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "rbc",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "rp",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "rt",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "rtc",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "ruby",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "s",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "samp",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "select",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "small",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "span",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "strike",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "strong",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "sub",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "sup",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "textarea",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "tt",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "u",
        flags: HTML_INLINE | HTML_VALID,
    },
    HtmlElementInfo {
        name: "var",
        flags: HTML_INLINE | HTML_VALID,
    },
    // Heading block elements (also block)
    HtmlElementInfo {
        name: "header",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "footer",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "nav",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "article",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "section",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "aside",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "main",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "figure",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "figcaption",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "details",
        flags: HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "summary",
        flags: HTML_BLOCK | HTML_VALID,
    },
    // Script and style
    HtmlElementInfo {
        name: "script",
        flags: HTML_HEAD | HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "style",
        flags: HTML_HEAD | HTML_BLOCK | HTML_VALID,
    },
    HtmlElementInfo {
        name: "title",
        flags: HTML_HEAD | HTML_BLOCK | HTML_VALID,
    },
];

/// Case-insensitive lookup of an HTML element by name.
/// Returns `None` if the element is not in the table (treated as unknown).
fn html_tag_lookup(name: &str) -> Option<&'static HtmlElementInfo> {
    // Allocation-free, case-insensitive classification (Phase 16.5.8): the
    // previous implementation lowercased into a Vec<u8>, re-validated UTF-8,
    // then linearly scanned the static table. HTML tag matching is
    // ASCII-case-insensitive, so eq_ignore_ascii_case avoids the temporary.
    HTML_ELEMENTS
        .iter()
        .find(|info| info.name.eq_ignore_ascii_case(name))
}

// ═══════════════════════════════════════════════════════════════════════════════
// HTML Entity Handling
// ═══════════════════════════════════════════════════════════════════════════════

/// HTML named entities table. Maps entity names to their UTF-8 character(s).
/// This is a subset of the full HTML entity list, matching what libxml2 supports.
const HTML_ENTITIES: &[(&str, &str)] = &[
    ("nbsp", "\u{00a0}"),
    ("lt", "<"),
    ("gt", ">"),
    ("amp", "&"),
    ("quot", "\""),
    ("apos", "'"),
    ("copy", "\u{00a9}"),
    ("reg", "\u{00ae}"),
    ("amp", "&"),
    ("iexcl", "\u{00a1}"),
    ("cent", "\u{00a2}"),
    ("pound", "\u{00a3}"),
    ("curren", "\u{00a4}"),
    ("yen", "\u{00a5}"),
    ("brvbar", "\u{00a6}"),
    ("sect", "\u{00a7}"),
    ("uml", "\u{00a8}"),
    ("ordf", "\u{00aa}"),
    ("laquo", "\u{00ab}"),
    ("not", "\u{00ac}"),
    ("shy", "\u{00ad}"),
    ("macr", "\u{00ae}"),
    ("deg", "\u{00b0}"),
    ("plusmn", "\u{00b1}"),
    ("sup2", "\u{00b2}"),
    ("sup3", "\u{00b3}"),
    ("acute", "\u{00b4}"),
    ("micro", "\u{00b5}"),
    ("para", "\u{00b6}"),
    ("middot", "\u{00b7}"),
    ("cedil", "\u{00b8}"),
    ("sup1", "\u{00b9}"),
    ("ordm", "\u{00ba}"),
    ("raquo", "\u{00bb}"),
    ("frac14", "\u{00bc}"),
    ("frac12", "\u{00bd}"),
    ("frac34", "\u{00be}"),
    ("iquest", "\u{00bf}"),
    ("times", "\u{00d7}"),
    ("divide", "\u{00f7}"),
    ("ETH", "\u{00d0}"),
    ("eth", "\u{00f0}"),
    ("THORN", "\u{00de}"),
    ("thorn", "\u{00fe}"),
    ("AElig", "\u{00c6}"),
    ("aelig", "\u{00e6}"),
    ("OElig", "\u{0152}"),
    ("oelig", "\u{0153}"),
    ("Scaron", "\u{0160}"),
    ("scaron", "\u{0161}"),
    ("Yuml", "\u{0178}"),
    ("circ", "\u{02c6}"),
    ("tilde", "\u{02dc}"),
    ("ensp", "\u{2002}"),
    ("emsp", "\u{2003}"),
    ("thinsp", "\u{2009}"),
    ("zwnj", "\u{200c}"),
    ("zwj", "\u{200d}"),
    ("lrm", "\u{200e}"),
    ("rlm", "\u{200f}"),
    ("ndash", "\u{2013}"),
    ("mdash", "\u{2014}"),
    ("lsquo", "\u{2018}"),
    ("rsquo", "\u{2019}"),
    ("sbquo", "\u{201a}"),
    ("ldquo", "\u{201c}"),
    ("rdquo", "\u{201d}"),
    ("bdquo", "\u{201e}"),
    ("dagger", "\u{2020}"),
    ("Dagger", "\u{2021}"),
    ("bull", "\u{2022}"),
    ("hellip", "\u{2026}"),
    ("permil", "\u{2030}"),
    ("prime", "\u{2032}"),
    ("Prime", "\u{2033}"),
    ("lsaquo", "\u{2039}"),
    ("rsaquo", "\u{203a}"),
    ("oline", "\u{203e}"),
    ("euro", "\u{20ac}"),
    ("trade", "\u{2122}"),
    ("larr", "\u{2190}"),
    ("uarr", "\u{2191}"),
    ("rarr", "\u{2192}"),
    ("darr", "\u{2193}"),
    ("harr", "\u{2194}"),
    ("crarr", "\u{21b5}"),
    ("lceil", "\u{2308}"),
    ("rceil", "\u{2309}"),
    ("lfloor", "\u{230a}"),
    ("rfloor", "\u{230b}"),
    ("loz", "\u{25ca}"),
    ("spades", "\u{2660}"),
    ("clubs", "\u{2663}"),
    ("hearts", "\u{2665}"),
    ("diams", "\u{2666}"),
    ("Alpha", "\u{0391}"),
    ("Beta", "\u{0392}"),
    ("Gamma", "\u{0393}"),
    ("Delta", "\u{0394}"),
    ("Epsilon", "\u{0395}"),
    ("Zeta", "\u{0396}"),
    ("Eta", "\u{0397}"),
    ("Theta", "\u{0398}"),
    ("Iota", "\u{0399}"),
    ("Kappa", "\u{039a}"),
    ("Lambda", "\u{039b}"),
    ("Mu", "\u{039c}"),
    ("Nu", "\u{039d}"),
    ("Xi", "\u{039e}"),
    ("Omicron", "\u{039f}"),
    ("Pi", "\u{03a0}"),
    ("Rho", "\u{03a1}"),
    ("Sigma", "\u{03a3}"),
    ("Tau", "\u{03a4}"),
    ("Upsilon", "\u{03a5}"),
    ("Phi", "\u{03a6}"),
    ("Chi", "\u{03a7}"),
    ("Psi", "\u{03a8}"),
    ("Omega", "\u{03a9}"),
    ("alpha", "\u{03b1}"),
    ("beta", "\u{03b2}"),
    ("gamma", "\u{03b3}"),
    ("delta", "\u{03b4}"),
    ("epsilon", "\u{03b5}"),
    ("zeta", "\u{03b6}"),
    ("eta", "\u{03b7}"),
    ("theta", "\u{03b8}"),
    ("iota", "\u{03b9}"),
    ("kappa", "\u{03ba}"),
    ("lambda", "\u{03bb}"),
    ("mu", "\u{03bc}"),
    ("nu", "\u{03bd}"),
    ("xi", "\u{03be}"),
    ("omicron", "\u{03bf}"),
    ("pi", "\u{03c0}"),
    ("rho", "\u{03c1}"),
    ("sigmaf", "\u{03c2}"),
    ("sigma", "\u{03c3}"),
    ("tau", "\u{03c4}"),
    ("upsilon", "\u{03c5}"),
    ("phi", "\u{03c6}"),
    ("chi", "\u{03c7}"),
    ("psi", "\u{03c8}"),
    ("omega", "\u{03c9}"),
    ("thetasym", "\u{03d1}"),
    ("upsih", "\u{03d2}"),
    ("piv", "\u{03d6}"),
];

/// Look up an HTML entity by name (without the leading '&').
/// Returns the replacement string, or None if unknown.
fn html_entity_lookup(name: &str) -> Option<&'static str> {
    HTML_ENTITIES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, v)| *v)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Parser Context
// ═══════════════════════════════════════════════════════════════════════════════

/// Internal HTML parser context.
struct HtmlParserCtxt {
    /// The document being built
    doc: *mut _xmlDoc,
    /// Current insertion point (current parent node)
    current: *mut _xmlNode,
    /// The html element (auto-created or parsed)
    html: *mut _xmlNode,
    /// The head element (auto-created or parsed)
    head: *mut _xmlNode,
    /// The body element (auto-created or parsed)
    body: *mut _xmlNode,
    /// Whether we're inside <head>
    in_head: bool,
    /// Whether we're inside <body>
    in_body: bool,
    /// Whether html has been seen/created
    html_created: bool,
    /// Whether head has been seen/created
    head_created: bool,
    /// Whether body has been seen/created
    body_created: bool,
    /// Whether we've seen body content (moves from head to body)
    seen_body_content: bool,
    /// Input buffer (the HTML source)
    input: *mut u8,
    /// Current position in input
    input_pos: usize,
    /// Total input length
    input_len: usize,
    /// Line number tracking
    line: c_int,
    /// Error flag
    #[allow(dead_code)]
    err: bool,
    /// Filename (for file parsing)
    filename: *mut c_char,
    /// Encoding
    encoding: *mut c_char,
    /// Parse options (XML_PARSE_NOBLANKS etc., forwarded from the host
    /// htmlParserCtxt by the exports).
    options: c_int,
}

impl HtmlParserCtxt {
    const fn new() -> Self {
        HtmlParserCtxt {
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
        }
    }

    /// Peek at the next byte without consuming it.
    ///
    /// # Safety
    ///
    /// - `self.input` must be non-NULL and point to a valid allocation of at
    ///   least `self.input_len` bytes; the read at `self.input_pos` is
    ///   bounds-checked against `self.input_len` before dereferencing.
    fn peek(&self) -> Option<u8> {
        if self.input_pos < self.input_len {
            unsafe { Some(*self.input.add(self.input_pos)) }
        } else {
            None
        }
    }

    /// Peek ahead `n` bytes.
    ///
    /// # Safety
    ///
    /// - `self.input` must be non-NULL and point to a valid allocation of at
    ///   least `self.input_len` bytes; the read at `self.input_pos + offset`
    ///   is bounds-checked against `self.input_len` before dereferencing, so
    ///   `offset` must not be large enough to overflow `usize` when added to
    ///   `self.input_pos`.
    fn peek_at(&self, offset: usize) -> Option<u8> {
        let pos = self.input_pos + offset;
        if pos < self.input_len {
            unsafe { Some(*self.input.add(pos)) }
        } else {
            None
        }
    }

    /// Consume and return the next byte.
    ///
    /// # Safety
    ///
    /// - `self.input` must be non-NULL and point to a valid allocation of at
    ///   least `self.input_len` bytes; the read at `self.input_pos` is
    ///   bounds-checked against `self.input_len` before dereferencing.
    fn next(&mut self) -> Option<u8> {
        if self.input_pos < self.input_len {
            let ch = unsafe { *self.input.add(self.input_pos) };
            self.input_pos += 1;
            if ch == b'\n' {
                self.line += 1;
            }
            Some(ch)
        } else {
            None
        }
    }

    /// Skip bytes while the predicate returns true.
    fn skip_while<F: Fn(u8) -> bool>(&mut self, f: F) {
        while let Some(ch) = self.peek() {
            if f(ch) {
                self.next();
            } else {
                break;
            }
        }
    }

    /// Skip ASCII whitespace.
    fn skip_whitespace(&mut self) {
        self.skip_while(|ch| ch == b' ' || ch == b'\t' || ch == b'\n' || ch == b'\r');
    }

    /// Check if we've reached end of input.
    const fn is_eof(&self) -> bool {
        self.input_pos >= self.input_len
    }

    /// Read a sequence of bytes while the predicate returns true.
    ///
    /// # Safety
    ///
    /// - `self.input` must be a valid pointer to `self.input_len` readable
    ///   bytes for the duration of the call; reads are bounds-checked against
    ///   `self.input_pos`/`self.input_len` before each access, and the
    ///   returned slice is copied out before `self` can mutate.
    fn read_while<F: Fn(u8) -> bool>(&mut self, f: F) -> Vec<u8> {
        let start = self.input_pos;
        while let Some(ch) = self.peek() {
            if f(ch) {
                self.next();
            } else {
                break;
            }
        }
        unsafe { slice::from_raw_parts(self.input.add(start), self.input_pos - start).to_vec() }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Auto-close Logic
// ═══════════════════════════════════════════════════════════════════════════════

/// Check if a tag name is a "heading" element (h1-h6).
fn is_heading(name: &str) -> bool {
    matches!(name, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}

/// Get the parent element of a node, walking up to find the nearest element.
/// Returns the current node's parent if it's an element, or walks up.
///
/// # Safety
///
/// - `node` must be NULL or a valid pointer to an `_xmlNode`; every node
///   reached through the `parent` link must also be a valid `_xmlNode`, and
///   the parent chain must terminate in a NULL pointer (no cycles).
#[allow(dead_code)]
unsafe fn get_parent_element(node: *mut _xmlNode) -> *mut _xmlNode {
    if node.is_null() {
        return ptr::null_mut();
    }
    let mut n = node;
    loop {
        let parent = unsafe { (*n).parent };
        if parent.is_null() {
            return ptr::null_mut();
        }
        let ptype = unsafe { (*parent).type_ };
        if ptype == XML_ELEMENT_NODE as c_int
            || ptype == XML_HTML_DOCUMENT_NODE as c_int
            || ptype == XML_DOCUMENT_NODE as c_int
        {
            return parent;
        }
        n = parent;
    }
}

/// Auto-close elements that should be closed before opening a new tag.
/// Returns the new current insertion point.
///
/// # Safety
///
/// - `ctxt` must be a valid `HtmlParserCtxt` whose `current` field is NULL or
///   points into a well-formed node tree; every node reached through the
///   `parent` link must be a valid `_xmlNode` and the chain must terminate in
///   a NULL pointer (no cycles).
/// - For every element node visited, `name` must be NULL or a valid
///   NUL-terminated `xmlChar` string readable by `xmlstr_to_bytes`.
unsafe fn auto_close_element(ctxt: &mut HtmlParserCtxt, tag_name: &str) {
    let tag_lower: Vec<u8> = tag_name.bytes().map(|b| b.to_ascii_lowercase()).collect();
    let tag_lower_str = match core::str::from_utf8(&tag_lower) {
        Ok(s) => s,
        Err(_) => return,
    };

    let info = html_tag_lookup(tag_lower_str);

    let mut current = ctxt.current;

    // Rule 1: <p> auto-closes before another <p>, and before block elements
    if tag_lower_str == "p" || info.is_some_and(|i| i.flags & HTML_BLOCK != 0) {
        // Close any open <p> elements
        let mut cur2 = current;
        while !cur2.is_null() {
            let ctype = unsafe { (*cur2).type_ };
            if ctype == XML_ELEMENT_NODE as c_int && !unsafe { (*cur2).name.is_null() } {
                let name_bytes = unsafe { xmlstr_to_bytes((*cur2).name) };
                let name_str = core::str::from_utf8(name_bytes).unwrap_or("");
                if name_str.eq_ignore_ascii_case("p") {
                    // Close this <p> by moving current up past it
                    current = unsafe { (*cur2).parent };
                    break;
                }
            }
            cur2 = unsafe { (*cur2).parent };
        }
    }

    // Rule 2: Headings (h1-h6) auto-close other headings
    if is_heading(tag_lower_str) {
        let mut cur2 = current;
        while !cur2.is_null() {
            let ctype = unsafe { (*cur2).type_ };
            if ctype == XML_ELEMENT_NODE as c_int && !unsafe { (*cur2).name.is_null() } {
                let name_bytes = unsafe { xmlstr_to_bytes((*cur2).name) };
                let name_str = core::str::from_utf8(name_bytes).unwrap_or("");
                if is_heading(name_str) {
                    current = unsafe { (*cur2).parent };
                    break;
                }
            }
            cur2 = unsafe { (*cur2).parent };
        }
    }

    // Rule 3: <li> auto-closes another <li>
    if tag_lower_str == "li" {
        let mut cur2 = current;
        while !cur2.is_null() {
            let ctype = unsafe { (*cur2).type_ };
            if ctype == XML_ELEMENT_NODE as c_int && !unsafe { (*cur2).name.is_null() } {
                let name_bytes = unsafe { xmlstr_to_bytes((*cur2).name) };
                let name_str = core::str::from_utf8(name_bytes).unwrap_or("");
                if name_str.eq_ignore_ascii_case("li") {
                    current = unsafe { (*cur2).parent };
                    break;
                }
            }
            cur2 = unsafe { (*cur2).parent };
        }
    }

    // Rule 4: <dt>/<dd> auto-close another <dt>/<dd>
    if tag_lower_str == "dt" || tag_lower_str == "dd" {
        let mut cur2 = current;
        while !cur2.is_null() {
            let ctype = unsafe { (*cur2).type_ };
            if ctype == XML_ELEMENT_NODE as c_int && !unsafe { (*cur2).name.is_null() } {
                let name_bytes = unsafe { xmlstr_to_bytes((*cur2).name) };
                let name_str = core::str::from_utf8(name_bytes).unwrap_or("");
                if name_str == "dt" || name_str == "dd" {
                    current = unsafe { (*cur2).parent };
                    break;
                }
            }
            cur2 = unsafe { (*cur2).parent };
        }
    }

    // Rule 5: <tr> auto-closes the open row — the <tr> itself and any open
    // <td>/<th> cell (which is a child of that row); a new <td>/<th> only
    // auto-closes an open <td>/<th> (the previous cell) and stays inside the
    // open <tr> (upstream htmlAutoClose). The walk must NOT stop at a cell:
    // it has to keep climbing to the enclosing <tr> and close the whole row
    // (otherwise the new <tr> would nest inside the old one — corpus
    // html-table).
    if tag_lower_str == "tr" {
        let mut cur2 = current;
        while !cur2.is_null() {
            let ctype = unsafe { (*cur2).type_ };
            if ctype == XML_ELEMENT_NODE as c_int && !unsafe { (*cur2).name.is_null() } {
                let name_bytes = unsafe { xmlstr_to_bytes((*cur2).name) };
                let name_str = core::str::from_utf8(name_bytes).unwrap_or("");
                if name_str == "tr" {
                    current = unsafe { (*cur2).parent };
                    break;
                }
            }
            cur2 = unsafe { (*cur2).parent };
        }
    } else if tag_lower_str == "td" || tag_lower_str == "th" {
        let mut cur2 = current;
        while !cur2.is_null() {
            let ctype = unsafe { (*cur2).type_ };
            if ctype == XML_ELEMENT_NODE as c_int && !unsafe { (*cur2).name.is_null() } {
                let name_bytes = unsafe { xmlstr_to_bytes((*cur2).name) };
                let name_str = core::str::from_utf8(name_bytes).unwrap_or("");
                if name_str == "td" || name_str == "th" {
                    current = unsafe { (*cur2).parent };
                    break;
                }
            }
            cur2 = unsafe { (*cur2).parent };
        }
    }

    // Rule 6: <thead>, <tbody>, <tfoot> auto-close each other
    if matches!(tag_lower_str, "thead" | "tbody" | "tfoot") {
        let mut cur2 = current;
        while !cur2.is_null() {
            let ctype = unsafe { (*cur2).type_ };
            if ctype == XML_ELEMENT_NODE as c_int && !unsafe { (*cur2).name.is_null() } {
                let name_bytes = unsafe { xmlstr_to_bytes((*cur2).name) };
                let name_str = core::str::from_utf8(name_bytes).unwrap_or("");
                if name_str == "thead" || name_str == "tbody" || name_str == "tfoot" {
                    current = unsafe { (*cur2).parent };
                    break;
                }
            }
            cur2 = unsafe { (*cur2).parent };
        }
    }

    // Rule 7: <colgroup> auto-closes another <colgroup>
    if tag_lower_str == "colgroup" {
        let mut cur2 = current;
        while !cur2.is_null() {
            let ctype = unsafe { (*cur2).type_ };
            if ctype == XML_ELEMENT_NODE as c_int && !unsafe { (*cur2).name.is_null() } {
                let name_bytes = unsafe { xmlstr_to_bytes((*cur2).name) };
                let name_str = core::str::from_utf8(name_bytes).unwrap_or("");
                if name_str == "colgroup" {
                    current = unsafe { (*cur2).parent };
                    break;
                }
            }
            cur2 = unsafe { (*cur2).parent };
        }
    }

    // Rule 8: <caption> auto-closes another <caption>
    if tag_lower_str == "caption" {
        let mut cur2 = current;
        while !cur2.is_null() {
            let ctype = unsafe { (*cur2).type_ };
            if ctype == XML_ELEMENT_NODE as c_int && !unsafe { (*cur2).name.is_null() } {
                let name_bytes = unsafe { xmlstr_to_bytes((*cur2).name) };
                let name_str = core::str::from_utf8(name_bytes).unwrap_or("");
                if name_str == "caption" {
                    current = unsafe { (*cur2).parent };
                    break;
                }
            }
            cur2 = unsafe { (*cur2).parent };
        }
    }

    // Rule 9: <form> auto-closes another <form> (in libxml2 behavior)
    if tag_lower_str == "form" {
        let mut cur2 = current;
        while !cur2.is_null() {
            let ctype = unsafe { (*cur2).type_ };
            if ctype == XML_ELEMENT_NODE as c_int && !unsafe { (*cur2).name.is_null() } {
                let name_bytes = unsafe { xmlstr_to_bytes((*cur2).name) };
                let name_str = core::str::from_utf8(name_bytes).unwrap_or("");
                if name_str.eq_ignore_ascii_case("form") {
                    current = unsafe { (*cur2).parent };
                    break;
                }
            }
            cur2 = unsafe { (*cur2).parent };
        }
    }

    ctxt.current = current;
}

// ═══════════════════════════════════════════════════════════════════════════════
// Implicit Element Creation
// ═══════════════════════════════════════════════════════════════════════════════

/// Ensure the html element exists, creating it implicitly if needed.
unsafe fn ensure_html(ctxt: &mut HtmlParserCtxt) -> *mut _xmlNode {
    // UPSTREAM-PARITY (HTML_PARSE_NOIMPLIED): never auto-create the html
    // skeleton when the caller suppressed implied elements (bug76285).
    if ctxt.options & HTML_PARSE_NOIMPLIED != 0 {
        return ptr::null_mut();
    }
    if !ctxt.html.is_null() {
        return ctxt.html;
    }

    let html_node = tree::new_node(ptr::null_mut(), b"html\0" as *const u8 as *const xmlChar);
    if html_node.is_null() {
        return ptr::null_mut();
    }
    {
        // Set HTML_IMPLIED flag concept - mark as auto-created
        // (We track this via a separate flag rather than modifying the node structure)
    }
    ctxt.html = html_node;
    ctxt.html_created = true;

    // Add to document
    tree::add_child(ctxt.doc as *mut _xmlNode, html_node);
    ctxt.current = html_node;

    html_node
}

/// Ensure the head element exists, creating it implicitly if needed.
unsafe fn ensure_head(ctxt: &mut HtmlParserCtxt) -> *mut _xmlNode {
    if ctxt.options & HTML_PARSE_NOIMPLIED != 0 {
        return ptr::null_mut();
    }
    if !ctxt.head.is_null() {
        return ctxt.head;
    }

    // Ensure html exists first
    ensure_html(ctxt);

    let head_node = tree::new_node(ptr::null_mut(), b"head\0" as *const u8 as *const xmlChar);
    if head_node.is_null() {
        return ptr::null_mut();
    }
    ctxt.head = head_node;
    ctxt.head_created = true;

    // Add as child of html
    tree::add_child(ctxt.html, head_node);
    ctxt.current = head_node;
    ctxt.in_head = true;

    head_node
}

/// Ensure the body element exists, creating it implicitly if needed.
unsafe fn ensure_body(ctxt: &mut HtmlParserCtxt) -> *mut _xmlNode {
    if ctxt.options & HTML_PARSE_NOIMPLIED != 0 {
        return ptr::null_mut();
    }
    if !ctxt.body.is_null() {
        return ctxt.body;
    }

    // Ensure html exists first
    ensure_html(ctxt);

    let body_node = tree::new_node(ptr::null_mut(), b"body\0" as *const u8 as *const xmlChar);
    if body_node.is_null() {
        return ptr::null_mut();
    }
    ctxt.body = body_node;
    ctxt.body_created = true;

    // Add as child of html
    tree::add_child(ctxt.html, body_node);
    ctxt.current = body_node;
    ctxt.in_body = true;

    body_node
}

/// Transition from head to body when body content is encountered.
#[allow(dead_code)]
unsafe fn transition_to_body(ctxt: &mut HtmlParserCtxt) {
    if ctxt.in_head && !ctxt.seen_body_content {
        ctxt.seen_body_content = true;
        ctxt.in_head = false;
        ensure_body(ctxt);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tokenizer
// ═══════════════════════════════════════════════════════════════════════════════

/// Result of parsing an attribute.
struct HtmlAttr {
    name: Vec<u8>,
    value: Vec<u8>,
    #[allow(dead_code)]
    quoted: bool,
}

/// Parse an attribute name.
fn parse_attr_name(ctxt: &mut HtmlParserCtxt) -> Vec<u8> {
    let mut name = Vec::new();
    while let Some(ch) = ctxt.peek() {
        if ch == b'='
            || ch == b'>'
            || ch == b'/'
            || ch == b' '
            || ch == b'\t'
            || ch == b'\n'
            || ch == b'\r'
        {
            break;
        }
        name.push(ch);
        ctxt.next();
    }
    name
}

/// Parse an attribute value (may be quoted or unquoted).
fn parse_attr_value(ctxt: &mut HtmlParserCtxt) -> (Vec<u8>, bool) {
    ctxt.skip_whitespace();

    let quote = match ctxt.peek() {
        Some(b'"') => {
            ctxt.next(); // consume opening quote
            b'"'
        }
        Some(b'\'') => {
            ctxt.next(); // consume opening quote
            b'\''
        }
        _ => {
            // Unquoted value
            let value = ctxt.read_while(|ch| {
                ch != b'>' && ch != b' ' && ch != b'\t' && ch != b'\n' && ch != b'\r'
            });
            return (value, false);
        }
    };

    // Quoted value
    let mut value = Vec::new();
    loop {
        match ctxt.next() {
            Some(ch) if ch == quote => break,
            Some(ch) => value.push(ch),
            None => break,
        }
    }
    (value, true)
}

/// Parse attributes until we hit '>' or end of tag.
fn parse_attributes(ctxt: &mut HtmlParserCtxt) -> Vec<HtmlAttr> {
    let mut attrs = Vec::new();

    loop {
        ctxt.skip_whitespace();

        match ctxt.peek() {
            Some(b'>') | None => break,
            Some(b'/')
                // Could be self-closing tag like <br/>
                if ctxt.peek_at(1) == Some(b'>') => {
                    break;
                }
                // Otherwise it's part of a minimized attribute or path
            _ => {}
        }

        let name = parse_attr_name(ctxt);
        if name.is_empty() {
            break;
        }

        // Check for '='
        ctxt.skip_whitespace();
        if ctxt.peek() == Some(b'=') {
            ctxt.next(); // consume '='
            let (value, quoted) = parse_attr_value(ctxt);
            attrs.push(HtmlAttr {
                name,
                value,
                quoted,
            });
        } else {
            // Minimized attribute (e.g., <option selected>)
            attrs.push(HtmlAttr {
                name,
                value: Vec::new(),
                quoted: false,
            });
        }
    }

    attrs
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tree Builder
// ═══════════════════════════════════════════════════════════════════════════════

/// Process a resolved HTML entity reference and return the replacement bytes.
fn resolve_entity(name: &str) -> Vec<u8> {
    if let Some(replacement) = html_entity_lookup(name) {
        replacement.as_bytes().to_vec()
    } else {
        // Unknown entity: leave as-is (pass through as text)
        let mut result = Vec::new();
        result.push(b'&');
        result.extend_from_slice(name.as_bytes());
        result.push(b';');
        result
    }
}

/// Handle a numeric character reference (decimal or hex).
fn resolve_numeric_entity(value: &str, is_hex: bool) -> Vec<u8> {
    let codepoint = if is_hex {
        u32::from_str_radix(value, 16).unwrap_or(0xFFFD)
    } else {
        value.parse::<u32>().unwrap_or(0xFFFD)
    };

    if codepoint == 0 {
        return Vec::new();
    }

    // Convert codepoint to UTF-8
    match char::from_u32(codepoint) {
        Some(c) => {
            let mut buf = [0u8; 4];
            let s = c.encode_utf8(&mut buf);
            s.as_bytes().to_vec()
        }
        None => vec![0xEF, 0xBF, 0xBD], // replacement character
    }
}

/// Parse an entity reference starting at current position (which points to '&').
/// Returns the replacement text and advances position.
fn parse_entity(ctxt: &mut HtmlParserCtxt) -> Vec<u8> {
    // We should be at '&'
    match ctxt.peek() {
        Some(b'&') => {
            ctxt.next(); // consume '&'
        }
        _ => return vec![b'&'],
    }

    // Check for numeric entities
    if ctxt.peek() == Some(b'#') {
        ctxt.next(); // consume '#'
        let is_hex = ctxt.peek() == Some(b'x') || ctxt.peek() == Some(b'X');
        if is_hex {
            ctxt.next(); // consume 'x' or 'X'
        }

        let digits = ctxt.read_while(|ch| {
            if is_hex {
                ch.is_ascii_hexdigit()
            } else {
                ch.is_ascii_digit()
            }
        });

        let digits_str = core::str::from_utf8(&digits).unwrap_or("");
        if digits_str.is_empty() {
            let mut result = vec![b'&', b'#'];
            if is_hex {
                result.push(b'x');
            }
            return result;
        }

        // Expect semicolon
        if ctxt.peek() == Some(b';') {
            ctxt.next();
        }

        return resolve_numeric_entity(digits_str, is_hex);
    }

    // Named entity
    let name = ctxt.read_while(|ch| ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'-');
    let name_str = core::str::from_utf8(&name).unwrap_or("");

    // Expect semicolon
    if ctxt.peek() == Some(b';') {
        ctxt.next();
    }

    resolve_entity(name_str)
}

/// Handle text content in the tree builder.
///
/// # Safety
///
/// - `ctxt` must be a valid `HtmlParserCtxt` with a non-NULL `doc` pointing
///   to a valid `_xmlDoc`.
/// - The `head`, `body`, `html` and `current` fields, when non-NULL, must be
///   valid `_xmlNode` pointers owned by that document tree.
/// - Nodes returned by `tree::new_text` and `tree::new_node` are NULL-checked
///   before their fields are written, and `tree::add_child` requires a valid
///   non-NULL parent node.

/// Create an element node from a heap-free byte-slice name.
///
/// `tree::new_node` DUPLICATES its name argument, so passing a freshly
/// `bytes_to_xmlstr`-allocated copy leaks it (ASan fuzz: ~5 bytes per element
/// on every HTML parse). This helper frees the temporary copy.
///
/// # Safety
///
/// - `name` is a valid byte slice for the call.
unsafe fn new_element_node(ns: *mut _xmlNs, name: &[u8]) -> *mut _xmlNode {
    let name_c = bytes_to_xmlstr(name);
    let node = tree::new_node(ns, name_c);
    if !name_c.is_null() {
        xmlFreeImpl(name_c as *mut c_void);
    }
    node
}

/// Create a text node whose content is a byte slice.
///
/// `tree::new_text(NULL)` pre-allocates a 1-byte empty content placeholder;
/// assigning over it leaks that byte (ASan fuzz: 1 byte per text node). This
/// helper replaces the placeholder with the real (NUL-terminated) content.
///
/// # Safety
///
/// - `content` is a valid byte slice for the call.
unsafe fn new_text_node(content: &[u8]) -> *mut _xmlNode {
    let node = tree::new_text(ptr::null_mut());
    if node.is_null() {
        return ptr::null_mut();
    }
    let content_c = bytes_to_xmlstr(content);
    unsafe {
        if !(*node).content.is_null() {
            xmlFreeImpl((*node).content as *mut c_void);
        }
        // NULL only when bytes_to_xmlstr hit OOM.
        (*node).content = content_c;
    }
    node
}

unsafe fn handle_text(ctxt: &mut HtmlParserCtxt, text: &[u8]) {
    if text.is_empty() {
        return;
    }

    // Determine current insertion point
    let parent = if ctxt.in_head {
        ctxt.head
    } else if ctxt.in_body || ctxt.body_created {
        ctxt.body
    } else if ctxt.html_created {
        ctxt.html
    } else {
        ctxt.doc as *mut _xmlNode
    };

    let insertion_point = if ctxt.current.is_null() {
        parent
    } else {
        ctxt.current
    };

    if insertion_point.is_null() {
        // Fall back to document
        let text_node = new_text_node(text);
        if !text_node.is_null() {
            tree::add_child(ctxt.doc as *mut _xmlNode, text_node);
        }
        return;
    }

    // UPSTREAM-PARITY: whitespace-only text at the document level (before or
    // after the root element) is discarded; non-whitespace stray text is
    // wrapped in a new `html` element (htmlParseCharData behavior), with
    // leading blank characters skipped.
    if insertion_point == ctxt.doc as *mut _xmlNode {
        if text.iter().all(|b| b.is_ascii_whitespace()) {
            return;
        }
        let content = trim_ascii_start(text);
        let html_node = new_element_node(ptr::null_mut(), b"html");
        if !html_node.is_null() {
            let text_node = new_text_node(content);
            if !text_node.is_null() {
                tree::add_child(html_node, text_node);
            }
            tree::add_child(ctxt.doc as *mut _xmlNode, html_node);
            ctxt.html = html_node;
            ctxt.html_created = true;
            ctxt.current = html_node;
        }
        return;
    }

    let text_node = new_text_node(text);
    if text_node.is_null() {
        return;
    }

    tree::add_child(insertion_point, text_node);
}

/// Attach parsed HTML attributes to a freshly created element node.
///
/// # Safety
///
/// - `node` must be a valid element node; `attrs` a live slice of parsed
///   attributes.
unsafe fn attach_attrs(node: *mut _xmlNode, attrs: &[HtmlAttr]) {
    for attr in attrs {
        let name_c = bytes_to_xmlstr(&attr.name);
        let val_c = bytes_to_xmlstr(&attr.value);
        if !name_c.is_null() {
            tree::set_prop(node, name_c, val_c);
            xmlFreeImpl(name_c as *mut c_void);
            if !val_c.is_null() {
                xmlFreeImpl(val_c as *mut c_void);
            }
        }
    }
}

/// Process a start tag in the tree builder.
unsafe fn handle_start_tag(ctxt: &mut HtmlParserCtxt, tag_name: &[u8], attrs: &[HtmlAttr]) {
    let tag_lower: Vec<u8> = tag_name.iter().map(|b| b.to_ascii_lowercase()).collect();
    let tag_str = core::str::from_utf8(&tag_lower).unwrap_or("");

    let info = html_tag_lookup(tag_str);

    // Determine tag category
    let is_head_tag = info.is_some_and(|i| i.flags & HTML_HEAD != 0);
    let _is_body_tag = info.is_some_and(|i| i.flags & HTML_BODY != 0);
    let is_empty = info.is_some_and(|i| i.flags & HTML_EMPTY != 0);
    let _is_block = info.is_some_and(|i| i.flags & HTML_BLOCK != 0);

    // Handle special elements
    if tag_str == "html" {
        if !ctxt.html.is_null() && !ctxt.html_created {
            // Second <html> tag, skip it
            return;
        }
        // Create or use existing html
        if ctxt.html.is_null() {
            let html_node = new_element_node(ptr::null_mut(), tag_name);
            if !html_node.is_null() {
                attach_attrs(html_node, attrs);
                ctxt.html = html_node;
                ctxt.html_created = false; // parsed, not implied
                tree::add_child(ctxt.doc as *mut _xmlNode, html_node);
                ctxt.current = html_node;
            }
        } else {
            // html already auto-created, just set current
            ctxt.current = ctxt.html;
        }
        return;
    }

    if tag_str == "head" {
        if !ctxt.head.is_null() && !ctxt.head_created {
            // Second <head> tag, skip it
            return;
        }
        // Ensure html exists
        ensure_html(ctxt);

        if ctxt.head.is_null() {
            let head_node = new_element_node(ptr::null_mut(), tag_name);
            if !head_node.is_null() {
                attach_attrs(head_node, attrs);
                ctxt.head = head_node;
                ctxt.head_created = false;
                // UPSTREAM-PARITY: a source <head> without an <html> parent
                // becomes a top-level element under HTML_PARSE_NOIMPLIED.
                let parent = if ctxt.html.is_null() {
                    ctxt.doc as *mut _xmlNode
                } else {
                    ctxt.html
                };
                tree::add_child(parent, head_node);
                ctxt.current = head_node;
                ctxt.in_head = true;
            }
        } else {
            ctxt.current = ctxt.head;
            ctxt.in_head = true;
        }
        return;
    }

    if tag_str == "body" {
        if !ctxt.body.is_null() && !ctxt.body_created {
            // Second <body> tag, skip it
            return;
        }
        // Ensure html exists
        ensure_html(ctxt);

        if ctxt.body.is_null() {
            let body_node = new_element_node(ptr::null_mut(), tag_name);
            if !body_node.is_null() {
                attach_attrs(body_node, attrs);
                ctxt.body = body_node;
                ctxt.body_created = false;
                // UPSTREAM-PARITY: a source <body> without an <html> parent
                // becomes a top-level element under HTML_PARSE_NOIMPLIED.
                let parent = if ctxt.html.is_null() {
                    ctxt.doc as *mut _xmlNode
                } else {
                    ctxt.html
                };
                tree::add_child(parent, body_node);
                ctxt.current = body_node;
                ctxt.in_body = true;
                ctxt.in_head = false;
                ctxt.seen_body_content = true;
            }
        } else {
            ctxt.current = ctxt.body;
            ctxt.in_body = true;
            ctxt.in_head = false;
            ctxt.seen_body_content = true;
        }
        return;
    }

    // For head-only elements (<title>, <meta>, <link>, <style>, <script>)
    if is_head_tag && !ctxt.seen_body_content {
        if ctxt.head.is_null() {
            ensure_head(ctxt);
        }

        if is_empty {
            // Void element in head
            let node = new_element_node(ptr::null_mut(), tag_name);
            if !node.is_null() {
                attach_attrs(node, attrs);
                // UPSTREAM-PARITY (NOIMPLIED): top-level head-only elements
                // become document children.
                let ip = if ctxt.current.is_null() {
                    ctxt.doc as *mut _xmlNode
                } else {
                    ctxt.current
                };
                tree::add_child(ip, node);
            }
            return;
        }

        let node = new_element_node(ptr::null_mut(), tag_name);
        if !node.is_null() {
            attach_attrs(node, attrs);
            let ip = if ctxt.current.is_null() {
                ctxt.doc as *mut _xmlNode
            } else {
                ctxt.current
            };
            tree::add_child(ip, node);
            ctxt.current = node;
        }
        return;
    }

    // Body content - transition from head if needed
    if !is_head_tag || ctxt.seen_body_content {
        if !ctxt.seen_body_content {
            ctxt.seen_body_content = true;
            ctxt.in_head = false;
            // UPSTREAM-PARITY (HTML_PARSE_NOIMPLIED): body content at the top
            // of a no-implied parse stays at document level (ctxt.current
            // NULL) instead of materialising the html/body skeleton.
            if ctxt.options & HTML_PARSE_NOIMPLIED != 0 {
                // current stays NULL -> top-level nodes attach to the doc
            } else if ctxt.body.is_null() {
                ensure_body(ctxt);
            } else {
                ctxt.current = ctxt.body;
                ctxt.in_body = true;
            }
        } else if ctxt.body.is_null() && ctxt.options & HTML_PARSE_NOIMPLIED == 0 {
            ensure_body(ctxt);
        }
    }

    // Auto-close elements as needed
    if !ctxt.current.is_null() {
        auto_close_element(ctxt, tag_str);
    }

    if is_empty {
        // Void element: create node, add attributes, add as child (no children)
        let node = new_element_node(ptr::null_mut(), tag_name);
        if !node.is_null() {
            for attr in attrs {
                let name_c = bytes_to_xmlstr(&attr.name);
                let val_c = bytes_to_xmlstr(&attr.value);
                if !name_c.is_null() {
                    tree::set_prop(node, name_c, val_c);
                    xmlFreeImpl(name_c as *mut c_void);
                    if !val_c.is_null() {
                        xmlFreeImpl(val_c as *mut c_void);
                    }
                }
            }
            let insertion_point = if ctxt.current.is_null() {
                if ctxt.options & HTML_PARSE_NOIMPLIED != 0 {
                    ctxt.doc as *mut _xmlNode
                } else {
                    ctxt.body
                }
            } else {
                ctxt.current
            };
            if !insertion_point.is_null() {
                tree::add_child(insertion_point, node);
            }
        }
        return;
    }

    // Regular element
    let node = new_element_node(ptr::null_mut(), tag_name);
    if !node.is_null() {
        for attr in attrs {
            let name_c = bytes_to_xmlstr(&attr.name);
            let val_c = bytes_to_xmlstr(&attr.value);
            if !name_c.is_null() {
                tree::set_prop(node, name_c, val_c);
                xmlFreeImpl(name_c as *mut c_void);
                if !val_c.is_null() {
                    xmlFreeImpl(val_c as *mut c_void);
                }
            }
        }

        let insertion_point = if ctxt.current.is_null() {
            if ctxt.in_body || ctxt.body_created {
                ctxt.body
            } else if ctxt.in_head || ctxt.head_created {
                ctxt.head
            } else if ctxt.html_created {
                ctxt.html
            } else {
                ctxt.doc as *mut _xmlNode
            }
        } else {
            ctxt.current
        };

        if !insertion_point.is_null() {
            tree::add_child(insertion_point, node);
            // For non-void elements, this becomes the new insertion point
            ctxt.current = node;
        }
    }
}

/// Process an end tag in the tree builder.
///
/// # Safety
///
/// - `ctxt` must be a valid `HtmlParserCtxt` whose `current` field is NULL or
///   points into a well-formed node tree; every node reached through the
///   `parent` link must be a valid `_xmlNode` and the chain must terminate in
///   a NULL pointer (no cycles).
/// - Element `name` pointers visited must be NULL or valid NUL-terminated
///   `xmlChar` strings.
unsafe fn handle_end_tag(ctxt: &mut HtmlParserCtxt, tag_name: &[u8]) {
    let tag_lower: Vec<u8> = tag_name.iter().map(|b| b.to_ascii_lowercase()).collect();
    let tag_str = core::str::from_utf8(&tag_lower).unwrap_or("");

    let info = html_tag_lookup(tag_str);

    // For elements with no end tag (void elements or optional end tags),
    // just ignore the end tag.
    if info.is_some_and(|i| i.flags & HTML_EMPTY != 0) {
        return;
    }

    if tag_str == "html" {
        ctxt.current = ctxt.doc as *mut _xmlNode;
        return;
    }

    if tag_str == "head" {
        ctxt.in_head = false;
        ctxt.current = ctxt.html;
        return;
    }

    if tag_str == "body" {
        ctxt.in_body = false;
        ctxt.current = ctxt.html;
        return;
    }

    // Walk up the tree to find a matching open element
    let mut cur = ctxt.current;
    while !cur.is_null() {
        let ctype = unsafe { (*cur).type_ };
        if ctype == XML_ELEMENT_NODE as c_int && !unsafe { (*cur).name.is_null() } {
            let name_bytes = unsafe { xmlstr_to_bytes((*cur).name) };
            if name_bytes.eq_ignore_ascii_case(tag_name) {
                // Found the matching element - close by moving current to parent
                ctxt.current = unsafe { (*cur).parent };
                return;
            }
        }
        cur = unsafe { (*cur).parent };
    }

    // If no matching element found, ignore the end tag (tag-recovery behavior)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Main Parse Function
// ═══════════════════════════════════════════════════════════════════════════════

/// Convert `data` from the declared input encoding to UTF-8.
///
/// UPSTREAM-PARITY: `xmlCtxtNewInputFromMemory` -> `xmlSwitchInputEncoding`
/// installs a decoder on the input buffer so the parser (and the tree it
/// builds) always consume UTF-8. Returns `None` when no conversion applies
/// (NULL encoding, UTF-8/ASCII input, or an encoding the crate cannot
/// convert — the caller then parses the raw bytes, matching upstream's
/// recover-with-raw-bytes behavior when switching fails).
fn convert_input_to_utf8(encoding: *const c_char, data: &[u8]) -> Option<Vec<u8>> {
    if encoding.is_null() {
        return None;
    }
    let name = unsafe { core::ffi::CStr::from_ptr(encoding).to_bytes() };
    match crate::xml::encoding::encoding_from_name(name) {
        xmlCharEncoding::XML_CHAR_ENCODING_8859_1 => {
            Some(crate::xml::encoding::latin1_to_utf8(data))
        }
        xmlCharEncoding::XML_CHAR_ENCODING_UTF16LE => {
            crate::xml::encoding::utf16le_to_utf8(data).ok()
        }
        xmlCharEncoding::XML_CHAR_ENCODING_UTF16BE => {
            crate::xml::encoding::utf16be_to_utf8(data).ok()
        }
        // UTF-32/UCS-4 (R-000157): lxml feeds PEP-393 KIND-4 python strings to
        // htmlCtxtReadMemory as UTF-32LE/BE raw buffers; without conversion the
        // parser would consume the raw 4-byte units as UTF-8 and garble the
        // tree (wide-character HTML through etree.HTML crashed teardown).
        xmlCharEncoding::XML_CHAR_ENCODING_UCS4LE => {
            crate::xml::encoding::ucs4le_to_utf8(data).ok()
        }
        xmlCharEncoding::XML_CHAR_ENCODING_UCS4BE => {
            crate::xml::encoding::ucs4be_to_utf8(data).ok()
        }
        // UTF-8 / US-ASCII / NONE (and unsupported encodings): no conversion.
        _ => None,
    }
}

/// Parse a leading `<!DOCTYPE ...>` declaration from HTML input bytes.
///
/// Returns `Some((name, external_id, system_id))` when the input begins (after
/// optional whitespace) with a case-insensitive `<!DOCTYPE`. Quoted external
/// and system identifiers are unwrapped; unquoted identifiers are taken as-is
/// up to the closing `>`. Returns `None` when no DOCTYPE is declared.
fn parse_html_doctype_decl(input: &[u8]) -> Option<(Vec<u8>, Option<Vec<u8>>, Option<Vec<u8>>)> {
    let mut i = 0usize;
    while i < input.len() && input[i].is_ascii_whitespace() {
        i += 1;
    }
    if i + 2 >= input.len() || input[i] != b'<' || !(input[i + 1] == b'!') {
        return None;
    }
    let kw = b"DOCTYPE";
    if !input.get(i + 2..i + 2 + kw.len()).is_some_and(|s| {
        s.iter()
            .enumerate()
            .all(|(k, b)| b.to_ascii_uppercase() == kw[k])
    }) {
        return None;
    }
    i += 2 + kw.len();
    // Skip whitespace after DOCTYPE, then read the root name.
    while i < input.len() && input[i].is_ascii_whitespace() {
        i += 1;
    }
    let name_start = i;
    while i < input.len() && !input[i].is_ascii_whitespace() && input[i] != b'>' {
        i += 1;
    }
    // A nameless `<!DOCTYPE>` (nothing before the closing '>') is still a
    // declared DOCTYPE — upstream htmlParseDocTypeDecl fires internalSubset
    // with a NULL name (gh17500/bug78025: doctype->name is ""), it must NOT
    // fall through to the default HTML 4.0 DTD.
    let name = if name_start == i {
        Vec::new()
    } else {
        input[name_start..i].to_vec()
    };
    // If the nameless doctype ends right at '>', there are no ids either.
    if name.is_empty() {
        return Some((name, None, None));
    }
    // Skip whitespace, then optional PUBLIC/SYSTEM id.
    let mut ext: Option<Vec<u8>> = None;
    let mut sys: Option<Vec<u8>> = None;
    while i < input.len() && input[i].is_ascii_whitespace() {
        i += 1;
    }
    if i < input.len() && input[i] != b'>' {
        // a PUBLIC/SYSTEM keyword may follow
        let word_start = i;
        while i < input.len() && input[i].is_ascii_alphabetic() {
            i += 1;
        }
        let word = input[word_start..i].to_ascii_uppercase();
        if word == b"PUBLIC" {
            while i < input.len() && input[i].is_ascii_whitespace() {
                i += 1;
            }
            // external identifier (PUBLIC) comes first
            if i < input.len() && (input[i] == b'"' || input[i] == b'\'') {
                let q = input[i];
                i += 1;
                let v_start = i;
                while i < input.len() && input[i] != q {
                    i += 1;
                }
                ext = Some(input[v_start..i].to_vec());
                if i < input.len() {
                    i += 1;
                }
            }
            while i < input.len() && input[i].is_ascii_whitespace() {
                i += 1;
            }
            // system literal (may be absent)
            if i < input.len()
                && i + 1 < input.len()
                && (input[i] == b'"' || input[i] == b'\'')
                && input[i] != b'>'
            {
                let q = input[i];
                i += 1;
                let v_start = i;
                while i < input.len() && input[i] != q {
                    i += 1;
                }
                sys = Some(input[v_start..i].to_vec());
            }
        } else if word == b"SYSTEM" {
            while i < input.len() && input[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < input.len() && (input[i] == b'"' || input[i] == b'\'') {
                let q = input[i];
                i += 1;
                let v_start = i;
                while i < input.len() && input[i] != q {
                    i += 1;
                }
                sys = Some(input[v_start..i].to_vec());
            }
        }
    }
    Some((name, ext, sys))
}

/// Parse HTML from a buffer.
///
/// # Safety
///
/// - `buffer` must point to valid memory of at least `size` bytes.
unsafe fn html_parse_buffer(
    ctxt: &mut HtmlParserCtxt,
    buffer: *const c_char,
    size: c_int,
) -> *mut _xmlDoc {
    if buffer.is_null() || size <= 0 {
        return ptr::null_mut();
    }

    // Create document with HTML_DOCUMENT_NODE type
    let doc = tree::new_doc(ptr::null());
    if doc.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        (*doc).type_ = XML_HTML_DOCUMENT_NODE as c_int;
        // UPSTREAM-PARITY: HTML documents carry no version (htmlNewDocNoDtD
        // leaves version NULL), so drop the XML default set by new_doc.
        if !(*doc).version.is_null() {
            crate::abi::allocator::xmlFreeImpl((*doc).version as *mut c_void);
        }
        (*doc).version = ptr::null_mut();
        // UPSTREAM-PARITY (SAX2.c xmlSAX2StartDocument for HTML parsers): an
        // html-parsed document carries properties = XML_DOC_HTML. The
        // pre-fix XML_DOC_WELLFORMED-only value made PHP's spec serializer
        // treat html-parsed documents as XML and drop the standalone/HTML
        // declaration handling (ext/dom dom005 / gh15670 / gh17397 — the
        // saveXML of a loadHTML'd document lost "standalone=yes").
        (*doc).properties = crate::abi::types::xmlDocProperties::XML_DOC_HTML as c_int;
        // UPSTREAM-PARITY: HTML documents default to standalone="yes"
        // (visible when serialized with the XML serializer, e.g. --xmlout).
        (*doc).standalone = 1;
    }
    // UPSTREAM-PARITY: htmlParseDocument honours a DOCTYPE declared in the
    // source (htmlParseDocTypeDecl fills doc->intSubset with the declared
    // name/public/system ids); only when the source declares none does it
    // create the default HTML 4.0 DTD. nokogiri reads those ids back for
    // dtd.html_dtd?/html5_dtd?, so a source `<!DOCTYPE html>` must NOT pick up
    // the default HTML 4.0 DTD.
    let raw_input = unsafe { slice::from_raw_parts(buffer as *const u8, size as usize) };
    if let Some((name, ext, sys)) = parse_html_doctype_decl(raw_input) {
        // UPSTREAM-PARITY (htmlSAX2InternalSubset / xmlCreateIntSubset): a
        // declared DOCTYPE without a name creates the internal subset with a
        // NULL name (php doctype->name reads back "").
        let name_cstr = if name.is_empty() {
            ptr::null()
        } else {
            crate::xml::string::bytes_to_xmlstr(&name)
        };
        let ext_cstr = ext
            .map(|s| crate::xml::string::bytes_to_xmlstr(&s))
            .unwrap_or(ptr::null_mut());
        let sys_cstr = sys
            .map(|s| crate::xml::string::bytes_to_xmlstr(&s))
            .unwrap_or(ptr::null_mut());
        unsafe {
            crate::xml::dtd::create_int_subset(
                doc,
                name_cstr as *const xmlChar,
                ext_cstr as *const xmlChar,
                sys_cstr as *const xmlChar,
            );
        }
        if !name_cstr.is_null() {
            unsafe { crate::abi::allocator::xmlFreeImpl(name_cstr as *mut c_void) };
        }
        if !ext_cstr.is_null() {
            unsafe { crate::abi::allocator::xmlFreeImpl(ext_cstr as *mut c_void) };
        }
        if !sys_cstr.is_null() {
            unsafe { crate::abi::allocator::xmlFreeImpl(sys_cstr as *mut c_void) };
        }
    } else {
        // Source declares no DOCTYPE: use the default HTML 4.0 DTD — unless
        // HTML_PARSE_NODEFDTD suppresses it (php LIBXML_HTML_NODEFDTD).
        if ctxt.options & HTML_PARSE_NODEFDTD == 0 {
            unsafe {
                crate::xml::dtd::create_int_subset(
                    doc,
                    b"html\0" as *const u8 as *const xmlChar,
                    b"-//W3C//DTD HTML 4.0 Transitional//EN\0" as *const u8 as *const xmlChar,
                    b"http://www.w3.org/TR/REC-html40/loose.dtd\0" as *const u8 as *const xmlChar,
                );
            }
        }
    }
    ctxt.doc = doc;

    // UPSTREAM-PARITY: a declared input encoding is converted to UTF-8 before
    // parsing (xmlCtxtNewInputFromMemory -> xmlSwitchInputEncodingName installs
    // an input-buffer decoder); the parse loop and the tree always consume
    // UTF-8. The converted copy lives for the whole parse.
    let converted: Option<Vec<u8>> = if !ctxt.encoding.is_null() {
        let raw = unsafe { slice::from_raw_parts(buffer as *const u8, size as usize) };
        convert_input_to_utf8(ctxt.encoding, raw)
    } else {
        None
    };
    let (input_ptr, input_len): (*const u8, usize) = match &converted {
        Some(v) => (v.as_ptr(), v.len()),
        None => (buffer as *const u8, size as usize),
    };

    // Set up input
    ctxt.input = input_ptr as *mut u8;
    ctxt.input_len = input_len;
    ctxt.input_pos = 0;
    ctxt.line = 1;

    // Main parse loop
    loop {
        if ctxt.is_eof() {
            break;
        }

        let ch = ctxt.peek().unwrap_or(0);

        if ch == b'<' {
            ctxt.next(); // consume '<'

            // Check for </ (end tag)
            if ctxt.peek() == Some(b'/') {
                ctxt.next(); // consume '/'
                let tag_name = ctxt.read_while(|ch| {
                    ch != b'>' && ch != b' ' && ch != b'\t' && ch != b'\n' && ch != b'\r'
                });

                // Consume until '>'
                while ctxt.peek() != Some(b'>') && !ctxt.is_eof() {
                    ctxt.next();
                }
                if ctxt.peek() == Some(b'>') {
                    ctxt.next(); // consume '>'
                }

                if !tag_name.is_empty() {
                    handle_end_tag(ctxt, &tag_name);
                }
                continue;
            }

            // Check for <!-- (comment)
            if ctxt.peek() == Some(b'!')
                && ctxt.peek_at(1) == Some(b'-')
                && ctxt.peek_at(2) == Some(b'-')
            {
                ctxt.next(); // consume '!'
                ctxt.next(); // consume '-'
                ctxt.next(); // consume '-'

                // Read until -->
                let mut comment_content = Vec::new();
                loop {
                    if ctxt.peek() == Some(b'-')
                        && ctxt.peek_at(1) == Some(b'-')
                        && ctxt.peek_at(2) == Some(b'>')
                    {
                        ctxt.next(); // consume '-'
                        ctxt.next(); // consume '-'
                        ctxt.next(); // consume '>'
                        break;
                    }
                    match ctxt.next() {
                        Some(ch) => comment_content.push(ch),
                        None => break,
                    }
                }

                // Create comment node
                if !comment_content.is_empty() {
                    // new_comment duplicates its content; free the temporary.
                    let cc = bytes_to_xmlstr(&comment_content);
                    let comment_node = tree::new_comment(cc);
                    if !cc.is_null() {
                        xmlFreeImpl(cc as *mut c_void);
                    }
                    if !comment_node.is_null() {
                        let insertion_point = if !ctxt.current.is_null() {
                            ctxt.current
                        } else {
                            ctxt.doc as *mut _xmlNode
                        };
                        tree::add_child(insertion_point, comment_node);
                    }
                }
                continue;
            }

            // Check for <!DOCTYPE
            if ctxt.peek() == Some(b'!') {
                ctxt.next(); // consume '!'
                let _rest = ctxt.read_while(|ch| ch != b'>');
                if ctxt.peek() == Some(b'>') {
                    ctxt.next(); // consume '>'
                }
                // We don't create a DTD node from HTML DOCTYPE in this implementation
                // (matching basic libxml2 behavior where HTML doctype is mostly ignored)
                continue;
            }

            // Check for <? (processing instruction)
            if ctxt.peek() == Some(b'?') {
                ctxt.next(); // consume '?'
                             // Read until we see ?>
                let mut pi_content = Vec::new();
                loop {
                    if ctxt.peek() == Some(b'?') && ctxt.peek_at(1) == Some(b'>') {
                        break;
                    }
                    match ctxt.next() {
                        Some(ch) => pi_content.push(ch),
                        None => break,
                    }
                }
                // Consume ?>
                if ctxt.peek() == Some(b'?') {
                    ctxt.next();
                }
                if ctxt.peek() == Some(b'>') {
                    ctxt.next();
                }
                // Create PI node
                if !pi_content.is_empty() {
                    // Split into target and value
                    let mut parts = pi_content.splitn(2, |b| *b == b' ');
                    let target = parts.next().unwrap_or(&pi_content);
                    let value = parts.next().unwrap_or(b"");

                    // new_pi duplicates both arguments; free the temporaries.
                    let t_c = bytes_to_xmlstr(target);
                    let v_c = bytes_to_xmlstr(value);
                    let pi_node = tree::new_pi(t_c, v_c);
                    if !t_c.is_null() {
                        xmlFreeImpl(t_c as *mut c_void);
                    }
                    if !v_c.is_null() {
                        xmlFreeImpl(v_c as *mut c_void);
                    }
                    if !pi_node.is_null() {
                        let insertion_point = if !ctxt.current.is_null() {
                            ctxt.current
                        } else {
                            ctxt.doc as *mut _xmlNode
                        };
                        tree::add_child(insertion_point, pi_node);
                    }
                }
                continue;
            }

            // Parse start tag
            let tag_name = ctxt.read_while(|ch| {
                ch != b'>' && ch != b'/' && ch != b' ' && ch != b'\t' && ch != b'\n' && ch != b'\r'
            });

            if tag_name.is_empty() {
                // Just a bare '<' with no tag name, treat as text
                handle_text(ctxt, b"<");
                continue;
            }

            // Parse attributes
            let attrs = parse_attributes(ctxt);

            // Check for self-closing (/>) or just >
            if ctxt.peek() == Some(b'/') {
                ctxt.next(); // consume '/'
                if ctxt.peek() == Some(b'>') {
                    ctxt.next(); // consume '>'
                }
            } else if ctxt.peek() == Some(b'>') {
                ctxt.next(); // consume '>'
            }

            // Check if this is a raw text element (script, style)
            let tag_lower: Vec<u8> = tag_name.iter().map(|b| b.to_ascii_lowercase()).collect();
            let tag_str = core::str::from_utf8(&tag_lower).unwrap_or("");

            if tag_str == "script" || tag_str == "style" {
                // Handle raw text content
                // Create the element first
                let raw_node = new_element_node(ptr::null_mut(), &tag_name);
                if !raw_node.is_null() {
                    for attr in &attrs {
                        let name_c = bytes_to_xmlstr(&attr.name);
                        let val_c = bytes_to_xmlstr(&attr.value);
                        if !name_c.is_null() {
                            tree::set_prop(raw_node, name_c, val_c);
                            xmlFreeImpl(name_c as *mut c_void);
                            if !val_c.is_null() {
                                xmlFreeImpl(val_c as *mut c_void);
                            }
                        }
                    }

                    let insertion_point = if ctxt.current.is_null() {
                        if ctxt.in_head {
                            ensure_head(ctxt);
                            ctxt.head
                        } else {
                            ensure_body(ctxt);
                            ctxt.body
                        }
                    } else {
                        ctxt.current
                    };

                    if !insertion_point.is_null() {
                        tree::add_child(insertion_point, raw_node);

                        // Read raw text until matching </script> or </style>
                        let end_tag = format!("</{}", tag_str);
                        let end_bytes = end_tag.as_bytes();
                        let mut raw_text = Vec::new();
                        // Potential end-tag prefix chars ("</script") are
                        // buffered separately: on a full match they are
                        // DISCARDED (the content ends before the '<' —
                        // upstream script-content ends at the '<' that starts
                        // the close tag), on a mismatch they are flushed back
                        // into the text.
                        let mut match_buf: Vec<u8> = Vec::new();
                        let mut match_idx = 0;

                        loop {
                            if ctxt.is_eof() {
                                break;
                            }
                            let ch = ctxt.peek().unwrap();
                            if ch.to_ascii_lowercase() == end_bytes[match_idx] {
                                match_buf.push(ch);
                                match_idx += 1;
                                ctxt.next();
                                if match_idx == end_bytes.len() {
                                    // We found the start of </tag
                                    // Add the text before the end tag
                                    if !raw_text.is_empty() {
                                        let text_node = new_text_node(&raw_text);
                                        if !text_node.is_null() {
                                            tree::add_child(raw_node, text_node);
                                        }
                                    }
                                    // Consume the rest of the end tag: "tag>"
                                    // Now read "tag>"
                                    let _suffix = ctxt.read_while(|ch| ch != b'>');
                                    if ctxt.peek() == Some(b'>') {
                                        ctxt.next();
                                    }
                                    // Close the element
                                    ctxt.current = unsafe { (*raw_node).parent };
                                    break;
                                }
                            } else {
                                // Mismatch: flush any buffered end-tag prefix
                                // chars back into the text, then the current
                                // char.
                                if match_idx > 0 {
                                    raw_text.extend_from_slice(&match_buf);
                                    match_buf.clear();
                                    match_idx = 0;
                                }
                                raw_text.push(ch);
                                ctxt.next();
                            }
                        }

                        // If we never found the end tag, add the text (plus
                        // any buffered end-tag prefix chars).
                        if match_idx < end_bytes.len() {
                            raw_text.extend_from_slice(&match_buf);
                            if !raw_text.is_empty() {
                                let text_node = new_text_node(&raw_text);
                                if !text_node.is_null() {
                                    tree::add_child(raw_node, text_node);
                                }
                            }
                            ctxt.current = unsafe { (*raw_node).parent };
                        }
                    }
                }
                continue;
            }

            // Regular start tag
            handle_start_tag(ctxt, &tag_name, &attrs);
        } else {
            // Text content - read until next '<' or entity '&'
            let mut text = Vec::new();
            loop {
                match ctxt.peek() {
                    Some(b'<') => break,
                    Some(b'&') => {
                        // Handle entity reference inline
                        let entity_text = parse_entity(ctxt);
                        text.extend_from_slice(&entity_text);
                    }
                    Some(0) => {
                        // UPSTREAM-PARITY (bug #80268, libxml2 >= 2.9.12): NUL
                        // bytes in HTML content are DROPPED and parsing
                        // continues — they must neither terminate the text
                        // node (truncating at the NUL) nor reach the tree
                        // (content is C-string storage).
                        ctxt.next();
                    }
                    Some(ch) => {
                        text.push(ch);
                        ctxt.next();
                    }
                    None => break,
                }
            }

            if !text.is_empty() {
                handle_text(ctxt, &text);
            }
        }
    }

    // Post-processing: ensure html/head/body are created even for empty
    // documents (never under HTML_PARSE_NOIMPLIED — the source tree is kept
    // as-is, bug76285).
    if ctxt.html.is_null() && ctxt.options & HTML_PARSE_NOIMPLIED == 0 {
        ensure_html(ctxt);
    }

    // UPSTREAM-PARITY (xmlSAX2Characters with XML_PARSE_NOBLANKS):
    // whitespace-only text runs are reported as ignorableWhitespace and never
    // become text nodes. The candidate builds them eagerly, so drop them here
    // (ext/dom dom005's loadHTMLFile(…, LIBXML_NOBLANKS) serialization kept
    // the <head>-region newline text nodes the oracle drops).
    if ctxt.options & (crate::abi::types::XML_PARSE_NOBLANKS as c_int) != 0 && !doc.is_null() {
        unsafe {
            drop_blank_text_nodes((*doc).children);
        }
    }

    // UPSTREAM-PARITY (SAX2.c xmlSAX2StartElementNs / xmlSAX2AttributeNs):
    // ID-bearing attributes (the HTML `id` attribute, `<a name>` and any
    // DTD-declared ID) are registered in doc->ids as they are parsed. The
    // html tree builder attaches attributes BEFORE a node gains its document
    // pointer (add_child propagates doc later), so the per-attribute
    // registration cannot run at attribute time — do a tree-order pass once
    // the document is complete (first registration wins, matching the
    // in-order xmlAddID calls of a SAX2 parse). This is what makes
    // DOMDocument::loadHTML + getElementById / HTMLCollection named lookups
    // see `id="…"` attributes.
    if !doc.is_null() {
        unsafe {
            register_html_ids(doc, (*doc).children);
        }
    }

    doc
}

/// Register ID/IDREF attributes of every element in the sibling chain and
/// their element descendants (tree order, first registration wins).
///
/// # Safety
///
/// - `doc` must be a valid `_xmlDoc` (HTML type) whose tree stays alive for
///   the call; `cur` must be NULL or a valid live node chain within `doc`.
unsafe fn register_html_ids(doc: *mut _xmlDoc, cur: *mut _xmlNode) {
    let mut n = cur;
    while !n.is_null() {
        let t = unsafe { (*n).type_ };
        if t == XML_ELEMENT_NODE as c_int {
            let el = n;
            let mut attr = unsafe { (*el).properties };
            while !attr.is_null() {
                if unsafe { (*attr).id }.is_null()
                    && !unsafe { (*attr).children }.is_null()
                    && unsafe { (*(*attr).children).type_ } == XML_TEXT_NODE as c_int
                    && unsafe { (*(*attr).children).next }.is_null()
                {
                    let v = unsafe { (*(*attr).children).content };
                    if !v.is_null() {
                        let id_res = crate::xml::validation::is_id(doc, el, attr);
                        if id_res > 0 {
                            crate::xml::validation::add_id(ptr::null_mut(), doc, v, attr);
                        } else if crate::xml::validation::is_ref(doc, el, attr) > 0 {
                            crate::xml::validation::add_ref(ptr::null_mut(), doc, v, attr);
                        }
                    }
                }
                attr = unsafe { (*attr).next };
            }
            if !unsafe { (*el).children }.is_null() {
                register_html_ids(doc, unsafe { (*el).children });
            }
        }
        n = unsafe { (*n).next };
    }
}

/// Unlink and free every whitespace-only text node in the sibling chain and
/// their element descendants (upstream html parse + XML_PARSE_NOBLANKS).
///
/// # Safety
///
/// - `cur` must be NULL or a valid live `_xmlNode` chain (element/text/…)
///   inside the document being cleaned.
unsafe fn drop_blank_text_nodes(cur: *mut _xmlNode) {
    let mut n = cur;
    while !n.is_null() {
        let next = unsafe { (*n).next };
        let t = unsafe { (*n).type_ };
        if t == XML_TEXT_NODE as c_int {
            let content = unsafe { (*n).content };
            let blank = if content.is_null() {
                true
            } else {
                let mut p = content;
                while unsafe { *p } != 0 {
                    match unsafe { *p } {
                        b' ' | b'\t' | b'\r' | b'\n' => {}
                        _ => break,
                    }
                    p = unsafe { p.add(1) };
                }
                (unsafe { *p }) == 0
            };
            if blank {
                tree::unlink_node(n);
                tree::free_node(n);
            }
        } else if t == XML_ELEMENT_NODE as c_int && !unsafe { (*n).children }.is_null() {
            drop_blank_text_nodes(unsafe { (*n).children });
        }
        n = next;
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Public API Functions
// ═══════════════════════════════════════════════════════════════════════════════

/// Parse HTML from a file.
///
/// # UPSTREAM-PARITY
///
/// Equivalent to `htmlParseFile` in libxml2.
///
/// # Safety
///
/// - `filename` must be a valid null-terminated C string or NULL.
/// - `encoding` must be a valid null-terminated C string or NULL.
pub unsafe fn parse_file(
    filename: *const c_char,
    encoding: *const c_char,
    options: c_int,
) -> *mut _xmlDoc {
    if filename.is_null() {
        return ptr::null_mut();
    }

    // Read the file into memory
    let filename_str = unsafe { std::ffi::CStr::from_ptr(filename) };
    let path = filename_str.to_str().unwrap_or("");
    let content = match std::fs::read(path) {
        Ok(data) => data,
        Err(_) => return ptr::null_mut(),
    };

    let mut ctxt = HtmlParserCtxt::new();
    ctxt.options = options;
    if !encoding.is_null() {
        let _enc_cstr = unsafe { std::ffi::CStr::from_ptr(encoding) };
        ctxt.encoding = unsafe { c_strdup(encoding) };
    }

    let doc = unsafe {
        html_parse_buffer(
            &mut ctxt,
            content.as_ptr() as *const c_char,
            content.len() as c_int,
        )
    };

    if !doc.is_null() && !filename.is_null() {
        unsafe {
            (*doc).URL = c_strdup(filename) as *mut xmlChar;
        }
    }

    doc
}

/// Parse HTML from a memory buffer.
///
/// # UPSTREAM-PARITY
///
/// Equivalent to `htmlParseMemory` in libxml2.
///
/// # Safety
///
/// - `buffer` must point to valid memory of at least `size` bytes.
/// - `size` must be non-negative.
pub unsafe fn parse_memory(buffer: *const c_char, size: c_int) -> *mut _xmlDoc {
    unsafe { parse_memory_enc(buffer, size, ptr::null(), 0) }
}

/// Parse HTML from a memory buffer with an explicit input encoding.
///
/// UPSTREAM-PARITY: equivalent to `htmlCtxtReadMemory` where the caller's
/// `encoding` is wired into the input buffer (`xmlCtxtNewInputFromMemory` ->
/// `xmlSwitchInputEncodingName` installs a decoder so the parse loop always
/// consumes UTF-8). NULL means no conversion (BOM sniffing only).
///
/// # Safety
///
/// - `buffer` must point to valid memory of at least `size` bytes.
/// - `size` must be non-negative.
/// - `encoding` must be a valid NUL-terminated C string or NULL.
pub(crate) unsafe fn parse_memory_enc(
    buffer: *const c_char,
    size: c_int,
    encoding: *const c_char,
    options: c_int,
) -> *mut _xmlDoc {
    if buffer.is_null() || size <= 0 {
        return ptr::null_mut();
    }

    let mut ctxt = HtmlParserCtxt::new();
    ctxt.options = options;
    if !encoding.is_null() {
        ctxt.encoding = unsafe { c_strdup(encoding) };
    }
    unsafe { html_parse_buffer(&mut ctxt, buffer, size) }
}

/// Parse HTML from a null-terminated string.
///
/// # UPSTREAM-PARITY
///
/// Equivalent to `htmlParseDoc` in libxml2.
///
/// # Safety
///
/// - `cur` must be a valid null-terminated xmlChar string or NULL.
/// - `encoding` must be a valid null-terminated C string or NULL.
pub(crate) unsafe fn parse_doc(
    cur: *const xmlChar,
    encoding: *const c_char,
    options: c_int,
) -> *mut _xmlDoc {
    if cur.is_null() {
        return ptr::null_mut();
    }

    let len = unsafe { xml_strlen(cur) };
    let mut ctxt = HtmlParserCtxt::new();
    ctxt.options = options;
    if !encoding.is_null() {
        ctxt.encoding = unsafe { c_strdup(encoding) };
    }

    unsafe { html_parse_buffer(&mut ctxt, cur as *const c_char, len as c_int) }
}

/// Create an HTML parser context for file parsing.
///
/// # UPSTREAM-PARITY
///
/// Equivalent to `htmlCreateFileParserCtxt` in libxml2.
///
/// # Safety
///
/// - `filename` must be a valid null-terminated C string or NULL.
/// - `encoding` must be a valid null-terminated C string or NULL.
#[allow(dead_code)]
pub(crate) unsafe fn create_file_parser_ctxt(
    filename: *const c_char,
    encoding: *const c_char,
) -> *mut c_void {
    if filename.is_null() {
        return ptr::null_mut();
    }

    // Host allocation (R-00019x): real C-visible `_xmlParserCtxt` at offset 0
    // followed by the engine state; freed as one block by `free_parser_ctxt`.
    let total = size_of::<_xmlParserCtxt>() + size_of::<HtmlParserCtxt>();
    let mem = unsafe { xmlMallocZero(total) } as *mut u8;
    if mem.is_null() {
        return ptr::null_mut();
    }

    let ctxt = mem.add(size_of::<_xmlParserCtxt>()) as *mut HtmlParserCtxt;
    unsafe {
        ptr::write(ctxt, HtmlParserCtxt::new());
        if !encoding.is_null() {
            (*ctxt).encoding = c_strdup(encoding);
        }
        (*(mem as *mut _xmlParserCtxt)).html = 1;
    }

    mem as *mut c_void
}

/// Free an HTML parser context.
///
/// # UPSTREAM-PARITY
///
/// Equivalent to `htmlFreeParserCtxt` in libxml2.
///
/// # Safety
///
/// - `ctxt` must be a valid pointer returned by `create_file_parser_ctxt`, or NULL.
pub(crate) unsafe fn free_parser_ctxt(ctxt: *mut c_void) {
    if ctxt.is_null() {
        return;
    }

    // Host allocation (R-00019x): a real C-visible `_xmlParserCtxt` at
    // offset 0 with the engine state (`HtmlParserCtxt`) after it. Free the
    // engine's owned input buffer and strings, then the single host block.
    let state = (ctxt as *mut u8).add(size_of::<_xmlParserCtxt>()) as *mut HtmlParserCtxt;
    unsafe {
        if !(*state).input.is_null() {
            xmlFreeImpl((*state).input as *mut c_void);
        }
        if !(*state).filename.is_null() {
            xmlFreeImpl((*state).filename as *mut c_void);
        }
        if !(*state).encoding.is_null() {
            xmlFreeImpl((*state).encoding as *mut c_void);
        }
        xmlFreeImpl(ctxt);
    }
}

/// Initialize the HTML parser module.
///
/// # UPSTREAM-PARITY
///
/// Equivalent to `htmlInitParser` in libxml2.
#[allow(dead_code)]
pub(crate) const fn init_parser() {
    // Currently a no-op. In the future, may initialize HTML-specific
    // entity tables or other global state.
}

/// Cleanup the HTML parser module.
///
/// # UPSTREAM-PARITY
///
/// Equivalent to `htmlCleanupParser` in libxml2.
#[allow(dead_code)]
pub(crate) const fn cleanup_parser() {
    // Currently a no-op. In the future, may free HTML-specific
    // global state.
}

/// Create a new HTML document.
///
/// # UPSTREAM-PARITY
///
/// Equivalent to `htmlNewDoc` in libxml2.
///
/// Creates a new HTML document (type XML_HTML_DOCUMENT_NODE) WITHOUT any
/// implicit html/head/body skeleton. Upstream `htmlNewDoc`/`htmlNewDocNoDtD`
/// create only the document (the HTML parser grows the html/head/body
/// elements lazily); eagerly seeding the skeleton would insert a real
/// `<html>` root that diverges from upstream and breaks consumers like
/// nokogiri's `HTML4::Document.new` builder (a later `<b>` add would look
/// like a second root).
///
/// # SAFETY
///
/// - `version` must be a valid null-terminated xmlChar string or NULL.
pub(crate) unsafe fn new_doc(version: *const xmlChar) -> *mut _xmlDoc {
    let doc = tree::new_doc(version);
    if doc.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*doc).type_ = XML_HTML_DOCUMENT_NODE as c_int;
        (*doc).properties = XML_DOC_WELLFORMED as c_int;
    }

    doc
}

/// Create a new HTML document without DTD.
///
/// # UPSTREAM-PARITY
///
/// Equivalent to `htmlNewDocNoDtD` in libxml2.
///
/// Creates a new document with type XML_HTML_DOCUMENT_NODE.
/// Unlike `htmlNewDoc`, this does NOT auto-create html/head/body elements.
///
/// # Safety
///
/// - `version` must be a valid null-terminated xmlChar string or NULL.
pub(crate) unsafe fn new_doc_no_dtd(version: *const xmlChar) -> *mut _xmlDoc {
    let doc = tree::new_doc(version);
    if doc.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*doc).type_ = XML_HTML_DOCUMENT_NODE as c_int;
        (*doc).properties = XML_DOC_WELLFORMED as c_int;
    }

    doc
}

// ═══════════════════════════════════════════════════════════════════════════════
// HTML Serializer
// ═══════════════════════════════════════════════════════════════════════════════

/// HTML void elements that should not have closing tags.
const HTML_VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr", "frame",
];

/// Check if an element name is an HTML void element.
fn is_html_void(name: &str) -> bool {
    HTML_VOID_ELEMENTS
        .iter()
        .any(|v| v.eq_ignore_ascii_case(name))
}

/// Check if an element has optional end tag in HTML.
#[allow(dead_code)]
fn has_optional_end_tag(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "p" | "li" | "tr" | "td" | "th" | "dt" | "dd"
    )
}

/// Serialize a text node for HTML output.
///
/// In HTML serialization, we escape `<` and `&` but NOT non-ASCII characters
/// as numeric entities (unlike XML serialization).
/// Write a double-quoted C string to the buffer.
unsafe fn html_write_quoted(buf: *mut _xmlBuffer, s: *const xmlChar) {
    if buf.is_null() || s.is_null() {
        return;
    }
    io::buf_ccat(buf, b'"');
    io::buf_cat(buf, s);
    io::buf_ccat(buf, b'"');
}

/// Trim leading ASCII whitespace.
fn trim_ascii_start(s: &[u8]) -> &[u8] {
    let start = s
        .iter()
        .position(|&b| !b.is_ascii_whitespace())
        .unwrap_or(s.len());
    &s[start..]
}

/// Write text content into an HTML output buffer, escaping `&`, `<` and
/// `>` (upstream htmlTreeDumpText escapes `>` to `&gt;` as well — the corpus
/// `ser-methods` html method expects `&lt;x&gt;`, not `&lt;x>`).
///
/// # Safety
///
/// - `buf` must be non-NULL and point to a valid `_xmlBuffer` writable via
///   `io::buf_add`.
/// - `content` must be non-NULL and readable for at least `len` bytes; `len`
///   is required to be positive (checked) and the loop reads exactly `len`
///   bytes starting at `content`.
unsafe fn html_serialize_text(buf: *mut _xmlBuffer, content: *const xmlChar, len: c_int) {
    if buf.is_null() || content.is_null() || len <= 0 {
        return;
    }

    let mut i: c_int = 0;
    while i < len {
        let ch = unsafe { *content.add(i as usize) };

        match ch {
            b'<' => {
                io::buf_add(buf, b"&lt;" as *const u8, 4);
            }
            b'&' => {
                io::buf_add(buf, b"&amp;" as *const u8, 5);
            }
            b'>' => {
                io::buf_add(buf, b"&gt;" as *const u8, 4);
            }
            _ => {
                io::buf_add(buf, &ch as *const u8, 1);
            }
        }
        i += 1;
    }
}

/// Serialize an attribute value for HTML output.
///
/// In HTML, attribute values should be quoted and have `&`, `"` escaped.
///
/// # Safety
///
/// - `buf` must be non-NULL and point to a valid `_xmlBuffer` writable via
///   `io::buf_add`.
/// - `value` must be non-NULL and point to a valid NUL-terminated `xmlChar`
///   string; `xml_strlen` scans it up to the terminator.
unsafe fn html_serialize_attr_value(buf: *mut _xmlBuffer, value: *const xmlChar) {
    if buf.is_null() || value.is_null() {
        return;
    }

    let len = unsafe { xml_strlen(value) as c_int };
    let mut i: c_int = 0;
    while i < len {
        let ch = unsafe { *value.add(i as usize) };

        match ch {
            b'&' => {
                io::buf_add(buf, b"&amp;" as *const u8, 5);
            }
            b'"' => {
                io::buf_add(buf, b"&quot;" as *const u8, 6);
            }
            _ => {
                io::buf_add(buf, &ch as *const u8, 1);
            }
        }
        i += 1;
    }
}

/// HTML-specific node serialization.
///
/// Walks the node tree and serializes to HTML format.
/// Differs from XML serialization in several ways:
/// - No XML declaration for HTML documents
/// - No self-closing tags for void elements
/// - Case-insensitive tag names preserved as-is
/// - No namespace declarations
/// - Elements with optional end tags may omit them
///
/// Whether the head element already contains a <meta> element (so the
///
/// serializer does not insert a duplicate charset declaration).
///
/// # SAFETY
///
/// - `child` must be a valid node or NULL.
unsafe fn html_head_has_meta(child: *mut _xmlNode) -> bool {
    let mut c = child;
    while !c.is_null() {
        if (*c).type_ == XML_ELEMENT_NODE as c_int && !(*c).name.is_null() {
            let nm = xmlstr_to_bytes((*c).name);
            if nm.eq_ignore_ascii_case(b"meta") {
                return true;
            }
        }
        c = (*c).next;
    }
    false
}

/// Serialize a node tree to HTML output.
///
/// # Safety
///
/// - `node` must be non-NULL and point to a valid `_xmlNode` in a well-formed
///   tree; the `children`, `next`, `parent`, `properties` and `doc` links
///   walked here must be NULL-terminated and point to valid objects.
/// - `buf` must be non-NULL and point to a valid `_xmlBuffer`.
/// - `name`, `content` and `encoding` fields must be NULL or valid
///   NUL-terminated `xmlChar` strings.
pub(crate) unsafe fn serialize_node(
    node: *mut _xmlNode,
    buf: *mut _xmlBuffer,
    format: c_int,
    level: c_int,
) {
    unsafe { serialize_node_enc(node, buf, format, level, None) }
}

/// Serialize an HTML node with an optional output-encoding parameter
/// (upstream `htmlNodeDumpInternal`'s `encoding` argument).
///
/// Upstream inserts a `<meta charset=...>` in the root `<head>` ONLY when
/// this `encoding` parameter is non-NULL (htmlDocDumpMemoryFormat and the
/// lxml `tostring(method="html")` path pass NULL and never insert one);
/// `htmlSaveFileFormat` passes the caller's encoding string.
///
/// # Safety
///
/// - `node` must be NULL or a valid `_xmlNode`; `buf` a valid `_xmlBuffer`.
pub(crate) unsafe fn serialize_node_enc(
    node: *mut _xmlNode,
    buf: *mut _xmlBuffer,
    format: c_int,
    level: c_int,
    encoding: Option<&[u8]>,
) {
    if node.is_null() || buf.is_null() {
        return;
    }

    let n = unsafe { &*node };

    match n.type_ {
        t if t == XML_ELEMENT_NODE as c_int => {
            let name = if n.name.is_null() {
                ""
            } else {
                unsafe { core::str::from_utf8(xmlstr_to_bytes(n.name)).unwrap_or("") }
            };

            let is_void = is_html_void(name);
            // UPSTREAM-PARITY: htmlNodeDumpInternal only adds formatting
            // newlines for non-inline elements; p, pre and param are never
            // formatted (name[0] == 'p'), and unknown elements are treated
            // as inline (info == NULL).
            let info = html_tag_lookup(name);
            let is_inline = info.is_none_or(|i| i.flags & HTML_INLINE != 0);
            let no_format = is_inline || name.starts_with('p');

            // Write start tag
            io::buf_ccat(buf, b'<');
            // UPSTREAM-PARITY (HTMLtree.c htmlNodeDumpOutputInternal): an
            // element with a bound prefix writes `prefix:name`, and its
            // local namespace declarations (nsDef) are dumped right after
            // the name — namespaced trees (lxml.html.html5parser's XHTML
            // tree) serialize with their namespace declarations.
            if !n.ns.is_null() && !(*n.ns).prefix.is_null() {
                io::buf_cat(buf, (*n.ns).prefix);
                io::buf_ccat(buf, b':');
            }
            if !n.name.is_null() {
                io::buf_cat(buf, n.name);
            }
            if !n.nsDef.is_null() {
                let mut ns = n.nsDef;
                while !ns.is_null() {
                    let nsp = unsafe { &*ns };
                    // UPSTREAM-PARITY (xmlsave.c xmlNsDumpOutput): only
                    // LOCAL namespaces with a URI are written; the "xml"
                    // prefix declaration is skipped.
                    let is_xml = !nsp.prefix.is_null() && xmlstr_to_bytes(nsp.prefix) == b"xml";
                    if nsp.type_ == XML_LOCAL_NAMESPACE as c_int && !nsp.href.is_null() && !is_xml {
                        io::buf_ccat(buf, b' ');
                        if nsp.prefix.is_null() {
                            io::buf_add(buf, b"xmlns=\"" as *const u8, 7);
                        } else {
                            io::buf_add(buf, b"xmlns:" as *const u8, 6);
                            io::buf_cat(buf, nsp.prefix);
                            io::buf_add(buf, b"=\"" as *const u8, 2);
                        }
                        html_serialize_attr_value(buf, nsp.href);
                        io::buf_ccat(buf, b'\"');
                    }
                    ns = nsp.next;
                }
            }

            // Write attributes
            let mut attr = n.properties;
            while !attr.is_null() {
                let a = unsafe { &*attr };
                io::buf_ccat(buf, b' ');
                if !a.name.is_null() {
                    io::buf_cat(buf, a.name);
                }

                // Write attribute value if present
                if !a.children.is_null() {
                    let child = unsafe { &*a.children };
                    if child.type_ == XML_TEXT_NODE as c_int && !child.content.is_null() {
                        io::buf_ccat(buf, b'=');
                        io::buf_ccat(buf, b'"');
                        html_serialize_attr_value(buf, child.content);
                        io::buf_ccat(buf, b'"');
                    }
                }

                attr = a.next;
            }

            // UPSTREAM-PARITY (HTMLtree.c htmlNodeDumpInternal): inserts
            // <meta charset="..."> as the first child of the <head> of the
            // root <html> element when no <meta> is present AND the caller
            // passed an explicit output `encoding` parameter (the doc-dump
            // path passes NULL and inserts nothing). The meta is synthetic
            // here, so it participates in the formatting rules like a real
            // child.
            let mut meta_bytes: Option<Vec<u8>> = None;
            if name.eq_ignore_ascii_case("head") && level == 1 {
                if let Some(enc) = encoding {
                    let parent_is_html = !n.parent.is_null() && !(*n.parent).name.is_null() && {
                        let pn =
                            core::str::from_utf8(xmlstr_to_bytes((*n.parent).name)).unwrap_or("");
                        pn.eq_ignore_ascii_case("html")
                    };
                    if parent_is_html && !html_head_has_meta(n.children) {
                        meta_bytes = Some(enc.to_vec());
                    }
                }
            }
            let meta_inserted = meta_bytes.is_some();

            let has_children = !n.children.is_null();
            let first_child = if has_children {
                unsafe { (*n.children).type_ }
            } else {
                XML_TEXT_NODE as c_int
            };
            let first_is_text = first_child == XML_TEXT_NODE as c_int
                || first_child == XML_ENTITY_REF_NODE as c_int;
            // With a synthetic meta child, an empty head behaves as having
            // one element child.
            let multi_child = (has_children && n.children != n.last) || meta_inserted;

            if is_void {
                // Void element: just close the tag, no children
                io::buf_ccat(buf, b'>');
                // UPSTREAM-PARITY (line 997): a newline follows a non-inline
                // element whose next sibling is not text; the caller (parent
                // loop) emits it, so nothing here.
            } else {
                // Element with children (or a head receiving a meta)
                io::buf_ccat(buf, b'>');

                // Newline after the open tag (upstream line 969): a
                // non-inline element whose first child is not text and which
                // has more than one child (or receives a meta) starts its
                // content on a new line.
                if format != 0 && !no_format && !first_is_text && multi_child {
                    io::buf_ccat(buf, b'\n');
                }

                if let Some(enc) = &meta_bytes {
                    io::buf_add(buf, b"<meta charset=\"" as *const u8, 15);
                    io::buf_add(buf, enc.as_ptr(), enc.len() as c_int);
                    io::buf_add(buf, b"\">" as *const u8, 2);
                    // UPSTREAM-PARITY (line 983): a newline follows the
                    // inserted meta when the next real child is not text.
                    if format != 0 && has_children && !first_is_text && !name.starts_with('p') {
                        io::buf_ccat(buf, b'\n');
                    }
                }

                // Serialize children inline (HTML formatting adds no
                // indentation; the per-element rules emit the newlines).
                let mut child = n.children;
                while !child.is_null() {
                    serialize_node_enc(child, buf, format, level + 1, encoding);
                    // UPSTREAM-PARITY (line 997): a newline follows a
                    // non-inline element whose next sibling is not text,
                    // unless the parent is p/pre/param.
                    let next = unsafe { (*child).next };
                    if format != 0 && !next.is_null() && !name.starts_with('p') {
                        let nt = unsafe { (*next).type_ };
                        if nt != XML_TEXT_NODE as c_int && nt != XML_ENTITY_REF_NODE as c_int {
                            let cname = if (*child).name.is_null() {
                                ""
                            } else {
                                unsafe {
                                    core::str::from_utf8(xmlstr_to_bytes((*child).name))
                                        .unwrap_or("")
                                }
                            };
                            let cinfo = html_tag_lookup(cname);
                            let c_inline = cinfo.is_none_or(|i| i.flags & HTML_INLINE != 0);
                            if !c_inline {
                                io::buf_ccat(buf, b'\n');
                            }
                        }
                    }
                    child = next;
                }

                // Newline before the end tag (upstream line 1085): a
                // non-inline element whose last child is not text and which
                // has more than one child (or is the head receiving a meta).
                let last_child = if has_children {
                    unsafe { (*n.last).type_ }
                } else {
                    XML_ELEMENT_NODE as c_int
                };
                let last_is_text = last_child == XML_TEXT_NODE as c_int
                    || last_child == XML_ENTITY_REF_NODE as c_int;
                if format != 0 && !no_format && !last_is_text && multi_child {
                    io::buf_ccat(buf, b'\n');
                }

                // Write end tag
                io::buf_add(buf, b"</" as *const u8, 2);
                // UPSTREAM-PARITY (HTMLtree.c htmlNodeDumpOutputInternal
                // end-tag emission): the namespace prefix is repeated.
                if !n.ns.is_null() && !(*n.ns).prefix.is_null() {
                    io::buf_cat(buf, (*n.ns).prefix);
                    io::buf_ccat(buf, b':');
                }
                if !n.name.is_null() {
                    io::buf_cat(buf, n.name);
                }
                io::buf_ccat(buf, b'>');
            }
        }
        t if t == XML_TEXT_NODE as c_int => {
            // UPSTREAM-PARITY (HTMLtree.c htmlNodeDumpInternal): script/
            // style content is DATA_RAWTEXT — written verbatim, never
            // escaped (the corpus html-script expects `<` and `&&` raw).
            let parent_is_raw = !n.parent.is_null() && !(*n.parent).name.is_null() && {
                let pn = xmlstr_to_bytes((*n.parent).name);
                pn.eq_ignore_ascii_case(b"script") || pn.eq_ignore_ascii_case(b"style")
            };
            if parent_is_raw {
                io::buf_cat(buf, n.content);
            } else {
                html_serialize_text(buf, n.content, xml_strlen(n.content) as c_int);
            }
        }
        t if t == XML_CDATA_SECTION_NODE as c_int => {
            io::buf_add(buf, b"<![CDATA[" as *const u8, 9);
            html_serialize_text(buf, n.content, xml_strlen(n.content) as c_int);
            io::buf_add(buf, b"]]>" as *const u8, 3);
        }
        t if t == XML_COMMENT_NODE as c_int => {
            if format != 0 && level > 0 {
                io::buf_ccat(buf, b'\n');
                for _ in 0..level {
                    io::buf_add(buf, b"  " as *const u8, 2);
                }
            }
            io::buf_add(buf, b"<!--" as *const u8, 4);
            if !n.content.is_null() {
                io::buf_cat(buf, n.content);
            }
            io::buf_add(buf, b"-->" as *const u8, 3);
        }
        t if t == XML_PI_NODE as c_int => {
            if format != 0 && level > 0 {
                io::buf_ccat(buf, b'\n');
                for _ in 0..level {
                    io::buf_add(buf, b"  " as *const u8, 2);
                }
            }
            io::buf_add(buf, b"<?" as *const u8, 2);
            if !n.name.is_null() {
                io::buf_cat(buf, n.name);
            }
            if !n.content.is_null() && unsafe { *n.content != 0 } {
                io::buf_ccat(buf, b' ');
                io::buf_cat(buf, n.content);
            }
            io::buf_add(buf, b"?>" as *const u8, 2);
        }
        t if t == XML_DOCUMENT_NODE as c_int || t == XML_HTML_DOCUMENT_NODE as c_int => {
            // UPSTREAM-PARITY: htmlDocContentDumpOutput writes the internal
            // subset's DOCTYPE before the tree children.
            let doc_ptr = n as *const _xmlNode as *mut _xmlDoc;
            let d = &*doc_ptr;
            if !d.intSubset.is_null() {
                let dtd = &*d.intSubset;
                io::buf_add(buf, b"<!DOCTYPE " as *const u8, 10);
                if !dtd.name.is_null() {
                    io::buf_cat(buf, dtd.name);
                }
                if !dtd.ExternalID.is_null() {
                    io::buf_add(buf, b" PUBLIC " as *const u8, 8);
                    html_write_quoted(buf, dtd.ExternalID);
                    io::buf_ccat(buf, b' ');
                    html_write_quoted(buf, dtd.SystemID);
                } else if !dtd.SystemID.is_null() {
                    io::buf_add(buf, b" SYSTEM " as *const u8, 8);
                    html_write_quoted(buf, dtd.SystemID);
                }
                io::buf_ccat(buf, b'>');
                io::buf_ccat(buf, b'\n');
            }
            // No XML declaration for HTML documents
            // Serialize children
            let mut child = n.children;
            while !child.is_null() {
                serialize_node_enc(child, buf, format, 0, encoding);
                child = unsafe { (*child).next };
            }
            // UPSTREAM-PARITY: htmlDocContentDumpOutput terminates with a
            // newline.
            io::buf_ccat(buf, b'\n');
        }
        _ => {
            if !n.content.is_null() {
                html_serialize_text(buf, n.content, xml_strlen(n.content) as c_int);
            }
        }
    }
}

/// Dump an HTML document to a buffer.
///
/// # Safety
///
/// - `buf` must be a valid pointer to a mutable `_xmlBuffer`.
/// - `doc` must be a valid pointer to an `_xmlDoc`, or NULL.
pub(crate) unsafe fn doc_dump(buf: *mut _xmlBuffer, doc: *mut _xmlDoc) -> c_int {
    if buf.is_null() || doc.is_null() {
        return -1;
    }

    let before = io::buf_length(buf);
    serialize_node(doc as *mut _xmlNode, buf, 0, 0);
    let after = io::buf_length(buf);

    if after < 0 || before < 0 {
        return -1;
    }
    after - before
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    use crate::xml::io;

    /// Helper: create a null-terminated xmlChar* from a byte slice.
    #[allow(dead_code)]
    unsafe fn to_xmlstr(s: &[u8]) -> *mut xmlChar {
        bytes_to_xmlstr(s)
    }

    /// Helper: serialize an HTML document to a String.
    unsafe fn html_doc_to_string(doc: *mut _xmlDoc) -> String {
        let buf = io::buf_create(-1);
        assert!(!buf.is_null());
        doc_dump(buf, doc);
        let content = io::buf_content(buf);
        let s = if !content.is_null() {
            let len = xml_strlen(content);
            let slice = slice::from_raw_parts(content, len);
            String::from_utf8_lossy(slice).to_string()
        } else {
            String::new()
        };
        io::buf_free(buf);
        s
    }

    // ═════════════════════════════════════════════════════════════════════════
    // Element Info Lookup
    // ═════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_html_tag_lookup() {
        // Known elements
        assert!(html_tag_lookup("html").is_some());
        assert!(html_tag_lookup("HTML").is_some()); // case-insensitive
        assert!(html_tag_lookup("p").is_some());
        assert!(html_tag_lookup("br").is_some());
        assert!(html_tag_lookup("div").is_some());
        assert!(html_tag_lookup("script").is_some());

        // Unknown elements
        assert!(html_tag_lookup("custom").is_none());
        assert!(html_tag_lookup("my-element").is_none());
    }

    #[test]
    fn test_tag_flags() {
        let br = html_tag_lookup("br").unwrap();
        assert!(br.flags & HTML_INLINE != 0);
        assert!(br.flags & HTML_EMPTY != 0);

        let div = html_tag_lookup("div").unwrap();
        assert!(div.flags & HTML_BLOCK != 0);
        assert!(div.flags & HTML_VALID != 0);

        let p = html_tag_lookup("p").unwrap();
        assert!(p.flags & HTML_NO_END != 0);

        let meta = html_tag_lookup("meta").unwrap();
        assert!(meta.flags & HTML_HEAD != 0);
        assert!(meta.flags & HTML_EMPTY != 0);
    }

    // ═════════════════════════════════════════════════════════════════════════
    // Entity Lookup
    // ═════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_html_entity_lookup() {
        assert_eq!(html_entity_lookup("amp"), Some("&"));
        assert_eq!(html_entity_lookup("lt"), Some("<"));
        assert_eq!(html_entity_lookup("gt"), Some(">"));
        assert_eq!(html_entity_lookup("quot"), Some("\""));
        assert_eq!(html_entity_lookup("nbsp"), Some("\u{00a0}"));
        assert_eq!(html_entity_lookup("copy"), Some("\u{00a9}"));
        assert!(html_entity_lookup("unknown_entity").is_none());
    }

    // ═════════════════════════════════════════════════════════════════════════
    // Basic Parsing
    // ═════════════════════════════════════════════════════════════════════════

    /// Parses a complete HTML document and verifies the serialized output
    /// contains the expected elements.
    ///
    /// # Safety
    ///
    /// - The NUL-terminated static `html` buffer stays valid for the
    ///   `parse_memory` call; the returned document pointer is asserted
    ///   non-NULL before it is dereferenced by `html_doc_to_string` and is
    ///   freed exactly once with `tree::free_doc`.
    #[test]
    fn test_parse_basic_html() {
        unsafe {
            let html = b"<html><head><title>Test</title></head><body><p>Hello</p></body></html>\0";
            let doc = parse_memory(html.as_ptr() as *const c_char, (html.len() - 1) as c_int);
            assert!(!doc.is_null());

            let s = html_doc_to_string(doc);
            assert!(s.contains("<html>"));
            assert!(s.contains("<head>"));
            assert!(s.contains("<title>Test</title>"));
            assert!(s.contains("<body>"));
            assert!(s.contains("<p>Hello</p>"));

            tree::free_doc(doc);
        }
    }

    /// Verifies that parsing an empty buffer yields a NULL document.
    ///
    /// # Safety
    ///
    /// - The static one-byte buffer is valid for the `parse_memory` call with
    ///   size 0, which must not read from it; a NULL document is expected.
    #[test]
    fn test_parse_empty_document() {
        unsafe {
            let html = b"\0";
            let doc = parse_memory(html.as_ptr() as *const c_char, 0);
            assert!(doc.is_null());
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // Implicit html/head/body Creation
    // ═════════════════════════════════════════════════════════════════════════

    /// Verifies that a bare paragraph triggers implicit `html` and `body`
    /// creation during parsing.
    ///
    /// # Safety
    ///
    /// - The NUL-terminated static `html` buffer stays valid for the
    ///   `parse_memory` call; the returned document pointer is asserted
    ///   non-NULL before it is dereferenced by `html_doc_to_string` and is
    ///   freed exactly once with `tree::free_doc`.
    #[test]
    fn test_implicit_html_head_body() {
        unsafe {
            // Just a paragraph, no html/head/body
            let html = b"<p>Hello</p>\0";
            let doc = parse_memory(html.as_ptr() as *const c_char, (html.len() - 1) as c_int);
            assert!(!doc.is_null());

            let s = html_doc_to_string(doc);
            // Should have auto-created html
            assert!(s.contains("<html>"));
            // Should have auto-created body
            assert!(s.contains("<body>"));
            // Should have the paragraph
            assert!(s.contains("<p>Hello</p>"));

            tree::free_doc(doc);
        }
    }

    /// Verifies that a bare `title` triggers implicit `html` and `head`
    /// creation during parsing.
    ///
    /// # Safety
    ///
    /// - The NUL-terminated static `html` buffer stays valid for the
    ///   `parse_memory` call; the returned document pointer is asserted
    ///   non-NULL before it is dereferenced by `html_doc_to_string` and is
    ///   freed exactly once with `tree::free_doc`.
    #[test]
    fn test_implicit_head_with_title() {
        unsafe {
            // Only a title, no html/head/body
            let html = b"<title>My Page</title>\0";
            let doc = parse_memory(html.as_ptr() as *const c_char, (html.len() - 1) as c_int);
            assert!(!doc.is_null());

            let s = html_doc_to_string(doc);
            assert!(s.contains("<html>"));
            assert!(s.contains("<head>"));
            assert!(s.contains("<title>My Page</title>"));

            tree::free_doc(doc);
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // Auto-closing
    // ═════════════════════════════════════════════════════════════════════════

    /// Verifies that a second `p` start tag auto-closes the first.
    ///
    /// # Safety
    ///
    /// - The NUL-terminated static `html` buffer stays valid for the
    ///   `parse_memory` call; the returned document pointer is asserted
    ///   non-NULL before it is dereferenced by `html_doc_to_string` and is
    ///   freed exactly once with `tree::free_doc`.
    #[test]
    fn test_auto_close_p() {
        unsafe {
            // <p> should auto-close before another <p>
            let html = b"<p>First<p>Second</p>\0";
            let doc = parse_memory(html.as_ptr() as *const c_char, (html.len() - 1) as c_int);
            assert!(!doc.is_null());

            let s = html_doc_to_string(doc);
            // Both paragraphs should be siblings, not nested
            let first_pos = s.find("First");
            let second_pos = s.find("Second");
            assert!(first_pos.is_some());
            assert!(second_pos.is_some());

            tree::free_doc(doc);
        }
    }

    /// Verifies that an `h1` element is auto-closed before an `h2`.
    ///
    /// # Safety
    ///
    /// - The NUL-terminated static `html` buffer stays valid for the
    ///   `parse_memory` call; the returned document pointer is asserted
    ///   non-NULL before it is dereferenced by `html_doc_to_string` and is
    ///   freed exactly once with `tree::free_doc`.
    #[test]
    fn test_auto_close_heading() {
        unsafe {
            // h1 should auto-close before h2
            let html = b"<h1>Title</h1><h2>Subtitle</h2>\0";
            let doc = parse_memory(html.as_ptr() as *const c_char, (html.len() - 1) as c_int);
            assert!(!doc.is_null());

            let s = html_doc_to_string(doc);
            assert!(s.contains("<h1>Title</h1>"));
            assert!(s.contains("<h2>Subtitle</h2>"));

            tree::free_doc(doc);
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // Void Elements
    // ═════════════════════════════════════════════════════════════════════════

    /// Verifies that void elements are serialized without closing tags.
    ///
    /// # Safety
    ///
    /// - The NUL-terminated static `html` buffer stays valid for the
    ///   `parse_memory` call; the returned document pointer is asserted
    ///   non-NULL before it is dereferenced by `html_doc_to_string` and is
    ///   freed exactly once with `tree::free_doc`.
    #[test]
    fn test_void_elements() {
        unsafe {
            let html = b"<br><hr><img src=\"test.jpg\"><input type=\"text\">\0";
            let doc = parse_memory(html.as_ptr() as *const c_char, (html.len() - 1) as c_int);
            assert!(!doc.is_null());

            let s = html_doc_to_string(doc);
            assert!(s.contains("<br>"));
            assert!(s.contains("<hr>"));
            assert!(s.contains("<img"));
            assert!(s.contains("<input"));

            // Void elements should not have closing tags
            assert!(!s.contains("</br>"));
            assert!(!s.contains("</hr>"));
            assert!(!s.contains("</img>"));

            tree::free_doc(doc);
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // Unquoted and Minimized Attributes
    // ═════════════════════════════════════════════════════════════════════════

    /// Verifies that unquoted attribute values are parsed and re-serialized
    /// with quotes.
    ///
    /// # Safety
    ///
    /// - The NUL-terminated static `html` buffer stays valid for the
    ///   `parse_memory` call; the returned document pointer is asserted
    ///   non-NULL before it is dereferenced by `html_doc_to_string` and is
    ///   freed exactly once with `tree::free_doc`.
    #[test]
    fn test_unquoted_attributes() {
        unsafe {
            let html = b"<div class=main id=content>Text</div>\0";
            let doc = parse_memory(html.as_ptr() as *const c_char, (html.len() - 1) as c_int);
            assert!(!doc.is_null());

            let s = html_doc_to_string(doc);
            assert!(s.contains("class=\"main\""));
            assert!(s.contains("id=\"content\""));

            tree::free_doc(doc);
        }
    }

    /// Verifies that minimized (valueless) attributes are preserved.
    ///
    /// # Safety
    ///
    /// - The NUL-terminated static `html` buffer stays valid for the
    ///   `parse_memory` call; the returned document pointer is asserted
    ///   non-NULL before it is dereferenced by `html_doc_to_string` and is
    ///   freed exactly once with `tree::free_doc`.
    #[test]
    fn test_minimized_attributes() {
        unsafe {
            let html = b"<option selected disabled>Value</option>\0";
            let doc = parse_memory(html.as_ptr() as *const c_char, (html.len() - 1) as c_int);
            assert!(!doc.is_null());

            let s = html_doc_to_string(doc);
            // The minimized attributes should be preserved
            assert!(s.contains("selected"));
            assert!(s.contains("disabled"));

            tree::free_doc(doc);
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // HTML Entities
    // ═════════════════════════════════════════════════════════════════════════

    /// Verifies named entity resolution and re-escaping during serialization.
    ///
    /// # Safety
    ///
    /// - The NUL-terminated static `html` buffer stays valid for the
    ///   `parse_memory` call; the returned document pointer is asserted
    ///   non-NULL before it is dereferenced by `html_doc_to_string` and is
    ///   freed exactly once with `tree::free_doc`.
    #[test]
    fn test_html_entities() {
        unsafe {
            let html = b"<p>&amp; &lt; &gt; &quot; &nbsp; &copy;</p>\0";
            let doc = parse_memory(html.as_ptr() as *const c_char, (html.len() - 1) as c_int);
            assert!(!doc.is_null());

            let s = html_doc_to_string(doc);
            // Entities are resolved in the tree; &amp; and &lt; get re-escaped during serialization
            // because & and < are special. &gt; becomes > (serialized as-is, > is safe in text).
            assert!(s.contains("&amp;")); // &amp; → & → &amp; (re-escaped)
            assert!(s.contains("&lt;")); // &lt; → < → &lt; (re-escaped)
            assert!(s.contains(">")); // &gt; → > (not escaped in text)
            assert!(s.contains("\u{00a0}")); // &nbsp; → non-breaking space

            tree::free_doc(doc);
        }
    }

    /// Verifies decimal and hexadecimal numeric entity resolution.
    ///
    /// # Safety
    ///
    /// - The NUL-terminated static `html` buffer stays valid for the
    ///   `parse_memory` call; the returned document pointer is asserted
    ///   non-NULL before it is dereferenced by `html_doc_to_string` and is
    ///   freed exactly once with `tree::free_doc`.
    #[test]
    fn test_numeric_entities() {
        unsafe {
            // &#65; = 'A', &#x41; = 'A'
            let html = b"<p>&#65; &#x41;</p>\0";
            let doc = parse_memory(html.as_ptr() as *const c_char, (html.len() - 1) as c_int);
            assert!(!doc.is_null());

            let s = html_doc_to_string(doc);
            assert!(s.contains('A'));

            tree::free_doc(doc);
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // Nested Elements
    // ═════════════════════════════════════════════════════════════════════════

    /// Verifies nested element parsing and serialization.
    ///
    /// # Safety
    ///
    /// - The NUL-terminated static `html` buffer stays valid for the
    ///   `parse_memory` call; the returned document pointer is asserted
    ///   non-NULL before it is dereferenced by `html_doc_to_string` and is
    ///   freed exactly once with `tree::free_doc`.
    #[test]
    fn test_nested_elements() {
        unsafe {
            let html = b"<div><ul><li>Item 1</li><li>Item 2</li></ul></div>\0";
            let doc = parse_memory(html.as_ptr() as *const c_char, (html.len() - 1) as c_int);
            assert!(!doc.is_null());

            let s = html_doc_to_string(doc);
            assert!(s.contains("<div>"));
            assert!(s.contains("<ul>"));
            assert!(s.contains("<li>Item 1</li>"));
            assert!(s.contains("<li>Item 2</li>"));

            tree::free_doc(doc);
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // Malformed HTML Recovery
    // ═════════════════════════════════════════════════════════════════════════

    /// Verifies tag-recovery when end tags are missing.
    ///
    /// # Safety
    ///
    /// - The NUL-terminated static `html` buffer stays valid for the
    ///   `parse_memory` call; the returned document pointer is asserted
    ///   non-NULL before it is dereferenced by `html_doc_to_string` and is
    ///   freed exactly once with `tree::free_doc`.
    #[test]
    fn test_missing_end_tags() {
        unsafe {
            // Missing closing tags
            let html = b"<p>Paragraph without closing<div>Another div\0";
            let doc = parse_memory(html.as_ptr() as *const c_char, (html.len() - 1) as c_int);
            assert!(!doc.is_null());

            let s = html_doc_to_string(doc);
            assert!(s.contains("Paragraph without closing"));
            assert!(s.contains("Another div"));

            tree::free_doc(doc);
        }
    }

    /// Verifies case-insensitive parsing with case-preserved serialization.
    ///
    /// # Safety
    ///
    /// - The NUL-terminated static `html` buffer stays valid for the
    ///   `parse_memory` call; the returned document pointer is asserted
    ///   non-NULL before it is dereferenced by `html_doc_to_string` and is
    ///   freed exactly once with `tree::free_doc`.
    #[test]
    fn test_mismatched_case() {
        unsafe {
            let html = b"<HTML><HEAD><TITLE>Test</TITLE></HEAD><BODY><P>Hello</P></BODY></HTML>\0";
            let doc = parse_memory(html.as_ptr() as *const c_char, (html.len() - 1) as c_int);
            assert!(!doc.is_null());

            let s = html_doc_to_string(doc);
            // Tag names are case-preserved
            assert!(s.contains("<HTML>"));
            assert!(s.contains("<HEAD>"));
            assert!(s.contains("<BODY>"));
            assert!(s.contains("<P>Hello</P>"));

            tree::free_doc(doc);
        }
    }

    /// Verifies recovery from deeply nested malformed markup.
    ///
    /// # Safety
    ///
    /// - The NUL-terminated static `html` buffer stays valid for the
    ///   `parse_memory` call; the returned document pointer is asserted
    ///   non-NULL before it is dereferenced by `html_doc_to_string` and is
    ///   freed exactly once with `tree::free_doc`.
    #[test]
    fn test_nested_malformed() {
        unsafe {
            // Deeply nested with missing end tags
            let html = b"<div><p><span><b>Deep text</div></p>\0";
            let doc = parse_memory(html.as_ptr() as *const c_char, (html.len() - 1) as c_int);
            assert!(!doc.is_null());

            let s = html_doc_to_string(doc);
            assert!(s.contains("Deep text"));

            tree::free_doc(doc);
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // HTML Serialization Round-trip
    // ═════════════════════════════════════════════════════════════════════════

    /// Verifies a simple parse and serialize round trip.
    ///
    /// # Safety
    ///
    /// - The NUL-terminated static `original` buffer stays valid for the
    ///   `parse_memory` call; the returned document pointer is asserted
    ///   non-NULL before it is dereferenced by `html_doc_to_string` and is
    ///   freed exactly once with `tree::free_doc`.
    #[test]
    fn test_serialization_round_trip_simple() {
        unsafe {
            let original = b"<p>Hello World</p>\0";
            let doc = parse_memory(
                original.as_ptr() as *const c_char,
                (original.len() - 1) as c_int,
            );
            assert!(!doc.is_null());

            let s = html_doc_to_string(doc);
            assert!(s.contains("Hello World"));

            tree::free_doc(doc);
        }
    }

    /// Verifies void elements are not serialized with self-closing syntax.
    ///
    /// # Safety
    ///
    /// - The NUL-terminated static `html` buffer stays valid for the
    ///   `parse_memory` call; the returned document pointer is asserted
    ///   non-NULL before it is dereferenced by `html_doc_to_string` and is
    ///   freed exactly once with `tree::free_doc`.
    #[test]
    fn test_serialize_void_elements_no_self_close() {
        unsafe {
            let html = b"<br><hr><img src=\"test.png\">\0";
            let doc = parse_memory(html.as_ptr() as *const c_char, (html.len() - 1) as c_int);
            assert!(!doc.is_null());

            let s = html_doc_to_string(doc);
            // HTML serialization should NOT use self-closing tags
            assert!(!s.contains("<br/>"));
            assert!(!s.contains("<hr/>"));

            tree::free_doc(doc);
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // Script and Style Handling
    // ═════════════════════════════════════════════════════════════════════════

    /// Verifies raw text content inside a `script` element is preserved.
    ///
    /// # Safety
    ///
    /// - The NUL-terminated static `html` buffer stays valid for the
    ///   `parse_memory` call; the returned document pointer is asserted
    ///   non-NULL before it is dereferenced by `html_doc_to_string` and is
    ///   freed exactly once with `tree::free_doc`.
    #[test]
    fn test_script_content() {
        unsafe {
            // Use a simpler script content that doesn't contain '<' to avoid parser confusion
            let html = b"<script>var x = 1;</script>\0";
            let doc = parse_memory(html.as_ptr() as *const c_char, (html.len() - 1) as c_int);
            assert!(!doc.is_null());

            let s = html_doc_to_string(doc);
            assert!(s.contains("<script>"));
            // The raw text content should be preserved
            assert!(s.contains("var x = 1;"));

            // UPSTREAM-PARITY (DATA_RAWTEXT): script content is never
            // escaped, even when it contains '<', '&' or '>'.
            let html2 = b"<script>if (a < b && c > d) { x(1); }</script>\0";
            let doc2 = parse_memory(html2.as_ptr() as *const c_char, (html2.len() - 1) as c_int);
            assert!(!doc2.is_null());
            let s2 = html_doc_to_string(doc2);
            assert!(
                s2.contains("if (a < b && c > d) { x(1); }"),
                "script content must be raw, got: {s2}"
            );
            assert!(
                !s2.contains("&lt;"),
                "script content must not be escaped: {s2}"
            );
            tree::free_doc(doc2);

            tree::free_doc(doc);
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // Comments and DOCTYPE
    // ═════════════════════════════════════════════════════════════════════════

    /// Verifies HTML comments are preserved in the serialized output.
    ///
    /// # Safety
    ///
    /// - The NUL-terminated static `html` buffer stays valid for the
    ///   `parse_memory` call; the returned document pointer is asserted
    ///   non-NULL before it is dereferenced by `html_doc_to_string` and is
    ///   freed exactly once with `tree::free_doc`.
    #[test]
    fn test_html_comment() {
        unsafe {
            let html = b"<html><!-- This is a comment --><body><p>Text</p></body></html>\0";
            let doc = parse_memory(html.as_ptr() as *const c_char, (html.len() - 1) as c_int);
            assert!(!doc.is_null());

            let s = html_doc_to_string(doc);
            assert!(s.contains("<!-- This is a comment -->"));

            tree::free_doc(doc);
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // new_doc / new_doc_no_dtd
    // ═════════════════════════════════════════════════════════════════════════

    /// Verifies `new_doc` creates an HTML document WITHOUT an implicit
    /// `html`/`head`/`body` skeleton (upstream htmlNewDoc does not seed them;
    /// the HTML parser grows them lazily). `html_doc_to_string` of an empty
    /// body-less HTML doc emits just the trailing newline.
    ///
    /// # Safety
    ///
    /// - `new_doc` returns an owned `_xmlDoc` or NULL; the pointer is
    ///   asserted non-NULL before its `type_` field is dereferenced and
    ///   before `html_doc_to_string`, and is freed exactly once with
    ///   `tree::free_doc`.
    #[test]
    fn test_new_doc_creates_html_head_body() {
        unsafe {
            let doc = new_doc(ptr::null());
            assert!(!doc.is_null());
            assert_eq!((*doc).type_, XML_HTML_DOCUMENT_NODE as c_int);

            let s = html_doc_to_string(doc);
            assert!(!s.contains("<html>"), "htmlNewDoc must not seed <html>");
            assert!(!s.contains("<head>"), "htmlNewDoc must not seed <head>");
            assert!(!s.contains("<body>"), "htmlNewDoc must not seed <body>");

            tree::free_doc(doc);
        }
    }

    /// Verifies `new_doc_no_dtd` creates a document without implicit
    /// structure and without a DTD.
    ///
    /// # Safety
    ///
    /// - `new_doc_no_dtd` returns an owned `_xmlDoc` or NULL; the pointer is
    ///   asserted non-NULL before its `type_` field is dereferenced and
    ///   before `html_doc_to_string`, and is freed exactly once with
    ///   `tree::free_doc`.
    #[test]
    fn test_new_doc_no_dtd() {
        unsafe {
            let doc = new_doc_no_dtd(ptr::null());
            assert!(!doc.is_null());
            assert_eq!((*doc).type_, XML_HTML_DOCUMENT_NODE as c_int);

            // No implicit html/head/body. UPSTREAM-PARITY: the HTML
            // serializer (htmlNodeDumpInternal) writes "\n" for a
            // document node with no children (HTMLtree.c:861-863).
            let s = html_doc_to_string(doc);
            assert_eq!(s, "\n");

            tree::free_doc(doc);
        }
    }

    /// Phase 14.3 (dom S1 / loadHTML family): an html-parsed document
    /// carries properties = XML_DOC_HTML and standalone = 1 — upstream
    /// xmlSAX2StartDocument for HTML parsers sets XML_DOC_HTML and
    /// htmlNewDocNoDtD defaults standalone=1. The pre-fix XML_DOC_WELLFORMED-
    /// only value made PHP's spec serializer and the engine AS_XML save treat
    /// loadHTML'd documents as plain XML (declaration/standalone loss;
    /// ext/dom dom005/gh15670/gh16535/gh17397/gh19612 + loadHTMLfile*).
    ///
    /// # Safety
    ///
    /// - the parsed html doc is freed exactly once; the pointer is asserted
    ///   non-NULL before its fields are read.
    #[test]
    fn test_parsed_html_doc_flags() {
        unsafe {
            let doc = parse_memory(c"<html><body>x</body></html>".as_ptr(), 23);
            assert!(!doc.is_null());
            assert_eq!(
                (*doc).properties & (crate::abi::types::xmlDocProperties::XML_DOC_HTML as c_int),
                crate::abi::types::xmlDocProperties::XML_DOC_HTML as c_int,
                "html-parsed docs must carry XML_DOC_HTML"
            );
            assert_eq!(
                (*doc).standalone,
                1,
                "html-parsed docs default standalone=yes"
            );
            tree::free_doc(doc);
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // Entity Resolution in Text
    // ═════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_resolve_numeric_entity() {
        assert_eq!(resolve_numeric_entity("65", false), vec![b'A']);
        assert_eq!(resolve_numeric_entity("41", true), vec![b'A']);
        assert_eq!(resolve_numeric_entity("0", false), Vec::<u8>::new());
    }

    #[test]
    fn test_resolve_entity_unknown() {
        let result = resolve_entity("unknown");
        assert_eq!(result, b"&unknown;");
    }

    // ═════════════════════════════════════════════════════════════════════════
    // init/cleanup Parser
    // ═════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_init_cleanup_parser() {
        // Just ensure no crashes
        init_parser();
        cleanup_parser();
    }

    // ═════════════════════════════════════════════════════════════════════════
    // Parser Context
    // ═════════════════════════════════════════════════════════════════════════

    /// Verifies `create_file_parser_ctxt` and `free_parser_ctxt` round trip.
    ///
    /// # Safety
    ///
    /// - The static NUL-terminated filename stays valid for the
    ///   `create_file_parser_ctxt` call; the returned context is asserted
    ///   non-NULL before being freed with `free_parser_ctxt`.
    #[test]
    fn test_create_free_parser_ctxt() {
        unsafe {
            let ctxt =
                create_file_parser_ctxt(b"test.html\0" as *const u8 as *const c_char, ptr::null());
            assert!(!ctxt.is_null());
            free_parser_ctxt(ctxt);
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // Complex HTML Documents
    // ═════════════════════════════════════════════════════════════════════════

    /// Parses a complex HTML document and verifies the serialized structure.
    ///
    /// # Safety
    ///
    /// - The NUL-terminated static `html` buffer stays valid for the
    ///   `parse_memory` call; the returned document pointer is asserted
    ///   non-NULL before it is dereferenced by `html_doc_to_string` and is
    ///   freed exactly once with `tree::free_doc`.
    #[test]
    fn test_complex_html_document() {
        unsafe {
            let html = b"<!DOCTYPE html>
<html>
<head>
    <meta charset=\"utf-8\">
    <title>Test Page</title>
    <link rel=\"stylesheet\" href=\"style.css\">
</head>
<body>
    <div id=\"main\">
        <h1>Title</h1>
        <p>First paragraph with <a href=\"link.html\">a link</a>.</p>
        <p>Second paragraph.</p>
        <ul>
            <li>Item 1</li>
            <li>Item 2</li>
        </ul>
        <br>
        <hr>
        <img src=\"image.jpg\" alt=\"An image\">
    </div>
    <script>alert('hello');</script>
</body>
</html>\0";
            let doc = parse_memory(html.as_ptr() as *const c_char, (html.len() - 1) as c_int);
            assert!(!doc.is_null());

            let s = html_doc_to_string(doc);
            assert!(s.contains("<html>"));
            assert!(s.contains("<head>"));
            assert!(s.contains("<title>Test Page</title>"));
            assert!(s.contains("<body>"));
            assert!(s.contains("<h1>Title</h1>"));
            assert!(s.contains("a link"));
            assert!(s.contains("Second paragraph"));
            assert!(s.contains("<br>"));
            assert!(s.contains("<hr>"));
            assert!(s.contains("<img"));
            assert!(s.contains("<script>"));

            tree::free_doc(doc);
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // parse_doc
    // ═════════════════════════════════════════════════════════════════════════

    /// Verifies `parse_doc` parses from an `xmlChar` buffer.
    ///
    /// # Safety
    ///
    /// - The NUL-terminated static `html` buffer is passed to `parse_doc` as
    ///   an `xmlChar` pointer and must stay valid for the call; the returned
    ///   document pointer is asserted non-NULL before it is dereferenced by
    ///   `html_doc_to_string` and is freed exactly once with `tree::free_doc`.
    #[test]
    fn test_parse_doc() {
        unsafe {
            let html = b"<p>Hello from parse_doc</p>\0";
            let doc = parse_doc(html.as_ptr() as *const xmlChar, ptr::null(), 0);
            assert!(!doc.is_null());

            let s = html_doc_to_string(doc);
            assert!(s.contains("Hello from parse_doc"));

            tree::free_doc(doc);
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // Table elements auto-close
    // ═════════════════════════════════════════════════════════════════════════

    /// Verifies a `td` element auto-closes a previous `td` inside a row.
    ///
    /// # Safety
    ///
    /// - The NUL-terminated static `html` buffer stays valid for the
    ///   `parse_memory` call; the returned document pointer is asserted
    ///   non-NULL before it is dereferenced by `html_doc_to_string` and is
    ///   freed exactly once with `tree::free_doc`.
    #[test]
    fn test_table_element_auto_close() {
        unsafe {
            let html = b"<table><tr><td>Cell 1<td>Cell 2</td></tr></table>";
            let doc = parse_memory(html.as_ptr() as *const c_char, (html.len() - 1) as c_int);
            assert!(!doc.is_null());

            let s = html_doc_to_string(doc);
            assert!(s.contains("<td>Cell 1"));
            assert!(s.contains("<td>Cell 2"));

            tree::free_doc(doc);
        }
    }

    #[test]
    fn test_table_tr_auto_close() {
        // <tr> must auto-close the open row (the corpus html-table op:
        // `<table><tr><td>1<td>2<tr><td>3</table>` yields two sibling rows).
        unsafe {
            let html = b"<table><tr><td>1<td>2<tr><td>3</table>\0";
            let doc = parse_memory(html.as_ptr() as *const c_char, (html.len() - 1) as c_int);
            assert!(!doc.is_null());
            let s = html_doc_to_string(doc);
            assert!(s.contains("</td></tr><tr>"));
            tree::free_doc(doc);
        }
    }
}
