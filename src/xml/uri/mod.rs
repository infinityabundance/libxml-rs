//! URI/IRI handling (§26, §85 Phase 4).
//!
//! libxml2's own URI parser implementation. NOT a general URI library —
//! parity of malformed-URI handling is UPSTREAM_QUIRK territory.
//!
//! This module implements libxml2's URI parsing subsystem in native Rust,
//! covering URI parsing, normalization, resolution, and C ABI-compatible
//! wrapper functions.
//!
//! # Upstream contract
//!
//! Mirrors upstream `uri.c` (`SRC-LIBXML2-2.15.0-URI-C`, parity target
//! libxml2 2.15.3 oracle); the parse/resolve/build surface follows
//! RFC3986/RFC3987 but keeps libxml2 permissive parsing of malformed URIs.
//! `xmlParseURI` returns a `_xmlURI` C-layout object (R-000132 fixed a
//! non-C-layout UriParts object being handed to C callers as xmlURIPtr;
//! R-000133 closed the declared-but-unexported header functions).
//!
//! # Conceptual behavior
//!
//! The parser tokenizes scheme, authority (userinfo@host:port), path,
//! query and fragment with libxml2 character classification (unreserved /
//! reserved / gen-delim / sub-delim), then percent-decodes per component.
//! Normalization and resolution (`xmlBuildURI`, `xmlNormalizeURIPath`)
//! implement the RFC3986 reference-resolution algorithm with the upstream
//! quirks retained.
//!
//! # Ownership & safety invariants
//!
//! `xmlParseURI` returns an object the caller frees with `xmlFreeURI`
//! (caller frees); the struct owns its component strings. Internal parse
//! state is stack-local — no global tables, so no locking is required.
//!
//! # Historical quirks & epochs
//!
//! The permissive malformed-URI behavior is a long-standing upstream quirk;
//! the canonicalization matrix case `c14n` is byte-identical across the
//! whole 2.7.8 → 2.15.3 span (SEMANTIC_EPOCHS.md stable cases), and
//! `xmlC14NExecute` enforces the absolute-URI rule this module parses
//! (R-000166).
//!
//! # Deliberate oddities
//!
//! Malformed URIs are accepted where a strict RFC3986 parser would reject
//! them (upstream quirk, e.g. missing scheme, stray percent signs) — this
//! module is deliberately NOT a general URI library.
//!
//! # Proving courts
//!
//! Exercised by the C14N court family (relative-URI rejection byte-identical
//! in both inclusive and exclusive modes, R-000166), the CLI differential
//! courts and cargo test URI round-trip suites.
//!
//! # Tempting simplifications that would break parity
//!
//! Do not replace the hand-written parser with a strict RFC3986 crate:
//! consumers feed it already-escaped or partially-escaped URLs that
//! libxml2 tolerates, and rejection would change observable behavior. Do
//! not return a Rust struct as xmlURIPtr: R-000132 proved the C-layout
//! contract the header promises.

#![allow(clippy::missing_safety_doc)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use crate::abi::allocator;
use crate::abi::types::*;
use core::ffi::c_void;
use core::ptr;
use std::os::raw::{c_char, c_int};

// ── URI character classification ────────────────────────────────────────────

/// Check if a byte is a valid URI unreserved character.
/// Unreserved characters are: ALPHA, DIGIT, '-', '.', '_', '~'
const fn is_unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'_' || b == b'~'
}

/// Check if a byte is a valid URI reserved character.
/// Reserved characters are gen-delims and sub-delims.
#[allow(dead_code)]
const fn is_reserved(b: u8) -> bool {
    is_gen_delim(b) || is_sub_delim(b)
}

/// Check if a byte is a valid URI scheme character.
/// Scheme characters are: ALPHA, DIGIT, '+', '-', '.'
const fn is_scheme_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.'
}

/// Check if a byte is a URI gen-delimiter.
#[allow(dead_code)]
const fn is_gen_delim(b: u8) -> bool {
    matches!(b, b':' | b'/' | b'?' | b'#' | b'[' | b']' | b'@')
}

/// Check if a byte is a URI sub-delimiter.
#[allow(dead_code)]
const fn is_sub_delim(b: u8) -> bool {
    matches!(
        b,
        b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'='
    )
}

// ── Percent encoding / decoding ─────────────────────────────────────────────

/// Decode a hex digit character to its numeric value.
const fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Percent-decode a URI component in-place.
/// Returns a new `Vec<u8>` with percent-encoded sequences decoded.
fn percent_decode(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i] == b'%' && i + 2 < data.len() {
            if let Some(h) = hex_val(data[i + 1]) {
                if let Some(l) = hex_val(data[i + 2]) {
                    result.push((h << 4) | l);
                    i += 3;
                    continue;
                }
            }
        }
        result.push(data[i]);
        i += 1;
    }
    result
}

/// Percent-encode a byte for use in a URI.
/// Returns a new `Vec<u8>` with non-unreserved/non-reserved bytes percent-encoded.
#[allow(dead_code)]
fn percent_encode(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(data.len());
    for &b in data {
        if is_unreserved(b) || is_reserved(b) || b == b'%' {
            result.push(b);
        } else {
            result.extend_from_slice(format!("%{:02X}", b).as_bytes());
        }
    }
    result
}

// ── URI structure ───────────────────────────────────────────────────────────

/// Parsed URI components.
///
/// This is the internal Rust representation, corresponding to libxml2's
/// `xmlURI` struct but using safe Rust types.
#[derive(Debug, Clone, Default)]
pub(crate) struct UriParts {
    pub scheme: Option<Vec<u8>>,    // e.g., "http", "file"
    pub opaque: Option<Vec<u8>>,    // opaque part (for non-hierarchical URIs)
    pub authority: Option<Vec<u8>>, // e.g., "user@host:port"
    pub server: Option<Vec<u8>>,    // server part of authority
    pub user: Option<Vec<u8>>,      // user info
    pub host: Option<Vec<u8>>,      // host
    pub port: c_int,                // port number (0 = not specified)
    pub path: Option<Vec<u8>>,      // path (e.g., "/dir/file.xml")
    pub query: Option<Vec<u8>>,     // query string (after ?)
    pub fragment: Option<Vec<u8>>,  // fragment (after #)
    pub path_raw: Option<Vec<u8>>,  // raw (un-escaped) path
    #[allow(dead_code)]
    pub clean_path: Option<Vec<u8>>, // cleaned/normalized path
}

// ═══════════════════════════════════════════════════════════════════════════════
// C-ABI URI object (struct _xmlURI layout)
// ═══════════════════════════════════════════════════════════════════════════════
//
// The public `xmlURIPtr` returned by `xmlParseURI`/`xmlCreateURI` must be
// readable by C consumers as `struct _xmlURI` (upstream uri.h):
//
// ```c
// struct _xmlURI {
//     char *scheme;     char *opaque;   char *authority;
//     char *server;     char *user;     int port;
//     char *path;       char *query;    char *fragment;
//     int  cleanup;     char *query_raw;
// };
// ```
//
// sizeof == 104, _Alignof == 8 on x86-64 (verified by the ABI probe). The
// object is allocated as a `Box<CXmlUri>`; every string field is an
// allocator-owned (`xmlMalloc`) null-terminated copy, so C code may read and
// (with `xmlFreeURI`) release them exactly as with upstream libxml2.
//
// Internal Rust-only fields (`host`, `path_raw`, `clean_path`) cannot be
// represented in the C struct; they are kept in the internal [`UriParts`]
// only. Conversions are lossless for the C-visible fields.

#[repr(C)]
struct CXmlUri {
    scheme: *mut c_char,
    opaque: *mut c_char,
    authority: *mut c_char,
    server: *mut c_char,
    user: *mut c_char,
    port: c_int,
    path: *mut c_char,
    query: *mut c_char,
    fragment: *mut c_char,
    cleanup: c_int,
    query_raw: *mut c_char,
}

impl Default for CXmlUri {
    fn default() -> Self {
        CXmlUri {
            scheme: ptr::null_mut(),
            opaque: ptr::null_mut(),
            authority: ptr::null_mut(),
            server: ptr::null_mut(),
            user: ptr::null_mut(),
            port: 0,
            path: ptr::null_mut(),
            query: ptr::null_mut(),
            fragment: ptr::null_mut(),
            cleanup: 0,
            query_raw: ptr::null_mut(),
        }
    }
}

/// Allocate an allocator-owned null-terminated copy of `bytes`, or NULL.
///
/// # Safety
///
/// - `bytes` must be a valid slice readable for `b.len()` bytes; the
///   returned pointer is allocated with the libxml2 allocator and must be
///   released with `xmlFreeImpl`, or is NULL when `bytes` is empty or the
///   allocation fails.
unsafe fn to_c_str(bytes: Option<&[u8]>) -> *mut c_char {
    let b = match bytes {
        Some(b) if !b.is_empty() => b,
        _ => return ptr::null_mut(),
    };
    let p = unsafe { allocator::xmlMallocImpl(b.len() + 1) as *mut u8 };
    if p.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::copy_nonoverlapping(b.as_ptr(), p, b.len());
        *p.add(b.len()) = 0;
    }
    p as *mut c_char
}

/// Read an allocator-owned C string back into `Vec<u8>` (empty when NULL).
///
/// # Safety
///
/// - `p` must be NULL or a valid pointer to a NUL-terminated C string that
///   stays valid and unmodified for the duration of the call; the bytes are
///   copied out, so the caller keeps ownership of the string.
unsafe fn from_c_str(p: *const c_char) -> Option<Vec<u8>> {
    if p.is_null() {
        return None;
    }
    let len = unsafe { libc::strlen(p) };
    if len == 0 {
        return None;
    }
    let slice = unsafe { core::slice::from_raw_parts(p as *const u8, len) };
    Some(slice.to_vec())
}

/// Free a C-ABI URI object and all its strings.
///
/// # Safety
///
/// - `uri` must be NULL or a pointer previously produced by `parts_to_c`
///   (a `Box<CXmlUri>`); every non-NULL string field must have been
///   allocated with `xmlMallocImpl`. After the call the pointer and its
///   strings are dangling and must not be used or freed again.
unsafe fn free_c_uri(uri: *mut CXmlUri) {
    if uri.is_null() {
        return;
    }
    unsafe {
        let u = &*uri;
        for p in [
            u.scheme,
            u.opaque,
            u.authority,
            u.server,
            u.user,
            u.path,
            u.query,
            u.fragment,
            u.query_raw,
        ] {
            if !p.is_null() {
                allocator::xmlFreeImpl(p as *mut c_void);
            }
        }
        drop(Box::from_raw(uri));
    }
}

/// Convert internal parts to a C-ABI URI object (allocates).
///
/// # Safety
///
/// - `parts` must be a valid `UriParts` reference; the returned pointer
///   owns a `Box<CXmlUri>` whose string fields are `xmlMallocImpl`
///   allocations, and must be released with `free_c_uri` exactly once.
unsafe fn parts_to_c(parts: &UriParts) -> *mut CXmlUri {
    let boxed = Box::new(CXmlUri {
        scheme: unsafe { to_c_str(parts.scheme.as_deref()) },
        opaque: unsafe { to_c_str(parts.opaque.as_deref()) },
        authority: unsafe { to_c_str(parts.authority.as_deref()) },
        server: unsafe { to_c_str(parts.server.as_deref()) },
        user: unsafe { to_c_str(parts.user.as_deref()) },
        port: parts.port,
        path: unsafe { to_c_str(parts.path.as_deref()) },
        query: unsafe { to_c_str(parts.query.as_deref()) },
        fragment: unsafe { to_c_str(parts.fragment.as_deref()) },
        cleanup: 0,
        query_raw: unsafe { to_c_str(parts.query.as_deref()) },
    });
    Box::into_raw(boxed)
}

/// Convert a C-ABI URI object back to internal parts (copies strings).
///
/// # Safety
///
/// - `uri` must be non-NULL and point to a valid, initialized `CXmlUri`
///   (for example one produced by `xmlParseURI` or `parts_to_c`) whose
///   string fields are NULL or valid NUL-terminated strings; the object
///   must stay alive for the duration of the call since the fields are
///   copied out.
unsafe fn c_to_parts(uri: *const CXmlUri) -> UriParts {
    let u = unsafe { &*uri };
    UriParts {
        scheme: unsafe { from_c_str(u.scheme) },
        opaque: unsafe { from_c_str(u.opaque) },
        authority: unsafe { from_c_str(u.authority) },
        server: unsafe { from_c_str(u.server) },
        user: unsafe { from_c_str(u.user) },
        host: None,
        port: u.port,
        path: unsafe { from_c_str(u.path) },
        query: unsafe { from_c_str(u.query) },
        fragment: unsafe { from_c_str(u.fragment) },
        path_raw: None,
        clean_path: None,
    }
}

// ── URI parsing ─────────────────────────────────────────────────────────────

/// Find the scheme in a URI string.
/// Returns `(start_of_scheme, end_of_scheme)` if found.
/// The scheme must start with a letter and be followed by "://" or ":" (non-hierarchical).
const fn find_scheme(uri: &[u8]) -> Option<(usize, usize)> {
    if uri.is_empty() {
        return None;
    }
    // Scheme must start with a letter
    if !uri[0].is_ascii_alphabetic() {
        return None;
    }
    // Scan for ':' or end
    let mut i = 1;
    while i < uri.len() && is_scheme_char(uri[i]) {
        i += 1;
    }
    if i < uri.len() && uri[i] == b':' {
        // Check if it's "://" (hierarchical) or just ":" (opaque)
        Some((0, i))
    } else {
        None
    }
}

/// Parse the authority part of a URI.
/// Input is the authority string (e.g., "user@host:port").
/// Returns (user, host, port).
fn parse_authority(auth: &[u8]) -> (Option<Vec<u8>>, Option<Vec<u8>>, c_int) {
    let mut user: Option<Vec<u8>> = None;
    let mut host: Option<Vec<u8>> = None;
    let mut port: c_int = 0;

    if auth.is_empty() {
        return (None, None, 0);
    }

    // Split on '@' for user info
    let (user_part, _host_part) = if let Some(at_pos) = auth.iter().position(|&b| b == b'@') {
        user = Some(auth[..at_pos].to_vec());
        (&auth[at_pos + 1..], true)
    } else {
        (auth, false)
    };

    // The remaining part is host:port
    // Check for IPv6 literal [::1]
    if user_part.starts_with(b"[") {
        // Find the closing bracket
        if let Some(close_bracket) = user_part.iter().position(|&b| b == b']') {
            let host_end = close_bracket + 1;
            host = Some(user_part[..host_end].to_vec());
            // Check for port after the closing bracket
            if host_end < user_part.len() && user_part[host_end] == b':' {
                let port_str = &user_part[host_end + 1..];
                if !port_str.is_empty() {
                    let port_str_decoded = core::str::from_utf8(port_str).unwrap_or("");
                    port = port_str_decoded.parse::<c_int>().unwrap_or(0);
                }
            }
        } else {
            // No closing bracket, take everything as host
            host = Some(user_part.to_vec());
        }
    } else {
        // Split on ':' for port
        if let Some(colon_pos) = user_part.iter().position(|&b| b == b':') {
            host = Some(user_part[..colon_pos].to_vec());
            let port_str = &user_part[colon_pos + 1..];
            if !port_str.is_empty() {
                let port_str_decoded = core::str::from_utf8(port_str).unwrap_or("");
                port = port_str_decoded.parse::<c_int>().unwrap_or(0);
            }
        } else {
            host = Some(user_part.to_vec());
        }
    }

    (user, host, port)
}

/// Parse a URI string into its components.
///
/// This implements libxml2's own URI parsing logic, following the patterns
/// used in the upstream `xmlParseURI` function.
///
/// Returns `None` on failure.
pub(crate) fn parse_uri(str: &[u8]) -> Option<UriParts> {
    if str.is_empty() {
        return None;
    }

    // UPSTREAM-PARITY (uri.c 2.15 xmlParseURIReference): the strict 3986
    // scanner rejects any raw byte outside the URI grammar. The union of the
    // per-component character classes is: unreserved (ALPHA/DIGIT/-._~),
    // sub-delims (!$&'()*+,;=), the reserved delimiters (:/?#@[]) and
    // pct-encoded triplets. Everything else — spaces, control bytes, bytes
    // >= 0x80 (non-ASCII must be pct-encoded), and the excluded ASCII
    // punctuation "<>\^`{|} — fails the parse (lxml `_uriValidOrRaise` calls
    // xmlParseURI and rejects invalid namespace URIs through this).
    if !uri_raw_bytes_valid(str) {
        return None;
    }

    let mut parts = UriParts::default();
    let mut remaining = str;

    // 1. Extract scheme
    if let Some((_start, end)) = find_scheme(remaining) {
        parts.scheme = Some(remaining[..end].to_vec());
        remaining = &remaining[end + 1..]; // skip ':'

        // Check if it's hierarchical (://)
        if remaining.starts_with(b"//") {
            remaining = &remaining[2..];
            // Parse authority: everything up to '/', '?', or '#'
            let auth_end = remaining
                .iter()
                .position(|&b| b == b'/' || b == b'?' || b == b'#')
                .unwrap_or(remaining.len());
            let authority = &remaining[..auth_end];
            // Always store authority (even if empty) to preserve "file:///" style URIs
            parts.authority = if authority.is_empty() {
                Some(Vec::new())
            } else {
                Some(authority.to_vec())
            };
            if !authority.is_empty() {
                let (user, host, port) = parse_authority(authority);
                parts.user = user;
                parts.host = host;
                parts.port = port;
                if let Some(ref host_val) = parts.host {
                    // Reconstruct server part (without user@)
                    let mut server = host_val.clone();
                    if port != 0 {
                        server.extend_from_slice(format!(":{}", port).as_bytes());
                    }
                    parts.server = Some(server);
                }
            }
            remaining = &remaining[auth_end..];
        } else {
            // Opaque URI: scheme:rest
            // The opaque part is everything up to '#' or end
            if let Some(frag_pos) = remaining.iter().position(|&b| b == b'#') {
                parts.opaque = Some(remaining[..frag_pos].to_vec());
                parts.fragment = Some(remaining[frag_pos + 1..].to_vec());
            } else {
                parts.opaque = Some(remaining.to_vec());
            }
            // For opaque URIs, the "path" is the opaque part
            parts.path = parts.opaque.clone();
            return Some(parts);
        }
    }

    // 2. Extract path
    // Path is everything up to '?' or '#'
    let query_pos = remaining.iter().position(|&b| b == b'?');
    let frag_pos = remaining.iter().position(|&b| b == b'#');

    let path_end = match (query_pos, frag_pos) {
        (Some(q), Some(f)) => q.min(f),
        (Some(q), None) => q,
        (None, Some(f)) => f,
        (None, None) => remaining.len(),
    };

    if path_end > 0 {
        let path = remaining[..path_end].to_vec();
        parts.path = Some(path.clone());
        parts.path_raw = Some(path);
    }

    // 3. Extract query
    if let Some(qpos) = query_pos {
        let qstart = qpos + 1;
        let qend = frag_pos.unwrap_or(remaining.len());
        if qstart < qend {
            parts.query = Some(remaining[qstart..qend].to_vec());
        }
    }

    // 4. Extract fragment
    if let Some(fpos) = frag_pos {
        let fstart = fpos + 1;
        if fstart < remaining.len() {
            parts.fragment = Some(remaining[fstart..].to_vec());
        }
    }

    Some(parts)
}

/// Whether every raw byte of `uri` is allowed by the RFC 3986 grammar
/// (see `parse_uri`): unreserved, sub-delims, the reserved delimiters, or a
/// `%XX` pct-encoding triplet.
fn uri_raw_bytes_valid(uri: &[u8]) -> bool {
    const fn is_unreserved_byte(b: u8) -> bool {
        b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')
    }
    const fn is_sub_delim_byte(b: u8) -> bool {
        matches!(
            b,
            b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'='
        )
    }
    let mut i = 0usize;
    while i < uri.len() {
        let b = uri[i];
        if is_unreserved_byte(b)
            || is_sub_delim_byte(b)
            || matches!(b, b':' | b'/' | b'?' | b'#' | b'@' | b'[' | b']')
        {
            i += 1;
            continue;
        }
        if b == b'%' && i + 2 < uri.len() {
            let h1 = uri[i + 1];
            let h2 = uri[i + 2];
            if h1.is_ascii_hexdigit() && h2.is_ascii_hexdigit() {
                i += 3;
                continue;
            }
        }
        return false;
    }
    true
}

/// Parse a URI from a null-terminated C string.
///
/// Returns a heap-allocated `UriParts`, or null on failure.
/// The caller must free the returned pointer with [`free_uri_parts`].
#[allow(dead_code)]
pub(crate) fn parse_uri_cstr(str: *const xmlChar) -> *mut UriParts {
    if str.is_null() {
        return ptr::null_mut();
    }

    let len = unsafe { libc::strlen(str as *const libc::c_char) };
    let slice = unsafe { core::slice::from_raw_parts(str, len) };

    match parse_uri(slice) {
        Some(parts) => {
            let boxed = Box::new(parts);
            Box::into_raw(boxed)
        }
        None => ptr::null_mut(),
    }
}

/// Free a heap-allocated `UriParts` that was created by [`parse_uri_cstr`].
///
/// # Safety
///
/// `parts` must have been allocated by [`parse_uri_cstr`] and not yet freed.
#[allow(dead_code)]
pub(crate) unsafe fn free_uri_parts(parts: *mut UriParts) {
    if !parts.is_null() {
        drop(Box::from_raw(parts));
    }
}

// ── URI operations ──────────────────────────────────────────────────────────

/// Build a URI string from its components.
pub(crate) fn build_uri(parts: &UriParts) -> Vec<u8> {
    let mut result = Vec::new();

    // Scheme
    if let Some(ref scheme) = parts.scheme {
        result.extend_from_slice(scheme);
        result.push(b':');
    }

    // Authority
    if let Some(ref authority) = parts.authority {
        result.extend_from_slice(b"//");
        result.extend_from_slice(authority);
    } else if parts.host.is_some() {
        // Reconstruct authority from components
        result.extend_from_slice(b"//");
        if let Some(ref user) = parts.user {
            result.extend_from_slice(user);
            result.push(b'@');
        }
        if let Some(ref host) = parts.host {
            result.extend_from_slice(host);
        }
        if parts.port != 0 {
            result.push(b':');
            result.extend_from_slice(format!("{}", parts.port).as_bytes());
        }
    }

    // Path
    if let Some(ref path) = parts.path {
        result.extend_from_slice(path);
    } else if let Some(ref opaque) = parts.opaque {
        result.extend_from_slice(opaque);
    }

    // Query
    if let Some(ref query) = parts.query {
        result.push(b'?');
        result.extend_from_slice(query);
    }

    // Fragment
    if let Some(ref fragment) = parts.fragment {
        result.push(b'#');
        result.extend_from_slice(fragment);
    }

    result
}

/// Normalize a URI path (remove "." and ".." segments).
///
/// This implements the same logic as libxml2's `xmlNormalizeURIPath`.
/// It processes path segments and resolves "." and ".." references.
pub(crate) fn normalize_uri_path(uri: &[u8]) -> Vec<u8> {
    if uri.is_empty() {
        return Vec::new();
    }

    let absolute = uri.starts_with(b"/");
    let ends_with_slash = uri.ends_with(b"/");

    let parts: Vec<&[u8]> = uri.split(|&b| b == b'/').collect();
    let mut segments: Vec<&[u8]> = Vec::new();

    for segment in parts {
        if segment == b"." || segment.is_empty() {
            // Skip "." segments and empty segments (from leading/trailing/double slashes)
            continue;
        }
        if segment == b".." {
            // Remove the last segment if possible
            segments.pop();
        } else {
            segments.push(segment);
        }
    }

    let mut result = Vec::new();
    if absolute {
        result.push(b'/');
    }
    for (i, seg) in segments.iter().enumerate() {
        if i > 0 {
            result.push(b'/');
        }
        result.extend_from_slice(seg);
    }

    // Preserve trailing slash if original had one
    if ends_with_slash && !segments.is_empty() {
        result.push(b'/');
    }

    // If the result is empty and the path was absolute, return "/"
    if result.is_empty() && absolute {
        result.push(b'/');
    }

    result
}

/// Get the scheme part of a URI.
#[allow(dead_code)]
pub(crate) fn get_scheme(uri: &[u8]) -> Option<Vec<u8>> {
    if let Some((_start, end)) = find_scheme(uri) {
        Some(uri[_start..end].to_vec())
    } else {
        None
    }
}

/// Check if a URI is absolute (has a scheme).
pub(crate) const fn is_absolute(uri: &[u8]) -> bool {
    find_scheme(uri).is_some()
}

/// Resolve a relative URI against a base URI.
///
/// Both are byte slices. Returns the resolved absolute URI.
///
/// This implements the resolution algorithm from RFC 3986 §5.3,
/// matching libxml2's `xmlBuildURI` behavior.
pub(crate) fn resolve_uri(base: &[u8], relative: &[u8]) -> Option<Vec<u8>> {
    if base.is_empty() {
        return if relative.is_empty() {
            None
        } else {
            Some(relative.to_vec())
        };
    }

    // If the relative URI is absolute, return it as-is
    if is_absolute(relative) {
        return Some(relative.to_vec());
    }

    // Parse the base URI
    let base_parts = parse_uri(base)?;

    // If the relative URI is empty, return the base
    if relative.is_empty() {
        return Some(build_uri(&base_parts));
    }

    // Parse the relative URI parts manually (simpler than full parse)
    let rel_str = relative;

    let mut result = UriParts {
        scheme: base_parts.scheme.clone(),
        ..Default::default()
    };

    if rel_str.starts_with(b"//") {
        // Network-path reference: starts with "//"
        // Authority is everything up to '/' or end
        let rest = &rel_str[2..];
        let auth_end = rest.iter().position(|&b| b == b'/').unwrap_or(rest.len());
        let auth = rest[..auth_end].to_vec();
        let (user, host, port) = parse_authority(&auth);
        result.authority = Some(auth);
        result.user = user;
        result.host = host;
        result.port = port;

        let path_rest = if auth_end < rest.len() {
            &rest[auth_end..]
        } else {
            b""
        };
        // Parse path, query, fragment from remaining
        parse_path_query_fragment(path_rest, &mut result);
    } else if rel_str.starts_with(b"/") {
        // Absolute path reference
        parse_path_query_fragment(rel_str, &mut result);
        // Inherit authority from base
        result.authority = base_parts.authority.clone();
        result.user = base_parts.user.clone();
        result.host = base_parts.host.clone();
        result.port = base_parts.port;
    } else {
        // Relative path reference
        // Start with base path's directory
        let base_path = base_parts.path.as_deref().unwrap_or(b"");
        let base_dir = if let Some(last_slash) = base_path.iter().rposition(|&b| b == b'/') {
            &base_path[..=last_slash]
        } else {
            b""
        };

        // Parse the relative part
        let mut combined = Vec::from(base_dir);
        combined.extend_from_slice(rel_str);
        parse_path_query_fragment(&combined, &mut result);

        // Inherit authority from base
        result.authority = base_parts.authority.clone();
        result.user = base_parts.user.clone();
        result.host = base_parts.host.clone();
        result.port = base_parts.port;
    }

    // Normalize the path
    if let Some(ref path) = result.path {
        let normalized = normalize_uri_path(path);
        result.path = Some(normalized);
    }

    Some(build_uri(&result))
}

/// Helper: parse path, query, and fragment from the remainder of a URI.
fn parse_path_query_fragment(input: &[u8], parts: &mut UriParts) {
    // Find '?' and '#'
    let query_pos = input.iter().position(|&b| b == b'?');
    let frag_pos = input.iter().position(|&b| b == b'#');

    let path_end = match (query_pos, frag_pos) {
        (Some(q), Some(f)) => q.min(f),
        (Some(q), None) => q,
        (None, Some(f)) => f,
        (None, None) => input.len(),
    };

    if path_end > 0 {
        parts.path = Some(input[..path_end].to_vec());
        parts.path_raw = parts.path.clone();
    }

    // Query
    if let Some(qpos) = query_pos {
        let qstart = qpos + 1;
        let qend = frag_pos.unwrap_or(input.len());
        if qstart < qend {
            parts.query = Some(input[qstart..qend].to_vec());
        }
    }

    // Fragment
    if let Some(fpos) = frag_pos {
        let fstart = fpos + 1;
        if fstart < input.len() {
            parts.fragment = Some(input[fstart..].to_vec());
        }
    }
}

// ── C ABI-compatible wrapper functions ──────────────────────────────────────

/// `xmlURIPtr xmlParseURI(const char *str)`
///
/// Parse a URI from a C string. Returns an opaque pointer to a heap-allocated
/// `UriParts`, or null on failure.
///
/// The caller must free the result with [`xmlFreeURI`].
///
/// # Safety
///
/// `str` must be a valid null-terminated C string.
pub(crate) unsafe fn xmlParseURI(str: *const c_char) -> *mut c_void {
    if str.is_null() {
        return ptr::null_mut();
    }
    let len = libc::strlen(str);
    let slice = unsafe { core::slice::from_raw_parts(str as *const u8, len) };
    match parse_uri(slice) {
        Some(parts) => unsafe { parts_to_c(&parts) as *mut c_void },
        None => ptr::null_mut(),
    }
}

/// `void xmlFreeURI(xmlURIPtr uri)`
///
/// Free a URI structure previously returned by [`xmlParseURI`] or [`xmlCreateURI`].
///
/// # Safety
///
/// `uri` must have been allocated by [`xmlParseURI`] or [`xmlCreateURI`] and not yet freed.
pub(crate) unsafe fn xmlFreeURI(uri: *mut c_void) {
    if !uri.is_null() {
        unsafe { free_c_uri(uri as *mut CXmlUri) };
    }
}

/// `xmlURIPtr xmlCreateURI(void)`
///
/// Create an empty URI structure.
/// Returns an opaque pointer to a heap-allocated, zero-initialized `CXmlUri`
/// (C-ABI layout matching `struct _xmlURI`).
///
/// The caller must free the result with [`xmlFreeURI`].
pub(crate) fn xmlCreateURI() -> *mut c_void {
    let boxed = Box::new(CXmlUri::default());
    Box::into_raw(boxed) as *mut c_void
}

/// `xmlChar *xmlSaveUri(xmlURIPtr uri)`
///
/// Serialize a URI structure back to a string.
/// Returns a null-terminated `xmlChar*` string allocated with `xmlMalloc`,
/// or null on failure.
///
/// The caller must free the result with `xmlFree`.
///
/// # Safety
///
/// `uri` must be a valid pointer to a `CXmlUri` previously created by
/// [`xmlParseURI`] or [`xmlCreateURI`].
pub(crate) unsafe fn xmlSaveUri(uri: *mut c_void) -> *mut xmlChar {
    if uri.is_null() {
        return ptr::null_mut();
    }
    let parts = unsafe { c_to_parts(uri as *const CXmlUri) };
    let result = build_uri(&parts);
    if result.is_empty() {
        return ptr::null_mut();
    }
    // Allocate with xmlMalloc and copy
    let len = result.len();
    let ptr = unsafe { allocator::xmlMallocImpl(len + 1) as *mut u8 };
    if ptr.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::copy_nonoverlapping(result.as_ptr(), ptr, len);
        *ptr.add(len) = 0; // null terminator
    }
    ptr as *mut xmlChar
}

/// `int xmlParseURIReference(xmlURIPtr uri, const char *str)`
///
/// Parse a URI string into an EXISTING URI structure (upstream uri.c
/// `xmlParseURIReference`): the string is parsed and the fields of `uri` are
/// replaced. Returns 0 on success, -1 on failure. On failure the URI
/// structure is left untouched.
///
/// # Safety
///
/// `uri` must be a valid pointer to a `CXmlUri` previously created by
/// [`xmlParseURI`] or [`xmlCreateURI`]; `str` must be a valid
/// null-terminated C string.
pub(crate) unsafe fn xmlParseURIReference(uri: *mut c_void, str: *const c_char) -> c_int {
    if uri.is_null() || str.is_null() {
        return -1;
    }
    let len = unsafe { libc::strlen(str) };
    let slice = unsafe { core::slice::from_raw_parts(str as *const u8, len) };
    let parts = match parse_uri(slice) {
        Some(p) => p,
        None => return -1,
    };
    let fresh = unsafe { parts_to_c(&parts) };
    if fresh.is_null() {
        return -1;
    }
    // swap the parsed fields into the caller's structure, then release the
    // temporary shell (the strings were moved, so nothing is leaked)
    unsafe {
        let dst = &mut *(uri as *mut CXmlUri);
        let src = &mut *fresh;
        core::mem::swap(dst, src);
        free_c_uri(fresh);
    }
    0
}

/// `int xmlNormalizeURIPath(char *path)`
///
/// Normalize a URI path IN PLACE (upstream uri.c `xmlNormalizeURIPath`):
/// remove `.` and `..` segments per RFC 3986 §5.2.4, keeping the leading
/// `/`. Returns 0 on success, -1 on failure (e.g. `..` above the root).
///
/// The candidate's internal normalizer (`normalize_uri_path`) implements the
/// same algorithm on byte slices; this wrapper applies it to the C buffer in
/// place.
///
/// # Safety
///
/// `path` must be a valid, writable, null-terminated C string buffer that
/// is at least `strlen(path) + 1` bytes long.
pub(crate) unsafe fn xmlNormalizeURIPath(path: *mut c_char) -> c_int {
    if path.is_null() {
        return -1;
    }
    // Faithful port of upstream uri.c `xmlNormalizeURIPath`: operates in
    // place, removes `.`/`..` segments, and fails with -1 when `..` would
    // climb above the root or when the path does not start with '/'
    // (upstream only normalizes absolute paths).
    unsafe {
        let mut cur = path;
        if *cur == b'/' as c_char {
            cur = cur.add(1);
        } else {
            return -1;
        }
        let mut out = path;
        while *cur != 0 {
            let c0 = *cur as u8;
            let c1 = *cur.add(1) as u8;
            let c2 = *cur.add(2) as u8;
            // "./" segment: skip
            if c0 == b'.' && c1 == b'/' {
                cur = cur.add(2);
                continue;
            }
            // "../" segment: back up one segment, fail if at the root
            if c0 == b'.' && c1 == b'.' && c2 == b'/' {
                if out == path {
                    return -1;
                }
                out = out.sub(1);
                while out > path && *out.sub(1) != b'/' as c_char {
                    out = out.sub(1);
                }
                cur = cur.add(3);
                continue;
            }
            // trailing "." — drop it and finish
            if c0 == b'.' && c1 == 0 {
                break;
            }
            // trailing ".." — back up one segment, fail if at the root
            if c0 == b'.' && c1 == b'.' && c2 == 0 {
                if out == path {
                    return -1;
                }
                out = out.sub(1);
                while out > path && *out.sub(1) != b'/' as c_char {
                    out = out.sub(1);
                }
                break;
            }
            *out = *cur;
            out = out.add(1);
            cur = cur.add(1);
        }
        *out = 0;
    }
    0
}

/// `xmlChar *xmlURIEscapeStr(unsigned char *str, unsigned char *list)`
///
/// Percent-escape a string for use in a URI.
/// Characters in `list` are NOT escaped (they're treated as safe).
///
/// Returns a null-terminated `xmlChar*` string allocated with `xmlMalloc`,
/// or null on failure.
///
/// # Safety
///
/// `str` must be a valid null-terminated C string. `list` may be null.
pub(crate) unsafe fn xmlURIEscapeStr(str: *const xmlChar, list: *const xmlChar) -> *mut xmlChar {
    if str.is_null() {
        return ptr::null_mut();
    }
    let str_len = unsafe { libc::strlen(str as *const libc::c_char) };
    let str_slice = unsafe { core::slice::from_raw_parts(str, str_len) };

    // Build the safe-set: unreserved + reserved + chars in `list`
    let mut safe_set = [false; 256];
    for b in 0u8..=255 {
        if is_unreserved(b) || b == b'%' {
            safe_set[b as usize] = true;
        }
    }
    if !list.is_null() {
        let list_len = unsafe { libc::strlen(list as *const libc::c_char) };
        let list_slice = unsafe { core::slice::from_raw_parts(list, list_len) };
        for &b in list_slice {
            safe_set[b as usize] = true;
        }
    }

    // Build the result
    let mut result = Vec::with_capacity(str_slice.len() * 3);
    for &b in str_slice {
        if safe_set[b as usize] {
            result.push(b);
        } else {
            result.extend_from_slice(format!("%{:02X}", b).as_bytes());
        }
    }

    let len = result.len();
    let ptr = unsafe { allocator::xmlMallocImpl(len + 1) as *mut u8 };
    if ptr.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::copy_nonoverlapping(result.as_ptr(), ptr, len);
        *ptr.add(len) = 0;
    }
    ptr as *mut xmlChar
}

/// `xmlChar *xmlURIUnescapeString(const char *str, int len, char *target)`
///
/// Unescape a percent-encoded URI string.
///
/// If `len` is negative or zero, the string is assumed to be null-terminated
/// and its whole length is used (upstream uri.c: `if (len <= 0) len =
/// strlen(str)`) — PHP's stream wrapper calls it as
/// `xmlURIUnescapeString(filename, 0, NULL)`.
/// If `target` is not null, the result is written there (and returned).
/// Otherwise, a new buffer is allocated with `xmlMalloc`.
///
/// Returns the unescaped string, or null on failure.
///
/// # Safety
///
/// `str` must be a valid C string (null-terminated if `len` <= 0).
/// `target` must be large enough to hold the result if not null.
pub(crate) unsafe fn xmlURIUnescapeString(
    str: *const c_char,
    len: c_int,
    target: *mut c_char,
) -> *mut c_char {
    if str.is_null() {
        return ptr::null_mut();
    }
    let slice = if len <= 0 {
        let cstr_len = unsafe { libc::strlen(str) };
        unsafe { core::slice::from_raw_parts(str as *const u8, cstr_len) }
    } else {
        unsafe { core::slice::from_raw_parts(str as *const u8, len as usize) }
    };

    let decoded = percent_decode(slice);

    if !target.is_null() {
        unsafe {
            ptr::copy_nonoverlapping(decoded.as_ptr(), target as *mut u8, decoded.len());
            *((target as *mut u8).add(decoded.len())) = 0;
        }
        return target;
    }

    let out_len = decoded.len();
    let ptr = unsafe { allocator::xmlMallocImpl(out_len + 1) as *mut u8 };
    if ptr.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::copy_nonoverlapping(decoded.as_ptr(), ptr, out_len);
        *ptr.add(out_len) = 0;
    }
    ptr as *mut c_char
}

/// `xmlURIPtr xmlParseURIRaw(const char *str, int raw)`
///
/// Parse a URI from a C string.
/// The `raw` flag is currently unused (reserved for future behavior).
///
/// Returns an opaque pointer to a heap-allocated `UriParts`, or null on failure.
///
/// The caller must free the result with [`xmlFreeURI`].
///
/// # Safety
///
/// `str` must be a valid null-terminated C string.
#[allow(dead_code)]
pub(crate) unsafe fn xmlParseURIRaw(str: *const c_char, _raw: c_int) -> *mut c_void {
    unsafe { xmlParseURI(str) }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── URI character classification ───────────────────────────────────────

    #[test]
    fn test_is_unreserved() {
        assert!(is_unreserved(b'a'));
        assert!(is_unreserved(b'Z'));
        assert!(is_unreserved(b'0'));
        assert!(is_unreserved(b'-'));
        assert!(is_unreserved(b'.'));
        assert!(is_unreserved(b'_'));
        assert!(is_unreserved(b'~'));
        assert!(!is_unreserved(b':'));
        assert!(!is_unreserved(b'/'));
        assert!(!is_unreserved(b'%'));
        assert!(!is_unreserved(b' '));
    }

    #[test]
    fn test_is_reserved() {
        assert!(is_reserved(b':'));
        assert!(is_reserved(b'/'));
        assert!(is_reserved(b'?'));
        assert!(is_reserved(b'#'));
        assert!(is_reserved(b'@'));
        assert!(is_reserved(b'!'));
        assert!(is_reserved(b'$'));
        assert!(is_reserved(b'&'));
        assert!(is_reserved(b'('));
        assert!(is_reserved(b')'));
        assert!(!is_reserved(b'a'));
        assert!(!is_reserved(b' '));
    }

    #[test]
    fn test_is_scheme_char() {
        assert!(is_scheme_char(b'a'));
        assert!(is_scheme_char(b'Z'));
        assert!(is_scheme_char(b'0'));
        assert!(is_scheme_char(b'+'));
        assert!(is_scheme_char(b'-'));
        assert!(is_scheme_char(b'.'));
        assert!(!is_scheme_char(b':'));
        assert!(!is_scheme_char(b'/'));
        assert!(!is_scheme_char(b' '));
    }

    // ── Percent encoding / decoding ────────────────────────────────────────

    #[test]
    fn test_percent_decode_simple() {
        assert_eq!(percent_decode(b"hello"), b"hello");
        assert_eq!(percent_decode(b"%68%65%6C%6C%6F"), b"hello");
        assert_eq!(percent_decode(b"%48%65%6C%6C%6F"), b"Hello");
        assert_eq!(percent_decode(b"a%20b"), b"a b");
    }

    #[test]
    fn test_percent_decode_invalid() {
        // Invalid percent sequence: keep as-is
        assert_eq!(percent_decode(b"%XX"), b"%XX");
        assert_eq!(percent_decode(b"%2"), b"%2");
        assert_eq!(percent_decode(b"%"), b"%");
        assert_eq!(percent_decode(b"%%20"), b"% ");
    }

    #[test]
    fn test_percent_decode_empty() {
        assert_eq!(percent_decode(b""), b"");
    }

    #[test]
    fn test_percent_encode() {
        assert_eq!(percent_encode(b"hello"), b"hello");
        assert_eq!(percent_encode(b"hello world"), b"hello%20world");
        assert_eq!(percent_encode(b"a/b"), b"a/b"); // '/' is reserved, keep as-is
    }

    // ── URI parsing ────────────────────────────────────────────────────────

    #[test]
    fn test_parse_http_uri() {
        let parts =
            parse_uri(b"http://example.com/path/to/file.xml?query=1#frag").expect("should parse");
        assert_eq!(parts.scheme, Some(b"http".to_vec()));
        assert_eq!(parts.authority, Some(b"example.com".to_vec()));
        assert_eq!(parts.host, Some(b"example.com".to_vec()));
        assert_eq!(parts.port, 0);
        assert_eq!(parts.path, Some(b"/path/to/file.xml".to_vec()));
        assert_eq!(parts.query, Some(b"query=1".to_vec()));
        assert_eq!(parts.fragment, Some(b"frag".to_vec()));
    }

    #[test]
    fn test_parse_https_uri() {
        let parts = parse_uri(b"https://example.com:443/path").expect("should parse");
        assert_eq!(parts.scheme, Some(b"https".to_vec()));
        assert_eq!(parts.host, Some(b"example.com".to_vec()));
        assert_eq!(parts.port, 443);
        assert_eq!(parts.path, Some(b"/path".to_vec()));
    }

    #[test]
    fn test_parse_file_uri() {
        let parts = parse_uri(b"file:///etc/hosts").expect("should parse");
        assert_eq!(parts.scheme, Some(b"file".to_vec()));
        assert!(parts.authority.is_none() || parts.authority.as_deref() == Some(b""));
        assert_eq!(parts.path, Some(b"/etc/hosts".to_vec()));
    }

    #[test]
    fn test_parse_file_uri_with_host() {
        // file://hostname/path is also valid
        let parts = parse_uri(b"file://localhost/etc/hosts").expect("should parse");
        assert_eq!(parts.scheme, Some(b"file".to_vec()));
        assert_eq!(parts.host, Some(b"localhost".to_vec()));
        assert_eq!(parts.path, Some(b"/etc/hosts".to_vec()));
    }

    #[test]
    fn test_parse_relative_uri() {
        let parts = parse_uri(b"/path/to/file.xml").expect("should parse");
        assert!(parts.scheme.is_none());
        assert_eq!(parts.path, Some(b"/path/to/file.xml".to_vec()));
    }

    #[test]
    fn test_parse_relative_uri_with_query() {
        let parts = parse_uri(b"file.xml?query=1").expect("should parse");
        assert!(parts.scheme.is_none());
        assert_eq!(parts.path, Some(b"file.xml".to_vec()));
        assert_eq!(parts.query, Some(b"query=1".to_vec()));
    }

    #[test]
    fn test_parse_uri_with_user_info() {
        let parts = parse_uri(b"ftp://user@host.com:21/path").expect("should parse");
        assert_eq!(parts.scheme, Some(b"ftp".to_vec()));
        assert_eq!(parts.user, Some(b"user".to_vec()));
        assert_eq!(parts.host, Some(b"host.com".to_vec()));
        assert_eq!(parts.port, 21);
        assert_eq!(parts.path, Some(b"/path".to_vec()));
    }

    #[test]
    fn test_parse_uri_with_user_password() {
        let parts = parse_uri(b"ftp://user:pass@host.com/path").expect("should parse");
        assert_eq!(parts.scheme, Some(b"ftp".to_vec()));
        assert_eq!(parts.user, Some(b"user:pass".to_vec()));
        assert_eq!(parts.host, Some(b"host.com".to_vec()));
        assert_eq!(parts.path, Some(b"/path".to_vec()));
    }

    #[test]
    fn test_parse_opaque_uri() {
        let parts = parse_uri(b"mailto:user@example.com").expect("should parse");
        assert_eq!(parts.scheme, Some(b"mailto".to_vec()));
        assert_eq!(parts.opaque, Some(b"user@example.com".to_vec()));
        assert!(parts.authority.is_none());
    }

    #[test]
    fn test_parse_opaque_uri_with_fragment() {
        let parts = parse_uri(b"urn:isbn:0-395-36341-1#frag").expect("should parse");
        assert_eq!(parts.scheme, Some(b"urn".to_vec()));
        assert_eq!(parts.opaque, Some(b"isbn:0-395-36341-1".to_vec()));
        assert_eq!(parts.fragment, Some(b"frag".to_vec()));
    }

    #[test]
    fn test_parse_empty_uri() {
        assert!(parse_uri(b"").is_none());
    }

    #[test]
    fn test_parse_uri_fragment_only() {
        let parts = parse_uri(b"#fragment").expect("should parse");
        assert!(parts.scheme.is_none());
        assert!(parts.path.is_none());
        assert_eq!(parts.fragment, Some(b"fragment".to_vec()));
    }

    #[test]
    fn test_parse_uri_query_only() {
        let parts = parse_uri(b"?query").expect("should parse");
        assert!(parts.scheme.is_none());
        assert!(parts.path.is_none());
        assert_eq!(parts.query, Some(b"query".to_vec()));
    }

    #[test]
    fn test_parse_uri_with_ipv6_host() {
        let parts = parse_uri(b"http://[::1]:8080/path").expect("should parse");
        assert_eq!(parts.scheme, Some(b"http".to_vec()));
        assert_eq!(parts.host, Some(b"[::1]".to_vec()));
        assert_eq!(parts.port, 8080);
        assert_eq!(parts.path, Some(b"/path".to_vec()));
    }

    #[test]
    fn test_parse_uri_no_path() {
        let parts = parse_uri(b"http://example.com").expect("should parse");
        assert_eq!(parts.scheme, Some(b"http".to_vec()));
        assert_eq!(parts.host, Some(b"example.com".to_vec()));
        assert!(parts.path.is_none());
    }

    #[test]
    fn test_parse_uri_no_path_with_query() {
        let parts = parse_uri(b"http://example.com?query").expect("should parse");
        assert_eq!(parts.scheme, Some(b"http".to_vec()));
        assert_eq!(parts.host, Some(b"example.com".to_vec()));
        assert!(parts.path.is_none());
        assert_eq!(parts.query, Some(b"query".to_vec()));
    }

    // ── URI building ───────────────────────────────────────────────────────

    #[test]
    fn test_build_uri() {
        let parts = UriParts {
            scheme: Some(b"http".to_vec()),
            host: Some(b"example.com".to_vec()),
            port: 8080,
            path: Some(b"/path".to_vec()),
            query: Some(b"q=1".to_vec()),
            fragment: Some(b"frag".to_vec()),
            ..Default::default()
        };
        assert_eq!(build_uri(&parts), b"http://example.com:8080/path?q=1#frag");
    }

    #[test]
    fn test_build_uri_simple() {
        let parts = UriParts {
            scheme: Some(b"http".to_vec()),
            host: Some(b"example.com".to_vec()),
            path: Some(b"/".to_vec()),
            ..Default::default()
        };
        assert_eq!(build_uri(&parts), b"http://example.com/");
    }

    #[test]
    fn test_build_uri_opaque() {
        let parts = UriParts {
            scheme: Some(b"mailto".to_vec()),
            opaque: Some(b"user@example.com".to_vec()),
            ..Default::default()
        };
        assert_eq!(build_uri(&parts), b"mailto:user@example.com");
    }

    #[test]
    fn test_build_uri_relative() {
        let parts = UriParts {
            path: Some(b"/relative/path".to_vec()),
            ..Default::default()
        };
        assert_eq!(build_uri(&parts), b"/relative/path");
    }

    // ── URI normalization ──────────────────────────────────────────────────

    #[test]
    fn test_normalize_uri_path_simple() {
        assert_eq!(normalize_uri_path(b"/foo/bar"), b"/foo/bar");
        assert_eq!(normalize_uri_path(b"/foo/./bar"), b"/foo/bar");
        assert_eq!(normalize_uri_path(b"/foo/../bar"), b"/bar");
        assert_eq!(normalize_uri_path(b"/foo/bar/.."), b"/foo");
        assert_eq!(normalize_uri_path(b"/"), b"/");
    }

    #[test]
    fn test_normalize_uri_path_relative() {
        assert_eq!(normalize_uri_path(b"foo/bar"), b"foo/bar");
        assert_eq!(normalize_uri_path(b"foo/./bar"), b"foo/bar");
        assert_eq!(normalize_uri_path(b"foo/../bar"), b"bar");
    }

    #[test]
    fn test_normalize_uri_path_double_dot_overflow() {
        // ".." above root should just be removed
        assert_eq!(normalize_uri_path(b"/a/../../b"), b"/b");
        assert_eq!(normalize_uri_path(b"/../b"), b"/b");
    }

    #[test]
    fn test_normalize_uri_path_empty() {
        assert_eq!(normalize_uri_path(b""), b"");
    }

    #[test]
    fn test_normalize_uri_path_dots_only() {
        assert_eq!(normalize_uri_path(b"./././."), b"");
        assert_eq!(normalize_uri_path(b"/./././"), b"/");
    }

    // ── URI scheme / absolute check ────────────────────────────────────────

    #[test]
    fn test_get_scheme() {
        assert_eq!(get_scheme(b"http://example.com"), Some(b"http".to_vec()));
        assert_eq!(get_scheme(b"https://example.com"), Some(b"https".to_vec()));
        assert_eq!(get_scheme(b"file:///path"), Some(b"file".to_vec()));
        assert_eq!(get_scheme(b"ftp://host"), Some(b"ftp".to_vec()));
        assert_eq!(get_scheme(b"mailto:user@host"), Some(b"mailto".to_vec()));
        assert_eq!(get_scheme(b"urn:isbn:1234"), Some(b"urn".to_vec()));
        assert_eq!(get_scheme(b"/path"), None);
        assert_eq!(get_scheme(b"relative"), None);
        assert_eq!(get_scheme(b""), None);
    }

    #[test]
    fn test_is_absolute() {
        assert!(is_absolute(b"http://example.com"));
        assert!(is_absolute(b"file:///path"));
        assert!(is_absolute(b"mailto:user@host"));
        assert!(!is_absolute(b"/path"));
        assert!(!is_absolute(b"relative"));
        assert!(!is_absolute(b""));
    }

    // ── URI resolution ─────────────────────────────────────────────────────

    #[test]
    fn test_resolve_uri_absolute_relative() {
        let result =
            resolve_uri(b"http://example.com/base/", b"relative.xml").expect("should resolve");
        assert_eq!(result, b"http://example.com/base/relative.xml");
    }

    #[test]
    fn test_resolve_uri_absolute_absolute() {
        let result = resolve_uri(
            b"http://example.com/base/",
            b"http://other.com/absolute.xml",
        )
        .expect("should resolve");
        assert_eq!(result, b"http://other.com/absolute.xml");
    }

    #[test]
    fn test_resolve_uri_root_relative() {
        let result =
            resolve_uri(b"http://example.com/base/file.xml", b"/root.xml").expect("should resolve");
        assert_eq!(result, b"http://example.com/root.xml");
    }

    #[test]
    fn test_resolve_uri_network_path() {
        let result = resolve_uri(b"http://example.com/base/file.xml", b"//other.com/root.xml")
            .expect("should resolve");
        assert_eq!(result, b"http://other.com/root.xml");
    }

    #[test]
    fn test_resolve_uri_parent_traversal() {
        let result = resolve_uri(b"http://example.com/a/b/c/file.xml", b"../../d/file.xml")
            .expect("should resolve");
        assert_eq!(result, b"http://example.com/a/d/file.xml");
    }

    #[test]
    fn test_resolve_uri_with_query() {
        let result =
            resolve_uri(b"http://example.com/base/", b"file.xml?query=1").expect("should resolve");
        assert_eq!(result, b"http://example.com/base/file.xml?query=1");
    }

    #[test]
    fn test_resolve_uri_with_fragment() {
        let result =
            resolve_uri(b"http://example.com/base/file.xml", b"#frag").expect("should resolve");
        // A fragment-only reference with no path should resolve to base's directory
        // with the fragment replaced.
        assert_eq!(result, b"http://example.com/base/#frag");
    }

    #[test]
    fn test_resolve_uri_empty_base() {
        let result = resolve_uri(b"", b"relative.xml");
        assert_eq!(result, Some(b"relative.xml".to_vec()));
    }

    #[test]
    fn test_resolve_uri_empty_relative() {
        let result = resolve_uri(b"http://example.com/base/", b"");
        assert!(result.is_some());
        // Should return base URI
        assert_eq!(result.unwrap(), b"http://example.com/base/");
    }

    #[test]
    fn test_resolve_uri_both_empty() {
        assert!(resolve_uri(b"", b"").is_none());
    }

    #[test]
    fn test_resolve_uri_file_scheme() {
        let result = resolve_uri(b"file:///base/dir/", b"file.xml").expect("should resolve");
        assert_eq!(result, b"file:///base/dir/file.xml");
    }

    #[test]
    fn test_resolve_uri_deep_relative() {
        let result = resolve_uri(
            b"http://example.com/a/b/c/d/e/file.xml",
            b"../../../../x/y/z/file.xml",
        )
        .expect("should resolve");
        assert_eq!(result, b"http://example.com/a/x/y/z/file.xml");
    }

    // ── C ABI wrapper functions ────────────────────────────────────────────

    /// Create a URI with `xmlCreateURI` and release it with `xmlFreeURI`.
    ///
    /// # Safety
    ///
    /// - `xmlCreateURI` returns an allocator-owned `CXmlUri` or NULL; the
    ///   non-NULL result asserted here must be freed exactly once with
    ///   `xmlFreeURI` before the test ends.
    #[test]
    fn test_xml_create_and_free_uri() {
        unsafe {
            let uri = xmlCreateURI();
            assert!(!uri.is_null());
            xmlFreeURI(uri);
        }
    }

    /// Parse a C string URI and read back its fields before freeing.
    ///
    /// # Safety
    ///
    /// - `cstr` points to a static NUL-terminated string valid for the
    ///   call; the returned `CXmlUri` is valid while its fields are read
    ///   and must be freed with `xmlFreeURI`.
    #[test]
    fn test_xml_parse_uri() {
        unsafe {
            let cstr = c"http://example.com/path".as_ptr() as *const c_char;
            let uri = xmlParseURI(cstr);
            assert!(!uri.is_null());
            let parts = &*(uri as *const CXmlUri);
            assert_eq!(from_c_str(parts.scheme), Some(b"http".to_vec()));
            assert_eq!(from_c_str(parts.server), Some(b"example.com".to_vec()));
            assert_eq!(from_c_str(parts.path), Some(b"/path".to_vec()));
            xmlFreeURI(uri);
        }
    }

    /// `xmlParseURI` must accept a NULL pointer and report failure.
    ///
    /// # Safety
    ///
    /// - `xmlParseURI` handles a NULL `str` without dereferencing it and
    ///   returns NULL; no pointer is read or freed in this test.
    #[test]
    fn test_xml_parse_uri_null() {
        unsafe {
            let uri = xmlParseURI(ptr::null());
            assert!(uri.is_null());
        }
    }

    /// Round-trip a parsed URI through `xmlSaveUri` and compare strings.
    ///
    /// # Safety
    ///
    /// - `cstr` is a valid NUL-terminated string; `saved` is an
    ///   allocator-owned buffer freed with `xmlFreeImpl`, and `uri` is
    ///   freed with `xmlFreeURI`; both pointers must stay valid until
    ///   their respective frees.
    #[test]
    fn test_xml_save_uri() {
        unsafe {
            let cstr = c"http://example.com:8080/path?q=1#f".as_ptr() as *const c_char;
            let uri = xmlParseURI(cstr);
            assert!(!uri.is_null());
            let saved = xmlSaveUri(uri);
            assert!(!saved.is_null());
            let saved_str = std::ffi::CStr::from_ptr(saved as *const c_char);
            assert_eq!(saved_str.to_bytes(), b"http://example.com:8080/path?q=1#f");
            allocator::xmlFreeImpl(saved as *mut core::ffi::c_void);
            xmlFreeURI(uri);
        }
    }

    /// Escape a C string with `xmlURIEscapeStr` and a NULL safe list.
    ///
    /// # Safety
    ///
    /// - `cstr` must be a valid NUL-terminated string and the NULL safe
    ///   list is accepted by the API; the returned buffer is
    ///   allocator-owned and freed with `xmlFreeImpl`.
    #[test]
    fn test_xml_escape_str() {
        unsafe {
            let cstr = c"hello world".as_ptr() as *const xmlChar;
            let result = xmlURIEscapeStr(cstr, ptr::null());
            assert!(!result.is_null());
            let result_str = std::ffi::CStr::from_ptr(result as *const c_char);
            assert_eq!(result_str.to_bytes(), b"hello%20world");
            allocator::xmlFreeImpl(result as *mut core::ffi::c_void);
        }
    }

    /// Escape a C string with `xmlURIEscapeStr` and a non-NULL safe list.
    ///
    /// # Safety
    ///
    /// - `cstr` and `safe` must be valid NUL-terminated strings; the
    ///   returned buffer is allocator-owned and freed with `xmlFreeImpl`.
    #[test]
    fn test_xml_escape_str_with_safe_list() {
        unsafe {
            let cstr = c"hello world".as_ptr() as *const xmlChar;
            let safe = c" ".as_ptr() as *const xmlChar;
            let result = xmlURIEscapeStr(cstr, safe);
            assert!(!result.is_null());
            let result_str = std::ffi::CStr::from_ptr(result as *const c_char);
            assert_eq!(result_str.to_bytes(), b"hello world"); // space is in safe list
            allocator::xmlFreeImpl(result as *mut core::ffi::c_void);
        }
    }

    /// Unescape a percent-encoded C string with `xmlURIUnescapeString`.
    ///
    /// # Safety
    ///
    /// - `cstr` must be a valid NUL-terminated string; the returned buffer
    ///   is allocator-owned and freed with `xmlFreeImpl`.
    #[test]
    fn test_xml_unescape_string() {
        unsafe {
            let cstr = c"hello%20world".as_ptr() as *const c_char;
            let result = xmlURIUnescapeString(cstr, -1, ptr::null_mut());
            assert!(!result.is_null());
            let result_str = std::ffi::CStr::from_ptr(result);
            assert_eq!(result_str.to_bytes(), b"hello world");
            allocator::xmlFreeImpl(result as *mut core::ffi::c_void);
        }
    }

    /// Unescape an explicit-length percent-encoded string.
    ///
    /// # Safety
    ///
    /// - `cstr` must be valid for the declared length (a static string)
    ///   and the returned buffer is allocator-owned, freed with
    ///   `xmlFreeImpl`.
    #[test]
    fn test_xml_unescape_string_with_len() {
        unsafe {
            let cstr = c"hello%20world".as_ptr() as *const c_char;
            let result = xmlURIUnescapeString(cstr, 13, ptr::null_mut());
            assert!(!result.is_null());
            let result_str = std::ffi::CStr::from_ptr(result);
            assert_eq!(result_str.to_bytes(), b"hello world");
            allocator::xmlFreeImpl(result as *mut core::ffi::c_void);
        }
    }

    /// A `len` of 0 means "whole NUL-terminated string" (upstream uri.c
    /// `if (len <= 0) len = strlen(str)`) — PHP's stream wrapper calls
    /// `xmlURIUnescapeString(filename, 0, NULL)` on plain file paths, so a
    /// literal 0-length decode would hand it an empty path (SP-14.3.6 W6:
    /// the xmlwriter openUri regression this guards).
    ///
    /// # Safety
    ///
    /// - `cstr` must be a valid NUL-terminated static string; the returned
    ///   buffer is allocator-owned and freed with `xmlFreeImpl`.
    #[test]
    fn test_xml_unescape_string_len_zero_means_whole() {
        unsafe {
            let cstr = c"004.xml".as_ptr() as *const c_char;
            let result = xmlURIUnescapeString(cstr, 0, ptr::null_mut());
            assert!(!result.is_null());
            let result_str = std::ffi::CStr::from_ptr(result);
            assert_eq!(result_str.to_bytes(), b"004.xml");
            allocator::xmlFreeImpl(result as *mut core::ffi::c_void);

            let cstr2 = c"a%20b.xml".as_ptr() as *const c_char;
            let result2 = xmlURIUnescapeString(cstr2, 0, ptr::null_mut());
            assert!(!result2.is_null());
            let result_str2 = std::ffi::CStr::from_ptr(result2);
            assert_eq!(result_str2.to_bytes(), b"a b.xml");
            allocator::xmlFreeImpl(result2 as *mut core::ffi::c_void);
        }
    }

    /// Parse a raw C string URI with `xmlParseURIRaw` and read fields.
    ///
    /// # Safety
    ///
    /// - `cstr` must be a valid NUL-terminated string; the returned
    ///   `CXmlUri` is valid while read and freed with `xmlFreeURI`.
    #[test]
    fn test_xml_parse_uri_raw() {
        unsafe {
            let cstr = c"http://example.com".as_ptr() as *const c_char;
            let uri = xmlParseURIRaw(cstr, 0);
            assert!(!uri.is_null());
            let parts = &*(uri as *const CXmlUri);
            assert_eq!(from_c_str(parts.scheme), Some(b"http".to_vec()));
            assert_eq!(from_c_str(parts.server), Some(b"example.com".to_vec()));
            xmlFreeURI(uri);
        }
    }

    /// `xmlFreeURI` must tolerate a NULL pointer.
    ///
    /// # Safety
    ///
    /// - `xmlFreeURI(NULL)` is a documented no-op; no pointer is
    ///   dereferenced or freed.
    #[test]
    fn test_xml_free_null() {
        unsafe {
            // Should not crash
            xmlFreeURI(ptr::null_mut());
        }
    }

    // ── Edge cases ─────────────────────────────────────────────────────────

    #[test]
    fn test_parse_uri_scheme_only() {
        let parts = parse_uri(b"http:").expect("should parse");
        assert_eq!(parts.scheme, Some(b"http".to_vec()));
        assert!(parts.opaque.is_none() || parts.opaque.as_deref() == Some(b""));
    }

    #[test]
    fn test_parse_uri_with_trailing_slash() {
        let parts = parse_uri(b"http://example.com/").expect("should parse");
        assert_eq!(parts.scheme, Some(b"http".to_vec()));
        assert_eq!(parts.path, Some(b"/".to_vec()));
    }

    #[test]
    fn test_parse_uri_with_double_slash_path() {
        let parts = parse_uri(b"http://example.com//path").expect("should parse");
        assert_eq!(parts.scheme, Some(b"http".to_vec()));
        assert_eq!(parts.path, Some(b"//path".to_vec()));
    }

    #[test]
    fn test_parse_uri_no_scheme_colon() {
        // A string with a colon but no valid scheme (doesn't start with letter)
        let parts = parse_uri(b"123:path");
        // This should be treated as a relative path, since '1' is not a letter
        assert!(parts.is_some());
        let p = parts.unwrap();
        assert!(p.scheme.is_none());
        assert_eq!(p.path, Some(b"123:path".to_vec()));
    }

    #[test]
    fn test_parse_uri_ftp_with_home_dir() {
        let parts = parse_uri(b"ftp://host/home/user/file.txt").expect("should parse");
        assert_eq!(parts.scheme, Some(b"ftp".to_vec()));
        assert_eq!(parts.host, Some(b"host".to_vec()));
        assert_eq!(parts.path, Some(b"/home/user/file.txt".to_vec()));
    }

    #[test]
    fn test_parse_uri_scheme_case() {
        let parts = parse_uri(b"HTTP://example.com/Path").expect("should parse");
        assert_eq!(parts.scheme, Some(b"HTTP".to_vec()));
        assert_eq!(parts.host, Some(b"example.com".to_vec()));
        assert_eq!(parts.path, Some(b"/Path".to_vec()));
    }

    #[test]
    fn test_normalize_path_complex() {
        assert_eq!(normalize_uri_path(b"/a/b/c/./../../g"), b"/a/g");
        assert_eq!(normalize_uri_path(b"mid/content=5/../6"), b"mid/6");
    }

    #[test]
    fn test_resolve_uri_same_directory() {
        let result =
            resolve_uri(b"http://example.com/a/b/c.html", b"d.html").expect("should resolve");
        assert_eq!(result, b"http://example.com/a/b/d.html");
    }

    #[test]
    fn test_resolve_uri_complex_traversal() {
        let result = resolve_uri(b"http://a/b/c/d;p?q", b"g/h/../i/./j#f").expect("should resolve");
        let result_str = core::str::from_utf8(&result).unwrap_or("");
        assert!(result_str.contains("http://a/b/c/g/i/j"));
    }

    // ── Hex value helper ───────────────────────────────────────────────────

    #[test]
    fn test_hex_val() {
        assert_eq!(hex_val(b'0'), Some(0));
        assert_eq!(hex_val(b'9'), Some(9));
        assert_eq!(hex_val(b'a'), Some(10));
        assert_eq!(hex_val(b'f'), Some(15));
        assert_eq!(hex_val(b'A'), Some(10));
        assert_eq!(hex_val(b'F'), Some(15));
        assert_eq!(hex_val(b'g'), None);
        assert_eq!(hex_val(b'z'), None);
        assert_eq!(hex_val(b'%'), None);
    }

    // ── Parse URI C string ─────────────────────────────────────────────────

    /// Parse a C string URI into an internal `UriParts` box and free it.
    ///
    /// # Safety
    ///
    /// - `cstr` must be a valid NUL-terminated string; the pointer
    ///   returned by `parse_uri_cstr` is non-NULL (asserted), valid for
    ///   reading while its fields are checked, and must be released with
    ///   `free_uri_parts` exactly once.
    #[test]
    fn test_parse_uri_cstr() {
        let cstr = c"http://example.com/path".as_ptr() as *const xmlChar;
        let ptr = parse_uri_cstr(cstr);
        assert!(!ptr.is_null());
        unsafe {
            assert_eq!((*ptr).scheme, Some(b"http".to_vec()));
            assert_eq!((*ptr).host, Some(b"example.com".to_vec()));
            free_uri_parts(ptr);
        }
    }

    #[test]
    fn test_parse_uri_cstr_null() {
        let ptr = parse_uri_cstr(ptr::null());
        assert!(ptr.is_null());
    }

    #[test]
    fn test_parse_uri_cstr_invalid() {
        let cstr = c"".as_ptr() as *const xmlChar;
        let ptr = parse_uri_cstr(cstr);
        assert!(ptr.is_null());
    }
}
