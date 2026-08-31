//! C ABI versioning — LIBXML2_VERSION, LIBXSLT_VERSION, runtime version APIs (§83, §84).
//!
//! This module implements the public C ABI version functions:
//! - `xmlLibxmlVersion()` — returns LIBXML2_VERSION as integer
//! - `xmlLibxmlVersionString()` — returns LIBXML2_VERSION string pointer
//! - `xmlParserVersion()` — alias for xmlLibxmlVersionString
//! - `xmlCheckVersion()` — runtime version compatibility check
//! - `xsltLibxsltVersion()` — returns LIBXSLT_VERSION as integer
//! - `xsltLibxsltVersionString()` — returns LIBXSLT_VERSION string pointer
//! - `xsltCheckVersion()` — runtime XSLT version compatibility check
//!
//! # Phase 1 status
//!
//! Complete — all version APIs are implemented.
//!
//! # Compatibility profile
//!
//! Currently targeting libxml2 2.15.3 / libxslt 1.1.45 compatibility
//! (the oracle toolchain on the reference system).
//!
//! # UPSTREAM-PARITY
//!
//! Upstream version format: major * 10000 + minor * 100 + micro
//! Example: 2.15.3 → 21503

#![allow(non_upper_case_globals)]

use core::ffi::c_char;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;
use std::os::raw::c_int;

// ═══════════════════════════════════════════════════════════════════════════════
// Target Version Constants
// ═══════════════════════════════════════════════════════════════════════════════

/// The target libxml2 version we aim to be compatible with.
const TARGET_LIBXML2_MAJOR: c_int = 2;
const TARGET_LIBXML2_MINOR: c_int = 15;
const TARGET_LIBXML2_MICRO: c_int = 3;

/// The target libxslt version we aim to be compatible with.
const TARGET_LIBXSLT_MAJOR: c_int = 1;
const TARGET_LIBXSLT_MINOR: c_int = 1;
const TARGET_LIBXSLT_MICRO: c_int = 45;

/// The version string for libxml2 compatibility.
const LIBXML2_VERSION_STRING: &[u8; 7] = b"2.15.3\0";

/// The version string for libxslt compatibility.
const LIBXSLT_VERSION_STRING: &[u8; 7] = b"1.1.45\0";

// ═══════════════════════════════════════════════════════════════════════════════
// Version Macros (also defined in types.rs for compile-time use)
// ═══════════════════════════════════════════════════════════════════════════════

/// Compute the numeric version from major/minor/micro components.
#[inline]
pub const fn version_number(major: c_int, minor: c_int, micro: c_int) -> c_int {
    major * 10000 + minor * 100 + micro
}

/// libxml2 version as a number: 2 * 10000 + 15 * 100 + 3 = 21503
pub const LIBXML2_VERSION_NUM: c_int = version_number(
    TARGET_LIBXML2_MAJOR,
    TARGET_LIBXML2_MINOR,
    TARGET_LIBXML2_MICRO,
);

/// libxslt version as a number: 1 * 10000 + 1 * 100 + 45 = 10145
pub const LIBXSLT_VERSION_NUM: c_int = version_number(
    TARGET_LIBXSLT_MAJOR,
    TARGET_LIBXSLT_MINOR,
    TARGET_LIBXSLT_MICRO,
);

// ═══════════════════════════════════════════════════════════════════════════════
// Initialization Tracking
// ═══════════════════════════════════════════════════════════════════════════════

/// Whether the library has been initialized.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Mark the library as initialized.
pub fn set_initialized() {
    INITIALIZED.store(true, Ordering::Release);
}

/// Check whether the library has been initialized.
pub fn is_initialized() -> bool {
    INITIALIZED.load(Ordering::Acquire)
}

// ═══════════════════════════════════════════════════════════════════════════════
// libxml2 Version Functions
// ═══════════════════════════════════════════════════════════════════════════════

/// Return the libxml2 version as an integer.
///
/// Returns `major * 10000 + minor * 100 + micro`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlLibxmlVersion(void);
/// ```
///
/// Oracle behavior (2.15.3): returns 21503.
pub const fn xmlLibxmlVersion() -> c_int {
    LIBXML2_VERSION_NUM
}

/// Return the libxml2 version as a static C string.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const char *xmlLibxmlVersionString(void);
/// ```
///
/// Oracle behavior (2.15.3): returns pointer to "2.15.3".
pub const fn xmlLibxmlVersionString() -> *const c_char {
    LIBXML2_VERSION_STRING.as_ptr() as *const c_char
}

/// Return the parser version string (alias for `xmlLibxmlVersionString`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const char *xmlParserVersion(void);
/// ```
pub const fn xmlParserVersion() -> *const c_char {
    xmlLibxmlVersionString()
}

/// Check that the library version is at least `version`.
///
/// # Returns
///
/// - 0 if the library version is >= `version`
/// - -1 if the library version is < `version`
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlCheckVersion(int version);
/// ```
///
/// Oracle behavior: compares LIBXML2_VERSION (compiled-in) against `version`.
/// Returns 0 if compatible, -1 if not.
///
/// # SAFETY
///
/// The function touches crate-global state only; it is safe
/// as long as the caller respects the library's global
/// initialization/cleanup ordering (xmlInitParser before use,
/// xmlCleanupParser only after all users are done).
///
/// Violating the global lifecycle ordering, or calling this after
/// teardown or from a signal handler, is undefined behavior.
#[no_mangle]
pub const unsafe extern "C" fn xmlCheckVersion(version: c_int) -> c_int {
    if LIBXML2_VERSION_NUM >= version {
        0
    } else {
        -1
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// libxslt Version Functions
// ═══════════════════════════════════════════════════════════════════════════════

/// Return the libxslt version as an integer.
///
/// Returns `major * 10000 + minor * 100 + micro`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltLibxsltVersion(void);
/// ```
pub const fn xsltLibxsltVersion() -> c_int {
    LIBXSLT_VERSION_NUM
}

/// Return the libxslt version as a static C string.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const char *xsltLibxsltVersionString(void);
/// ```
pub const fn xsltLibxsltVersionString() -> *const c_char {
    LIBXSLT_VERSION_STRING.as_ptr() as *const c_char
}

/// Convert a C string pointer to a byte slice (NULL-safe).
///
/// # SAFETY
///
/// - `ptr` must be a valid null-terminated C string or NULL.
pub unsafe fn c_str_to_bytes<'a>(ptr: *const c_char) -> Option<&'a [u8]> {
    if ptr.is_null() {
        return None;
    }
    let len = unsafe { libc::strlen(ptr) };
    Some(unsafe { core::slice::from_raw_parts(ptr as *const u8, len) })
}

/// Check that the XSLT library version is at least `version`.
///
/// # Returns
///
/// - 0 if the library version is >= `version`
/// - -1 if the library version is < `version`
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltCheckVersion(int version);
/// ```
pub const fn xsltCheckVersion(version: c_int) -> c_int {
    if LIBXSLT_VERSION_NUM >= version {
        0
    } else {
        -1
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Feature Detection
// ═══════════════════════════════════════════════════════════════════════════════

// ═══════════════════════════════════════════════════════════════════════════════
// Compile-time Version Macros (for Rust consumers)
// ═══════════════════════════════════════════════════════════════════════════════

/// The libxml2 version as a number (compile-time constant).
pub const LIBXML2_VERSION: c_int = LIBXML2_VERSION_NUM;

/// The libxml2 version major number.
pub const LIBXML2_VERSION_MAJOR: c_int = TARGET_LIBXML2_MAJOR;

/// The libxml2 version minor number.
pub const LIBXML2_VERSION_MINOR: c_int = TARGET_LIBXML2_MINOR;

/// The libxml2 version micro number.
pub const LIBXML2_VERSION_MICRO: c_int = TARGET_LIBXML2_MICRO;

/// The libxml2 version as a number (alternate name).
pub const LIBXML2_VERSION_NUMBER: c_int = LIBXML2_VERSION_NUM;

/// Extra version suffix (empty string for release versions).
pub const LIBXML2_VERSION_EXTRA: &[u8; 1] = b"\0";

/// The libxslt version as a number (compile-time constant).
pub const LIBXSLT_VERSION: c_int = LIBXSLT_VERSION_NUM;

/// The libxslt version major number.
pub const LIBXSLT_VERSION_MAJOR: c_int = TARGET_LIBXSLT_MAJOR;

/// The libxslt version minor number.
pub const LIBXSLT_VERSION_MINOR: c_int = TARGET_LIBXSLT_MINOR;

/// The libxslt version micro number.
pub const LIBXSLT_VERSION_MICRO: c_int = TARGET_LIBXSLT_MICRO;

/// The libxslt version as a number (alternate name).
pub const LIBXSLT_VERSION_NUMBER: c_int = LIBXSLT_VERSION_NUM;

/// Extra version suffix for libxslt (empty string for release versions).
pub const LIBXSLT_VERSION_EXTRA: &[u8; 1] = b"\0";

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
