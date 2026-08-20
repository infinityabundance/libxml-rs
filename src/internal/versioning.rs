//! Internal version management (§83, §84).
//!
//! Backs the public C ABI version APIs (xmlLibxmlVersion, xmlCheckVersion, etc.)
//! with Rust-side version information.
//!
//! # Phase 0 status
//!
//! Minimal stub implementation returning the target compatibility version (libxml2 2.12.x).

use std::os::raw::c_char;
use std::sync::OnceLock;

static VERSION_STRING: OnceLock<Vec<u8>> = OnceLock::new();

/// Return a pointer to a static C string containing the libxml version.
pub fn version_string() -> *const c_char {
    let bytes = VERSION_STRING.get_or_init(|| {
        // Target libxml2 2.12.0 compatibility for now.
        b"2.12.0\0".to_vec()
    });
    bytes.as_ptr() as *const c_char
}

/// Check that the library version is at least the requested version.
/// Returns 0 if compatible, -1 if not.
pub fn check_version(version: std::os::raw::c_int) -> std::os::raw::c_int {
    let our_version = 2 * 10000 + 12 * 100 + 0; // 2.12.0
    if our_version >= version {
        0
    } else {
        -1
    }
}
