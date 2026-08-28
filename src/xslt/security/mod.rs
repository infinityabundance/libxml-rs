//! XSLT security preferences (§33, §85 Phase 8).
//!
//! Implements the xsltSecurityPrefs API for controlling what operations
//! are permitted during XSLT transformations.
//!
//! # Upstream mapping
//!
//! | Function | Header |
//! |---|---|
//! | `xsltNewSecurityPrefs` | xslt.h |
//! | `xsltFreeSecurityPrefs` | xslt.h |
//! | `xsltSetSecurityPrefs` | xslt.h |
//! | `xsltGetSecurityPrefs` | xslt.h |
//! | `xsltSetDefaultSecurityPrefs` | xslt.h |
//! | `xsltGetDefaultSecurityPrefs` | xslt.h |

#![allow(
    clippy::missing_inline_in_public_items,
    clippy::must_use_candidate,
    clippy::missing_safety_doc
)]

use crate::abi::structs::*;
use crate::abi::types::*;
use std::ffi::c_void;
use std::os::raw::c_int;
use std::ptr;
use std::sync::Mutex;

/// Security option: read a file from the filesystem.
pub const XSLT_SECPREF_READ_FILE: c_int = 1;

/// Security option: write a file to the filesystem.
pub const XSLT_SECPREF_WRITE_FILE: c_int = 2;

/// Security option: create a directory on the filesystem.
pub const XSLT_SECPREF_CREATE_DIRECTORY: c_int = 3;

/// Security option: read a network resource.
pub const XSLT_SECPREF_READ_NETWORK: c_int = 4;

/// Security option: write to a network resource.
pub const XSLT_SECPREF_WRITE_NETWORK: c_int = 5;

/// Default security preference value (alias for [`XSLT_SECPREF_DENY`]).
pub const XSLT_SECPREF_DEFAULT: c_int = 0;

/// Security preference value: deny the operation.
pub const XSLT_SECPREF_DENY: c_int = 0;

/// Security preference value: allow the operation.
pub const XSLT_SECPREF_ALLOW: c_int = 1;

/// Internal security preferences structure.
///
/// Stores the current allow/deny setting for each of the five
/// controllable security options.
#[repr(C)]
pub struct XsltSecurityPrefs {
    /// Allow reading files from the filesystem.
    pub readFile: c_int,
    /// Allow writing files to the filesystem.
    pub writeFile: c_int,
    /// Allow creating directories on the filesystem.
    pub createDirectory: c_int,
    /// Allow reading network resources.
    pub readNetwork: c_int,
    /// Allow writing to network resources.
    pub writeNetwork: c_int,
}

/// Wrapper around `*mut c_void` that implements `Send` so it can be stored
/// in a `Mutex`.
///
/// # Safety
///
/// The caller is responsible for ensuring that the pointed-to value is
/// accessed in a thread-safe manner. The global default is only ever written
/// or read through the `Mutex` guard, so concurrent access is serialized.
#[repr(transparent)]
struct SecurityPrefsPtr(*mut c_void);

// SAFETY: Access to the wrapped pointer is serialized via Mutex, making
// it safe to send between threads.
unsafe impl Send for SecurityPrefsPtr {}

/// Global default security preferences, stored as a raw pointer behind a
/// [`Mutex`] for thread-safe access.
static DEFAULT_SECURITY_PREFS: Mutex<Option<SecurityPrefsPtr>> = Mutex::new(None);

/// Create new security preferences with default (allow) settings.
///
/// Returns a raw pointer to a heap-allocated [`XsltSecurityPrefs`] with all
/// options set to [`XSLT_SECPREF_ALLOW`]. The caller is responsible for
/// freeing the returned pointer via [`xsltFreeSecurityPrefs`].
///
/// # Returns
///
/// A non-null pointer to the newly allocated security preferences on success.
///
/// # Safety
///
/// The caller must ensure the returned pointer is eventually freed with
/// [`xsltFreeSecurityPrefs`] to avoid memory leaks.
#[no_mangle]
pub unsafe extern "C" fn xsltNewSecurityPrefs() -> *mut c_void {
    let prefs = Box::new(XsltSecurityPrefs {
        readFile: XSLT_SECPREF_ALLOW,
        writeFile: XSLT_SECPREF_ALLOW,
        createDirectory: XSLT_SECPREF_ALLOW,
        readNetwork: XSLT_SECPREF_ALLOW,
        writeNetwork: XSLT_SECPREF_ALLOW,
    });
    Box::into_raw(prefs) as *mut c_void
}

/// Free security preferences previously allocated by [`xsltNewSecurityPrefs`].
///
/// # Safety
///
/// - `sec` must be a pointer returned by [`xsltNewSecurityPrefs`] that has
///   not yet been freed.
/// - After this call, `sec` is dangling and must not be dereferenced.
#[no_mangle]
pub unsafe extern "C" fn xsltFreeSecurityPrefs(sec: *mut c_void) {
    if !sec.is_null() {
        let _ = Box::from_raw(sec as *mut XsltSecurityPrefs);
    }
}

/// Set a security preference for the given options structure.
///
/// # Arguments
///
/// * `sec` - Pointer to security preferences (must be non-null).
/// * `option` - One of `XSLT_SECPREF_READ_FILE`, `XSLT_SECPREF_WRITE_FILE`,
///   `XSLT_SECPREF_CREATE_DIRECTORY`, `XSLT_SECPREF_READ_NETWORK`, or
///   `XSLT_SECPREF_WRITE_NETWORK`.
/// * `value` - The value to set (typically [`XSLT_SECPREF_ALLOW`] or
///   [`XSLT_SECPREF_DENY`]).
///
/// # Returns
///
/// `0` on success, or `-1` if `sec` is null or `option` is invalid.
///
/// # Safety
///
/// `sec` must point to a valid [`XsltSecurityPrefs`] structure obtained from
/// [`xsltNewSecurityPrefs`] that has not yet been freed.
#[no_mangle]
pub unsafe extern "C" fn xsltSetSecurityPrefs(
    sec: *mut c_void,
    option: c_int,
    value: c_int,
) -> c_int {
    if sec.is_null() {
        return -1;
    }
    let prefs = &mut *(sec as *mut XsltSecurityPrefs);
    match option {
        XSLT_SECPREF_READ_FILE => prefs.readFile = value,
        XSLT_SECPREF_WRITE_FILE => prefs.writeFile = value,
        XSLT_SECPREF_CREATE_DIRECTORY => prefs.createDirectory = value,
        XSLT_SECPREF_READ_NETWORK => prefs.readNetwork = value,
        XSLT_SECPREF_WRITE_NETWORK => prefs.writeNetwork = value,
        _ => return -1,
    }
    0
}

/// Get the current value of a security preference.
///
/// # Arguments
///
/// * `sec` - Pointer to security preferences. If null, returns
///   [`XSLT_SECPREF_DENY`] for all options.
/// * `option` - One of `XSLT_SECPREF_READ_FILE`, `XSLT_SECPREF_WRITE_FILE`,
///   `XSLT_SECPREF_CREATE_DIRECTORY`, `XSLT_SECPREF_READ_NETWORK`, or
///   `XSLT_SECPREF_WRITE_NETWORK`.
///
/// # Returns
///
/// The current preference value, or [`XSLT_SECPREF_DENY`] if `sec` is null
/// or `option` is invalid.
///
/// # Safety
///
/// If `sec` is non-null, it must point to a valid [`XsltSecurityPrefs`]
/// structure obtained from [`xsltNewSecurityPrefs`] that has not yet been
/// freed.
#[no_mangle]
pub unsafe extern "C" fn xsltGetSecurityPrefs(sec: *mut c_void, option: c_int) -> c_int {
    if sec.is_null() {
        return XSLT_SECPREF_DENY;
    }
    let prefs = &*(sec as *mut XsltSecurityPrefs);
    match option {
        XSLT_SECPREF_READ_FILE => prefs.readFile,
        XSLT_SECPREF_WRITE_FILE => prefs.writeFile,
        XSLT_SECPREF_CREATE_DIRECTORY => prefs.createDirectory,
        XSLT_SECPREF_READ_NETWORK => prefs.readNetwork,
        XSLT_SECPREF_WRITE_NETWORK => prefs.writeNetwork,
        _ => XSLT_SECPREF_DENY,
    }
}

/// Set the default security preferences used by new transformations.
///
/// The provided pointer is stored as the global default. It is the caller's
/// responsibility to manage the lifetime of the pointed-to preferences.
///
/// # Safety
///
/// `sec` must point to a valid [`XsltSecurityPrefs`] structure that remains
/// valid for the duration it is set as the default (i.e., until replaced by
/// another call to this function).
#[no_mangle]
pub unsafe extern "C" fn xsltSetDefaultSecurityPrefs(sec: *mut c_void) {
    let mut guard = DEFAULT_SECURITY_PREFS.lock().unwrap();
    *guard = Some(SecurityPrefsPtr(sec));
}

/// Get the current default security preferences.
///
/// # Returns
///
/// A pointer to the default security preferences previously set with
/// [`xsltSetDefaultSecurityPrefs`], or a null pointer if none have been set.
///
/// # Safety
///
/// The returned pointer is only valid as long as no other call to
/// [`xsltSetDefaultSecurityPrefs`] has replaced it and the original
/// [`XsltSecurityPrefs`] has not been freed.
#[no_mangle]
pub unsafe extern "C" fn xsltGetDefaultSecurityPrefs() -> *mut c_void {
    let guard = DEFAULT_SECURITY_PREFS.lock().unwrap();
    guard.as_ref().map_or(ptr::null_mut(), |p| p.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that creating and freeing security prefs works.
    #[test]
    fn test_new_free_security_prefs() {
        unsafe {
            let prefs = xsltNewSecurityPrefs();
            assert!(!prefs.is_null());
            xsltFreeSecurityPrefs(prefs);
        }
    }

    /// Verify that freeing a null pointer is a no-op.
    #[test]
    fn test_free_null() {
        unsafe {
            // Should not panic or crash.
            xsltFreeSecurityPrefs(ptr::null_mut());
        }
    }

    /// Verify that newly created prefs have all options set to ALLOW.
    #[test]
    fn test_new_prefs_defaults_allow() {
        unsafe {
            let prefs = xsltNewSecurityPrefs();
            assert_eq!(
                xsltGetSecurityPrefs(prefs, XSLT_SECPREF_READ_FILE),
                XSLT_SECPREF_ALLOW
            );
            assert_eq!(
                xsltGetSecurityPrefs(prefs, XSLT_SECPREF_WRITE_FILE),
                XSLT_SECPREF_ALLOW
            );
            assert_eq!(
                xsltGetSecurityPrefs(prefs, XSLT_SECPREF_CREATE_DIRECTORY),
                XSLT_SECPREF_ALLOW
            );
            assert_eq!(
                xsltGetSecurityPrefs(prefs, XSLT_SECPREF_READ_NETWORK),
                XSLT_SECPREF_ALLOW
            );
            assert_eq!(
                xsltGetSecurityPrefs(prefs, XSLT_SECPREF_WRITE_NETWORK),
                XSLT_SECPREF_ALLOW
            );
            xsltFreeSecurityPrefs(prefs);
        }
    }

    /// Verify that setting and getting each option round-trips correctly.
    #[test]
    fn test_set_and_get() {
        unsafe {
            let prefs = xsltNewSecurityPrefs();

            // Set all to DENY
            assert_eq!(
                xsltSetSecurityPrefs(prefs, XSLT_SECPREF_READ_FILE, XSLT_SECPREF_DENY),
                0
            );
            assert_eq!(
                xsltSetSecurityPrefs(prefs, XSLT_SECPREF_WRITE_FILE, XSLT_SECPREF_DENY),
                0
            );
            assert_eq!(
                xsltSetSecurityPrefs(prefs, XSLT_SECPREF_CREATE_DIRECTORY, XSLT_SECPREF_DENY),
                0
            );
            assert_eq!(
                xsltSetSecurityPrefs(prefs, XSLT_SECPREF_READ_NETWORK, XSLT_SECPREF_DENY),
                0
            );
            assert_eq!(
                xsltSetSecurityPrefs(prefs, XSLT_SECPREF_WRITE_NETWORK, XSLT_SECPREF_DENY),
                0
            );

            // Verify all are DENY
            assert_eq!(
                xsltGetSecurityPrefs(prefs, XSLT_SECPREF_READ_FILE),
                XSLT_SECPREF_DENY
            );
            assert_eq!(
                xsltGetSecurityPrefs(prefs, XSLT_SECPREF_WRITE_FILE),
                XSLT_SECPREF_DENY
            );
            assert_eq!(
                xsltGetSecurityPrefs(prefs, XSLT_SECPREF_CREATE_DIRECTORY),
                XSLT_SECPREF_DENY
            );
            assert_eq!(
                xsltGetSecurityPrefs(prefs, XSLT_SECPREF_READ_NETWORK),
                XSLT_SECPREF_DENY
            );
            assert_eq!(
                xsltGetSecurityPrefs(prefs, XSLT_SECPREF_WRITE_NETWORK),
                XSLT_SECPREF_DENY
            );

            // Set each individually back to ALLOW
            assert_eq!(
                xsltSetSecurityPrefs(prefs, XSLT_SECPREF_READ_FILE, XSLT_SECPREF_ALLOW),
                0
            );
            assert_eq!(
                xsltGetSecurityPrefs(prefs, XSLT_SECPREF_READ_FILE),
                XSLT_SECPREF_ALLOW
            );
            assert_eq!(
                xsltGetSecurityPrefs(prefs, XSLT_SECPREF_WRITE_FILE),
                XSLT_SECPREF_DENY
            );

            assert_eq!(
                xsltSetSecurityPrefs(prefs, XSLT_SECPREF_WRITE_FILE, XSLT_SECPREF_ALLOW),
                0
            );
            assert_eq!(
                xsltGetSecurityPrefs(prefs, XSLT_SECPREF_WRITE_FILE),
                XSLT_SECPREF_ALLOW
            );

            assert_eq!(
                xsltSetSecurityPrefs(prefs, XSLT_SECPREF_CREATE_DIRECTORY, XSLT_SECPREF_ALLOW),
                0
            );
            assert_eq!(
                xsltGetSecurityPrefs(prefs, XSLT_SECPREF_CREATE_DIRECTORY),
                XSLT_SECPREF_ALLOW
            );

            assert_eq!(
                xsltSetSecurityPrefs(prefs, XSLT_SECPREF_READ_NETWORK, XSLT_SECPREF_ALLOW),
                0
            );
            assert_eq!(
                xsltGetSecurityPrefs(prefs, XSLT_SECPREF_READ_NETWORK),
                XSLT_SECPREF_ALLOW
            );

            assert_eq!(
                xsltSetSecurityPrefs(prefs, XSLT_SECPREF_WRITE_NETWORK, XSLT_SECPREF_ALLOW),
                0
            );
            assert_eq!(
                xsltGetSecurityPrefs(prefs, XSLT_SECPREF_WRITE_NETWORK),
                XSLT_SECPREF_ALLOW
            );

            xsltFreeSecurityPrefs(prefs);
        }
    }

    /// Verify that setting/getting with null pointer returns error/default.
    #[test]
    fn test_null_pointer() {
        unsafe {
            assert_eq!(
                xsltSetSecurityPrefs(ptr::null_mut(), XSLT_SECPREF_READ_FILE, XSLT_SECPREF_ALLOW),
                -1
            );
            assert_eq!(
                xsltGetSecurityPrefs(ptr::null_mut(), XSLT_SECPREF_READ_FILE),
                XSLT_SECPREF_DENY
            );
        }
    }

    /// Verify that an invalid option returns error/default.
    #[test]
    fn test_invalid_option() {
        unsafe {
            let prefs = xsltNewSecurityPrefs();
            assert_eq!(xsltSetSecurityPrefs(prefs, 99, XSLT_SECPREF_ALLOW), -1);
            assert_eq!(xsltGetSecurityPrefs(prefs, 99), XSLT_SECPREF_DENY);
            xsltFreeSecurityPrefs(prefs);
        }
    }

    /// Verify that default security prefs set/get round-trip correctly.
    #[test]
    fn test_default_security_prefs() {
        unsafe {
            // Initially null
            assert!(xsltGetDefaultSecurityPrefs().is_null());

            // Create prefs and set as default
            let prefs = xsltNewSecurityPrefs();
            xsltSetDefaultSecurityPrefs(prefs);

            // Retrieve and verify
            let retrieved = xsltGetDefaultSecurityPrefs();
            assert_eq!(retrieved, prefs);

            // Verify we can read from the default
            assert_eq!(
                xsltGetSecurityPrefs(retrieved, XSLT_SECPREF_READ_FILE),
                XSLT_SECPREF_ALLOW
            );

            // Clear the default by setting null
            xsltSetDefaultSecurityPrefs(ptr::null_mut());
            assert!(xsltGetDefaultSecurityPrefs().is_null());

            // Free the original prefs
            xsltFreeSecurityPrefs(prefs);
        }
    }
}
