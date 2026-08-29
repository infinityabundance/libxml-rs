//! EXSLT Strings (str:) — str:tokenize, str:replace, str:padding, str:align,
//! str:concat, str:split, str:encode-uri, str:decode-uri (§35).
//!
//! # UPSTREAM-PARITY
//!
//! Upstream libxslt (libexslt/strings.c) semantics:
//!
//! - `str:tokenize(string, delimiters)` — splits the string on any of the
//!   delimiter characters (default: whitespace); returns a node-set of text
//!   nodes.
//! - `str:replace(string, search, replace)` — replaces every occurrence of
//!   the search string with the replacement.
//! - `str:padding(length, character)` — a string of `length` copies of the
//!   first character of `character` (default: space).
//! - `str:align(string, padding, alignment)` — pads/truncates `string` to
//!   the length of `padding`, aligning left, right, or center.
//! - `str:concat(sep, node-set)` — concatenates the string values of the
//!   node-set separated by `sep`.
//! - `str:split(string, delimiter)` — splits on the exact delimiter
//!   (default: space) and returns a node-set of text nodes.
//! - `str:encode-uri(string, escape)` — percent-encodes the string;
//!   `escape` selects which characters are escaped (default: none beyond
//!   the required set). Non-ASCII characters are UTF-8 encoded.
//! - `str:decode-uri(string)` — percent-decodes the string.

use super::{register, ExsltFunction};
use crate::xml::xpath::context::XPathContext;
use crate::xml::xpath::types::{node_string_value, NodeSet, XPathValue};

/// str:tokenize(string, delimiters) — split on any delimiter character.
fn tokenize_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let s = str_at(args, 0);
    let delims: Vec<u8> = match args.get(1) {
        Some(v) => v.as_string().into_bytes(),
        None => b" \t\n\r".to_vec(),
    };
    let mut out = NodeSet::new();
    let mut current: Vec<u8> = Vec::new();
    for b in s.bytes() {
        if delims.contains(&b) {
            if !current.is_empty() {
                push_text(&mut out, &current);
                current.clear();
            }
        } else {
            current.push(b);
        }
    }
    if !current.is_empty() {
        push_text(&mut out, &current);
    }
    Ok(XPathValue::NodeSet(out))
}

/// str:split(string, delimiter) — split on the exact delimiter substring.
fn split_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let s = str_at(args, 0);
    let delim = match args.get(1) {
        Some(v) => v.as_string(),
        None => " ".to_string(),
    };
    let mut out = NodeSet::new();
    if delim.is_empty() {
        push_text(&mut out, s.as_bytes());
        return Ok(XPathValue::NodeSet(out));
    }
    for part in s.split(&delim) {
        push_text(&mut out, part.as_bytes());
    }
    Ok(XPathValue::NodeSet(out))
}

/// str:replace(string, search, replace) — replace all occurrences.
fn replace_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let s = str_at(args, 0);
    let search = str_at(args, 1);
    let replace = str_at(args, 2);
    if search.is_empty() {
        return Ok(XPathValue::String(s));
    }
    Ok(XPathValue::String(s.replace(&search, &replace)))
}

/// str:padding(length, character) — a repeated-character padding string.
fn padding_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let len = match args.first() {
        Some(v) => v.as_number().round() as i64,
        None => return Ok(XPathValue::String(String::new())),
    };
    if len <= 0 {
        return Ok(XPathValue::String(String::new()));
    }
    let ch = match args.get(1) {
        Some(v) => v.as_string().chars().next().unwrap_or(' '),
        None => ' ',
    };
    Ok(XPathValue::String(ch.to_string().repeat(len as usize)))
}

/// str:align(string, padding, alignment) — pad/truncate to padding's length.
fn align_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let s = str_at(args, 0);
    let padding = str_at(args, 1);
    let alignment = match args.get(2) {
        Some(v) => v.as_string(),
        None => String::new(),
    };
    let target = padding.chars().count();
    let len = s.chars().count();
    if len >= target {
        // Truncate (upstream truncates to the padding width).
        return Ok(XPathValue::String(s.chars().take(target).collect()));
    }
    let fill = target - len;
    let out = match alignment.as_str() {
        "right" => format!("{}{}", " ".repeat(fill), s),
        "center" => {
            // Upstream rounds the extra space to the LEFT for odd fills.
            let left = (fill + 1) / 2;
            let right = fill - left;
            format!("{}{}{}", " ".repeat(left), s, " ".repeat(right))
        }
        _ => format!("{}{}", s, " ".repeat(fill)), // left (default)
    };
    Ok(XPathValue::String(out))
}

/// str:concat(sep, node-set) — concatenate string values with a separator.
fn concat_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let sep = str_at(args, 0);
    let ns = match args.get(1) {
        Some(XPathValue::NodeSet(ns)) => ns.clone(),
        _ => NodeSet::new(),
    };
    let parts: Vec<String> = ns.iter().map(|n| node_string_value(n)).collect();
    Ok(XPathValue::String(parts.join(&sep)))
}

/// str:encode-uri(string, escape) — percent-encode a URI.
fn encode_uri_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let s = str_at(args, 0);
    let escape = match args.get(1) {
        Some(v) => v.as_string(),
        None => String::new(),
    };
    // Characters allowed unescaped in a URI (RFC 3986 unreserved + reserved,
    // plus '%'); everything else (space, controls, non-ASCII) is escaped.
    let allowed = |c: char| {
        c.is_ascii_alphanumeric()
            || matches!(
                c,
                '-' | '_'
                    | '.'
                    | '~'
                    | ':'
                    | '/'
                    | '?'
                    | '#'
                    | '['
                    | ']'
                    | '@'
                    | '!'
                    | '$'
                    | '&'
                    | '\''
                    | '('
                    | ')'
                    | '*'
                    | '+'
                    | ','
                    | ';'
                    | '='
                    | '%'
            )
    };
    // The `escape` parameter selects additional characters to escape
    // (e.g. "all" escapes everything except the unreserved set).
    let escape_all = escape == "all" || escape.contains("ALL");
    let mut out = String::new();
    for c in s.chars() {
        let mut buf = [0u8; 4];
        let bytes = c.encode_utf8(&mut buf).as_bytes();
        let need_escape = if escape_all {
            !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~'))
        } else {
            !allowed(c) || !c.is_ascii() || (escape.contains(c) && c.is_ascii())
        };
        if need_escape {
            for b in bytes {
                out.push_str(&format!("%{:02X}", b));
            }
        } else {
            out.push(c);
        }
    }
    Ok(XPathValue::String(out))
}

/// str:decode-uri(string) — percent-decode a URI.
fn decode_uri_fn(_ctx: &mut XPathContext, args: &[XPathValue]) -> Result<XPathValue, String> {
    let s = str_at(args, 0);
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let h = hex_val(bytes[i + 1]);
            let l = hex_val(bytes[i + 2]);
            if let (Some(h), Some(l)) = (h, l) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    Ok(XPathValue::String(
        String::from_utf8_lossy(&out).into_owned(),
    ))
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn str_at(args: &[XPathValue], index: usize) -> String {
    match args.get(index) {
        Some(v) => v.as_string(),
        None => String::new(),
    }
}

/// Create a text node with the given bytes and push it into a node-set.
fn push_text(ns: &mut NodeSet, bytes: &[u8]) {
    let mut buf = bytes.to_vec();
    buf.push(0);
    let node =
        unsafe { crate::xml::tree::new_text(buf.as_ptr() as *const crate::abi::types::xmlChar) };
    if !node.is_null() {
        ns.push(node);
    }
}

/// Register all `str:` functions.
pub fn register_all() {
    register("str:tokenize", tokenize_fn as ExsltFunction);
    register("str:split", split_fn as ExsltFunction);
    register("str:replace", replace_fn as ExsltFunction);
    register("str:padding", padding_fn as ExsltFunction);
    register("str:align", align_fn as ExsltFunction);
    register("str:concat", concat_fn as ExsltFunction);
    register("str:encode-uri", encode_uri_fn as ExsltFunction);
    register("str:decode-uri", decode_uri_fn as ExsltFunction);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::xpath::context::XPathContext;
    use core::ptr;

    fn ctx() -> XPathContext {
        XPathContext::new(ptr::null_mut())
    }

    #[test]
    fn test_tokenize() {
        let mut c = ctx();
        let r = tokenize_fn(&mut c, &[XPathValue::String("a b\tc".to_string())]).unwrap();
        let ns = r.as_node_set();
        let values: Vec<String> = ns.iter().map(|n| node_string_value(n)).collect();
        assert_eq!(values, vec!["a", "b", "c"]);
        for n in ns.iter() {
            unsafe { crate::xml::tree::free_node(n) };
        }
    }

    #[test]
    fn test_replace() {
        let mut c = ctx();
        let r = replace_fn(
            &mut c,
            &[
                XPathValue::String("hello world".to_string()),
                XPathValue::String("world".to_string()),
                XPathValue::String("there".to_string()),
            ],
        )
        .unwrap();
        assert_eq!(r.as_string(), "hello there");
    }

    #[test]
    fn test_padding() {
        let mut c = ctx();
        let r = padding_fn(
            &mut c,
            &[
                XPathValue::Number(5.0),
                XPathValue::String("ab".to_string()),
            ],
        )
        .unwrap();
        assert_eq!(r.as_string(), "aaaaa");
        let r = padding_fn(&mut c, &[XPathValue::Number(3.0)]).unwrap();
        assert_eq!(r.as_string(), "   ");
    }

    #[test]
    fn test_align() {
        let mut c = ctx();
        let r = align_fn(
            &mut c,
            &[
                XPathValue::String("ab".to_string()),
                XPathValue::String("     ".to_string()),
                XPathValue::String("right".to_string()),
            ],
        )
        .unwrap();
        assert_eq!(r.as_string(), "   ab");
        let r = align_fn(
            &mut c,
            &[
                XPathValue::String("ab".to_string()),
                XPathValue::String("     ".to_string()),
                XPathValue::String("center".to_string()),
            ],
        )
        .unwrap();
        assert_eq!(r.as_string(), "  ab ");
    }

    #[test]
    fn test_concat() {
        let a = unsafe {
            crate::xml::tree::new_text(b"x\0".as_ptr() as *const crate::abi::types::xmlChar)
        };
        let b = unsafe {
            crate::xml::tree::new_text(b"y\0".as_ptr() as *const crate::abi::types::xmlChar)
        };
        let mut ns = NodeSet::new();
        ns.push(a);
        ns.push(b);
        let mut c = ctx();
        let r = concat_fn(
            &mut c,
            &[XPathValue::String(",".to_string()), XPathValue::NodeSet(ns)],
        )
        .unwrap();
        assert_eq!(r.as_string(), "x,y");
        unsafe {
            crate::xml::tree::free_node(a);
            crate::xml::tree::free_node(b);
        }
    }

    #[test]
    fn test_encode_decode_uri() {
        let mut c = ctx();
        let r = encode_uri_fn(
            &mut c,
            &[
                XPathValue::String("a b/c".to_string()),
                XPathValue::String(String::new()),
            ],
        )
        .unwrap();
        assert_eq!(r.as_string(), "a%20b/c");
        let r = decode_uri_fn(&mut c, &[XPathValue::String("a%20b%2Fc".to_string())]).unwrap();
        assert_eq!(r.as_string(), "a b/c");
    }
}
