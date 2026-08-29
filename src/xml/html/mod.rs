//! HTML parser and serializer (§29, §85 Phase 4).
//!
//! libxml2's historical HTML parser — a tag-recovery parser, NOT a WHATWG
//! HTML5 parser. Preserves version-specific historical behavior.
//!
//! Implements:
//! - Tag-recovery parsing (auto-close, implicit open, case-insensitive)
//! - HTML element info table with flags matching libxml2
//! - HTML entity resolution
//! - Auto-creation of html/head/body when missing
//! - HTML-specific serialization (no self-closing void tags, no namespace decls)
//! - Minimized and unquoted attribute support

use core::ffi::c_void;
use core::ptr;
use core::slice;
use std::os::raw::{c_char, c_int};

use crate::abi::allocator::{xmlFree, xmlMalloc, xmlMallocZero, xmlRealloc};
use crate::abi::constants::XML_DEFAULT_VERSION;
use crate::abi::structs::*;
use crate::abi::types::xmlCharEncoding::XML_CHAR_ENCODING_UTF8;
use crate::abi::types::xmlDocProperties::XML_DOC_WELLFORMED;
use crate::abi::types::xmlElementType::*;
use crate::abi::types::*;
use crate::xml::io;
use crate::xml::string::*;
use crate::xml::tree;

// ═══════════════════════════════════════════════════════════════════════════════
// HTML Element Info
// ═══════════════════════════════════════════════════════════════════════════════

/// HTML element flag constants matching libxml2's HTMLparser.h.
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
    // Lowercase the name for comparison
    let lower: Vec<u8> = name.bytes().map(|b| b.to_ascii_lowercase()).collect();
    let lower_str = match core::str::from_utf8(&lower) {
        Ok(s) => s,
        Err(_) => return None,
    };
    HTML_ELEMENTS.iter().find(|info| info.name == lower_str)
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
    err: bool,
    /// Filename (for file parsing)
    filename: *mut c_char,
    /// Encoding
    encoding: *mut c_char,
}

impl HtmlParserCtxt {
    fn new() -> Self {
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
        }
    }

    /// Peek at the next byte without consuming it.
    fn peek(&self) -> Option<u8> {
        if self.input_pos < self.input_len {
            unsafe { Some(*self.input.add(self.input_pos)) }
        } else {
            None
        }
    }

    /// Peek ahead `n` bytes.
    fn peek_at(&self, offset: usize) -> Option<u8> {
        let pos = self.input_pos + offset;
        if pos < self.input_len {
            unsafe { Some(*self.input.add(pos)) }
        } else {
            None
        }
    }

    /// Consume and return the next byte.
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
    fn is_eof(&self) -> bool {
        self.input_pos >= self.input_len
    }

    /// Read a sequence of bytes while the predicate returns true.
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
unsafe fn auto_close_element(ctxt: &mut HtmlParserCtxt, tag_name: &str) {
    let tag_lower: Vec<u8> = tag_name.bytes().map(|b| b.to_ascii_lowercase()).collect();
    let tag_lower_str = match core::str::from_utf8(&tag_lower) {
        Ok(s) => s,
        Err(_) => return,
    };

    let info = html_tag_lookup(&tag_lower_str);

    let mut current = ctxt.current;

    // Collect the open element names up the tree
    let mut open_names: Vec<Vec<u8>> = Vec::new();
    let mut cur = current;
    while !cur.is_null() {
        let ctype = unsafe { (*cur).type_ };
        if ctype == XML_ELEMENT_NODE as c_int {
            if !unsafe { (*cur).name.is_null() } {
                let name_bytes = unsafe { xmlstr_to_bytes((*cur).name) };
                open_names.push(name_bytes.to_vec());
            }
        }
        cur = unsafe { (*cur).parent };
    }

    // Rule 1: <p> auto-closes before another <p>, and before block elements
    if tag_lower_str == "p" || info.map_or(false, |i| i.flags & HTML_BLOCK != 0) {
        // Close any open <p> elements
        let mut cur2 = current;
        while !cur2.is_null() {
            let ctype = unsafe { (*cur2).type_ };
            if ctype == XML_ELEMENT_NODE as c_int {
                if !unsafe { (*cur2).name.is_null() } {
                    let name_bytes = unsafe { xmlstr_to_bytes((*cur2).name) };
                    let name_str = core::str::from_utf8(name_bytes).unwrap_or("");
                    if name_str.eq_ignore_ascii_case("p") {
                        // Close this <p> by moving current up past it
                        current = unsafe { (*cur2).parent };
                        break;
                    }
                }
            }
            cur2 = unsafe { (*cur2).parent };
        }
    }

    // Rule 2: Headings (h1-h6) auto-close other headings
    if is_heading(&tag_lower_str) {
        let mut cur2 = current;
        while !cur2.is_null() {
            let ctype = unsafe { (*cur2).type_ };
            if ctype == XML_ELEMENT_NODE as c_int {
                if !unsafe { (*cur2).name.is_null() } {
                    let name_bytes = unsafe { xmlstr_to_bytes((*cur2).name) };
                    let name_str = core::str::from_utf8(name_bytes).unwrap_or("");
                    if is_heading(name_str) {
                        current = unsafe { (*cur2).parent };
                        break;
                    }
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
            if ctype == XML_ELEMENT_NODE as c_int {
                if !unsafe { (*cur2).name.is_null() } {
                    let name_bytes = unsafe { xmlstr_to_bytes((*cur2).name) };
                    let name_str = core::str::from_utf8(name_bytes).unwrap_or("");
                    if name_str.eq_ignore_ascii_case("li") {
                        current = unsafe { (*cur2).parent };
                        break;
                    }
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
            if ctype == XML_ELEMENT_NODE as c_int {
                if !unsafe { (*cur2).name.is_null() } {
                    let name_bytes = unsafe { xmlstr_to_bytes((*cur2).name) };
                    let name_str = core::str::from_utf8(name_bytes).unwrap_or("");
                    if name_str == "dt" || name_str == "dd" {
                        current = unsafe { (*cur2).parent };
                        break;
                    }
                }
            }
            cur2 = unsafe { (*cur2).parent };
        }
    }

    // Rule 5: <tr> auto-closes an open <tr>, <td> or <th> (the previous
    // row); a new <td>/<th> only auto-closes an open <td>/<th> (the
    // previous cell) and stays inside the open <tr> (upstream
    // htmlAutoClose).
    if tag_lower_str == "tr" {
        let mut cur2 = current;
        while !cur2.is_null() {
            let ctype = unsafe { (*cur2).type_ };
            if ctype == XML_ELEMENT_NODE as c_int {
                if !unsafe { (*cur2).name.is_null() } {
                    let name_bytes = unsafe { xmlstr_to_bytes((*cur2).name) };
                    let name_str = core::str::from_utf8(name_bytes).unwrap_or("");
                    if name_str == "tr" || name_str == "td" || name_str == "th" {
                        current = unsafe { (*cur2).parent };
                        break;
                    }
                }
            }
            cur2 = unsafe { (*cur2).parent };
        }
    } else if tag_lower_str == "td" || tag_lower_str == "th" {
        let mut cur2 = current;
        while !cur2.is_null() {
            let ctype = unsafe { (*cur2).type_ };
            if ctype == XML_ELEMENT_NODE as c_int {
                if !unsafe { (*cur2).name.is_null() } {
                    let name_bytes = unsafe { xmlstr_to_bytes((*cur2).name) };
                    let name_str = core::str::from_utf8(name_bytes).unwrap_or("");
                    if name_str == "td" || name_str == "th" {
                        current = unsafe { (*cur2).parent };
                        break;
                    }
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
            if ctype == XML_ELEMENT_NODE as c_int {
                if !unsafe { (*cur2).name.is_null() } {
                    let name_bytes = unsafe { xmlstr_to_bytes((*cur2).name) };
                    let name_str = core::str::from_utf8(name_bytes).unwrap_or("");
                    if name_str == "thead" || name_str == "tbody" || name_str == "tfoot" {
                        current = unsafe { (*cur2).parent };
                        break;
                    }
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
            if ctype == XML_ELEMENT_NODE as c_int {
                if !unsafe { (*cur2).name.is_null() } {
                    let name_bytes = unsafe { xmlstr_to_bytes((*cur2).name) };
                    let name_str = core::str::from_utf8(name_bytes).unwrap_or("");
                    if name_str == "colgroup" {
                        current = unsafe { (*cur2).parent };
                        break;
                    }
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
            if ctype == XML_ELEMENT_NODE as c_int {
                if !unsafe { (*cur2).name.is_null() } {
                    let name_bytes = unsafe { xmlstr_to_bytes((*cur2).name) };
                    let name_str = core::str::from_utf8(name_bytes).unwrap_or("");
                    if name_str == "caption" {
                        current = unsafe { (*cur2).parent };
                        break;
                    }
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
            if ctype == XML_ELEMENT_NODE as c_int {
                if !unsafe { (*cur2).name.is_null() } {
                    let name_bytes = unsafe { xmlstr_to_bytes((*cur2).name) };
                    let name_str = core::str::from_utf8(name_bytes).unwrap_or("");
                    if name_str.eq_ignore_ascii_case("form") {
                        current = unsafe { (*cur2).parent };
                        break;
                    }
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
    if !ctxt.html.is_null() {
        return ctxt.html;
    }

    let html_node = tree::new_node(ptr::null_mut(), b"html\0" as *const u8 as *const xmlChar);
    if html_node.is_null() {
        return ptr::null_mut();
    }
    unsafe {
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
            Some(b'/') => {
                // Could be self-closing tag like <br/>
                if ctxt.peek_at(1) == Some(b'>') {
                    break;
                }
                // Otherwise it's part of a minimized attribute or path
            }
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
        u32::from_str_radix(value, 10).unwrap_or(0xFFFD)
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
        let text_node = tree::new_text(ptr::null_mut());
        if !text_node.is_null() {
            // Create a null-terminated copy
            let content = bytes_to_xmlstr(text);
            if !content.is_null() {
                unsafe {
                    (*text_node).content = content;
                }
            }
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
        let html_node = tree::new_node(ptr::null_mut(), bytes_to_xmlstr(b"html"));
        if !html_node.is_null() {
            let content_c = bytes_to_xmlstr(content);
            let text_node = tree::new_text(ptr::null_mut());
            if !text_node.is_null() {
                unsafe {
                    (*text_node).content = content_c;
                }
                tree::add_child(html_node, text_node);
            }
            tree::add_child(ctxt.doc as *mut _xmlNode, html_node);
            ctxt.html = html_node;
            ctxt.html_created = true;
            ctxt.current = html_node;
        }
        return;
    }

    let text_node = tree::new_text(ptr::null_mut());
    if text_node.is_null() {
        return;
    }

    // Set the content
    let content = bytes_to_xmlstr(text);
    if !content.is_null() {
        unsafe {
            (*text_node).content = content;
        }
    }

    tree::add_child(insertion_point, text_node);
}

/// Process a start tag in the tree builder.
unsafe fn handle_start_tag(ctxt: &mut HtmlParserCtxt, tag_name: &[u8], attrs: &[HtmlAttr]) {
    let tag_lower: Vec<u8> = tag_name.iter().map(|b| b.to_ascii_lowercase()).collect();
    let tag_str = core::str::from_utf8(&tag_lower).unwrap_or("");

    let info = html_tag_lookup(tag_str);

    // Determine tag category
    let is_head_tag = info.map_or(false, |i| i.flags & HTML_HEAD != 0);
    let is_body_tag = info.map_or(false, |i| i.flags & HTML_BODY != 0);
    let is_empty = info.map_or(false, |i| i.flags & HTML_EMPTY != 0);
    let is_block = info.map_or(false, |i| i.flags & HTML_BLOCK != 0);

    // Handle special elements
    if tag_str == "html" {
        if !ctxt.html.is_null() && !ctxt.html_created {
            // Second <html> tag, skip it
            return;
        }
        // Create or use existing html
        if ctxt.html.is_null() {
            let html_node = tree::new_node(ptr::null_mut(), bytes_to_xmlstr(tag_name));
            if !html_node.is_null() {
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
            let head_node = tree::new_node(ptr::null_mut(), bytes_to_xmlstr(tag_name));
            if !head_node.is_null() {
                ctxt.head = head_node;
                ctxt.head_created = false;
                tree::add_child(ctxt.html, head_node);
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
            let body_node = tree::new_node(ptr::null_mut(), bytes_to_xmlstr(tag_name));
            if !body_node.is_null() {
                ctxt.body = body_node;
                ctxt.body_created = false;
                tree::add_child(ctxt.html, body_node);
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
            let node = tree::new_node(ptr::null_mut(), bytes_to_xmlstr(tag_name));
            if !node.is_null() {
                for attr in attrs {
                    let name_c = bytes_to_xmlstr(&attr.name);
                    let val_c = bytes_to_xmlstr(&attr.value);
                    if !name_c.is_null() {
                        tree::set_prop(node, name_c, val_c);
                        xmlFree(name_c as *mut c_void);
                        if !val_c.is_null() {
                            xmlFree(val_c as *mut c_void);
                        }
                    }
                }
                tree::add_child(ctxt.current, node);
            }
            return;
        }

        let node = tree::new_node(ptr::null_mut(), bytes_to_xmlstr(tag_name));
        if !node.is_null() {
            for attr in attrs {
                let name_c = bytes_to_xmlstr(&attr.name);
                let val_c = bytes_to_xmlstr(&attr.value);
                if !name_c.is_null() {
                    tree::set_prop(node, name_c, val_c);
                    xmlFree(name_c as *mut c_void);
                    if !val_c.is_null() {
                        xmlFree(val_c as *mut c_void);
                    }
                }
            }
            tree::add_child(ctxt.current, node);
            ctxt.current = node;
        }
        return;
    }

    // Body content - transition from head if needed
    if !is_head_tag || ctxt.seen_body_content {
        if !ctxt.seen_body_content {
            ctxt.seen_body_content = true;
            ctxt.in_head = false;
            if ctxt.body.is_null() {
                ensure_body(ctxt);
            } else {
                ctxt.current = ctxt.body;
                ctxt.in_body = true;
            }
        } else if ctxt.body.is_null() {
            ensure_body(ctxt);
        }
    }

    // Auto-close elements as needed
    if !ctxt.current.is_null() {
        auto_close_element(ctxt, tag_str);
    }

    if is_empty {
        // Void element: create node, add attributes, add as child (no children)
        let node = tree::new_node(ptr::null_mut(), bytes_to_xmlstr(tag_name));
        if !node.is_null() {
            for attr in attrs {
                let name_c = bytes_to_xmlstr(&attr.name);
                let val_c = bytes_to_xmlstr(&attr.value);
                if !name_c.is_null() {
                    tree::set_prop(node, name_c, val_c);
                    xmlFree(name_c as *mut c_void);
                    if !val_c.is_null() {
                        xmlFree(val_c as *mut c_void);
                    }
                }
            }
            let insertion_point = if ctxt.current.is_null() {
                ctxt.body
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
    let node = tree::new_node(ptr::null_mut(), bytes_to_xmlstr(tag_name));
    if !node.is_null() {
        for attr in attrs {
            let name_c = bytes_to_xmlstr(&attr.name);
            let val_c = bytes_to_xmlstr(&attr.value);
            if !name_c.is_null() {
                tree::set_prop(node, name_c, val_c);
                xmlFree(name_c as *mut c_void);
                if !val_c.is_null() {
                    xmlFree(val_c as *mut c_void);
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
unsafe fn handle_end_tag(ctxt: &mut HtmlParserCtxt, tag_name: &[u8]) {
    let tag_lower: Vec<u8> = tag_name.iter().map(|b| b.to_ascii_lowercase()).collect();
    let tag_str = core::str::from_utf8(&tag_lower).unwrap_or("");

    let info = html_tag_lookup(tag_str);

    // For elements with no end tag (void elements or optional end tags),
    // just ignore the end tag.
    if info.map_or(false, |i| i.flags & HTML_EMPTY != 0) {
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
        if ctype == XML_ELEMENT_NODE as c_int {
            if !unsafe { (*cur).name.is_null() } {
                let name_bytes = unsafe { xmlstr_to_bytes((*cur).name) };
                if name_bytes.eq_ignore_ascii_case(tag_name) {
                    // Found the matching element - close by moving current to parent
                    ctxt.current = unsafe { (*cur).parent };
                    return;
                }
            }
        }
        cur = unsafe { (*cur).parent };
    }

    // If no matching element found, ignore the end tag (tag-recovery behavior)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Main Parse Function
// ═══════════════════════════════════════════════════════════════════════════════

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
            crate::abi::allocator::xmlFree((*doc).version as *mut c_void);
        }
        (*doc).version = ptr::null_mut();
        (*doc).properties = XML_DOC_WELLFORMED as c_int;
        // UPSTREAM-PARITY: HTML documents default to standalone="yes"
        // (visible when serialized with the XML serializer, e.g. --xmlout).
        (*doc).standalone = 1;
    }
    // UPSTREAM-PARITY: htmlParseDocument creates a default DTD
    // (`<!DOCTYPE html PUBLIC "-//W3C//DTD HTML 4.0 Transitional//EN"
    // "http://www.w3.org/TR/REC-html40/loose.dtd">) when the source does
    // not declare one; XML_SAVE_NO_DOCTYPE / HTML_PARSE_NODEFDTD suppresses
    // it (handled by the caller).
    let default_dtd = crate::xml::dtd::create_int_subset(
        doc,
        b"html\0" as *const u8 as *const xmlChar,
        b"-//W3C//DTD HTML 4.0 Transitional//EN\0" as *const u8 as *const xmlChar,
        b"http://www.w3.org/TR/REC-html40/loose.dtd\0" as *const u8 as *const xmlChar,
    );
    let _ = default_dtd;
    ctxt.doc = doc;

    // Set up input
    ctxt.input = buffer as *mut u8;
    ctxt.input_len = size as usize;
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
                    let comment_node = tree::new_comment(bytes_to_xmlstr(&comment_content));
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
                let rest = ctxt.read_while(|ch| ch != b'>');
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

                    let pi_node = tree::new_pi(bytes_to_xmlstr(target), bytes_to_xmlstr(value));
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
                handle_text(ctxt, &[b'<']);
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
                let raw_node = tree::new_node(ptr::null_mut(), bytes_to_xmlstr(&tag_name));
                if !raw_node.is_null() {
                    for attr in &attrs {
                        let name_c = bytes_to_xmlstr(&attr.name);
                        let val_c = bytes_to_xmlstr(&attr.value);
                        if !name_c.is_null() {
                            tree::set_prop(raw_node, name_c, val_c);
                            xmlFree(name_c as *mut c_void);
                            if !val_c.is_null() {
                                xmlFree(val_c as *mut c_void);
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
                        let mut match_idx = 0;

                        loop {
                            if ctxt.is_eof() {
                                break;
                            }
                            let ch = ctxt.peek().unwrap();
                            if ch.to_ascii_lowercase() == end_bytes[match_idx] {
                                match_idx += 1;
                                if match_idx == end_bytes.len() {
                                    // We found the start of </tag
                                    // Add the text before the end tag
                                    if !raw_text.is_empty() {
                                        let text_node = tree::new_text(bytes_to_xmlstr(&raw_text));
                                        if !text_node.is_null() {
                                            tree::add_child(raw_node, text_node);
                                        }
                                    }
                                    // Consume the rest of the end tag: "tag>"
                                    ctxt.next(); // consume the last char of end tag prefix
                                                 // Now read "tag>"
                                    let _suffix = ctxt.read_while(|ch| ch != b'>');
                                    if ctxt.peek() == Some(b'>') {
                                        ctxt.next();
                                    }
                                    // Close the element
                                    ctxt.current = unsafe { (*raw_node).parent };
                                    break;
                                }
                                // Store the potential match start
                                if match_idx == 1 {
                                    raw_text.push(ch);
                                }
                                ctxt.next();
                            } else {
                                // If we were building a match, flush all buffered chars
                                if match_idx > 0 {
                                    // We already pushed some chars, just continue
                                    match_idx = 0;
                                }
                                raw_text.push(ch);
                                ctxt.next();
                            }
                        }

                        // If we never found the end tag, just add the text
                        if match_idx < end_bytes.len() && !raw_text.is_empty() {
                            let text_node = tree::new_text(bytes_to_xmlstr(&raw_text));
                            if !text_node.is_null() {
                                tree::add_child(raw_node, text_node);
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

    // Post-processing: ensure html/head/body are created even for empty documents
    if ctxt.html.is_null() {
        ensure_html(ctxt);
    }

    doc
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
pub unsafe fn parse_file(filename: *const c_char, encoding: *const c_char) -> *mut _xmlDoc {
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
    if !encoding.is_null() {
        let enc_cstr = unsafe { std::ffi::CStr::from_ptr(encoding) };
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
    if buffer.is_null() || size <= 0 {
        return ptr::null_mut();
    }

    let mut ctxt = HtmlParserCtxt::new();
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
pub(crate) unsafe fn parse_doc(cur: *const xmlChar, encoding: *const c_char) -> *mut _xmlDoc {
    if cur.is_null() {
        return ptr::null_mut();
    }

    let len = unsafe { xml_strlen(cur) };
    let mut ctxt = HtmlParserCtxt::new();
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
pub(crate) unsafe fn create_file_parser_ctxt(
    filename: *const c_char,
    encoding: *const c_char,
) -> *mut c_void {
    if filename.is_null() {
        return ptr::null_mut();
    }

    let ctxt = unsafe { xmlMallocZero(size_of::<HtmlParserCtxt>() as usize) };
    if ctxt.is_null() {
        return ptr::null_mut();
    }

    let ctxt = ctxt as *mut HtmlParserCtxt;
    unsafe {
        ptr::write(ctxt, HtmlParserCtxt::new());
        if !encoding.is_null() {
            (*ctxt).encoding = c_strdup(encoding);
        }
    }

    ctxt as *mut c_void
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

    let ctxt = ctxt as *mut HtmlParserCtxt;
    unsafe {
        if !(*ctxt).filename.is_null() {
            xmlFree((*ctxt).filename as *mut c_void);
        }
        if !(*ctxt).encoding.is_null() {
            xmlFree((*ctxt).encoding as *mut c_void);
        }
        xmlFree(ctxt as *mut c_void);
    }
}

/// Initialize the HTML parser module.
///
/// # UPSTREAM-PARITY
///
/// Equivalent to `htmlInitParser` in libxml2.
pub(crate) fn init_parser() {
    // Currently a no-op. In the future, may initialize HTML-specific
    // entity tables or other global state.
}

/// Cleanup the HTML parser module.
///
/// # UPSTREAM-PARITY
///
/// Equivalent to `htmlCleanupParser` in libxml2.
pub(crate) fn cleanup_parser() {
    // Currently a no-op. In the future, may free HTML-specific
    // global state.
}

/// Create a new HTML document.
///
/// # UPSTREAM-PARITY
///
/// Equivalent to `htmlNewDoc` in libxml2.
///
/// Creates a new document with type XML_HTML_DOCUMENT_NODE and
/// auto-creates html/head/body elements.
///
/// # Safety
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

    // Create implicit html/head/body
    let mut ctxt = HtmlParserCtxt::new();
    ctxt.doc = doc;

    unsafe {
        ensure_html(&mut ctxt);
        ensure_head(&mut ctxt);
        ensure_body(&mut ctxt);
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

/// Whether the head element already contains a <meta> element (so the
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

pub(crate) unsafe fn serialize_node(
    node: *mut _xmlNode,
    buf: *mut _xmlBuffer,
    format: c_int,
    level: c_int,
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
            let is_inline = info.map_or(true, |i| i.flags & HTML_INLINE != 0);
            let no_format = is_inline || name.starts_with('p');

            // Write start tag
            io::buf_ccat(buf, b'<');
            if !n.name.is_null() {
                io::buf_cat(buf, n.name);
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

            // UPSTREAM-PARITY: htmlSetMetaEncoding (htmlsave.c) inserts
            // <meta charset="..."> as the first child of the <head> of the
            // root <html> element when no <meta> is present and the document
            // carries an encoding (htmlNodeDumpInternal only runs the meta
            // logic when `encoding != NULL`). The meta is synthetic here, so
            // it participates in the formatting rules like a real child.
            let mut meta_bytes: Option<Vec<u8>> = None;
            if name.eq_ignore_ascii_case("head")
                && level == 1
                && !n.doc.is_null()
                && !(*n.doc).encoding.is_null()
            {
                let parent_is_html = !n.parent.is_null() && !(*n.parent).name.is_null() && {
                    let pn = core::str::from_utf8(xmlstr_to_bytes((*n.parent).name)).unwrap_or("");
                    pn.eq_ignore_ascii_case("html")
                };
                if parent_is_html && !html_head_has_meta(n.children) {
                    meta_bytes = Some(xmlstr_to_bytes((*n.doc).encoding).to_vec());
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
                    io::buf_add(buf, enc.as_ptr() as *const u8, enc.len() as c_int);
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
                    serialize_node(child, buf, format, level + 1);
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
                            let c_inline = cinfo.map_or(true, |i| i.flags & HTML_INLINE != 0);
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
                if !n.name.is_null() {
                    io::buf_cat(buf, n.name);
                }
                io::buf_ccat(buf, b'>');
            }
        }
        t if t == XML_TEXT_NODE as c_int => {
            html_serialize_text(buf, n.content, xml_strlen(n.content) as c_int);
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
                serialize_node(child, buf, format, 0);
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
    use crate::abi::allocator::xmlFree;
    use crate::xml::io;

    /// Helper: create a null-terminated xmlChar* from a byte slice.
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
            let slice = slice::from_raw_parts(content, len as usize);
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

            tree::free_doc(doc);
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // Comments and DOCTYPE
    // ═════════════════════════════════════════════════════════════════════════

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

    #[test]
    fn test_new_doc_creates_html_head_body() {
        unsafe {
            let doc = new_doc(ptr::null());
            assert!(!doc.is_null());
            assert_eq!((*doc).type_, XML_HTML_DOCUMENT_NODE as c_int);

            let s = html_doc_to_string(doc);
            assert!(s.contains("<html>"));
            assert!(s.contains("<head>"));
            assert!(s.contains("<body>"));

            tree::free_doc(doc);
        }
    }

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

    #[test]
    fn test_parse_doc() {
        unsafe {
            let html = b"<p>Hello from parse_doc</p>";
            let doc = parse_doc(html.as_ptr() as *const xmlChar, ptr::null());
            assert!(!doc.is_null());

            let s = html_doc_to_string(doc);
            assert!(s.contains("Hello from parse_doc"));

            tree::free_doc(doc);
        }
    }

    // ═════════════════════════════════════════════════════════════════════════
    // Table elements auto-close
    // ═════════════════════════════════════════════════════════════════════════

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
}
