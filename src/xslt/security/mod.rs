//! XSLT security preferences (§33, §85 Phase 8; R-000125 closure).
//!
//! Implements the `xsltSecurityPrefs` API for controlling what operations are
//! permitted during XSLT transformations.
//!
//! # Upstream mapping
//!
//! | Function | Header |
//! |---|---|
//! | `xsltNewSecurityPrefs` | libxslt/security.h |
//! | `xsltFreeSecurityPrefs` | libxslt/security.h |
//! | `xsltSetSecurityPrefs` | libxslt/security.h |
//! | `xsltGetSecurityPrefs` | libxslt/security.h |
//! | `xsltSetDefaultSecurityPrefs` | libxslt/security.h |
//! | `xsltGetDefaultSecurityPrefs` | libxslt/security.h |
//!
//! # UPSTREAM-PARITY (R-000125)
//!
//! The security model is **callback-based**, not value-based. Upstream
//! `xsltSecurityPrefs` holds five `xsltSecurityCheck` function pointers
//! (readFile / createFile / createDir / readNet / writeNet) and:
//!
//! - `xsltNewSecurityPrefs()` returns a **zeroed** block (all callbacks NULL);
//! - `xsltSetSecurityPrefs(sec, option, func)` stores the callback, with the
//!   upstream quirk that `XSLT_SECPREF_WRITE_FILE` writes the **createFile**
//!   slot (`security.c` `xsltSetSecurityPrefs` case);
//! - `xsltGetSecurityPrefs(sec, option)` returns the stored callback or NULL;
//! - the callbacks are never invoked by libxslt 1.1.42 itself — the surface is
//!   registration-only, exercised by consumers such as `xsltproc --nowrite`
//!   which registers `xsltSecurityForbid`.
//!
//! Earlier revisions of this module implemented a divergent int allow/deny
//! model (`xsltSetSecurityPrefs(sec, option, value: c_int)`); the module was
//! reimplemented to the upstream contract (R-000125, closed 11.1-G/H).
//!
//! # Proving courts
//!
//! XSLT-001 (xslt-family differential probe exercises `xsltSecurityAllow`,
//! `xsltSecurityForbid`, `xsltSetCtxtSecurityPrefs`), DSO-LOADER (R-000160
//! trivial-body exports), the callback round-trip unit tests in this
//! module, and the in-crate `cargo test` suites.
//!
//! # Tempting simplifications that would break parity
//!
//! - Reverting to an int allow/deny model (the pre-R-000125 design) is
//!   ABI-incompatible with downstream consumers that register callbacks;
//!   the callback slots are the upstream contract.
//! - Special-casing WRITE_FILE into its own slot (instead of the
//!   createFile quirk) breaks `xsltGetSecurityPrefs` round-trips and the
//!   differential probe.
//! - Making the default-prefs state per-transform instead of global would
//!   break `xsltSetDefaultSecurityPrefs`/`xsltGetDefaultSecurityPrefs`
//!   semantics.

#![allow(
    clippy::missing_inline_in_public_items,
    clippy::must_use_candidate,
    clippy::missing_safety_doc
)]

use std::ffi::{c_char, c_void};
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

/// Security check callback (upstream `xsltSecurityCheck`):
/// `int (*)(xsltSecurityPrefsPtr sec, xsltTransformContextPtr ctxt,
///           const char *value)` — returns non-zero to allow, 0 to deny.
pub type xsltSecurityCheck = unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_char) -> c_int;

/// Internal security preferences structure — five check callbacks, one per
/// controllable option (upstream `xsltSecurityPrefs` in security.c).
///
/// # ABI
/// The structure is private in upstream (defined only in security.c); the
/// Rust representation is opaque to C consumers, so the layout is internal.
#[derive(Debug)]
#[repr(C)]
pub struct XsltSecurityPrefs {
    pub(crate) readFile: Option<xsltSecurityCheck>,
    pub(crate) createFile: Option<xsltSecurityCheck>,
    pub(crate) createDir: Option<xsltSecurityCheck>,
    pub(crate) readNet: Option<xsltSecurityCheck>,
    pub(crate) writeNet: Option<xsltSecurityCheck>,
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

/// Create new security preferences with the upstream default: a **zeroed**
/// block whose five check callbacks are all NULL.
///
/// # Returns
///
/// A non-null pointer to the newly allocated security preferences on success.
/// The caller is responsible for freeing it with [`xsltFreeSecurityPrefs`].
///
/// # Safety
///
/// The returned pointer must eventually be freed with
/// [`xsltFreeSecurityPrefs`] to avoid memory leaks.
#[no_mangle]
pub unsafe extern "C" fn xsltNewSecurityPrefs() -> *mut c_void {
    let prefs = Box::new(XsltSecurityPrefs {
        readFile: None,
        createFile: None,
        createDir: None,
        readNet: None,
        writeNet: None,
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

/// Update a security option to use the given check callback.
///
/// UPSTREAM-PARITY: mirrors `security.c` `xsltSetSecurityPrefs`, including
/// the quirk that `XSLT_SECPREF_WRITE_FILE` stores into the `createFile`
/// slot. `option` uses the `xsltSecurityOption` enum values (1-5).
///
/// # Returns
///
/// 0 on success, -1 if `sec` is NULL or `option` is out of range.
///
/// # Safety
///
/// `sec` must point to a valid [`XsltSecurityPrefs`] obtained from
/// [`xsltNewSecurityPrefs`] that has not yet been freed.
#[no_mangle]
pub unsafe extern "C" fn xsltSetSecurityPrefs(
    sec: *mut c_void,
    option: c_int,
    func: Option<xsltSecurityCheck>,
) -> c_int {
    if sec.is_null() {
        return -1;
    }
    let prefs = &mut *(sec as *mut XsltSecurityPrefs);
    match option {
        XSLT_SECPREF_READ_FILE => prefs.readFile = func,
        // UPSTREAM-PARITY: WRITE_FILE writes createFile (upstream quirk).
        // Verified against oracle/historical/src/libxslt-1.1.42/libxslt/
        // security.c: xsltSetSecurityPrefs case XSLT_SECPREF_WRITE_FILE
        // stores func in sec->createFile; xsltGetSecurityPrefs reads it
        // back from the same slot. A separate writeNet slot also exists.
        XSLT_SECPREF_WRITE_FILE => prefs.createFile = func,
        XSLT_SECPREF_CREATE_DIRECTORY => prefs.createDir = func,
        XSLT_SECPREF_READ_NETWORK => prefs.readNet = func,
        XSLT_SECPREF_WRITE_NETWORK => prefs.writeNet = func,
        _ => return -1,
    }
    0
}

/// Look up the check callback configured for a security option.
///
/// # Returns
///
/// The stored callback, or NULL if `sec` is NULL or `option` is invalid or
/// unset (upstream `xsltGetSecurityPrefs` returns NULL in those cases).
///
/// # Safety
///
/// If `sec` is non-null, it must point to a valid [`XsltSecurityPrefs`]
/// obtained from [`xsltNewSecurityPrefs`] that has not yet been freed.
#[no_mangle]
pub unsafe extern "C" fn xsltGetSecurityPrefs(
    sec: *mut c_void,
    option: c_int,
) -> Option<xsltSecurityCheck> {
    if sec.is_null() {
        return None;
    }
    let prefs = &*(sec as *mut XsltSecurityPrefs);
    match option {
        XSLT_SECPREF_READ_FILE => prefs.readFile,
        XSLT_SECPREF_WRITE_FILE => prefs.createFile,
        XSLT_SECPREF_CREATE_DIRECTORY => prefs.createDir,
        XSLT_SECPREF_READ_NETWORK => prefs.readNet,
        XSLT_SECPREF_WRITE_NETWORK => prefs.writeNet,
        _ => None,
    }
}

/// Set the default security preferences used by new transformations.
///
/// The provided pointer is stored as the global default. It is the caller's
/// responsibility to manage the lifetime of the pointed-to preferences.
///
/// # Safety
///
/// `sec` must point to a valid [`XsltSecurityPrefs`] that remains valid for
/// the duration it is set as the default (i.e., until replaced by another
/// call to this function).
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
/// [`xsltSetDefaultSecurityPrefs`], or NULL if none have been set.
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

    /// The security API surface matches the upstream callback contract:
    /// set/get round-trip callbacks and default to NULL.
    ///
    /// # Safety
    ///
    /// - `prefs` is a valid `XsltSecurityPrefs` returned by
    ///   `xsltNewSecurityPrefs` (asserted non-NULL) and is released with
    ///   `xsltFreeSecurityPrefs`; the getter calls only read the callback
    ///   slots through this live pointer.
    #[test]
    fn test_set_get_callback_roundtrip() {
        unsafe {
            let prefs = xsltNewSecurityPrefs();
            assert!(!prefs.is_null());
            // Freshly created prefs have no callbacks configured.
            assert!(xsltGetSecurityPrefs(prefs, XSLT_SECPREF_READ_FILE).is_none());
            assert!(xsltGetSecurityPrefs(prefs, XSLT_SECPREF_WRITE_FILE).is_none());
            assert!(xsltGetSecurityPrefs(prefs, XSLT_SECPREF_CREATE_DIRECTORY).is_none());
            assert!(xsltGetSecurityPrefs(prefs, XSLT_SECPREF_READ_NETWORK).is_none());
            assert!(xsltGetSecurityPrefs(prefs, XSLT_SECPREF_WRITE_NETWORK).is_none());
            xsltFreeSecurityPrefs(prefs);
        }
    }

    /// Registering a callback per option and reading it back works, and the
    /// upstream WRITE_FILE -> createFile slot quirk is preserved.
    ///
    /// # Safety
    ///
    /// - `prefs` is a live `XsltSecurityPrefs` from `xsltNewSecurityPrefs`
    ///   released with `xsltFreeSecurityPrefs`.
    /// - `forbid` is a valid `unsafe extern "C"` callback whose address is
    ///   stable within the DSO, so storing it and comparing it by address
    ///   through `xsltSetSecurityPrefs`/`xsltGetSecurityPrefs` is
    ///   well-defined; the setters/getters never invoke the callback.
    ///
    /// UPSTREAM-PARITY: the round-trip asserts compare the stored callback
    /// against the registered one by address (the C API contract); on ELF
    /// platforms a symbol's address is stable within the DSO.
    #[test]
    #[allow(renamed_and_removed_lints, clippy::fn_address_comparisons)]
    fn test_set_get_registered_callback() {
        /// Deny-everything security check callback used to exercise the
        /// register/read-back surface.
        ///
        /// # Safety
        ///
        /// - The function ignores its raw-pointer arguments (`_sec`,
        ///   `_ctxt`, `_value`) and never dereferences them, so it is
        ///   callable with any pointer values passed with C ABI
        ///   conventions.
        unsafe extern "C" fn forbid(
            _sec: *mut c_void,
            _ctxt: *mut c_void,
            _value: *const c_char,
        ) -> c_int {
            0
        }
        let forbid: xsltSecurityCheck = forbid;
        unsafe {
            let prefs = xsltNewSecurityPrefs();
            assert_eq!(
                xsltSetSecurityPrefs(prefs, XSLT_SECPREF_READ_FILE, Some(forbid)),
                0
            );
            assert_eq!(
                xsltSetSecurityPrefs(prefs, XSLT_SECPREF_WRITE_FILE, Some(forbid)),
                0
            );
            assert_eq!(
                xsltGetSecurityPrefs(prefs, XSLT_SECPREF_READ_FILE),
                Some(forbid)
            );
            // WRITE_FILE reads back from the createFile slot (upstream quirk).
            assert_eq!(
                xsltGetSecurityPrefs(prefs, XSLT_SECPREF_WRITE_FILE),
                Some(forbid)
            );
            xsltFreeSecurityPrefs(prefs);
        }
    }

    /// Invalid options and NULL prefs return -1 / NULL like upstream.
    ///
    /// # Safety
    ///
    /// - `xsltSetSecurityPrefs`/`xsltGetSecurityPrefs` return early on a
    ///   NULL `sec` or an invalid option before dereferencing, so passing
    ///   `ptr::null_mut()` is safe; `prefs` is a live `XsltSecurityPrefs`
    ///   from `xsltNewSecurityPrefs` released with `xsltFreeSecurityPrefs`.
    #[test]
    fn test_invalid_option_and_null() {
        unsafe {
            assert_eq!(
                xsltSetSecurityPrefs(ptr::null_mut(), XSLT_SECPREF_READ_FILE, None),
                -1
            );
            let prefs = xsltNewSecurityPrefs();
            assert_eq!(xsltSetSecurityPrefs(prefs, 99, None), -1);
            assert!(xsltGetSecurityPrefs(prefs, 99).is_none());
            assert!(xsltGetSecurityPrefs(ptr::null_mut(), XSLT_SECPREF_READ_FILE).is_none());
            xsltFreeSecurityPrefs(prefs);
        }
    }

    /// Verify that creating and freeing security prefs works.
    ///
    /// # Safety
    ///
    /// - `prefs` is a pointer returned by `xsltNewSecurityPrefs`
    ///   (asserted non-NULL) that has not yet been freed, so
    ///   `xsltFreeSecurityPrefs` reconstructs the box and frees it exactly
    ///   once.
    #[test]
    fn test_new_free_security_prefs() {
        unsafe {
            let prefs = xsltNewSecurityPrefs();
            assert!(!prefs.is_null());
            xsltFreeSecurityPrefs(prefs);
        }
    }

    /// Verify that freeing a null pointer is a no-op.
    ///
    /// # Safety
    ///
    /// - `xsltFreeSecurityPrefs` checks for NULL before `Box::from_raw`,
    ///   so passing `ptr::null_mut()` performs no dereference or
    ///   deallocation.
    #[test]
    fn test_free_null() {
        unsafe {
            // Should not panic or crash.
            xsltFreeSecurityPrefs(ptr::null_mut());
        }
    }
}
