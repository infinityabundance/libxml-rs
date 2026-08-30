//! C ABI exports for the URI subsystem (`uri.h`) — family closure (11.1-I).
//!
//! This module implements the 11 URI entry points assigned to this
//! workstream:
//!
//! 1. `xmlBuildURI` / `xmlBuildURISafe`
//! 2. `xmlBuildRelativeURI` / `xmlBuildRelativeURISafe`
//! 3. `xmlCanonicPath`
//! 4. `xmlPathToURI`
//! 5. `xmlPrintURI`
//! 6. `xmlURIEscape`
//! 7. `xmlNormalizeWindowsPath`
//! 8. `xmlCheckLanguageID`
//! 9. `xmlParseURISafe`
//!
//! All functions follow the upstream `uri.c` / `xmlIO.c` / `parser.c`
//! implementations (libxml2 2.15.3, see `archaeology/libxml2-git`),
//! reusing the internal parser/resolver from `src/xml/uri/mod.rs`
//! (`parse_uri`, `build_uri`, `resolve_uri`, `normalize_uri_path`,
//! `xmlURIEscapeStr`, `xmlSaveUri`, `xmlParseURI`).
//!
//! All returned strings are allocated with `xmlMalloc` so C callers release
//! them with `xmlFree`, exactly as with upstream libxml2.
//!
//! The `*Safe` variants follow the upstream convention of returning an `int`
//! status code (0 = success, 1 = invalid argument/URI, -1 = allocation
//! failure) and storing the result through an out parameter.

#![allow(
    missing_docs,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals
)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use core::ffi::c_void;
use core::ptr;
use std::os::raw::{c_char, c_int};

use crate::abi::allocator;
use crate::abi::types::xmlChar;
use crate::xml::uri::{build_uri, parse_uri, resolve_uri, UriParts};

extern "C" {
    /// `size_t fwrite(const void *ptr, size_t size, size_t nmemb, FILE *stream);`
    ///
    /// `FILE*` is opaque to Rust, so the stream is carried as `*mut c_void`.
    fn fwrite(ptr: *const c_void, size: usize, nmemb: usize, stream: *mut c_void) -> usize;
}

// ── Internal helpers ─────────────────────────────────────────────────────────

/// View a NUL-terminated C string as a byte slice (empty when NULL).
///
/// # Safety
///
/// `p` must be a valid NUL-terminated string (or NULL) for the duration of
/// the returned slice.
unsafe fn cstr_bytes<'a>(p: *const c_char) -> &'a [u8] {
    if p.is_null() {
        return &[];
    }
    let len = unsafe { libc::strlen(p) };
    unsafe { core::slice::from_raw_parts(p as *const u8, len) }
}

/// Copy `bytes` into a fresh `xmlMalloc`'d NUL-terminated string.
///
/// Returns NULL on allocation failure. An empty `bytes` yields a valid
/// 1-byte NUL string (never NULL), matching `xmlStrdup("")`.
unsafe fn dup_c_str(bytes: &[u8]) -> *mut xmlChar {
    let len = bytes.len();
    let p = unsafe { allocator::xmlMalloc(len + 1) as *mut u8 };
    if p.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), p, len);
        *p.add(len) = 0;
    }
    p as *mut xmlChar
}

/// Escape `s` with `xmlURIEscapeStr` semantics (keep unreserved + '%' +
/// every byte in `list`), returning an `xmlMalloc`'d string.
///
/// Returns NULL on allocation failure.
unsafe fn escape_slice(s: &[u8], list: &[u8]) -> *mut xmlChar {
    let mut s_buf = s.to_vec();
    s_buf.push(0);
    let mut l_buf = list.to_vec();
    l_buf.push(0);
    unsafe {
        crate::xml::uri::xmlURIEscapeStr(
            s_buf.as_ptr() as *const xmlChar,
            l_buf.as_ptr() as *const xmlChar,
        )
    }
}

// ── RFC 3986 character grammar (upstream uri.c, with/without ALLOW_UNWISE) ──

/// `unreserved` per RFC 3986, plus the "unwise" set when `allow_unwise`
/// mirrors upstream `XML_URI_ALLOW_UNWISE` in `xmlIsUnreserved`.
fn v_unres(b: u8, allow_unwise: bool) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(b, b'-' | b'.' | b'_' | b'~')
        || (allow_unwise && matches!(b, b'{' | b'}' | b'|' | b'\\' | b'^' | b'[' | b']' | b'`'))
}

/// `sub-delims` per RFC 3986.
fn v_sub_delim(b: u8) -> bool {
    matches!(
        b,
        b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'='
    )
}

/// `pct-encoded = "%" HEXDIG HEXDIG` starting at position `i`.
fn v_pct(s: &[u8], i: usize) -> bool {
    i + 2 < s.len() && s[i] == b'%' && s[i + 1].is_ascii_hexdigit() && s[i + 2].is_ascii_hexdigit()
}

/// `pchar = unreserved / pct-encoded / sub-delims / ":" / "@"` at position `i`.
fn v_pchar(s: &[u8], i: usize, allow_unwise: bool) -> bool {
    v_unres(s[i], allow_unwise) || v_pct(s, i) || v_sub_delim(s[i]) || s[i] == b':' || s[i] == b'@'
}

/// Advance over a run of `pchar` bytes.
fn v_advance_pchar(s: &[u8], i: &mut usize, allow_unwise: bool) {
    while *i < s.len() && v_pchar(s, *i, allow_unwise) {
        if s[*i] == b'%' {
            *i += 3;
        } else {
            *i += 1;
        }
    }
}

/// `authority = [ userinfo "@" ] host [ ":" port ]`.
///
/// Mirrors the upstream character set: unreserved / pct-encoded /
/// sub-delims / ":" / "@", with `[...]` IP-literals accepted as a unit
/// (upstream `xmlParse3986IPLiteral`).
fn v_authority(s: &[u8], i: &mut usize, allow_unwise: bool) -> bool {
    while *i < s.len() && !matches!(s[*i], b'/' | b'?' | b'#') {
        let b = s[*i];
        if b == b'[' {
            if let Some(close) = s[*i + 1..].iter().position(|&c| c == b']') {
                *i += close + 2;
            } else {
                return false;
            }
        } else if v_unres(b, allow_unwise)
            || v_pct(s, *i)
            || v_sub_delim(b)
            || b == b':'
            || b == b'@'
        {
            if b == b'%' {
                *i += 3;
            } else {
                *i += 1;
            }
        } else {
            return false;
        }
    }
    true
}

/// `path-abempty = *( "/" segment )` — empty segments allowed.
fn v_path_abempty(s: &[u8], i: &mut usize, allow_unwise: bool) -> bool {
    while *i < s.len() && s[*i] == b'/' {
        *i += 1;
        v_advance_pchar(s, i, allow_unwise);
    }
    true
}

/// `path-absolute = "/" [ segment-nz *( "/" segment ) ]`, with `i` just
/// past the leading "/".
fn v_path_absolute(s: &[u8], i: &mut usize, allow_unwise: bool) -> bool {
    if *i < s.len() && !matches!(s[*i], b'?' | b'#') {
        if !v_pchar(s, *i, allow_unwise) {
            return false;
        }
        v_advance_pchar(s, i, allow_unwise);
    }
    v_path_abempty(s, i, allow_unwise)
}

/// `path-rootless = segment-nz *( "/" segment )`.
fn v_path_rootless(s: &[u8], i: &mut usize, allow_unwise: bool) -> bool {
    if *i >= s.len() || !v_pchar(s, *i, allow_unwise) {
        return false;
    }
    v_advance_pchar(s, i, allow_unwise);
    v_path_abempty(s, i, allow_unwise)
}

/// `path-noscheme = segment-nz-nc *( "/" segment )` — the first segment
/// must not contain ":".
fn v_path_noscheme(s: &[u8], i: &mut usize, allow_unwise: bool) -> bool {
    if *i >= s.len()
        || !(v_unres(s[*i], allow_unwise) || v_pct(s, *i) || v_sub_delim(s[*i]) || s[*i] == b'@')
    {
        return false;
    }
    while *i < s.len()
        && (v_unres(s[*i], allow_unwise) || v_pct(s, *i) || v_sub_delim(s[*i]) || s[*i] == b'@')
    {
        if s[*i] == b'%' {
            *i += 3;
        } else {
            *i += 1;
        }
    }
    v_path_abempty(s, i, allow_unwise)
}

/// Validate a URI reference against the upstream RFC 3986 grammar
/// (`xmlParse3986URIReference`, uri.c).
///
/// `allow_unwise` mirrors parsing with `XML_URI_ALLOW_UNWISE` set, which is
/// exactly how `xmlURIEscape` parses its input. `xmlParseURISafe` and the
/// `xmlBuildURI*` family parse without it.
fn uri_reference_valid(s: &[u8], allow_unwise: bool) -> bool {
    let n = s.len();
    let mut i = 0usize;
    let mut has_scheme = false;

    // scheme = ALPHA *( ALPHA / DIGIT / "+" / "-" / "." ) ":"
    if i < n && s[i].is_ascii_alphabetic() {
        let mut j = i;
        while j < n && (s[j].is_ascii_alphanumeric() || matches!(s[j], b'+' | b'-' | b'.')) {
            j += 1;
        }
        if j < n && s[j] == b':' {
            has_scheme = true;
            i = j + 1;
        }
    }

    if has_scheme {
        // hier-part
        if i + 1 < n && s[i] == b'/' && s[i + 1] == b'/' {
            i += 2;
            if !v_authority(s, &mut i, allow_unwise) || !v_path_abempty(s, &mut i, allow_unwise) {
                return false;
            }
        } else if i < n && s[i] == b'/' {
            i += 1;
            if !v_path_absolute(s, &mut i, allow_unwise) {
                return false;
            }
        } else if i < n && v_pchar(s, i, allow_unwise) {
            if !v_path_rootless(s, &mut i, allow_unwise) {
                return false;
            }
        }
    } else {
        // relative-ref
        if i + 1 < n && s[i] == b'/' && s[i + 1] == b'/' {
            i += 2;
            if !v_authority(s, &mut i, allow_unwise) || !v_path_abempty(s, &mut i, allow_unwise) {
                return false;
            }
        } else if i < n && s[i] == b'/' {
            i += 1;
            if !v_path_absolute(s, &mut i, allow_unwise) {
                return false;
            }
        } else if i < n && v_pchar(s, i, allow_unwise) {
            if !v_path_noscheme(s, &mut i, allow_unwise) {
                return false;
            }
        }
    }

    // [ "?" query ]
    if i < n && s[i] == b'?' {
        i += 1;
        while i < n && (v_pchar(s, i, allow_unwise) || matches!(s[i], b'/' | b'?')) {
            if s[i] == b'%' {
                i += 3;
            } else {
                i += 1;
            }
        }
    }
    // [ "#" fragment ]
    if i < n && s[i] == b'#' {
        i += 1;
        while i < n && (v_pchar(s, i, allow_unwise) || matches!(s[i], b'/' | b'?')) {
            if s[i] == b'%' {
                i += 3;
            } else {
                i += 1;
            }
        }
    }
    i == n
}

/// Port of upstream `xmlNormalizePath` (uri.c), a filesystem path
/// normalizer: collapses "./" and extra separators, resolves ".." segments,
/// keeps a leading "../" on relative paths and a trailing "/" or ".".
///
/// On Linux the only effect of `is_file` is keeping "." when the result
/// would otherwise be empty.
fn normalize_path(path: &[u8], is_file: bool) -> Vec<u8> {
    if path.is_empty() {
        return Vec::new();
    }
    let is_sep = |c: u8| c == b'/';
    let mut out: Vec<u8> = Vec::with_capacity(path.len());
    let mut cur = 0usize;
    let mut num_seg: i64 = 0;

    if is_sep(path[0]) {
        cur += 1;
        out.push(b'/');
    }

    while cur < path.len() {
        // Collapse multiple separators.
        while cur < path.len() && is_sep(path[cur]) {
            cur += 1;
        }
        if cur >= path.len() {
            break;
        }

        if path[cur] == b'.' {
            if cur + 1 >= path.len() {
                // "." at end of path → ignore.
                break;
            } else if is_sep(path[cur + 1]) {
                // Skip "./".
                cur += 2;
                continue;
            } else if path[cur + 1] == b'.' && (cur + 2 >= path.len() || is_sep(path[cur + 2])) {
                if num_seg > 0 {
                    // Remove the last segment and its trailing separator:
                    // the C code backs `out` up past the separator, then
                    // past the segment, stopping at the previous '/'.
                    out.pop(); // the separator after the segment
                    while !out.is_empty() && out.last() != Some(&b'/') {
                        out.pop(); // the segment itself
                    }
                    num_seg -= 1;
                    if cur + 2 >= path.len() {
                        break;
                    }
                    cur += 3;
                    continue;
                } else if path.get(out.len()).copied() == Some(b'/') {
                    // Ignore extraneous ".." in absolute paths.
                    if cur + 2 >= path.len() {
                        break;
                    }
                    cur += 3;
                    continue;
                } else {
                    // Keep "../" at the start of relative paths.
                    num_seg -= 1;
                }
            }
        }

        // Copy segment.
        while cur < path.len() && !is_sep(path[cur]) {
            out.push(path[cur]);
            cur += 1;
        }
        // Copy separator.
        if cur < path.len() {
            cur += 1;
            out.push(b'/');
        }
        num_seg += 1;
    }

    // Keep "." if the output is empty and it's a file.
    if is_file && out.is_empty() {
        out.push(b'.');
    }
    out
}

// ── xmlSaveUri-style serialization (upstream escaping rules) ─────────────────

/// `IS_UNRESERVED` from upstream uri.c: ALPHANUM + mark
/// (`- _ . ! ~ * ' ( )`).
fn save_unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(
            b,
            b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
        )
}

/// `IS_RESERVED` from upstream uri.c: `; / ? : @ & = + $ , [ ]`.
fn save_reserved(b: u8) -> bool {
    matches!(
        b,
        b';' | b'/' | b'?' | b':' | b'@' | b'&' | b'=' | b'+' | b'$' | b',' | b'[' | b']'
    )
}

fn push_escaped(out: &mut Vec<u8>, b: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.push(b'%');
    out.push(HEX[(b >> 4) as usize]);
    out.push(HEX[(b & 0x0f) as usize]);
}

/// Escape `s` into `out`, keeping bytes for which `keep` returns true.
fn save_escape(out: &mut Vec<u8>, s: &[u8], keep: impl Fn(u8) -> bool) {
    for &b in s {
        if keep(b) {
            out.push(b);
        } else {
            push_escaped(out, b);
        }
    }
}

/// Serialize parsed URI parts with the exact escaping of upstream
/// `xmlSaveUri` (uri.c) — used for the "return the URI itself" tail of
/// `xmlBuildRelativeURISafe`.
fn save_uri_parts(p: &UriParts) -> Vec<u8> {
    let mut out = Vec::new();

    if let Some(scheme) = p.scheme.as_deref() {
        out.extend_from_slice(scheme);
        out.push(b':');
    }

    if let Some(opaque) = p.opaque.as_deref() {
        // Kept: IS_UNRESERVED || IS_RESERVED.
        save_escape(&mut out, opaque, |b| save_unreserved(b) || save_reserved(b));
    } else if p.server.is_some() || p.port != 0 {
        out.extend_from_slice(b"//");
        if let Some(user) = p.user.as_deref() {
            // Kept: IS_UNRESERVED || ";:&=+$,."
            save_escape(&mut out, user, |b| {
                save_unreserved(b) || matches!(b, b';' | b':' | b'&' | b'=' | b'+' | b'$' | b',')
            });
            out.push(b'@');
        }
        if let Some(server) = p.server.as_deref() {
            // The internal representation already includes the port.
            out.extend_from_slice(server);
        }
    } else if let Some(authority) = p.authority.as_deref() {
        out.extend_from_slice(b"//");
        // Kept: IS_UNRESERVED || "$,;:@&=+."
        save_escape(&mut out, authority, |b| {
            save_unreserved(b) || matches!(b, b'$' | b',' | b';' | b':' | b'@' | b'&' | b'=' | b'+')
        });
    }

    if let Some(path) = p.path.as_deref() {
        // The colon in file:///d: must not be escaped (upstream special
        // case), or Windows accesses fail later.
        let mut path = path;
        if p.scheme.as_deref() == Some(b"file")
            && path.starts_with(b"/")
            && path.len() >= 3
            && path[1].is_ascii_alphabetic()
            && path[2] == b':'
        {
            out.extend_from_slice(&path[..3]);
            path = &path[3..];
        }
        // Kept: IS_UNRESERVED || "/;@&=+$,."
        save_escape(&mut out, path, |b| {
            save_unreserved(b) || matches!(b, b'/' | b';' | b'@' | b'&' | b'=' | b'+' | b'$' | b',')
        });
    }

    // The internal parser does not track query_raw, so the escaped query
    // form is used, as upstream does when query_raw is NULL.
    if let Some(query) = p.query.as_deref() {
        out.push(b'?');
        save_escape(&mut out, query, |b| save_unreserved(b) || save_reserved(b));
    }
    if let Some(fragment) = p.fragment.as_deref() {
        out.push(b'#');
        save_escape(&mut out, fragment, |b| {
            save_unreserved(b) || save_reserved(b)
        });
    }

    out
}

// ── xmlBuildURI / xmlBuildURISafe ────────────────────────────────────────────

/// Resolve a URI reference against a base URI (safe variant).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlBuildURISafe(const xmlChar *URI, const xmlChar *base, xmlChar **out);
/// ```
///
/// Implements RFC 3986 §5.2 resolution via the internal `resolve_uri` +
/// `build_uri`, matching upstream `xmlBuildURISafe` (uri.c):
///
/// - `out == NULL` → returns 1
/// - `URI == NULL` → returns 1
/// - `base == NULL` → `*out` is a copy of `URI` (returns 0)
/// - a non-empty `URI` must parse as a URI reference, otherwise 1
/// - an absolute `URI` (with a scheme) is returned unchanged
/// - a `base` without "://" is treated as a filesystem path
/// - an invalid `base` with "://" yields `*out` = the URI itself (0)
/// - an empty `URI` resolves to `base` with the base fragment ignored
///
/// Returns 0 on success, 1 if the URI/base is invalid, -1 on allocation
/// failure. On failure `*out` is left NULL.
///
/// # Safety
///
/// `URI`/`base` must be valid NUL-terminated strings or NULL; `out` must be
/// a valid pointer to a writable `xmlChar*`.
#[no_mangle]
pub unsafe extern "C" fn xmlBuildURISafe(
    uri: *const c_char,
    base: *const c_char,
    out: *mut *mut xmlChar,
) -> c_int {
    unsafe {
        if out.is_null() {
            return 1;
        }
        *out = ptr::null_mut();
        if uri.is_null() {
            return 1;
        }
        let uri_bytes = cstr_bytes(uri);

        if base.is_null() {
            // Upstream: base == NULL → val = xmlStrdup(URI).
            *out = dup_c_str(uri_bytes);
            return if (*out).is_null() { -1 } else { 0 };
        }

        let base_bytes = cstr_bytes(base);

        // Upstream parses the URI first (strictly); an invalid URI fails
        // regardless of the base.
        if !uri_bytes.is_empty() && !uri_reference_valid(uri_bytes, false) {
            return 1;
        }

        // Base without "://": treated as a filesystem path
        // (upstream xmlResolvePath) — approximated by the internal resolver.
        if !base_bytes.windows(3).any(|w| w == b"://") {
            let resolved = match resolve_uri(base_bytes, uri_bytes) {
                Some(v) => v,
                None => return 1,
            };
            *out = dup_c_str(&resolved);
            return if (*out).is_null() { -1 } else { 0 };
        }

        // Base with "://": full RFC 3986 merge.
        if uri_bytes.is_empty() {
            // Empty reference: upstream returns the base with the base
            // fragment ignored; an invalid base yields 1 with NULL.
            if !uri_reference_valid(base_bytes, false) {
                return 1;
            }
            let mut base_parts = match parse_uri(base_bytes) {
                Some(p) => p,
                None => return 1,
            };
            base_parts.fragment = None;
            let resolved = build_uri(&base_parts);
            *out = dup_c_str(&resolved);
            return if (*out).is_null() { -1 } else { 0 };
        }

        let ref_parts = match parse_uri(uri_bytes) {
            Some(p) => p,
            None => return 1,
        };
        if ref_parts.scheme.is_some() {
            // The URI is absolute — don't modify.
            *out = dup_c_str(uri_bytes);
            return if (*out).is_null() { -1 } else { 0 };
        }

        if !uri_reference_valid(base_bytes, false) {
            // Invalid base: upstream returns the URI itself with ret 0.
            let saved = build_uri(&ref_parts);
            *out = dup_c_str(&saved);
            return if (*out).is_null() { -1 } else { 0 };
        }

        // RFC 3986 §5.2.2: a reference with an empty path ("?query" or
        // "#fragment" only) inherits the base path; the base fragment is
        // ignored and the reference query (or the base query) applies.
        let resolved = if uri_bytes.starts_with(b"?") || uri_bytes.starts_with(b"#") {
            let mut base_parts = match parse_uri(base_bytes) {
                Some(p) => p,
                None => return 1,
            };
            let ref2 = match parse_uri(uri_bytes) {
                Some(p) => p,
                None => return 1,
            };
            if ref2.query.is_some() {
                base_parts.query = ref2.query;
            }
            base_parts.fragment = ref2.fragment;
            build_uri(&base_parts)
        } else {
            match resolve_uri(base_bytes, uri_bytes) {
                Some(v) => v,
                None => return 1,
            }
        };
        *out = dup_c_str(&resolved);
        if (*out).is_null() {
            -1
        } else {
            0
        }
    }
}

/// Resolve a URI reference against a base URI.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlBuildURI(const xmlChar *URI, const xmlChar *base);
/// ```
///
/// Returns the resolved URI (`xmlMalloc`'d, free with `xmlFree`) or NULL on
/// error. Both arguments may be NULL (then returns NULL); a NULL `base`
/// yields a copy of `URI`.
///
/// # Safety
///
/// `URI`/`base` must be valid NUL-terminated strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlBuildURI(uri: *const c_char, base: *const c_char) -> *mut xmlChar {
    let mut out: *mut xmlChar = ptr::null_mut();
    let ret = unsafe { xmlBuildURISafe(uri, base, &mut out) };
    if ret != 0 {
        return ptr::null_mut();
    }
    out
}

// ── xmlBuildRelativeURI / xmlBuildRelativeURISafe ───────────────────────────

/// Compute a relative URI from `URI` to `base` (safe variant).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlBuildRelativeURISafe(const xmlChar *URI, const xmlChar *base, xmlChar **out);
/// ```
///
/// Port of upstream `xmlBuildRelativeURISafe` (uri.c):
///
/// - `out == NULL` → returns 1
/// - `URI == NULL` or empty → returns 1
/// - if `URI` contains "://" it must parse as a URI reference; otherwise
///   `*out` is a copy of `URI` (0)
/// - strings without "://" are treated as filesystem paths (normalized)
/// - if `base` is NULL/empty/invalid, or the scheme/server/port differ,
///   `*out` is the URI itself
/// - identical paths yield the empty string; otherwise the shortest
///   relative reference (`../` groups plus the unique suffix) is computed
///
/// Returns 0 on success, 1 for invalid arguments, -1 on allocation failure.
/// On failure `*out` is left NULL.
///
/// # Safety
///
/// `URI`/`base` must be valid NUL-terminated strings or NULL; `out` must be
/// a valid pointer to a writable `xmlChar*`.
#[no_mangle]
pub unsafe extern "C" fn xmlBuildRelativeURISafe(
    uri: *const c_char,
    base: *const c_char,
    out: *mut *mut xmlChar,
) -> c_int {
    unsafe {
        if out.is_null() {
            return 1;
        }
        *out = ptr::null_mut();
        if uri.is_null() {
            return 1;
        }
        let uri_bytes = cstr_bytes(uri);
        if uri_bytes.is_empty() {
            return 1;
        }

        // Upstream xmlParseUriOrPath(URI, &ref): strings containing "://"
        // are parsed as URI references (invalid ones are returned as-is);
        // other strings are normalized filesystem paths.
        let mut ref_parts = match parse_uri_or_path(uri_bytes) {
            Ok(p) => p,
            Err(raw) => {
                *out = dup_c_str(&raw);
                return if (*out).is_null() { -1 } else { 0 };
            }
        };

        let mut val: *mut xmlChar = ptr::null_mut();
        let mut ret: c_int = 0;

        // "Return URI if base is empty" (base == NULL || base[0] == 0).
        let base_empty = base.is_null() || cstr_bytes(base).is_empty();
        if !base_empty {
            let base_bytes = cstr_bytes(base);
            match parse_uri_or_path(base_bytes) {
                Ok(base_parts) => {
                    if ref_parts.scheme != base_parts.scheme
                        || ref_parts.server != base_parts.server
                        || ref_parts.port != base_parts.port
                    {
                        // Scheme/server/port differ → return the URI.
                        // val stays NULL; the common tail saves ref_parts.
                    } else if ref_parts.path == base_parts.path {
                        // Identical paths → empty relative reference.
                        val = dup_c_str(b"");
                        if val.is_null() {
                            ret = -1;
                        }
                    } else if base_parts.path.is_none() {
                        // Base has no path: the whole ref path is the suffix.
                        val = escape_slice(ref_parts.path.as_deref().unwrap_or(b""), b"/;&=+$,");
                        if val.is_null() {
                            ret = -1;
                        }
                    } else {
                        // ref->path is guaranteed non-NULL from here on
                        // (upstream replaces a NULL path with "/").
                        if ref_parts.path.is_none() {
                            ref_parts.path = Some(b"/".to_vec());
                        }
                        let b = base_parts.path.as_deref().unwrap();
                        let r = ref_parts.path.as_deref().unwrap();

                        // "Return URI if URI and base aren't both absolute
                        // or both relative."
                        if (b.first() == Some(&b'/')) != (r.first() == Some(&b'/')) {
                            // val stays NULL → common tail saves ref_parts.
                        } else {
                            // Find the first differing byte.
                            let mut pos = 0usize;
                            while pos < b.len() && pos < r.len() && b[pos] == r[pos] {
                                pos += 1;
                            }
                            if pos == b.len() && pos == r.len() {
                                // Paths are byte-identical → empty reference.
                                val = dup_c_str(b"");
                                if val.is_null() {
                                    ret = -1;
                                }
                            } else {
                                // Back up in ref to the last '/' before pos;
                                // uptr is the unique suffix of the ref path.
                                let mut ix = pos;
                                while ix > 0 {
                                    if r[ix - 1] == b'/' {
                                        break;
                                    }
                                    ix -= 1;
                                }
                                let uptr = &r[ix..];

                                // Count '/' in base starting at the same ix.
                                let mut nbslash = 0usize;
                                let mut i = ix;
                                while i < b.len() {
                                    if b[i] == b'/' {
                                        nbslash += 1;
                                    }
                                    i += 1;
                                }
                                let len = uptr.len() + 1;

                                if nbslash == 0 && uptr.is_empty() {
                                    // e.g. URI="foo/" base="foo/bar" → "./"
                                    val = dup_c_str(b"./");
                                    if val.is_null() {
                                        ret = -1;
                                    }
                                } else if nbslash == 0 {
                                    val = escape_slice(uptr, b"/;&=+$,");
                                    if val.is_null() {
                                        ret = -1;
                                    }
                                } else {
                                    let mut buf = Vec::with_capacity(len + 3 * nbslash);
                                    for _ in 0..nbslash {
                                        buf.extend_from_slice(b"../");
                                    }
                                    if !uptr.is_empty() {
                                        if buf.last() == Some(&b'/') && uptr[0] == b'/' {
                                            // Avoid "../" + "/suffix" doubling
                                            // the separator (upstream vptr fix).
                                            buf.extend_from_slice(&uptr[1..]);
                                        } else {
                                            buf.extend_from_slice(uptr);
                                        }
                                    }
                                    val = escape_slice(&buf, b"/;&=+$,");
                                    if val.is_null() {
                                        ret = -1;
                                    } else {
                                        ret = 0;
                                    }
                                }
                            }
                        }
                    }
                }
                Err(_raw) => {
                    // "Return URI if base is invalid": val stays NULL → the
                    // common tail saves ref_parts.
                }
            }
        }

        // done: if ret == 0 && val == NULL → val = xmlSaveUri(ref).
        if ret == 0 && val.is_null() {
            let saved = save_uri_parts(&ref_parts);
            val = dup_c_str(&saved);
            if val.is_null() {
                ret = -1;
            }
        }
        if ret != 0 {
            if !val.is_null() {
                allocator::xmlFree(val as *mut c_void);
            }
            val = ptr::null_mut();
        }
        *out = val;
        ret
    }
}

/// Upstream `xmlParseUriOrPath`: parse `s` as a URI reference when it
/// contains "://" (returning the raw string on parse failure), otherwise
/// normalize it as a filesystem path and parse that.
fn parse_uri_or_path(s: &[u8]) -> Result<UriParts, Vec<u8>> {
    if s.windows(3).any(|w| w == b"://") {
        if !uri_reference_valid(s, false) {
            return Err(s.to_vec());
        }
        let mut parts = parse_uri(s).ok_or_else(|| s.to_vec())?;
        // Upstream xmlParseUriOrPath also normalizes the parsed path
        // (xmlNormalizePath(uri->path, /* isFile */ 0)).
        if let Some(path) = parts.path.take() {
            parts.path = Some(normalize_path(&path, false));
        }
        Ok(parts)
    } else {
        let norm = normalize_path(s, true);
        parse_uri(&norm).ok_or_else(|| s.to_vec())
    }
}

/// Compute a relative URI from `URI` to `base`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlBuildRelativeURI(const xmlChar *URI, const xmlChar *base);
/// ```
///
/// Returns the relative URI (`xmlMalloc`'d, free with `xmlFree`) or NULL if
/// not possible.
///
/// # Safety
///
/// `URI`/`base` must be valid NUL-terminated strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlBuildRelativeURI(
    uri: *const c_char,
    base: *const c_char,
) -> *mut xmlChar {
    let mut out: *mut xmlChar = ptr::null_mut();
    let ret = unsafe { xmlBuildRelativeURISafe(uri, base, &mut out) };
    if ret != 0 {
        return ptr::null_mut();
    }
    out
}

// ── xmlCanonicPath / xmlPathToURI ────────────────────────────────────────────

/// Prepares a path: if it contains "://" it is treated as a Legacy Extended
/// IRI and every character not allowed in URIs is escaped; otherwise the
/// path is copied unmodified.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlCanonicPath(const xmlChar *path);
/// ```
///
/// Returns NULL if `path` is NULL.
///
/// # Safety
///
/// `path` must be a valid NUL-terminated string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlCanonicPath(path: *const c_char) -> *mut xmlChar {
    if path.is_null() {
        return ptr::null_mut();
    }
    let bytes = cstr_bytes(path);
    if bytes.windows(3).any(|w| w == b"://") {
        // "Absolute uri": escape everything except reserved, unreserved
        // and the percent sign (upstream xmlCanonicPath).
        unsafe { escape_slice(bytes, b":/?#[]@!$&()*+,;='%") }
    } else {
        unsafe { dup_c_str(bytes) }
    }
}

/// Construct a URI expressing the existing path.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlPathToURI(const xmlChar *path);
/// ```
///
/// Upstream `xmlPathToURI` is a thin wrapper around `xmlCanonicPath`
/// (2.15.x uri.c), so this returns the same canonicalized path.
///
/// # Safety
///
/// `path` must be a valid NUL-terminated string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlPathToURI(path: *const c_char) -> *mut xmlChar {
    unsafe { xmlCanonicPath(path) }
}

// ── xmlPrintURI ──────────────────────────────────────────────────────────────

/// Print the URI string to a `FILE*` stream.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlPrintURI(FILE *stream, xmlURI *uri);
/// ```
///
/// Serializes `uri` with `xmlSaveUri` and writes it to `stream` with
/// `fwrite` (the upstream equivalent of `fprintf(stream, "%s", out)`).
/// A NULL `uri` or NULL `stream` is a silent no-op.
///
/// # Safety
///
/// `stream` must be a valid open `FILE*` (or NULL); `uri` must be a valid
/// `xmlURI` from `xmlParseURI`/`xmlCreateURI` (or NULL).
#[no_mangle]
pub unsafe extern "C" fn xmlPrintURI(stream: *mut c_void, uri: *mut c_void) {
    if stream.is_null() {
        return;
    }
    let out = unsafe { crate::xml::uri::xmlSaveUri(uri) };
    if out.is_null() {
        return;
    }
    let len = unsafe { libc::strlen(out as *const c_char) };
    unsafe {
        fwrite(out as *const c_void, 1, len, stream);
        allocator::xmlFree(out as *mut c_void);
    }
}

// ── xmlURIEscape ─────────────────────────────────────────────────────────────

/// Escape a URI string per RFC 2396 (deprecated upstream).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlURIEscape(const xmlChar *str);
/// ```
///
/// Port of upstream `xmlURIEscape` (uri.c): the string is parsed with
/// `XML_URI_ALLOW_UNWISE` (so "unwise" characters are permitted) and each
/// component is re-escaped with its component-specific safe list. Strings
/// that don't parse as a URI reference (e.g. containing a space) yield NULL.
///
/// # Safety
///
/// `str` must be a valid NUL-terminated string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlURIEscape(str: *const c_char) -> *mut xmlChar {
    if str.is_null() {
        return ptr::null_mut();
    }
    let bytes = cstr_bytes(str);
    if !uri_reference_valid(bytes, true) {
        return ptr::null_mut();
    }
    let parts = match parse_uri(bytes) {
        Some(p) => p,
        None => return ptr::null_mut(),
    };

    let mut result: Vec<u8> = Vec::new();

    // Scheme (upstream safe list "+-.").
    if let Some(ref scheme) = parts.scheme {
        let esc = unsafe { escape_slice(scheme, b"+-.") };
        if esc.is_null() {
            return ptr::null_mut();
        }
        result.extend_from_slice(unsafe { cstr_bytes(esc as *const c_char) });
        result.push(b':');
        unsafe { allocator::xmlFree(esc as *mut c_void) };
    }

    // Note: the C struct's `authority` and `opaque` fields are never set by
    // the parser (upstream xmlParse3986HierPart), so those blocks of the
    // upstream xmlURIEscape are dead code for parsed URIs and are not
    // emitted here. `scheme:rest` parses as a rootless path upstream.

    // User info.
    if let Some(ref user) = parts.user {
        let esc = unsafe { escape_slice(user, b";:&=+$,") };
        if esc.is_null() {
            return ptr::null_mut();
        }
        result.extend_from_slice(b"//");
        result.extend_from_slice(unsafe { cstr_bytes(esc as *const c_char) });
        result.push(b'@');
        unsafe { allocator::xmlFree(esc as *mut c_void) };
    }

    // Server (host part only; the port is emitted separately below).
    if let Some(ref host) = parts.host {
        let esc = unsafe { escape_slice(host, b"/?;:@") };
        if esc.is_null() {
            return ptr::null_mut();
        }
        if parts.user.is_none() {
            result.extend_from_slice(b"//");
        }
        result.extend_from_slice(unsafe { cstr_bytes(esc as *const c_char) });
        unsafe { allocator::xmlFree(esc as *mut c_void) };
    }

    // Port.
    if parts.port > 0 {
        result.push(b':');
        result.extend_from_slice(format!("{}", parts.port).as_bytes());
    }

    // Path.
    if let Some(ref path) = parts.path {
        let esc = unsafe { escape_slice(path, b":@&=+$,/?;") };
        if esc.is_null() {
            return ptr::null_mut();
        }
        result.extend_from_slice(unsafe { cstr_bytes(esc as *const c_char) });
        unsafe { allocator::xmlFree(esc as *mut c_void) };
    }

    // Query. (The internal parser does not track query_raw, so the escaped
    // form is used, as upstream does when query_raw is NULL.)
    if let Some(ref query) = parts.query {
        let esc = unsafe { escape_slice(query, b";/?:@&=+,$") };
        if esc.is_null() {
            return ptr::null_mut();
        }
        result.push(b'?');
        result.extend_from_slice(unsafe { cstr_bytes(esc as *const c_char) });
        unsafe { allocator::xmlFree(esc as *mut c_void) };
    } else if bytes.ends_with(b"?") {
        // The internal parser drops an empty query; upstream keeps the "?".
        result.push(b'?');
    }

    // Fragment.
    if let Some(ref fragment) = parts.fragment {
        let esc = unsafe { escape_slice(fragment, b"#") };
        if esc.is_null() {
            return ptr::null_mut();
        }
        result.push(b'#');
        result.extend_from_slice(unsafe { cstr_bytes(esc as *const c_char) });
        unsafe { allocator::xmlFree(esc as *mut c_void) };
    }

    unsafe { dup_c_str(&result) }
}

// ── xmlNormalizeWindowsPath ──────────────────────────────────────────────────

/// Normalize a Windows path.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xmlNormalizeWindowsPath(const xmlChar *path);
/// ```
///
/// Upstream libxml2 (2.15.x, xmlIO.c) marks this function deprecated —
/// "This never really worked" — and simply returns a copy of `path`.
/// Returns NULL if `path` is NULL.
///
/// # Safety
///
/// `path` must be a valid NUL-terminated string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlNormalizeWindowsPath(path: *const c_char) -> *mut xmlChar {
    if path.is_null() {
        return ptr::null_mut();
    }
    unsafe { dup_c_str(cstr_bytes(path)) }
}

// ── xmlCheckLanguageID ───────────────────────────────────────────────────────

/// Check whether a string is a valid language ID per RFC 3066.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlCheckLanguageID(const xmlChar *lang);
/// ```
///
/// Faithful port of upstream `xmlCheckLanguageID` (parser.c): returns 1 for
/// valid tags (including the deprecated IANA/user "i-*" and "x-*" forms),
/// 0 otherwise, 0 for NULL.
///
/// # Safety
///
/// `lang` must be a valid NUL-terminated string or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlCheckLanguageID(lang: *const c_char) -> c_int {
    if lang.is_null() {
        return 0;
    }
    check_language_id(unsafe { cstr_bytes(lang) })
}

fn lang_is_alpha(c: u8) -> bool {
    c.is_ascii_alphabetic()
}

fn lang_is_digit(c: u8) -> bool {
    c.is_ascii_digit()
}

/// Byte at `i`, or 0 past the end (mirrors reading the C NUL terminator).
fn lang_byte(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// The "variant" label of upstream xmlCheckLanguageID: `nxt` is just past
/// the variant subtag. Extensions and private-use subtags are not checked.
fn lang_variant(s: &[u8], nxt: usize) -> c_int {
    match lang_byte(s, nxt) {
        0 => 1,
        b'-' => 1,
        _ => 0,
    }
}

/// The "region" label: `nxt` is just past the region subtag.
fn lang_region(s: &[u8], nxt: usize) -> c_int {
    match lang_byte(s, nxt) {
        0 => return 1,
        b'-' => {}
        _ => return 0,
    }
    let mut nxt = nxt + 1;
    let cur = nxt;
    while lang_is_alpha(lang_byte(s, nxt)) {
        nxt += 1;
    }
    if !(5..=8).contains(&(nxt - cur)) {
        return 0;
    }
    lang_variant(s, nxt)
}

/// The "region_m49" label: `nxt` points at the first digit.
fn lang_region_m49(s: &[u8], nxt: usize) -> c_int {
    if lang_is_digit(lang_byte(s, nxt + 1)) && lang_is_digit(lang_byte(s, nxt + 2)) {
        lang_region(s, nxt + 3)
    } else {
        0
    }
}

/// The "script" label: `nxt` is just past the script subtag.
fn lang_script(s: &[u8], nxt: usize) -> c_int {
    match lang_byte(s, nxt) {
        0 => return 1,
        b'-' => {}
        _ => return 0,
    }
    let mut nxt = nxt + 1;
    let cur = nxt;
    if lang_is_digit(lang_byte(s, nxt)) {
        return lang_region_m49(s, nxt);
    }
    while lang_is_alpha(lang_byte(s, nxt)) {
        nxt += 1;
    }
    let len = nxt - cur;
    if (5..=8).contains(&len) {
        return lang_variant(s, nxt);
    }
    if len != 2 {
        return 0;
    }
    lang_region(s, nxt)
}

/// Port of upstream `xmlCheckLanguageID` (parser.c) over a byte slice.
fn check_language_id(s: &[u8]) -> c_int {
    // Deprecated IANA/user codes: "i-...", "I-...", "x-...", "X-...".
    let c0 = lang_byte(s, 0);
    let c1 = lang_byte(s, 1);
    if (c0 == b'i' || c0 == b'I' || c0 == b'x' || c0 == b'X') && c1 == b'-' {
        let mut cur = 2usize;
        while lang_is_alpha(lang_byte(s, cur)) {
            cur += 1;
        }
        return if lang_byte(s, cur) == 0 { 1 } else { 0 };
    }

    // Primary language subtag.
    let mut nxt = 0usize;
    while lang_is_alpha(lang_byte(s, nxt)) {
        nxt += 1;
    }
    let primary_len = nxt;
    if primary_len >= 4 {
        // Reserved language codes: 4..=8 chars and must end the tag.
        if primary_len > 8 || lang_byte(s, nxt) != 0 {
            return 0;
        }
        return 1;
    }
    if primary_len < 2 {
        return 0;
    }
    // We got an ISO 639 code.
    match lang_byte(s, nxt) {
        0 => return 1,
        b'-' => {}
        _ => return 0,
    }
    nxt += 1;
    let cur = nxt;

    // Next subtag: extlang / script / region / variant.
    if lang_is_digit(lang_byte(s, nxt)) {
        return lang_region_m49(s, nxt);
    }
    while lang_is_alpha(lang_byte(s, nxt)) {
        nxt += 1;
    }
    let len = nxt - cur;
    if len == 4 {
        return lang_script(s, nxt);
    }
    if len == 2 {
        return lang_region(s, nxt);
    }
    if (5..=8).contains(&len) {
        return lang_variant(s, nxt);
    }
    if len != 3 {
        return 0;
    }
    // We parsed an extlang.
    match lang_byte(s, nxt) {
        0 => return 1,
        b'-' => {}
        _ => return 0,
    }
    nxt += 1;
    let cur = nxt;

    // Now script or region or variant.
    if lang_is_digit(lang_byte(s, nxt)) {
        return lang_region_m49(s, nxt);
    }
    while lang_is_alpha(lang_byte(s, nxt)) {
        nxt += 1;
    }
    let len = nxt - cur;
    if len == 2 {
        return lang_region(s, nxt);
    }
    if (5..=8).contains(&len) {
        return lang_variant(s, nxt);
    }
    if len != 4 {
        return 0;
    }
    // We parsed a script → falls into the "script" handling.
    lang_script(s, nxt)
}

// ── xmlParseURISafe ──────────────────────────────────────────────────────────

/// Parse a URI reference (safe variant).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlParseURISafe(const char *str, xmlURI **uri);
/// ```
///
/// Returns 0 on success with `*uri` set to a newly created `xmlURI` (free
/// with `xmlFreeURI`), 1 if `str` is NULL/invalid or `uri` is NULL, -1 on
/// allocation failure. On failure `*uri` is left NULL. An empty string is
/// a valid (empty) URI reference.
///
/// # Safety
///
/// `str` must be a valid NUL-terminated string or NULL; `uri` must be a
/// valid pointer to a writable `xmlURI*`.
#[no_mangle]
pub unsafe extern "C" fn xmlParseURISafe(str: *const c_char, uri_out: *mut *mut c_void) -> c_int {
    unsafe {
        if uri_out.is_null() {
            return 1;
        }
        *uri_out = ptr::null_mut();
        if str.is_null() {
            return 1;
        }
        let bytes = cstr_bytes(str);
        if bytes.is_empty() {
            // The empty reference parses to an empty xmlURI upstream.
            *uri_out = crate::xml::uri::xmlCreateURI();
            return if (*uri_out).is_null() { -1 } else { 0 };
        }
        if !uri_reference_valid(bytes, false) {
            return 1;
        }
        let parsed = crate::xml::uri::xmlParseURI(str);
        if parsed.is_null() {
            return 1;
        }
        *uri_out = parsed;
        0
    }
}
