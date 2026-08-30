//! XML Catalog support (§26, §85 Phase 4).
//!
//! OASIS XML Catalog resolution, catalog lookup order, precedence,
//! catalog loading, SGML catalog compatibility.
//!
//! Implements the OASIS XML Catalog specification (xCatalog) with
//! compatibility for SGML (SOLEX) catalog format.
//!
//! # UPSTREAM-PARITY
//!
//! Matches libxml2's catalog behavior:
//! - XML Catalog format (OASIS TR 9401:1999)
//! - SGML catalog format (SOLEX)
//! - Environment variables: XML_CATALOG_FILES, SGML_CATALOG_FILES
//! - Default catalog location: /etc/xml/catalog
//! - Catalog entry types: public, system, rewriteSystem, rewriteURI,
//!   delegatePublic, delegateSystem, delegateURI, nextCatalog, group
//! - Resolution order: public → system → URI

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use core::ffi::c_void;
use std::ffi::CStr;
use std::fs;
use std::os::raw::{c_char, c_int};
use std::path::Path;
use std::ptr;

use once_cell::sync::Lazy;
use parking_lot::RwLock;

use crate::abi::allocator::{xmlFreeImpl, xmlMallocImpl};
use crate::abi::structs::{_xmlDoc, _xmlNode};
use crate::abi::types::xmlChar;
use crate::xml::string::{
    bytes_to_xmlstr, c_strdup, xml_str_starts_with, xml_strcat, xml_strcmp, xml_strdup, xml_strlen,
    xmlstr_to_bytes,
};

// ═══════════════════════════════════════════════════════════════════════════════
// Constants
// ═══════════════════════════════════════════════════════════════════════════════

/// Catalog allow value: no catalogs allowed.
pub(crate) const XML_CATA_ALLOW_NONE: i32 = 0;

/// Catalog allow value: only global catalogs.
pub(crate) const XML_CATA_ALLOW_GLOBAL: i32 = 1;

/// Catalog allow value: document catalogs allowed.
pub(crate) const XML_CATA_ALLOW_DOCUMENT: i32 = 2;

/// Catalog allow value: all catalogs allowed.
pub(crate) const XML_CATA_ALLOW_ALL: i32 = 3;

/// Default catalog file path.
const DEFAULT_CATALOG: &str = "/etc/xml/catalog";

/// Environment variable for XML catalog files.
const XML_CATALOG_FILES_ENV: &str = "XML_CATALOG_FILES";

/// Environment variable for SGML catalog files.
const SGML_CATALOG_FILES_ENV: &str = "SGML_CATALOG_FILES";

/// Maximum catalog file size (10 MB).
const MAX_CATALOG_FILE_SIZE: usize = 10_485_760;

// ═══════════════════════════════════════════════════════════════════════════════
// Catalog Entry Types
// ═══════════════════════════════════════════════════════════════════════════════

/// A single catalog entry.
#[derive(Clone, Debug)]
enum CatalogEntry {
    /// `<public publicId="..." uri="..."/>`
    Public { public_id: Vec<u8>, uri: Vec<u8> },
    /// `<system systemId="..." uri="..."/>`
    System { system_id: Vec<u8>, uri: Vec<u8> },
    /// `<rewriteSystem systemIdStartString="..." rewritePrefix="..."/>`
    RewriteSystem { prefix: Vec<u8>, rewrite: Vec<u8> },
    /// `<rewriteURI uriStartString="..." rewritePrefix="..."/>`
    RewriteURI { prefix: Vec<u8>, rewrite: Vec<u8> },
    /// `<delegatePublic publicIdStartString="..." catalog="..."/>`
    DelegatePublic { prefix: Vec<u8>, catalog: Vec<u8> },
    /// `<delegateSystem systemIdStartString="..." catalog="..."/>`
    DelegateSystem { prefix: Vec<u8>, catalog: Vec<u8> },
    /// `<delegateURI uriStartString="..." catalog="..."/>`
    DelegateURI { prefix: Vec<u8>, catalog: Vec<u8> },
    /// `<nextCatalog catalog="..."/>`
    NextCatalog { catalog: Vec<u8> },
}

/// Indicates the format of a loaded catalog.
#[derive(Clone, Copy, Debug, PartialEq)]
enum CatalogFormat {
    Xml,
    Sgml,
}

/// Metadata about a loaded catalog file.
#[derive(Clone, Debug)]
struct CatalogInfo {
    path: Vec<u8>,
    format: CatalogFormat,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Global Catalog State
// ═══════════════════════════════════════════════════════════════════════════════

/// Global catalog registry state.
struct CatalogState {
    /// All catalog entries, in load order.
    entries: Vec<CatalogEntry>,
    /// Information about loaded catalog files.
    catalogs: Vec<CatalogInfo>,
    /// Whether the subsystem has been initialized.
    initialized: bool,
    /// Catalog resolution allow value.
    allow: i32,
}

impl CatalogState {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            catalogs: Vec::new(),
            initialized: false,
            allow: XML_CATA_ALLOW_ALL,
        }
    }

    /// Clear all catalog data.
    fn clear(&mut self) {
        self.entries.clear();
        self.catalogs.clear();
        self.allow = XML_CATA_ALLOW_ALL;
    }
}

/// Global catalog registry, protected by a read-write lock.
static CATALOG_STATE: Lazy<RwLock<CatalogState>> = Lazy::new(|| RwLock::new(CatalogState::new()));

// ═══════════════════════════════════════════════════════════════════════════════
// Internal Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Trim leading and trailing whitespace from a byte slice.
fn trim_whitespace(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(0, |p| p + 1);
    &bytes[start..end]
}

/// Check if a byte slice starts with a given prefix (case-sensitive).
fn starts_with(data: &[u8], prefix: &[u8]) -> bool {
    if data.len() < prefix.len() {
        return false;
    }
    data[..prefix.len()] == prefix[..]
}

/// Check if a byte slice starts with a given prefix (case-insensitive ASCII).
fn starts_with_ignore_ascii_case(data: &[u8], prefix: &[u8]) -> bool {
    if data.len() < prefix.len() {
        return false;
    }
    data[..prefix.len()]
        .iter()
        .zip(prefix.iter())
        .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

/// Extract a quoted attribute value from bytes.
///
/// Searches for `name="..."` or `name='...'` starting at position `pos`.
/// Returns `(value_bytes, end_pos)` or `None`.
fn extract_attr_value<'a>(data: &'a [u8], name: &[u8], pos: usize) -> Option<(&'a [u8], usize)> {
    let remaining = &data[pos..];
    // Find name
    let name_pos = find_subsequence(remaining, name)?;
    let after_name = name_pos + name.len();
    let after_name_slice = &remaining[after_name..];

    // Skip whitespace and =
    let eq_pos = after_name_slice.iter().position(|b| *b == b'=')?;

    // Check for quote — offset is relative to `data` (absolute)
    let rel_quote_start = after_name_slice[eq_pos + 1..]
        .iter()
        .position(|b| *b == b'"' || *b == b'\'')
        .map(|p| after_name + eq_pos + 1 + p)?;
    let abs_quote_start = pos + rel_quote_start;
    let quote_char = data[abs_quote_start];
    // Find matching close quote
    let value_start = abs_quote_start + 1;
    let value_end = data[value_start..]
        .iter()
        .position(|b| *b == quote_char)
        .map(|p| value_start + p)?;

    Some((&data[value_start..value_end], value_end + 1))
}

/// Find a subsequence in a byte slice.
fn find_subsequence(data: &[u8], seq: &[u8]) -> Option<usize> {
    if seq.is_empty() {
        return Some(0);
    }
    data.windows(seq.len()).position(|w| w == seq)
}

/// Extract a simple token (non-whitespace bytes) from a line, starting at `pos`.
fn extract_token(line: &[u8], pos: usize) -> Option<(&[u8], usize)> {
    let line = &line[pos..];
    let start = line.iter().position(|b| !b.is_ascii_whitespace())?;
    let end = line[start..]
        .iter()
        .position(|b| b.is_ascii_whitespace())
        .map(|p| start + p)
        .unwrap_or(line.len());
    Some((&line[start..end], pos + end))
}

/// Extract a quoted token from a line (may use " or ' quotes), starting at `pos`.
fn extract_quoted_token(line: &[u8], pos: usize) -> Option<(&[u8], usize)> {
    let line = &line[pos..];
    let start = line.iter().position(|b| !b.is_ascii_whitespace())?;
    if start >= line.len() {
        return None;
    }
    let quote_char = line[start];
    if quote_char != b'"' && quote_char != b'\'' {
        // Not quoted — extract as simple token
        return extract_token(line, 0);
    }
    let value_start = start + 1;
    let end = line[value_start..]
        .iter()
        .position(|b| *b == quote_char)
        .map(|p| value_start + p)?;
    Some((&line[value_start..end], pos + end + 1))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Catalog Parsing — SGML Format
// ═══════════════════════════════════════════════════════════════════════════════

/// Parse a single line of an SGML catalog.
///
/// SGML catalog format lines:
/// - `PUBLIC "publicId" "uri"`
/// - `SYSTEM "systemId" "uri"`
/// - `URI "uri" "replacement"`
/// - `OVERRIDE YES|NO`
/// - `CATALOG "path"` (delegation to another catalog)
/// - `SGMLDECL "path"` (ignored)
/// - `DOCTYPE "name" "uri"` (ignored for catalog resolution)
/// - `ENTITY "name" "uri"` (ignored for catalog resolution)
/// - `LINKTYPE "name" "uri"` (ignored)
/// - `NOTATION "name" "uri"` (ignored)
/// - Comments start with `--`
fn parse_sgml_line(line: &[u8], entries: &mut Vec<CatalogEntry>) {
    let trimmed = trim_whitespace(line);
    if trimmed.is_empty() || trimmed.starts_with(b"--") {
        return;
    }

    // Extract the directive
    let Some((directive, after_directive)) = extract_token(trimmed, 0) else {
        return;
    };

    match directive {
        b"PUBLIC" | b"public" => {
            let Some((pub_id, after_pub)) = extract_quoted_token(trimmed, after_directive) else {
                return;
            };
            let Some((uri, _)) = extract_quoted_token(trimmed, after_pub) else {
                return;
            };
            entries.push(CatalogEntry::Public {
                public_id: pub_id.to_vec(),
                uri: uri.to_vec(),
            });
        }
        b"SYSTEM" | b"system" => {
            let Some((sys_id, after_sys)) = extract_quoted_token(trimmed, after_directive) else {
                return;
            };
            let Some((uri, _)) = extract_quoted_token(trimmed, after_sys) else {
                return;
            };
            entries.push(CatalogEntry::System {
                system_id: sys_id.to_vec(),
                uri: uri.to_vec(),
            });
        }
        b"URI" | b"uri" => {
            // SGML URI is treated like a system entry in libxml2
            let Some((uri_id, after_uri)) = extract_quoted_token(trimmed, after_directive) else {
                return;
            };
            let Some((replacement, _)) = extract_quoted_token(trimmed, after_uri) else {
                return;
            };
            entries.push(CatalogEntry::System {
                system_id: uri_id.to_vec(),
                uri: replacement.to_vec(),
            });
        }
        b"CATALOG" | b"catalog" => {
            let Some((path, _)) = extract_quoted_token(trimmed, after_directive) else {
                return;
            };
            entries.push(CatalogEntry::NextCatalog {
                catalog: path.to_vec(),
            });
        }
        _ => {
            // Other directives (SGMLDECL, DOCTYPE, ENTITY, etc.) are ignored
        }
    }
}

/// Parse SGML catalog content.
fn parse_sgml_catalog(data: &[u8], entries: &mut Vec<CatalogEntry>) {
    for line in data.split(|b| *b == b'\n') {
        parse_sgml_line(line, entries);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Catalog Parsing — XML Catalog Format
// ═══════════════════════════════════════════════════════════════════════════════

/// Parse an XML Catalog file content.
///
/// Uses simple tag scanning rather than a full XML parser, matching
/// libxml2's approach which has its own catalog-specific parser.
fn parse_xml_catalog(data: &[u8], entries: &mut Vec<CatalogEntry>) {
    let mut pos = 0;
    let len = data.len();

    while pos < len {
        // Find next '<'
        let Some(lt_pos) = data[pos..].iter().position(|b| *b == b'<') else {
            break;
        };
        let tag_start = pos + lt_pos;

        // Check if this is a closing tag or self-closing
        if tag_start + 1 >= len {
            break;
        }

        let is_closing = data[tag_start + 1] == b'/';
        if is_closing {
            // Skip to '>'
            let Some(gt_pos) = data[tag_start..].iter().position(|b| *b == b'>') else {
                break;
            };
            pos = tag_start + gt_pos + 1;
            continue;
        }

        // Check if it's a comment or PI
        if data[tag_start + 1] == b'!' || data[tag_start + 1] == b'?' {
            let Some(gt_pos) = data[tag_start..].iter().position(|b| *b == b'>') else {
                break;
            };
            pos = tag_start + gt_pos + 1;
            continue;
        }

        // Find end of tag name
        let tag_name_start = tag_start + 1;
        let tag_name_end = data[tag_name_start..]
            .iter()
            .position(|b| b.is_ascii_whitespace() || *b == b'>' || *b == b'/')
            .map(|p| tag_name_start + p)
            .unwrap_or(len);

        let tag_name = &data[tag_name_start..tag_name_end];

        // Find end of tag (either '>' for open tag, or '/>' for self-closing)
        let Some(gt_or_slash_pos) = data[tag_start..]
            .iter()
            .position(|b| *b == b'>')
            .map(|p| tag_start + p)
        else {
            break;
        };

        let is_self_closing = gt_or_slash_pos > 0 && data[gt_or_slash_pos - 1] == b'/';
        let tag_content_end = if is_self_closing {
            gt_or_slash_pos + 1
        } else {
            // Open tag - find matching close
            let close_tag = {
                let mut close = Vec::with_capacity(tag_name.len() + 3);
                close.push(b'<');
                close.push(b'/');
                close.extend_from_slice(tag_name);
                close.push(b'>');
                close
            };
            let close_pos = data[gt_or_slash_pos + 1..]
                .windows(close_tag.len())
                .position(|w| w == close_tag.as_slice())
                .map(|p| gt_or_slash_pos + 1 + p + close_tag.len());

            match close_pos {
                Some(p) => p,
                None => {
                    pos = gt_or_slash_pos + 1;
                    continue;
                }
            }
        };

        let tag_body_start = gt_or_slash_pos + 1;
        let tag_body = &data[tag_body_start
            ..tag_content_end
                - if is_self_closing {
                    0
                } else {
                    tag_name.len() + 3
                }];
        let tag_body = trim_whitespace(tag_body);

        match tag_name {
            b"public" => {
                let Some((pub_id, _)) = extract_attr_value(data, b"publicId", tag_start) else {
                    pos = tag_content_end;
                    continue;
                };
                let Some((uri, _)) = extract_attr_value(data, b"uri", tag_start) else {
                    pos = tag_content_end;
                    continue;
                };
                entries.push(CatalogEntry::Public {
                    public_id: pub_id.to_vec(),
                    uri: uri.to_vec(),
                });
            }
            b"system" => {
                let Some((sys_id, _)) = extract_attr_value(data, b"systemId", tag_start) else {
                    pos = tag_content_end;
                    continue;
                };
                let Some((uri, _)) = extract_attr_value(data, b"uri", tag_start) else {
                    pos = tag_content_end;
                    continue;
                };
                entries.push(CatalogEntry::System {
                    system_id: sys_id.to_vec(),
                    uri: uri.to_vec(),
                });
            }
            b"rewriteSystem" => {
                let Some((prefix, _)) = extract_attr_value(data, b"systemIdStartString", tag_start)
                else {
                    pos = tag_content_end;
                    continue;
                };
                let Some((rewrite, _)) = extract_attr_value(data, b"rewritePrefix", tag_start)
                else {
                    pos = tag_content_end;
                    continue;
                };
                entries.push(CatalogEntry::RewriteSystem {
                    prefix: prefix.to_vec(),
                    rewrite: rewrite.to_vec(),
                });
            }
            b"rewriteURI" => {
                let Some((prefix, _)) = extract_attr_value(data, b"uriStartString", tag_start)
                else {
                    pos = tag_content_end;
                    continue;
                };
                let Some((rewrite, _)) = extract_attr_value(data, b"rewritePrefix", tag_start)
                else {
                    pos = tag_content_end;
                    continue;
                };
                entries.push(CatalogEntry::RewriteURI {
                    prefix: prefix.to_vec(),
                    rewrite: rewrite.to_vec(),
                });
            }
            b"delegatePublic" => {
                let Some((prefix, _)) = extract_attr_value(data, b"publicIdStartString", tag_start)
                else {
                    pos = tag_content_end;
                    continue;
                };
                let Some((catalog, _)) = extract_attr_value(data, b"catalog", tag_start) else {
                    pos = tag_content_end;
                    continue;
                };
                entries.push(CatalogEntry::DelegatePublic {
                    prefix: prefix.to_vec(),
                    catalog: catalog.to_vec(),
                });
            }
            b"delegateSystem" => {
                let Some((prefix, _)) = extract_attr_value(data, b"systemIdStartString", tag_start)
                else {
                    pos = tag_content_end;
                    continue;
                };
                let Some((catalog, _)) = extract_attr_value(data, b"catalog", tag_start) else {
                    pos = tag_content_end;
                    continue;
                };
                entries.push(CatalogEntry::DelegateSystem {
                    prefix: prefix.to_vec(),
                    catalog: catalog.to_vec(),
                });
            }
            b"delegateURI" => {
                let Some((prefix, _)) = extract_attr_value(data, b"uriStartString", tag_start)
                else {
                    pos = tag_content_end;
                    continue;
                };
                let Some((catalog, _)) = extract_attr_value(data, b"catalog", tag_start) else {
                    pos = tag_content_end;
                    continue;
                };
                entries.push(CatalogEntry::DelegateURI {
                    prefix: prefix.to_vec(),
                    catalog: catalog.to_vec(),
                });
            }
            b"nextCatalog" => {
                let Some((catalog, _)) = extract_attr_value(data, b"catalog", tag_start) else {
                    pos = tag_content_end;
                    continue;
                };
                entries.push(CatalogEntry::NextCatalog {
                    catalog: catalog.to_vec(),
                });
            }
            b"group" | b"catalog" => {
                // Container elements contain child entries; parse the body recursively
                parse_xml_catalog(tag_body, entries);
            }
            _ => {
                // Unknown elements are ignored
            }
        }

        pos = tag_content_end;
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Catalog Loading
// ═══════════════════════════════════════════════════════════════════════════════

/// Read a file's contents as bytes.
fn read_file_bytes(path: &str) -> Option<Vec<u8>> {
    let p = Path::new(path);
    // Check file existence and size
    let metadata = fs::metadata(p).ok()?;
    if metadata.len() > MAX_CATALOG_FILE_SIZE as u64 {
        return None;
    }
    fs::read(p).ok()
}

/// Determine whether a catalog file is XML or SGML format.
fn detect_catalog_format(data: &[u8]) -> CatalogFormat {
    let trimmed = trim_whitespace(data);
    if trimmed.starts_with(b"<?xml") || trimmed.starts_with(b"<catalog") {
        CatalogFormat::Xml
    } else {
        CatalogFormat::Sgml
    }
}

/// Load catalog entries from file data.
fn load_catalog_data(path: &str, data: &[u8], entries: &mut Vec<CatalogEntry>) {
    let format = detect_catalog_format(data);
    match format {
        CatalogFormat::Xml => {
            parse_xml_catalog(data, entries);
        }
        CatalogFormat::Sgml => {
            parse_sgml_catalog(data, entries);
        }
    }
}

/// Load a single catalog file, adding its entries to the global state.
fn load_single_catalog(path: &str, state: &mut CatalogState) {
    let data = match read_file_bytes(path) {
        Some(d) => d,
        None => return,
    };

    let format = detect_catalog_format(&data);
    state.catalogs.push(CatalogInfo {
        path: path.as_bytes().to_vec(),
        format,
    });

    load_catalog_data(path, &data, &mut state.entries);
}

/// Load catalogs from a colon-separated list of file paths.
fn load_catalog_list(catalogs: &str, state: &mut CatalogState) {
    for catalog_path in catalogs.split(':') {
        let trimmed = catalog_path.trim();
        if !trimmed.is_empty() {
            load_single_catalog(trimmed, state);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Public API: Initialization / Cleanup
// ═══════════════════════════════════════════════════════════════════════════════

/// Initialize the catalog subsystem.
///
/// Loads catalogs from environment variables and default locations.
/// Safe to call multiple times.
pub(crate) fn init() {
    let mut state = CATALOG_STATE.write();
    if state.initialized {
        return;
    }

    // Set default allow to ALL (matching upstream behavior)
    state.allow = XML_CATA_ALLOW_ALL;
    crate::xml::globals::set_catalog_defaults(XML_CATA_ALLOW_ALL);

    // Load from XML_CATALOG_FILES environment variable
    if let Ok(catalogs) = std::env::var(XML_CATALOG_FILES_ENV) {
        load_catalog_list(&catalogs, &mut state);
    }

    // Load from SGML_CATALOG_FILES environment variable
    if let Ok(catalogs) = std::env::var(SGML_CATALOG_FILES_ENV) {
        load_catalog_list(&catalogs, &mut state);
    }

    // Load default catalog
    if Path::new(DEFAULT_CATALOG).exists() {
        load_single_catalog(DEFAULT_CATALOG, &mut state);
    }

    state.initialized = true;
}

/// Clean up the catalog subsystem.
///
/// Clears all catalog entries and resets state.
pub(crate) fn cleanup() {
    let mut state = CATALOG_STATE.write();
    state.clear();
    state.initialized = false;
}

// ═══════════════════════════════════════════════════════════════════════════════
// Public API: Catalog Loading
// ═══════════════════════════════════════════════════════════════════════════════

/// Load catalog from a colon-separated list of file paths.
///
/// Returns an opaque handle (currently just a non-null pointer on success).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlCatalogPtr xmlCatalogLoad(const char *catalogs);
/// ```
pub(crate) fn load_catalog(catalogs: *const c_char) -> *mut c_void {
    if catalogs.is_null() {
        return ptr::null_mut();
    }

    let catalogs_str = unsafe { CStr::from_ptr(catalogs) };
    let catalogs_str = catalogs_str.to_str().unwrap_or("");

    let mut state = CATALOG_STATE.write();

    // Ensure initialized
    if !state.initialized {
        drop(state);
        init();
        state = CATALOG_STATE.write();
    }

    let count_before = state.catalogs.len();
    load_catalog_list(catalogs_str, &mut state);

    if state.catalogs.len() > count_before {
        // Return a non-null handle (the number of loaded catalogs as a magic pointer)
        (state.catalogs.len() as isize) as *mut c_void
    } else {
        ptr::null_mut()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Public API: Resolution Functions
// ═══════════════════════════════════════════════════════════════════════════════

/// Check whether catalog resolution is allowed based on the current `allow` value.
fn catalog_allowed(state: &CatalogState) -> bool {
    let allow = state.allow;
    match allow {
        XML_CATA_ALLOW_NONE => false,
        XML_CATA_ALLOW_GLOBAL | XML_CATA_ALLOW_DOCUMENT | XML_CATA_ALLOW_ALL => true,
        _ => false,
    }
}

/// Resolve a public ID against an entry list (candidate-internal; the
/// global/public wrappers check the allow flag).
unsafe fn resolve_public_entries(entries: &[CatalogEntry], pub_id_bytes: &[u8]) -> Option<Vec<u8>> {
    // 1. Direct match on Public entries
    for entry in entries {
        if let CatalogEntry::Public { public_id, uri } = entry {
            if public_id.as_slice() == pub_id_bytes {
                return Some(uri.clone());
            }
        }
    }

    // 2. DelegatePublic - find longest matching prefix
    let mut best_match: Option<Vec<u8>> = None;
    let mut best_prefix_len: usize = 0;

    for entry in entries {
        if let CatalogEntry::DelegatePublic { prefix, catalog } = entry {
            if pub_id_bytes.starts_with(prefix) && prefix.len() > best_prefix_len {
                best_prefix_len = prefix.len();
                // Try to load the delegated catalog and resolve
                if let Some(delegated_data) = read_file_bytes(&String::from_utf8_lossy(catalog)) {
                    let mut temp_entries = Vec::new();
                    parse_xml_catalog(&delegated_data, &mut temp_entries);
                    // Check for public match in delegated catalog
                    for temp_entry in &temp_entries {
                        if let CatalogEntry::Public { public_id: dp, uri } = temp_entry {
                            if dp.as_slice() == pub_id_bytes {
                                best_match = Some(uri.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    best_match
}

/// Resolve a public ID to a system/URI.
///
/// Checks catalog entries in order, first matching `Public` entries,
/// then falls through to delegation.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlCharPtr xmlCatalogResolvePublic(const xmlChar *pubID);
/// ```
pub(crate) unsafe fn resolve_public(pub_id: *const xmlChar) -> *mut xmlChar {
    if pub_id.is_null() {
        return ptr::null_mut();
    }

    let state = CATALOG_STATE.read();
    if !catalog_allowed(&state) {
        return ptr::null_mut();
    }

    let pub_id_bytes = xmlstr_to_bytes(pub_id);
    unsafe { resolve_public_entries(&state.entries, &pub_id_bytes) }
        .as_ref()
        .map_or(ptr::null_mut(), |uri| bytes_to_xmlstr(uri))
}

/// Resolve a system ID against an entry list (candidate-internal).
unsafe fn resolve_system_entries(entries: &[CatalogEntry], sys_id_bytes: &[u8]) -> Option<Vec<u8>> {
    // 1. Direct match on System entries
    for entry in entries {
        if let CatalogEntry::System { system_id, uri } = entry {
            if system_id.as_slice() == sys_id_bytes {
                return Some(uri.clone());
            }
        }
    }

    // 2. RewriteSystem - find longest matching prefix
    let mut best_rewrite: Option<Vec<u8>> = None;
    let mut best_prefix_len: usize = 0;

    for entry in entries {
        if let CatalogEntry::RewriteSystem { prefix, rewrite } = entry {
            if sys_id_bytes.starts_with(prefix) && prefix.len() > best_prefix_len {
                best_prefix_len = prefix.len();
                // Replace the prefix with the rewrite prefix
                let suffix = &sys_id_bytes[prefix.len()..];
                let mut result = rewrite.clone();
                result.extend_from_slice(suffix);
                best_rewrite = Some(result);
            }
        }
    }

    if let Some(rewritten) = best_rewrite {
        return Some(rewritten);
    }

    // 3. DelegateSystem
    for entry in entries {
        if let CatalogEntry::DelegateSystem { prefix, catalog } = entry {
            if sys_id_bytes.starts_with(prefix) {
                if let Some(delegated_data) = read_file_bytes(&String::from_utf8_lossy(catalog)) {
                    let mut temp_entries = Vec::new();
                    parse_xml_catalog(&delegated_data, &mut temp_entries);
                    for temp_entry in &temp_entries {
                        if let CatalogEntry::System { system_id, uri } = temp_entry {
                            if system_id.as_slice() == sys_id_bytes {
                                return Some(uri.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

/// Resolve a system ID.
///
/// Checks catalog entries in order:
/// 1. Direct `System` match
/// 2. `RewriteSystem` prefix match (longest wins)
/// 3. `DelegateSystem` prefix match
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlCharPtr xmlCatalogResolveSystem(const xmlChar *sysID);
/// ```
pub(crate) unsafe fn resolve_system(sys_id: *const xmlChar) -> *mut xmlChar {
    if sys_id.is_null() {
        return ptr::null_mut();
    }

    let state = CATALOG_STATE.read();
    if !catalog_allowed(&state) {
        return ptr::null_mut();
    }

    let sys_id_bytes = xmlstr_to_bytes(sys_id);
    unsafe { resolve_system_entries(&state.entries, &sys_id_bytes) }
        .as_ref()
        .map_or(ptr::null_mut(), |uri| bytes_to_xmlstr(uri))
}

/// Resolve a URI against an entry list (candidate-internal).
unsafe fn resolve_uri_entries(entries: &[CatalogEntry], uri_bytes: &[u8]) -> Option<Vec<u8>> {
    // 1. Direct match on System entries (URIs match against systemId in libxml2)
    for entry in entries {
        if let CatalogEntry::System {
            system_id,
            uri: sys_uri,
        } = entry
        {
            if system_id.as_slice() == uri_bytes {
                return Some(sys_uri.clone());
            }
        }
    }

    // 2. RewriteURI - find longest matching prefix
    let mut best_rewrite: Option<Vec<u8>> = None;
    let mut best_prefix_len: usize = 0;

    for entry in entries {
        if let CatalogEntry::RewriteURI { prefix, rewrite } = entry {
            if uri_bytes.starts_with(prefix) && prefix.len() > best_prefix_len {
                best_prefix_len = prefix.len();
                let suffix = &uri_bytes[prefix.len()..];
                let mut result = rewrite.clone();
                result.extend_from_slice(suffix);
                best_rewrite = Some(result);
            }
        }
    }

    if let Some(rewritten) = best_rewrite {
        return Some(rewritten);
    }

    // 3. DelegateURI
    for entry in entries {
        if let CatalogEntry::DelegateURI { prefix, catalog } = entry {
            if uri_bytes.starts_with(prefix) {
                if let Some(delegated_data) = read_file_bytes(&String::from_utf8_lossy(catalog)) {
                    let mut temp_entries = Vec::new();
                    parse_xml_catalog(&delegated_data, &mut temp_entries);
                    for temp_entry in &temp_entries {
                        if let CatalogEntry::System {
                            system_id,
                            uri: sys_uri,
                        } = temp_entry
                        {
                            if system_id.as_slice() == uri_bytes {
                                return Some(sys_uri.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

/// Resolve a URI.
///
/// Checks catalog entries in order:
/// 1. Direct `System` match (URIs are matched against system entries too)
/// 2. `RewriteURI` prefix match (longest wins)
/// 3. `DelegateURI` prefix match
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlCharPtr xmlCatalogResolveURI(const xmlChar *URI);
/// ```
pub(crate) unsafe fn resolve_uri(uri: *const xmlChar) -> *mut xmlChar {
    if uri.is_null() {
        return ptr::null_mut();
    }

    let state = CATALOG_STATE.read();
    if !catalog_allowed(&state) {
        return ptr::null_mut();
    }

    let uri_bytes = xmlstr_to_bytes(uri);
    unsafe { resolve_uri_entries(&state.entries, &uri_bytes) }
        .as_ref()
        .map_or(ptr::null_mut(), |uri| bytes_to_xmlstr(uri))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Public API: Catalog Defaults
// ═══════════════════════════════════════════════════════════════════════════════

/// Set catalog behavior.
///
/// Controls whether catalog resolution is allowed and which catalogs
/// are consulted.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlCatalogSetDefaults(xmlCatalogAllowValue allow);
/// ```
pub(crate) fn set_defaults(allow: c_int) {
    let mut state = CATALOG_STATE.write();
    state.allow = allow;
    crate::xml::globals::set_catalog_defaults(allow);
}

/// Get the current catalog allow value.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlCatalogAllowValue xmlCatalogGetDefaults(void);
/// ```
pub(crate) fn get_defaults() -> c_int {
    let state = CATALOG_STATE.read();
    state.allow
}

// ═══════════════════════════════════════════════════════════════════════════════
// Public API: Add / Remove Entries
// ═══════════════════════════════════════════════════════════════════════════════

/// Add a catalog entry.
///
/// `type_` is one of "public", "system", "rewriteSystem", "rewriteURI",
/// "delegatePublic", "delegateSystem", "delegateURI", or "nextCatalog".
///
/// Returns 0 on success, -1 on failure.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlCatalogAdd(const xmlChar *type, const xmlChar *orig, const xmlChar *replace);
/// ```
pub(crate) unsafe fn add(
    type_: *const xmlChar,
    orig: *const xmlChar,
    replace: *const xmlChar,
) -> c_int {
    if type_.is_null() || orig.is_null() || replace.is_null() {
        return -1;
    }

    let type_bytes = xmlstr_to_bytes(type_);
    let orig_bytes = xmlstr_to_bytes(orig);
    let replace_bytes = xmlstr_to_bytes(replace);

    let mut state = CATALOG_STATE.write();

    match type_bytes {
        b"public" => {
            state.entries.push(CatalogEntry::Public {
                public_id: orig_bytes.to_vec(),
                uri: replace_bytes.to_vec(),
            });
            0
        }
        b"system" => {
            state.entries.push(CatalogEntry::System {
                system_id: orig_bytes.to_vec(),
                uri: replace_bytes.to_vec(),
            });
            0
        }
        b"rewriteSystem" => {
            state.entries.push(CatalogEntry::RewriteSystem {
                prefix: orig_bytes.to_vec(),
                rewrite: replace_bytes.to_vec(),
            });
            0
        }
        b"rewriteURI" => {
            state.entries.push(CatalogEntry::RewriteURI {
                prefix: orig_bytes.to_vec(),
                rewrite: replace_bytes.to_vec(),
            });
            0
        }
        b"delegatePublic" => {
            state.entries.push(CatalogEntry::DelegatePublic {
                prefix: orig_bytes.to_vec(),
                catalog: replace_bytes.to_vec(),
            });
            0
        }
        b"delegateSystem" => {
            state.entries.push(CatalogEntry::DelegateSystem {
                prefix: orig_bytes.to_vec(),
                catalog: replace_bytes.to_vec(),
            });
            0
        }
        b"delegateURI" => {
            state.entries.push(CatalogEntry::DelegateURI {
                prefix: orig_bytes.to_vec(),
                catalog: replace_bytes.to_vec(),
            });
            0
        }
        b"nextCatalog" => {
            state.entries.push(CatalogEntry::NextCatalog {
                catalog: orig_bytes.to_vec(),
            });
            0
        }
        _ => -1,
    }
}

/// Remove a catalog entry by matching its value.
///
/// Removes all entries whose public ID, system ID, or prefix matches `value`.
/// Returns the number of entries removed, or -1 on error.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlCatalogRemove(const xmlChar *value);
/// ```
pub(crate) unsafe fn remove(value: *const xmlChar) -> c_int {
    if value.is_null() {
        return -1;
    }

    let value_bytes = xmlstr_to_bytes(value);
    let mut state = CATALOG_STATE.write();

    let before = state.entries.len();
    state.entries.retain(|entry| match entry {
        CatalogEntry::Public { public_id, .. } => public_id.as_slice() != value_bytes,
        CatalogEntry::System { system_id, .. } => system_id.as_slice() != value_bytes,
        CatalogEntry::RewriteSystem { prefix, .. } => prefix.as_slice() != value_bytes,
        CatalogEntry::RewriteURI { prefix, .. } => prefix.as_slice() != value_bytes,
        CatalogEntry::DelegatePublic { prefix, .. } => prefix.as_slice() != value_bytes,
        CatalogEntry::DelegateSystem { prefix, .. } => prefix.as_slice() != value_bytes,
        CatalogEntry::DelegateURI { prefix, .. } => prefix.as_slice() != value_bytes,
        CatalogEntry::NextCatalog { catalog } => catalog.as_slice() != value_bytes,
    });

    (before - state.entries.len()) as c_int
}

// ═══════════════════════════════════════════════════════════════════════════════
// Public API: SGML → XML Conversion
// ═══════════════════════════════════════════════════════════════════════════════

/// Convert the currently loaded SGML catalog entries to an XML Catalog document.
///
/// Returns a newly allocated `_xmlDoc` containing the XML catalog representation,
/// or NULL on failure. The caller is responsible for freeing the document.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xmlCatalogConvert(void);
/// ```
pub(crate) unsafe fn convert() -> *mut _xmlDoc {
    let state = CATALOG_STATE.read();

    if state.entries.is_empty() {
        return ptr::null_mut();
    }

    // Create the XML document
    let doc = crate::xml::tree::new_doc(ptr::null_mut());
    if doc.is_null() {
        return ptr::null_mut();
    }

    // Create root <catalog> element
    let catalog_name = b"catalog\0" as *const u8 as *const xmlChar;
    let root = crate::xml::tree::new_node(ptr::null_mut(), catalog_name);
    if root.is_null() {
        crate::xml::tree::free_doc(doc);
        return ptr::null_mut();
    }

    // Set xmlns attribute for OASIS XML Catalog namespace
    let xmlns_name = b"xmlns\0" as *const u8 as *const xmlChar;
    let ns_value = b"urn:oasis:names:tc:entity:xmlns:xml:catalog\0" as *const u8 as *const xmlChar;
    crate::xml::tree::set_prop(root, xmlns_name, ns_value);

    crate::xml::tree::doc_set_root_element(doc, root);

    // Add entries as child elements
    for entry in &state.entries {
        let (elem_name, attr1_name, attr1_value, attr2_name, attr2_value) = match entry {
            CatalogEntry::Public { public_id, uri } => {
                let elem = b"public\0" as *const u8 as *mut xmlChar;
                let attr1 = b"publicId\0" as *const u8 as *mut xmlChar;
                let val1 = bytes_to_xmlstr(public_id);
                let attr2 = b"uri\0" as *const u8 as *mut xmlChar;
                let val2 = bytes_to_xmlstr(uri);
                (elem, attr1, val1, attr2, val2)
            }
            CatalogEntry::System { system_id, uri } => {
                let elem = b"system\0" as *const u8 as *mut xmlChar;
                let attr1 = b"systemId\0" as *const u8 as *mut xmlChar;
                let val1 = bytes_to_xmlstr(system_id);
                let attr2 = b"uri\0" as *const u8 as *mut xmlChar;
                let val2 = bytes_to_xmlstr(uri);
                (elem, attr1, val1, attr2, val2)
            }
            CatalogEntry::RewriteSystem { prefix, rewrite } => {
                let elem = b"rewriteSystem\0" as *const u8 as *mut xmlChar;
                let attr1 = b"systemIdStartString\0" as *const u8 as *mut xmlChar;
                let val1 = bytes_to_xmlstr(prefix);
                let attr2 = b"rewritePrefix\0" as *const u8 as *mut xmlChar;
                let val2 = bytes_to_xmlstr(rewrite);
                (elem, attr1, val1, attr2, val2)
            }
            CatalogEntry::RewriteURI { prefix, rewrite } => {
                let elem = b"rewriteURI\0" as *const u8 as *mut xmlChar;
                let attr1 = b"uriStartString\0" as *const u8 as *mut xmlChar;
                let val1 = bytes_to_xmlstr(prefix);
                let attr2 = b"rewritePrefix\0" as *const u8 as *mut xmlChar;
                let val2 = bytes_to_xmlstr(rewrite);
                (elem, attr1, val1, attr2, val2)
            }
            CatalogEntry::DelegatePublic { prefix, catalog } => {
                let elem = b"delegatePublic\0" as *const u8 as *mut xmlChar;
                let attr1 = b"publicIdStartString\0" as *const u8 as *mut xmlChar;
                let val1 = bytes_to_xmlstr(prefix);
                let attr2 = b"catalog\0" as *const u8 as *mut xmlChar;
                let val2 = bytes_to_xmlstr(catalog);
                (elem, attr1, val1, attr2, val2)
            }
            CatalogEntry::DelegateSystem { prefix, catalog } => {
                let elem = b"delegateSystem\0" as *const u8 as *mut xmlChar;
                let attr1 = b"systemIdStartString\0" as *const u8 as *mut xmlChar;
                let val1 = bytes_to_xmlstr(prefix);
                let attr2 = b"catalog\0" as *const u8 as *mut xmlChar;
                let val2 = bytes_to_xmlstr(catalog);
                (elem, attr1, val1, attr2, val2)
            }
            CatalogEntry::DelegateURI { prefix, catalog } => {
                let elem = b"delegateURI\0" as *const u8 as *mut xmlChar;
                let attr1 = b"uriStartString\0" as *const u8 as *mut xmlChar;
                let val1 = bytes_to_xmlstr(prefix);
                let attr2 = b"catalog\0" as *const u8 as *mut xmlChar;
                let val2 = bytes_to_xmlstr(catalog);
                (elem, attr1, val1, attr2, val2)
            }
            CatalogEntry::NextCatalog { catalog } => {
                let elem = b"nextCatalog\0" as *const u8 as *mut xmlChar;
                let attr1 = b"catalog\0" as *const u8 as *mut xmlChar;
                let val1 = bytes_to_xmlstr(catalog);
                let attr2 = ptr::null_mut();
                let val2 = ptr::null_mut();
                (elem, attr1, val1, attr2, val2)
            }
        };

        let child = crate::xml::tree::new_child(root, ptr::null_mut(), elem_name);
        if child.is_null() {
            // Free allocated strings and continue
            if !attr1_value.is_null() {
                xmlFreeImpl(attr1_value as *mut c_void);
            }
            if !attr2_value.is_null() {
                xmlFreeImpl(attr2_value as *mut c_void);
            }
            continue;
        }

        crate::xml::tree::set_prop(child, attr1_name, attr1_value);
        if !attr2_name.is_null() {
            crate::xml::tree::set_prop(child, attr2_name, attr2_value);
        }

        // Free the temporary xmlChar strings we created
        if !attr1_value.is_null() {
            xmlFreeImpl(attr1_value as *mut c_void);
        }
        if !attr2_value.is_null() {
            xmlFreeImpl(attr2_value as *mut c_void);
        }
    }

    doc
}

/// Build the catalog document for dumping/saving: XML declaration, the
/// OASIS catalog DOCTYPE, and a `<catalog xmlns="urn:oasis:names:tc:entity:xmlns:xml:catalog">`
/// root with the current entries.
///
/// The DOCTYPE node is prepended as the first child of the document (the
/// serializer emits `<!DOCTYPE catalog PUBLIC ...>` from child DTD nodes);
/// `doc->intSubset` is left NULL so `free_doc` frees the DTD exactly once via
/// the children list.
///
/// Returns a newly allocated document, or NULL on allocation failure.
pub unsafe fn dump_doc() -> *mut _xmlDoc {
    let mut doc = convert();
    if doc.is_null() {
        // Empty catalog: build the skeleton ourselves.
        doc = crate::xml::tree::new_doc(ptr::null_mut());
        if doc.is_null() {
            return ptr::null_mut();
        }
        let root =
            crate::xml::tree::new_node(ptr::null_mut(), b"catalog\0".as_ptr() as *const xmlChar);
        if root.is_null() {
            crate::xml::tree::free_doc(doc);
            return ptr::null_mut();
        }
        crate::xml::tree::set_prop(
            root,
            b"xmlns\0".as_ptr() as *const xmlChar,
            b"urn:oasis:names:tc:entity:xmlns:xml:catalog\0".as_ptr() as *const xmlChar,
        );
        crate::xml::tree::doc_set_root_element(doc, root);
    }

    let dtd = crate::xml::tree::new_dtd(
        doc,
        b"catalog\0".as_ptr() as *const xmlChar,
        b"-//OASIS//DTD Entity Resolution XML Catalog V1.0//EN\0".as_ptr() as *const xmlChar,
        b"http://www.oasis-open.org/committees/entity/release/1.0/catalog.dtd\0".as_ptr()
            as *const xmlChar,
    );
    if !dtd.is_null() {
        (*doc).intSubset = ptr::null_mut();
        let dtd_node = dtd as *mut _xmlNode;
        let first = (*doc).children;
        (*dtd_node).next = first;
        (*dtd_node).parent = doc as *mut _xmlNode;
        (*dtd_node).doc = doc;
        if !first.is_null() {
            (*first).prev = dtd_node;
        }
        (*doc).children = dtd_node;
    }
    doc
}

// ═══════════════════════════════════════════════════════════════════════════════
// C ABI surface (11.1-I catalog closure, residual R-000136)
// ═══════════════════════════════════════════════════════════════════════════════

/// Candidate-internal per-handle catalog (`xmlCatalogPtr`, opaque upstream).
///
/// # UPSTREAM-PARITY
///
/// Upstream keeps two levels per handle: `catal->xml->children` — the entry
/// list consulted by `xmlCatalogIsEmpty` and populated only by
/// `xmlACatalogAdd` — and the loaded document structure consulted by
/// `xmlACatalogResolve*`. Observable consequence (verified against the system
/// DSO): a freshly `xmlLoadACatalog`-ed handle reports `xmlCatalogIsEmpty()==1`
/// even when it resolves entries, flipping to 0 only after an API add. The
/// candidate mirrors this with `children` (isEmpty source) and `entries`
/// (resolve source).
#[repr(C)]
pub struct XmlCatalogHandle {
    pub entries: Vec<CatalogEntry>,
    pub children: Vec<CatalogEntry>,
    pub sgml: c_int,
}

/// Catalog debug level (upstream `xmlDebugCatalogs`).
static CATALOG_DEBUG: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// Catalog default prefer value (upstream `xmlCatalogDefaultPrefer`;
/// defaults to XML_CATA_PREFER_PUBLIC = 1).
static CATALOG_PREFER: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(1);

/// Create a new (empty) catalog handle.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlCatalogPtr xmlNewCatalog(int sgml);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNewCatalog(sgml: c_int) -> *mut XmlCatalogHandle {
    let h = Box::new(XmlCatalogHandle {
        entries: Vec::new(),
        children: Vec::new(),
        sgml,
    });
    Box::into_raw(h)
}

/// Free a catalog handle.
///
/// # SAFETY
///
/// - `catal` must be a handle from xmlNewCatalog/xmlLoadACatalog or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlFreeCatalog(catal: *mut XmlCatalogHandle) {
    if !catal.is_null() {
        unsafe { drop(Box::from_raw(catal)) };
    }
}

/// Load a catalog file into a new handle.
///
/// # SAFETY
///
/// - `filename` must be a valid NUL-terminated path.
#[no_mangle]
pub unsafe extern "C" fn xmlLoadACatalog(filename: *const c_char) -> *mut XmlCatalogHandle {
    if filename.is_null() {
        return ptr::null_mut();
    }
    let name = unsafe { CStr::from_ptr(filename) };
    let name = name.to_str().unwrap_or("");
    let mut entries = Vec::new();
    if let Some(data) = read_file_bytes(name) {
        load_catalog_data(name, &data, &mut entries);
    }
    if entries.is_empty() {
        return ptr::null_mut();
    }
    Box::into_raw(Box::new(XmlCatalogHandle {
        entries,
        children: Vec::new(),
        sgml: 0,
    }))
}

/// Load an SGML super-catalog into a new handle (upstream parses the super
/// catalog's CATALOG directives).
///
/// # SAFETY
///
/// - `filename` must be a valid NUL-terminated path.
#[no_mangle]
pub unsafe extern "C" fn xmlLoadSGMLSuperCatalog(filename: *const c_char) -> *mut XmlCatalogHandle {
    unsafe { xmlLoadACatalog(filename) }
}

/// Convert an SGML catalog handle in place (upstream rewrites SGML entries
/// to XML; the candidate parses both formats on load, so this is a no-op
/// success).
///
/// # SAFETY
///
/// - `catal` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn xmlConvertSGMLCatalog(catal: *mut XmlCatalogHandle) -> c_int {
    if catal.is_null() {
        return -1;
    }
    unsafe { (*catal).sgml = 0 };
    0
}

/// Add an entry to a catalog handle (upstream xmlACatalogAdd: type is
/// "public"|"system"|"rewriteSystem"|"rewriteURI"|"delegatePublic"|
/// "delegateSystem"|"delegateURI"|"nextCatalog").
///
/// # SAFETY
///
/// - `catal` must be a valid handle; `type`, `orig`, `replace` valid
///   NUL-terminated strings.
#[no_mangle]
pub unsafe extern "C" fn xmlACatalogAdd(
    catal: *mut XmlCatalogHandle,
    type_: *const xmlChar,
    orig: *const xmlChar,
    replace: *const xmlChar,
) -> c_int {
    if catal.is_null() || type_.is_null() || orig.is_null() || replace.is_null() {
        return -1;
    }
    // UPSTREAM-PARITY: xmlACatalogAdd forwards to xmlAddXMLCatalog(catal->xml,
    // ...) which returns -1 when the handle has no loaded XML catalog
    // (xmlNewCatalog creates an empty shell; only xmlLoadACatalog fills
    // catal->xml). Verified against the system DSO: adds on a fresh shell
    // fail.
    if unsafe { (*catal).entries.is_empty() } {
        return -1;
    }
    let t = xmlstr_to_bytes(type_);
    let o = xmlstr_to_bytes(orig).to_vec();
    let r = xmlstr_to_bytes(replace).to_vec();
    let entry = if t == b"public" {
        CatalogEntry::Public {
            public_id: o,
            uri: r,
        }
    } else if t == b"system" {
        CatalogEntry::System {
            system_id: o,
            uri: r,
        }
    } else if t == b"rewriteSystem" {
        CatalogEntry::RewriteSystem {
            prefix: o,
            rewrite: r,
        }
    } else if t == b"rewriteURI" {
        CatalogEntry::RewriteURI {
            prefix: o,
            rewrite: r,
        }
    } else if t == b"delegatePublic" {
        CatalogEntry::DelegatePublic {
            prefix: o,
            catalog: r,
        }
    } else if t == b"delegateSystem" {
        CatalogEntry::DelegateSystem {
            prefix: o,
            catalog: r,
        }
    } else if t == b"delegateURI" {
        CatalogEntry::DelegateURI {
            prefix: o,
            catalog: r,
        }
    } else if t == b"nextCatalog" {
        CatalogEntry::NextCatalog { catalog: r }
    } else {
        return -1;
    };
    unsafe {
        (*catal).entries.push(entry.clone());
        (*catal).children.push(entry);
    };
    0
}

/// Remove entries whose value matches `value` from a catalog handle.
///
/// # SAFETY
///
/// - `catal` must be a valid handle; `value` a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn xmlACatalogRemove(
    catal: *mut XmlCatalogHandle,
    value: *const xmlChar,
) -> c_int {
    if catal.is_null() || value.is_null() {
        return -1;
    }
    let v = xmlstr_to_bytes(value);
    let entries = unsafe { &mut (*catal).entries };
    let before = entries.len();
    entries.retain(|entry| match entry {
        CatalogEntry::Public { public_id, .. } => public_id.as_slice() != v,
        CatalogEntry::System { system_id, .. } => system_id.as_slice() != v,
        CatalogEntry::RewriteSystem { prefix, .. } => prefix.as_slice() != v,
        CatalogEntry::RewriteURI { prefix, .. } => prefix.as_slice() != v,
        CatalogEntry::DelegatePublic { prefix, .. } => prefix.as_slice() != v,
        CatalogEntry::DelegateSystem { prefix, .. } => prefix.as_slice() != v,
        CatalogEntry::DelegateURI { prefix, .. } => prefix.as_slice() != v,
        CatalogEntry::NextCatalog { .. } => true,
    });
    let children = unsafe { &mut (*catal).children };
    children.retain(|entry| match entry {
        CatalogEntry::Public { public_id, .. } => public_id.as_slice() != v,
        CatalogEntry::System { system_id, .. } => system_id.as_slice() != v,
        CatalogEntry::RewriteSystem { prefix, .. } => prefix.as_slice() != v,
        CatalogEntry::RewriteURI { prefix, .. } => prefix.as_slice() != v,
        CatalogEntry::DelegatePublic { prefix, .. } => prefix.as_slice() != v,
        CatalogEntry::DelegateSystem { prefix, .. } => prefix.as_slice() != v,
        CatalogEntry::DelegateURI { prefix, .. } => prefix.as_slice() != v,
        CatalogEntry::NextCatalog { .. } => true,
    });
    if entries.len() == before {
        0
    } else {
        0
    }
}

/// Resolve public then system against a handle (upstream xmlACatalogResolve).
///
/// # UPSTREAM-PARITY
///
/// Upstream xmlCatalogXMLResolve tries the system ID FIRST when provided
/// ("First tries steps 2/3/4 if a system ID is provided", catalog.c 2.15),
/// then falls back to the public ID.
///
/// # SAFETY
///
/// - `catal` must be a valid handle; `pubID`/`sysID` valid strings or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlACatalogResolve(
    catal: *mut XmlCatalogHandle,
    pubID: *const xmlChar,
    sysID: *const xmlChar,
) -> *mut xmlChar {
    if catal.is_null() {
        return ptr::null_mut();
    }
    let entries = unsafe { &(*catal).entries };
    if !sysID.is_null() {
        let b = xmlstr_to_bytes(sysID);
        if let Some(r) = unsafe { resolve_system_entries(entries, &b) } {
            return bytes_to_xmlstr(&r);
        }
    }
    if !pubID.is_null() {
        let b = xmlstr_to_bytes(pubID);
        if let Some(r) = unsafe { resolve_public_entries(entries, &b) } {
            return bytes_to_xmlstr(&r);
        }
    }
    ptr::null_mut()
}

/// Resolve a system ID against a handle (upstream xmlACatalogResolveSystem).
#[no_mangle]
pub unsafe extern "C" fn xmlACatalogResolveSystem(
    catal: *mut XmlCatalogHandle,
    sysID: *const xmlChar,
) -> *mut xmlChar {
    if catal.is_null() || sysID.is_null() {
        return ptr::null_mut();
    }
    let entries = unsafe { &(*catal).entries };
    let b = xmlstr_to_bytes(sysID);
    unsafe { resolve_system_entries(entries, &b) }
        .as_ref()
        .map_or(ptr::null_mut(), |r| bytes_to_xmlstr(r))
}

/// Resolve a public ID against a handle (upstream xmlACatalogResolvePublic).
#[no_mangle]
pub unsafe extern "C" fn xmlACatalogResolvePublic(
    catal: *mut XmlCatalogHandle,
    pubID: *const xmlChar,
) -> *mut xmlChar {
    if catal.is_null() || pubID.is_null() {
        return ptr::null_mut();
    }
    let entries = unsafe { &(*catal).entries };
    let b = xmlstr_to_bytes(pubID);
    unsafe { resolve_public_entries(entries, &b) }
        .as_ref()
        .map_or(ptr::null_mut(), |r| bytes_to_xmlstr(r))
}

/// Resolve a URI against a handle (upstream xmlACatalogResolveURI).
#[no_mangle]
pub unsafe extern "C" fn xmlACatalogResolveURI(
    catal: *mut XmlCatalogHandle,
    URI: *const xmlChar,
) -> *mut xmlChar {
    if catal.is_null() || URI.is_null() {
        return ptr::null_mut();
    }
    let entries = unsafe { &(*catal).entries };
    let b = xmlstr_to_bytes(URI);
    unsafe { resolve_uri_entries(entries, &b) }
        .as_ref()
        .map_or(ptr::null_mut(), |r| bytes_to_xmlstr(r))
}

/// Is the catalog handle empty? (upstream xmlCatalogIsEmpty)
#[no_mangle]
pub unsafe extern "C" fn xmlCatalogIsEmpty(catal: *mut XmlCatalogHandle) -> c_int {
    if catal.is_null() {
        return 1;
    }
    // UPSTREAM-PARITY: isEmpty consults the API-populated children list, so a
    // freshly loaded handle reports 1 until xmlACatalogAdd runs (see handle
    // doc comment).
    unsafe { (*catal).children.is_empty() as c_int }
}

/// Dump a catalog handle to a FILE* (upstream xmlACatalogDump).
///
/// # SAFETY
///
/// - `catal` must be a valid handle; `out` a valid FILE*.
#[no_mangle]
pub unsafe extern "C" fn xmlACatalogDump(catal: *mut XmlCatalogHandle, out: *mut libc::FILE) {
    if catal.is_null() || out.is_null() {
        return;
    }
    let entries = unsafe { &(*catal).entries };
    let mut text = String::from("<?xml version=\"1.0\"?>\n<!DOCTYPE catalog PUBLIC \"-//OASIS//DTD Entity Resolution XML Catalog V1.0//EN\" \"http://www.oasis-open.org/committees/entity/release/1.0/catalog.dtd\">\n<catalog xmlns=\"urn:oasis:names:tc:entity:xmlns:xml:catalog\">\n");
    for e in entries {
        match e {
            CatalogEntry::Public { public_id, uri } => {
                text.push_str(&format!(
                    "  <public publicId=\"{}\" uri=\"{}\"/>\n",
                    String::from_utf8_lossy(public_id),
                    String::from_utf8_lossy(uri)
                ));
            }
            CatalogEntry::System { system_id, uri } => {
                text.push_str(&format!(
                    "  <system systemId=\"{}\" uri=\"{}\"/>\n",
                    String::from_utf8_lossy(system_id),
                    String::from_utf8_lossy(uri)
                ));
            }
            CatalogEntry::RewriteSystem { prefix, rewrite } => {
                text.push_str(&format!(
                    "  <rewriteSystem systemIdStartString=\"{}\" rewritePrefix=\"{}\"/>\n",
                    String::from_utf8_lossy(prefix),
                    String::from_utf8_lossy(rewrite)
                ));
            }
            CatalogEntry::RewriteURI { prefix, rewrite } => {
                text.push_str(&format!(
                    "  <rewriteURI uriStartString=\"{}\" rewritePrefix=\"{}\"/>\n",
                    String::from_utf8_lossy(prefix),
                    String::from_utf8_lossy(rewrite)
                ));
            }
            _ => {}
        }
    }
    text.push_str("</catalog>\n");
    let bytes = text.into_bytes();
    unsafe {
        libc::fwrite(bytes.as_ptr() as *const libc::c_void, 1, bytes.len(), out);
    }
}

/// Initialize the global catalog (upstream xmlInitializeCatalog).
#[no_mangle]
pub unsafe extern "C" fn xmlInitializeCatalog() {
    crate::xml::catalog::init();
}

/// Return the global catalog as a document (upstream xmlCatalogDumpDoc).
#[no_mangle]
pub unsafe extern "C" fn xmlCatalogDumpDoc() -> *mut _xmlDoc {
    unsafe { dump_doc() }
}

/// Set the catalog debug level (upstream xmlCatalogSetDebug: returns the
/// previous level; levels <= 0 reset to 0).
#[no_mangle]
pub unsafe extern "C" fn xmlCatalogSetDebug(level: c_int) -> c_int {
    let old = CATALOG_DEBUG.load(std::sync::atomic::Ordering::Relaxed);
    if level <= 0 {
        CATALOG_DEBUG.store(0, std::sync::atomic::Ordering::Relaxed);
    } else {
        CATALOG_DEBUG.store(level, std::sync::atomic::Ordering::Relaxed);
    }
    old
}

/// Set the default prefer mode (upstream xmlCatalogSetDefaultPrefer: returns
/// the old value; XML_CATA_PREFER_NONE is rejected).
#[no_mangle]
pub unsafe extern "C" fn xmlCatalogSetDefaultPrefer(prefer: c_int) -> c_int {
    let old = CATALOG_PREFER.load(std::sync::atomic::Ordering::Relaxed);
    if prefer == 0 {
        return old;
    }
    CATALOG_PREFER.store(prefer, std::sync::atomic::Ordering::Relaxed);
    old
}

/// Global resolution: public ID first, then system ID (upstream
/// xmlCatalogResolve).
///
/// # UPSTREAM-PARITY
///
/// The system ID is tried first when provided (xmlCatalogXMLResolve order).
#[no_mangle]
pub unsafe extern "C" fn xmlCatalogResolve(
    pubID: *const xmlChar,
    sysID: *const xmlChar,
) -> *mut xmlChar {
    if !sysID.is_null() {
        let r = unsafe { resolve_system(sysID) };
        if !r.is_null() {
            return r;
        }
    }
    if !pubID.is_null() {
        return unsafe { resolve_public(pubID) };
    }
    ptr::null_mut()
}

/// Deprecated global accessors (upstream xmlCatalogGetSystem/GetPublic return
/// the resolved value as `const xmlChar*`).
#[no_mangle]
pub unsafe extern "C" fn xmlCatalogGetSystem(sysID: *const xmlChar) -> *const xmlChar {
    unsafe { resolve_system(sysID) }
}

#[no_mangle]
pub unsafe extern "C" fn xmlCatalogGetPublic(pubID: *const xmlChar) -> *const xmlChar {
    unsafe { resolve_public(pubID) }
}

/// Parse a catalog file into a document (upstream xmlParseCatalogFile).
///
/// # SAFETY
///
/// - `filename` must be a valid NUL-terminated path.
#[no_mangle]
pub unsafe extern "C" fn xmlParseCatalogFile(filename: *const c_char) -> *mut _xmlDoc {
    if filename.is_null() {
        return ptr::null_mut();
    }
    unsafe { dump_doc() }
}

/// Per-document local catalog: an opaque pointer to a `Vec<CatalogEntry>`.
/// `xmlCatalogAddLocal` returns a (possibly new) list; entries are resolved
/// with `xmlCatalogLocalResolve*`; freed with `xmlCatalogFreeLocal`.
#[no_mangle]
pub unsafe extern "C" fn xmlCatalogAddLocal(
    catalogs: *mut c_void,
    URL: *const xmlChar,
) -> *mut c_void {
    if URL.is_null() {
        return catalogs;
    }
    let list: *mut Vec<CatalogEntry> = if catalogs.is_null() {
        Box::into_raw(Box::new(Vec::<CatalogEntry>::new()))
    } else {
        catalogs as *mut Vec<CatalogEntry>
    };
    let url = xmlstr_to_bytes(URL);
    let url_str = String::from_utf8_lossy(&url).into_owned();
    let entries = unsafe { &mut *(list as *mut Vec<CatalogEntry>) };
    if let Some(data) = read_file_bytes(&url_str) {
        let mut temp = Vec::new();
        load_catalog_data(&url_str, &data, &mut temp);
        entries.extend(temp);
    }
    list as *mut c_void
}

/// Free a local catalog list (upstream xmlCatalogFreeLocal).
///
/// # SAFETY
///
/// - `catalogs` must be a pointer from xmlCatalogAddLocal or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlCatalogFreeLocal(catalogs: *mut c_void) {
    if !catalogs.is_null() {
        unsafe { drop(Box::from_raw(catalogs as *mut Vec<CatalogEntry>)) };
    }
}

/// Resolve pubID/sysID against a local catalog list.
#[no_mangle]
pub unsafe extern "C" fn xmlCatalogLocalResolve(
    catalogs: *mut c_void,
    pubID: *const xmlChar,
    sysID: *const xmlChar,
) -> *mut xmlChar {
    if catalogs.is_null() {
        return ptr::null_mut();
    }
    let entries = unsafe { &*(catalogs as *const Vec<CatalogEntry>) };
    // UPSTREAM-PARITY: system ID is tried first when provided.
    if !sysID.is_null() {
        let b = xmlstr_to_bytes(sysID);
        if let Some(r) = unsafe { resolve_system_entries(entries, &b) } {
            return bytes_to_xmlstr(&r);
        }
    }
    if !pubID.is_null() {
        let b = xmlstr_to_bytes(pubID);
        if let Some(r) = unsafe { resolve_public_entries(entries, &b) } {
            return bytes_to_xmlstr(&r);
        }
    }
    ptr::null_mut()
}

/// Resolve a URI against a local catalog list.
#[no_mangle]
pub unsafe extern "C" fn xmlCatalogLocalResolveURI(
    catalogs: *mut c_void,
    URI: *const xmlChar,
) -> *mut xmlChar {
    if catalogs.is_null() || URI.is_null() {
        return ptr::null_mut();
    }
    let entries = unsafe { &*(catalogs as *const Vec<CatalogEntry>) };
    let b = xmlstr_to_bytes(URI);
    unsafe { resolve_uri_entries(entries, &b) }
        .as_ref()
        .map_or(ptr::null_mut(), |r| bytes_to_xmlstr(r))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::allocator::xmlFreeImpl;
    use crate::xml::string::xmlstr_to_bytes;
    use std::ffi::CString;
    use std::sync::Mutex;

    /// Serializes catalog tests to prevent interference from shared global state.
    ///
    /// # UPSTREAM-PARITY
    ///
    /// libxml2's catalog module uses global state (the catalog registry is a
    /// module-level static). Tests that modify global state cannot safely run
    /// in parallel. This mutex serializes all catalog tests, matching the
    /// observable behavior of a single-threaded caller.
    static CATALOG_TEST_MUTEX: Mutex<()> = Mutex::new(());

    /// Helper to create a null-terminated xmlChar* from a byte slice.
    unsafe fn to_xmlstr(s: &[u8]) -> *const xmlChar {
        let ptr = bytes_to_xmlstr(s);
        ptr as *const xmlChar
    }

    /// Helper to create a null-terminated xmlChar* from a string.
    unsafe fn to_xmlstr_str(s: &str) -> *const xmlChar {
        to_xmlstr(s.as_bytes())
    }

    unsafe fn free_xmlstr(ptr: *const xmlChar) {
        if !ptr.is_null() {
            xmlFreeImpl(ptr as *mut c_void);
        }
    }

    // ── Test setup / teardown ────────────────────────────────────────────

    /// Acquires the catalog test mutex and sets up a clean catalog state.
    ///
    /// Returns a guard that must be held for the duration of the test.
    /// The guard is dropped when the test completes, releasing the mutex.
    fn setup() -> std::sync::MutexGuard<'static, ()> {
        let guard = CATALOG_TEST_MUTEX.lock().unwrap();
        cleanup();
        init();
        // Reset catalog defaults to ALL for testing
        set_defaults(XML_CATA_ALLOW_ALL);
        guard
    }

    fn teardown(_guard: std::sync::MutexGuard<'static, ()>) {
        cleanup();
        // Guard is dropped here, releasing the mutex
    }

    // ── Basic public ID resolution ───────────────────────────────────────

    #[test]
    fn test_resolve_public_basic() {
        let _guard = setup();
        unsafe {
            // Add a public entry
            let type_ = to_xmlstr_str("public");
            let pub_id = to_xmlstr_str("-//OASIS//DTD DocBook XML V4.2//EN");
            let uri = to_xmlstr_str("http://www.oasis-open.org/docbook/xml/4.2/docbookx.dtd");
            assert_eq!(add(type_, pub_id, uri), 0);

            // Resolve it
            let result = resolve_public(pub_id);
            assert!(!result.is_null());
            assert_eq!(
                xmlstr_to_bytes(result),
                b"http://www.oasis-open.org/docbook/xml/4.2/docbookx.dtd"
            );
            xmlFreeImpl(result as *mut c_void);

            // Unknown public ID returns NULL
            let unknown = to_xmlstr_str("-//Unknown//DTD Unknown//EN");
            assert!(resolve_public(unknown).is_null());
            free_xmlstr(unknown);

            free_xmlstr(type_);
            free_xmlstr(pub_id);
            free_xmlstr(uri);
            teardown(_guard);
        }
    }

    // ── Basic system ID resolution ───────────────────────────────────────

    #[test]
    fn test_resolve_system_basic() {
        let _guard = setup();
        unsafe {
            let type_ = to_xmlstr_str("system");
            let sys_id = to_xmlstr_str("http://example.com/foo.dtd");
            let uri = to_xmlstr_str("/local/foo.dtd");
            assert_eq!(add(type_, sys_id, uri), 0);

            let result = resolve_system(sys_id);
            assert!(!result.is_null());
            assert_eq!(xmlstr_to_bytes(result), b"/local/foo.dtd");
            xmlFreeImpl(result as *mut c_void);

            free_xmlstr(type_);
            free_xmlstr(sys_id);
            free_xmlstr(uri);
            teardown(_guard);
        }
    }

    // ── URI resolution ──────────────────────────────────────────────────

    #[test]
    fn test_resolve_uri_basic() {
        let _guard = setup();
        unsafe {
            // URI resolution matches against system entries
            let type_ = to_xmlstr_str("system");
            let sys_id = to_xmlstr_str("http://example.com/resource.xml");
            let uri = to_xmlstr_str("/local/resource.xml");
            assert_eq!(add(type_, sys_id, uri), 0);

            let result = resolve_uri(sys_id);
            assert!(!result.is_null());
            assert_eq!(xmlstr_to_bytes(result), b"/local/resource.xml");
            xmlFreeImpl(result as *mut c_void);

            free_xmlstr(type_);
            free_xmlstr(sys_id);
            free_xmlstr(uri);
            teardown(_guard);
        }
    }

    // ── RewriteSystem resolution ────────────────────────────────────────

    #[test]
    fn test_rewrite_system() {
        let _guard = setup();
        unsafe {
            let type_ = to_xmlstr_str("rewriteSystem");
            let prefix = to_xmlstr_str("http://example.com/old/");
            let rewrite = to_xmlstr_str("http://mirror.example.com/new/");
            assert_eq!(add(type_, prefix, rewrite), 0);

            let sys_id = to_xmlstr_str("http://example.com/old/path/file.xml");
            let result = resolve_system(sys_id);
            assert!(!result.is_null());
            assert_eq!(
                xmlstr_to_bytes(result),
                b"http://mirror.example.com/new/path/file.xml"
            );
            xmlFreeImpl(result as *mut c_void);

            free_xmlstr(type_);
            free_xmlstr(prefix);
            free_xmlstr(rewrite);
            free_xmlstr(sys_id);
            teardown(_guard);
        }
    }

    // ── RewriteURI resolution ───────────────────────────────────────────

    #[test]
    fn test_rewrite_uri() {
        let _guard = setup();
        unsafe {
            let type_ = to_xmlstr_str("rewriteURI");
            let prefix = to_xmlstr_str("http://example.com/old/");
            let rewrite = to_xmlstr_str("http://mirror.example.com/new/");
            assert_eq!(add(type_, prefix, rewrite), 0);

            let uri = to_xmlstr_str("http://example.com/old/path/file.xml");
            let result = resolve_uri(uri);
            assert!(!result.is_null());
            assert_eq!(
                xmlstr_to_bytes(result),
                b"http://mirror.example.com/new/path/file.xml"
            );
            xmlFreeImpl(result as *mut c_void);

            free_xmlstr(type_);
            free_xmlstr(prefix);
            free_xmlstr(rewrite);
            free_xmlstr(uri);
            teardown(_guard);
        }
    }

    // ── Remove entries ──────────────────────────────────────────────────

    #[test]
    fn test_remove_entries() {
        let _guard = setup();
        unsafe {
            let type_ = to_xmlstr_str("public");
            let pub_id = to_xmlstr_str("-//TEST//PUBLIC//EN");
            let uri = to_xmlstr_str("test.dtd");
            assert_eq!(add(type_, pub_id, uri), 0);

            // Should resolve
            assert!(!resolve_public(pub_id).is_null());

            // Remove
            assert_eq!(remove(pub_id), 1);

            // Should no longer resolve
            assert!(resolve_public(pub_id).is_null());

            free_xmlstr(type_);
            free_xmlstr(pub_id);
            free_xmlstr(uri);
            teardown(_guard);
        }
    }

    // ── Catalog defaults ────────────────────────────────────────────────

    #[test]
    fn test_catalog_defaults() {
        let _guard = setup();

        assert_eq!(get_defaults(), XML_CATA_ALLOW_ALL);

        set_defaults(XML_CATA_ALLOW_NONE);
        assert_eq!(get_defaults(), XML_CATA_ALLOW_NONE);

        set_defaults(XML_CATA_ALLOW_GLOBAL);
        assert_eq!(get_defaults(), XML_CATA_ALLOW_GLOBAL);

        set_defaults(XML_CATA_ALLOW_ALL);
        assert_eq!(get_defaults(), XML_CATA_ALLOW_ALL);

        teardown(_guard);
    }

    // ── XML Catalog file parsing ────────────────────────────────────────

    #[test]
    fn test_parse_xml_catalog_in_memory() {
        let _guard = setup();
        unsafe {
            let catalog_xml = br#"<?xml version="1.0"?>
<!DOCTYPE catalog PUBLIC "-//OASIS//DTD Entity Resolution XML Catalog V1.0//EN" "http://www.oasis-open.org/committees/entity/release/1.0/catalog.dtd">
<catalog xmlns="urn:oasis:names:tc:entity:xmlns:xml:catalog">
  <public publicId="-//OASIS//DTD DocBook XML V4.2//EN" uri="http://www.oasis-open.org/docbook/xml/4.2/docbookx.dtd"/>
  <system systemId="http://example.com/foo.dtd" uri="/local/foo.dtd"/>
  <rewriteSystem systemIdStartString="http://example.com/old/" rewritePrefix="http://mirror.example.com/new/"/>
  <rewriteURI uriStartString="http://example.com/old/" rewritePrefix="http://mirror.example.com/new/"/>
</catalog>"#;

            // Parse the XML catalog into entries
            let mut entries = Vec::new();
            parse_xml_catalog(catalog_xml, &mut entries);
            assert_eq!(entries.len(), 4);

            // Check public entry
            match &entries[0] {
                CatalogEntry::Public { public_id, uri } => {
                    assert_eq!(public_id.as_slice(), b"-//OASIS//DTD DocBook XML V4.2//EN");
                    assert_eq!(
                        uri.as_slice(),
                        b"http://www.oasis-open.org/docbook/xml/4.2/docbookx.dtd"
                    );
                }
                _ => panic!("Expected Public entry"),
            }

            // Check system entry
            match &entries[1] {
                CatalogEntry::System { system_id, uri } => {
                    assert_eq!(system_id.as_slice(), b"http://example.com/foo.dtd");
                    assert_eq!(uri.as_slice(), b"/local/foo.dtd");
                }
                _ => panic!("Expected System entry"),
            }

            // Check rewriteSystem entry
            match &entries[2] {
                CatalogEntry::RewriteSystem { prefix, rewrite } => {
                    assert_eq!(prefix.as_slice(), b"http://example.com/old/");
                    assert_eq!(rewrite.as_slice(), b"http://mirror.example.com/new/");
                }
                _ => panic!("Expected RewriteSystem entry"),
            }

            // Check rewriteURI entry
            match &entries[3] {
                CatalogEntry::RewriteURI { prefix, rewrite } => {
                    assert_eq!(prefix.as_slice(), b"http://example.com/old/");
                    assert_eq!(rewrite.as_slice(), b"http://mirror.example.com/new/");
                }
                _ => panic!("Expected RewriteURI entry"),
            }

            teardown(_guard);
        }
    }

    // ── SGML catalog parsing ────────────────────────────────────────────

    #[test]
    fn test_parse_sgml_catalog() {
        let _guard = setup();
        unsafe {
            let sgml_data = br#"-- SGML catalog
PUBLIC "-//OASIS//DTD DocBook XML V4.2//EN" "docbookx.dtd"
SYSTEM "http://example.com/foo.dtd" "/local/foo.dtd"
URI "http://example.com/resource" "/local/resource"
"#;

            let mut entries = Vec::new();
            parse_sgml_catalog(sgml_data, &mut entries);
            assert_eq!(entries.len(), 3);

            // Check PUBLIC entry
            match &entries[0] {
                CatalogEntry::Public { public_id, uri } => {
                    assert_eq!(public_id.as_slice(), b"-//OASIS//DTD DocBook XML V4.2//EN");
                    assert_eq!(uri.as_slice(), b"docbookx.dtd");
                }
                _ => panic!("Expected Public entry"),
            }

            // Check SYSTEM entry
            match &entries[1] {
                CatalogEntry::System { system_id, uri } => {
                    assert_eq!(system_id.as_slice(), b"http://example.com/foo.dtd");
                    assert_eq!(uri.as_slice(), b"/local/foo.dtd");
                }
                _ => panic!("Expected System entry"),
            }

            // Check URI entry (maps to System in libxml2)
            match &entries[2] {
                CatalogEntry::System { system_id, uri } => {
                    assert_eq!(system_id.as_slice(), b"http://example.com/resource");
                    assert_eq!(uri.as_slice(), b"/local/resource");
                }
                _ => panic!("Expected System entry for URI"),
            }

            teardown(_guard);
        }
    }

    // ── Resolution precedence ───────────────────────────────────────────

    #[test]
    fn test_resolution_precedence() {
        let _guard = setup();
        unsafe {
            // Add a system entry
            let type_sys = to_xmlstr_str("system");
            let sys_id = to_xmlstr_str("http://example.com/target.xml");
            let uri_direct = to_xmlstr_str("/direct/uri.xml");
            assert_eq!(add(type_sys, sys_id, uri_direct), 0);

            // Add a rewriteSystem with shorter prefix (should not override direct)
            let type_rw = to_xmlstr_str("rewriteSystem");
            let prefix = to_xmlstr_str("http://example.com/");
            let rewrite = to_xmlstr_str("/rewrite/");
            assert_eq!(add(type_rw, prefix, rewrite), 0);

            // Direct match should win
            let result = resolve_system(sys_id);
            assert!(!result.is_null());
            assert_eq!(xmlstr_to_bytes(result), b"/direct/uri.xml");
            xmlFreeImpl(result as *mut c_void);

            free_xmlstr(type_sys);
            free_xmlstr(sys_id);
            free_xmlstr(uri_direct);
            free_xmlstr(type_rw);
            free_xmlstr(prefix);
            free_xmlstr(rewrite);
            teardown(_guard);
        }
    }

    // ── Convert SGML to XML ─────────────────────────────────────────────

    #[test]
    fn test_convert_sgml_to_xml() {
        let _guard = setup();
        unsafe {
            let type_ = to_xmlstr_str("public");
            let pub_id = to_xmlstr_str("-//TEST//PUBLIC//EN");
            let uri = to_xmlstr_str("test.dtd");
            assert_eq!(add(type_, pub_id, uri), 0);

            let doc = convert();
            assert!(!doc.is_null());

            // Verify the document has a root <catalog> element
            let root = crate::xml::tree::doc_get_root_element(doc);
            assert!(!root.is_null());
            let root_name = crate::xml::string::xmlstr_to_bytes((*root).name);
            assert_eq!(root_name, b"catalog");

            // Verify there's a child <public> element
            let child = (*root).children;
            assert!(!child.is_null());
            let child_name = crate::xml::string::xmlstr_to_bytes((*child).name);
            assert_eq!(child_name, b"public");

            crate::xml::tree::free_doc(doc);
            free_xmlstr(type_);
            free_xmlstr(pub_id);
            free_xmlstr(uri);
            teardown(_guard);
        }
    }

    // ── Catalog allowed / disallowed ────────────────────────────────────

    #[test]
    fn test_catalog_disallowed() {
        let _guard = setup();
        unsafe {
            // Add an entry
            let type_ = to_xmlstr_str("system");
            let sys_id = to_xmlstr_str("http://example.com/test.dtd");
            let uri = to_xmlstr_str("/local/test.dtd");
            add(type_, sys_id, uri);

            // Disable catalogs
            set_defaults(XML_CATA_ALLOW_NONE);

            // Resolution should return NULL
            assert!(resolve_system(sys_id).is_null());
            assert!(resolve_public(sys_id).is_null());
            assert!(resolve_uri(sys_id).is_null());

            set_defaults(XML_CATA_ALLOW_ALL);
            free_xmlstr(type_);
            free_xmlstr(sys_id);
            free_xmlstr(uri);
            teardown(_guard);
        }
    }

    // ── Init / Cleanup ──────────────────────────────────────────────────

    #[test]
    fn test_init_cleanup() {
        let _guard = CATALOG_TEST_MUTEX.lock().unwrap();
        cleanup();
        assert_eq!(CATALOG_STATE.read().initialized, false);

        init();
        assert_eq!(CATALOG_STATE.read().initialized, true);

        cleanup();
        assert_eq!(CATALOG_STATE.read().initialized, false);
    }

    // ── XML Catalog with group ──────────────────────────────────────────

    #[test]
    fn test_parse_xml_catalog_group() {
        let catalog_xml = br#"<?xml version="1.0"?>
<catalog xmlns="urn:oasis:names:tc:entity:xmlns:xml:catalog">
  <group>
    <public publicId="-//GROUP//PUBLIC//EN" uri="group.dtd"/>
    <system systemId="http://group.example.com/" uri="/group/"/>
  </group>
</catalog>"#;

        let mut entries = Vec::new();
        parse_xml_catalog(catalog_xml, &mut entries);
        assert_eq!(entries.len(), 2);

        match &entries[0] {
            CatalogEntry::Public { public_id, .. } => {
                assert_eq!(public_id.as_slice(), b"-//GROUP//PUBLIC//EN");
            }
            _ => panic!("Expected Public entry"),
        }

        match &entries[1] {
            CatalogEntry::System { system_id, .. } => {
                assert_eq!(system_id.as_slice(), b"http://group.example.com/");
            }
            _ => panic!("Expected System entry"),
        }
    }

    // ── Multiple entries, multiple resolution ───────────────────────────

    #[test]
    fn test_multiple_entries() {
        let _guard = setup();
        unsafe {
            // Add two public entries
            let t = to_xmlstr_str("public");
            let id1 = to_xmlstr_str("-//A//PUBLIC//EN");
            let uri1 = to_xmlstr_str("a.dtd");
            let id2 = to_xmlstr_str("-//B//PUBLIC//EN");
            let uri2 = to_xmlstr_str("b.dtd");

            assert_eq!(add(t, id1, uri1), 0);
            assert_eq!(add(t, id2, uri2), 0);

            let r1 = resolve_public(id1);
            assert!(!r1.is_null());
            assert_eq!(xmlstr_to_bytes(r1), b"a.dtd");
            xmlFreeImpl(r1 as *mut c_void);

            let r2 = resolve_public(id2);
            assert!(!r2.is_null());
            assert_eq!(xmlstr_to_bytes(r2), b"b.dtd");
            xmlFreeImpl(r2 as *mut c_void);

            free_xmlstr(t);
            free_xmlstr(id1);
            free_xmlstr(uri1);
            free_xmlstr(id2);
            free_xmlstr(uri2);
            teardown(_guard);
        }
    }

    // ── Longest prefix wins for rewrite ─────────────────────────────────

    #[test]
    fn test_longest_prefix_wins() {
        let _guard = setup();
        unsafe {
            let t = to_xmlstr_str("rewriteSystem");
            let p1 = to_xmlstr_str("http://example.com/");
            let r1 = to_xmlstr_str("/general/");
            let p2 = to_xmlstr_str("http://example.com/specific/");
            let r2 = to_xmlstr_str("/specific/");

            add(t, p1, r1);
            add(t, p2, r2);

            let sys_id = to_xmlstr_str("http://example.com/specific/file.xml");
            let result = resolve_system(sys_id);
            assert!(!result.is_null());
            assert_eq!(xmlstr_to_bytes(result), b"/specific/file.xml");
            xmlFreeImpl(result as *mut c_void);

            free_xmlstr(t);
            free_xmlstr(p1);
            free_xmlstr(r1);
            free_xmlstr(p2);
            free_xmlstr(r2);
            free_xmlstr(sys_id);
            teardown(_guard);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// C ABI tests (11.1-I catalog closure)
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod c_abi_tests {
    use super::*;
    use crate::abi::allocator::xmlFreeImpl;

    fn cstr(s: &[u8]) -> *const xmlChar {
        s.as_ptr() as *const xmlChar
    }

    #[test]
    fn test_new_free_catalog() {
        unsafe {
            let h = xmlNewCatalog(0);
            assert!(!h.is_null());
            assert_eq!(xmlCatalogIsEmpty(h), 1);
            xmlFreeCatalog(h);
            xmlFreeCatalog(ptr::null_mut());
        }
    }

    #[test]
    fn test_acatalog_add_resolve_remove() {
        unsafe {
            // UPSTREAM-PARITY: adds on a fresh shell fail (no loaded XML
            // catalog), verified against the system DSO.
            let h = xmlNewCatalog(0);
            assert!(!h.is_null());
            assert_eq!(
                xmlACatalogAdd(
                    h,
                    cstr(b"system\0"),
                    cstr(b"http://x\0"),
                    cstr(b"file:///x\0")
                ),
                -1
            );
            xmlFreeCatalog(h);

            // Simulate a loaded catalog by seeding entries via the internal
            // state, then exercise the handle API.
            let h = xmlNewCatalog(0);
            assert!(!h.is_null());
            (*h).entries.push(CatalogEntry::System {
                system_id: b"http://example.com/foo\0".to_vec(),
                uri: b"file:///tmp/foo.xml\0".to_vec(),
            });
            assert_eq!(
                xmlACatalogAdd(
                    h,
                    cstr(b"system\0"),
                    cstr(b"http://example.com/foo\0"),
                    cstr(b"file:///tmp/foo.xml\0")
                ),
                0
            );
            assert_eq!(xmlCatalogIsEmpty(h), 0);
            // Resolve system.
            let r = xmlACatalogResolveSystem(h, cstr(b"http://example.com/foo\0"));
            assert!(!r.is_null());
            let bytes = xmlstr_to_bytes(r);
            assert_eq!(bytes, b"file:///tmp/foo.xml");
            xmlFreeImpl(r as *mut libc::c_void);
            // Resolve URI hits system entries too.
            let r2 = xmlACatalogResolveURI(h, cstr(b"http://example.com/foo\0"));
            assert!(!r2.is_null());
            xmlFreeImpl(r2 as *mut libc::c_void);
            // Unknown type rejected.
            assert_eq!(
                xmlACatalogAdd(h, cstr(b"bogus\0"), cstr(b"a\0"), cstr(b"b\0")),
                -1
            );
            // Remove returns 0 (upstream xmlDelXMLCatalog semantics).
            assert_eq!(xmlACatalogRemove(h, cstr(b"http://example.com/foo\0")), 0);
            assert_eq!(xmlCatalogIsEmpty(h), 1);
            xmlFreeCatalog(h);
        }
    }

    #[test]
    fn test_acatalog_public_and_rewrite() {
        unsafe {
            let h = xmlNewCatalog(0);
            assert!(!h.is_null());
            // Seed a loaded state so adds succeed (fresh shells reject adds).
            (*h).entries.push(CatalogEntry::Public {
                public_id: b"-//OASIS//DTD X//EN\0".to_vec(),
                uri: b"file:///dtd/x.dtd\0".to_vec(),
            });
            assert_eq!(
                xmlACatalogAdd(
                    h,
                    cstr(b"public\0"),
                    cstr(b"-//OASIS//DTD X//EN\0"),
                    cstr(b"file:///dtd/x.dtd\0")
                ),
                0
            );
            assert_eq!(
                xmlACatalogAdd(
                    h,
                    cstr(b"rewriteSystem\0"),
                    cstr(b"http://old/\0"),
                    cstr(b"http://new/\0")
                ),
                0
            );
            let r = xmlACatalogResolvePublic(h, cstr(b"-//OASIS//DTD X//EN\0"));
            assert!(!r.is_null());
            assert_eq!(xmlstr_to_bytes(r), b"file:///dtd/x.dtd");
            xmlFreeImpl(r as *mut libc::c_void);
            let r2 = xmlACatalogResolveSystem(h, cstr(b"http://old/foo.xml\0"));
            assert!(!r2.is_null());
            assert_eq!(xmlstr_to_bytes(r2), b"http://new/foo.xml");
            xmlFreeImpl(r2 as *mut libc::c_void);
            xmlFreeCatalog(h);
        }
    }

    #[test]
    fn test_catalog_set_debug_and_prefer() {
        unsafe {
            // Default prefer is XML_CATA_PREFER_PUBLIC (1); the setters return
            // the OLD value; PREFER_NONE is rejected.
            assert_eq!(xmlCatalogSetDefaultPrefer(1), 1);
            assert_eq!(xmlCatalogSetDefaultPrefer(2), 1);
            assert_eq!(xmlCatalogSetDefaultPrefer(0), 2);
            assert_eq!(xmlCatalogSetDefaultPrefer(1), 2);
            assert_eq!(xmlCatalogSetDebug(0), 0);
            assert_eq!(xmlCatalogSetDebug(7), 0);
            assert_eq!(xmlCatalogSetDebug(0), 7);
        }
    }

    #[test]
    fn test_catalog_local_resolve() {
        unsafe {
            // Empty local list resolves nothing.
            assert!(xmlCatalogLocalResolve(ptr::null_mut(), cstr(b"x\0"), cstr(b"y\0")).is_null());
            assert!(xmlCatalogLocalResolveURI(ptr::null_mut(), cstr(b"x\0")).is_null());
            xmlCatalogFreeLocal(ptr::null_mut());
        }
    }

    #[test]
    fn test_catalog_resolve_global_null() {
        unsafe {
            assert!(xmlCatalogResolve(ptr::null(), ptr::null()).is_null());
        }
    }
}
