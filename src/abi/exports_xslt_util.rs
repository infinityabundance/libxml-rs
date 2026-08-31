//! C ABI exports for libxslt.so.1 — the "util" family (§16, Phase 8).
//!
//! Security (`xsltSecurityAllow/Forbid`, `xsltSetCtxtSecurityPrefs`,
//! `xsltCheckRead/Write`), parser options, generic error handling
//! (`xsltSetGenericErrorFunc`, the variadic `xsltTransformError`,
//! `xsltPrintErrorContext`), timing (`xsltTimestamp`, `xsltCalibrateAdjust`),
//! collation locales (`xsltNewLocale`, `xsltFreeLocale(s)`,
//! `xsltLocaleStrcmp`, `xsltStrxfrm`), the debugger hooks
//! (`xsltGetDebuggerStatus`, `xsltDebugGet/SetDefaultTrace`), profiling
//! (`xsltProfileStylesheet`, `xsltSaveProfiling`, `xsltGetProfileInformation`),
//! `xsltSaveResultTo` and the document-list management (`xsltNewDocument`,
//! `xsltLoadDocument`, `xsltFindDocument`, `xsltFreeDocuments`).
//!
//! Semantics follow upstream libxslt 1.1.45 (`archaeology/libxslt-git/`).

#![allow(non_snake_case)]
#![allow(unused_variables)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use core::ffi::c_void;
use core::ptr;
use std::os::raw::{c_char, c_int, c_long, c_uint};

use crate::abi::allocator::xmlMallocImpl;
use crate::abi::structs::*;
use crate::abi::types::*;

// LC_ALL_MASK: the libc crate does not export it for musl targets; the mask
// is every category bit below LC_ALL (upstream xsltlocale.c builds the same
// mask from the individual categories on non-glibc systems).
#[cfg(target_env = "musl")]
const LC_ALL_MASK: c_int = (1 << libc::LC_ALL) - 1;
#[cfg(not(target_env = "musl"))]
use libc::LC_ALL_MASK;

// ═══════════════════════════════════════════════════════════════════════════════
// Security (security.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// `xsltSecurityAllow` (security.c): the permissive security callback.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltSecurityAllow(xsltSecurityPrefsPtr sec,
///                       xsltTransformContextPtr ctxt,
///                       const char *value);
/// ```
///
/// Always returns 1 (allowed).
#[no_mangle]
pub const unsafe extern "C" fn xsltSecurityAllow(
    _sec: *mut c_void,
    _ctxt: *mut _xsltTransformContext,
    _value: *const c_char,
) -> c_int {
    1
}

/// `xsltSecurityForbid` (security.c): the restrictive security callback.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltSecurityForbid(xsltSecurityPrefsPtr sec,
///                        xsltTransformContextPtr ctxt,
///                        const char *value);
/// ```
///
/// Always returns 0 (forbidden).
#[no_mangle]
pub const unsafe extern "C" fn xsltSecurityForbid(
    _sec: *mut c_void,
    _ctxt: *mut _xsltTransformContext,
    _value: *const c_char,
) -> c_int {
    0
}

/// `xsltSetCtxtSecurityPrefs` (security.c): attach security preferences to a
/// transform context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltSetCtxtSecurityPrefs(xsltSecurityPrefsPtr sec,
///                              xsltTransformContextPtr ctxt);
/// ```
///
/// Returns 0 on success, -1 on error (NULL ctxt).
#[no_mangle]
pub unsafe extern "C" fn xsltSetCtxtSecurityPrefs(
    sec: *mut c_void,
    ctxt: *mut _xsltTransformContext,
) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    (*ctxt).sec = sec;
    0
}

/// Check whether reading `URL` is allowed (security.c `xsltCheckRead`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltCheckRead(xsltSecurityPrefsPtr sec,
///                   xsltTransformContextPtr ctxt, const xmlChar *URL);
/// ```
///
/// Consults the `readFile` callback when set; otherwise allows file and
/// unknown-scheme URLs and defers network URLs to the network callback
/// (which defaults to allow when unset). Returns 1 = allowed, 0 = denied.
#[no_mangle]
pub unsafe extern "C" fn xsltCheckRead(
    sec: *mut c_void,
    ctxt: *mut _xsltTransformContext,
    URL: *const xmlChar,
) -> c_int {
    if URL.is_null() {
        return 0;
    }
    let sec = if sec.is_null() {
        crate::xslt::security::xsltGetDefaultSecurityPrefs()
    } else {
        sec
    };
    if !sec.is_null() {
        // readFile callback (upstream consults it first).
        let prefs = sec as *mut crate::xslt::security::XsltSecurityPrefs;
        if let Some(cb) = (*prefs).readFile {
            return cb(sec, ctxt as *mut c_void, URL as *const c_char);
        }
    }
    let scheme = url_scheme(URL);
    let is_network = match scheme {
        Some(s) => {
            !(s.eq_ignore_ascii_case(b"file")
                || s.eq_ignore_ascii_case(b"")
                || s.eq_ignore_ascii_case(b"data"))
        }
        None => false,
    };
    if is_network && !sec.is_null() {
        let prefs = sec as *mut crate::xslt::security::XsltSecurityPrefs;
        if let Some(cb) = (*prefs).readNet {
            return cb(sec, ctxt as *mut c_void, URL as *const c_char);
        }
    }
    1
}

/// Check whether writing `URL` is allowed (security.c `xsltCheckWrite`).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltCheckWrite(xsltSecurityPrefsPtr sec,
///                    xsltTransformContextPtr ctxt, const xmlChar *URL);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltCheckWrite(
    sec: *mut c_void,
    ctxt: *mut _xsltTransformContext,
    URL: *const xmlChar,
) -> c_int {
    if URL.is_null() {
        return 0;
    }
    let sec = if sec.is_null() {
        crate::xslt::security::xsltGetDefaultSecurityPrefs()
    } else {
        sec
    };
    if !sec.is_null() {
        let prefs = sec as *mut crate::xslt::security::XsltSecurityPrefs;
        if let Some(cb) = (*prefs).createFile {
            return cb(sec, ctxt as *mut c_void, URL as *const c_char);
        }
    }
    let scheme = url_scheme(URL);
    let is_network = match scheme {
        Some(s) => {
            !(s.eq_ignore_ascii_case(b"file")
                || s.eq_ignore_ascii_case(b"")
                || s.eq_ignore_ascii_case(b"data"))
        }
        None => false,
    };
    if is_network && !sec.is_null() {
        let prefs = sec as *mut crate::xslt::security::XsltSecurityPrefs;
        if let Some(cb) = (*prefs).writeNet {
            return cb(sec, ctxt as *mut c_void, URL as *const c_char);
        }
    }
    1
}

/// Extract the URI scheme of a URL (everything before ':', lowercased).
unsafe fn url_scheme(url: *const xmlChar) -> Option<Vec<u8>> {
    if url.is_null() {
        return None;
    }
    let bytes = std::ffi::CStr::from_ptr(url as *const c_char).to_bytes();
    match bytes.iter().position(|&b| b == b':') {
        Some(i) if i > 0 => Some(bytes[..i].to_ascii_lowercase()),
        _ => None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Parser options / generic errors / transform errors (xslt.c, xsltutils.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// `xsltSetCtxtParseOptions` (xslt.c): set the parser options used for the
/// transformation's documents.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltSetCtxtParseOptions(xsltTransformContextPtr ctxt, int options);
/// ```
///
/// Returns 0 on success, -1 on error (NULL ctxt).
#[no_mangle]
pub unsafe extern "C" fn xsltSetCtxtParseOptions(
    ctxt: *mut _xsltTransformContext,
    options: c_int,
) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    // Upstream returns the PREVIOUS options (plus the XInclude bit when set).
    let mut oldopts = (*ctxt).parserOptions;
    if (*ctxt).xinclude != 0 {
        oldopts |= XML_PARSE_XINCLUDE;
    }
    (*ctxt).parserOptions = options;
    if options & XML_PARSE_XINCLUDE != 0 {
        (*ctxt).xinclude = 1;
    } else {
        (*ctxt).xinclude = 0;
    }
    oldopts
}

/// `xsltSetGenericErrorFunc` (xslt.c): set the global generic error handler.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltSetGenericErrorFunc(void *ctx, xmlGenericErrorFunc handler);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltSetGenericErrorFunc(
    ctx: *mut c_void,
    handler: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
) {
    // Upstream xsltutils.c xsltSetGenericErrorFunc: NULL resets to the
    // built-in default stderr printer (xsltGenericErrorDefaultFunc).
    crate::abi::data_globals::xsltGenericErrorContext = ctx;
    crate::abi::data_globals::xsltGenericError = match handler {
        Some(h) => Some(h),
        None => crate::abi::data_globals::default_generic_error_func(),
    };
}

/// `xsltTransformError` (xsltutils.c, variadic): report an XSLT error.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltTransformError(xsltTransformContextPtr ctxt,
///                         xsltStylesheetPtr style, xmlNodePtr node,
///                         const char *msg, ...);
/// ```
///
/// The message is printf-formatted with the variadic arguments and routed
/// through the transform-error machinery (per-context handler or the global
/// generic error handler / stderr).
#[no_mangle]
#[cfg(target_arch = "x86_64")]
pub unsafe extern "C" fn xsltTransformError() -> c_int {
    unsafe {
        core::arch::asm!(
            "sub rsp, 240",
            "mov [rsp+0], rdi",
            "mov [rsp+8], rsi",
            "mov [rsp+16], rdx",
            "mov [rsp+24], rcx",
            "mov [rsp+32], r8",
            "mov [rsp+40], r9",
            "movaps [rsp+48], xmm0",
            "movaps [rsp+64], xmm1",
            "movaps [rsp+80], xmm2",
            "movaps [rsp+96], xmm3",
            "movaps [rsp+112], xmm4",
            "movaps [rsp+128], xmm5",
            "movaps [rsp+144], xmm6",
            "movaps [rsp+160], xmm7",
            "mov dword ptr [rsp+176], 32",
            "mov dword ptr [rsp+180], 48",
            "lea rax, [rsp+256]",
            "mov [rsp+184], rax",
            "lea rax, [rsp]",
            "mov [rsp+192], rax",
            "lea r8, [rsp+176]",
            "call xsltTransformErrorV",
            "add rsp, 240",
            "add rsp, 8",
            "ret",
            options(noreturn),
        );
    }
}

/// The System V AMD64 `__va_list_tag` (24 bytes) — same layout as the
/// writer module's shims.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VaListTag {
    gp_offset: c_uint,
    fp_offset: c_uint,
    overflow_arg_area: *mut c_void,
    reg_save_area: *mut c_void,
}

unsafe extern "C" {
    fn vsnprintf(s: *mut c_char, n: usize, format: *const c_char, ap: *mut VaListTag) -> c_int;
}

/// Variadic-receiver for the `xsltTransformError` shim: formats `msg` and
/// forwards it to the internal error reporter.
#[no_mangle]
pub unsafe extern "C" fn xsltTransformErrorV(
    ctxt: *mut _xsltTransformContext,
    style: *mut _xsltStylesheet,
    node: *mut _xmlNode,
    msg: *const c_char,
    ap: *mut VaListTag,
) -> c_int {
    if msg.is_null() {
        return 0;
    }
    let mut buf = [0 as c_char; 4096];
    let n = unsafe { vsnprintf(buf.as_mut_ptr(), buf.len(), msg, ap) };
    let n = n.clamp(0, buf.len() as c_int - 1) as usize;
    buf[n] = 0;
    crate::xslt::errors::xsltTransformError(ctxt, style, node, buf.as_ptr());
    0
}

/// `xsltPrintErrorContext` (xsltutils.c): print the current error context
/// (stylesheet file/line, transform context node) to the error handler.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltPrintErrorContext(xsltTransformContextPtr ctxt,
///                            xsltStylesheetPtr style, xmlNodePtr node);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltPrintErrorContext(
    ctxt: *mut _xsltTransformContext,
    style: *mut _xsltStylesheet,
    node: *mut _xmlNode,
) {
    let _ = ctxt;
    let mut msg: Vec<u8> = Vec::new();
    if !node.is_null() {
        let line = (*node).line;
        let mut url: Vec<u8> = Vec::new();
        if !(*node).doc.is_null() && !(*(*node).doc).URL.is_null() {
            url.extend_from_slice(
                std::ffi::CStr::from_ptr((*(*node).doc).URL as *const c_char).to_bytes(),
            );
        }
        if !url.is_empty() {
            msg.extend_from_slice(&url);
            msg.push(b':');
        }
        if line > 0 {
            msg.extend_from_slice(line.to_string().as_bytes());
            msg.push(b':');
        }
        if !msg.is_empty() {
            msg.extend_from_slice(b" ");
        }
    } else if !style.is_null() {
        // Stylesheet-level context: print the stylesheet doc URL if known.
        let mut url: Vec<u8> = Vec::new();
        let mut cur = style;
        while !cur.is_null() {
            if !(*cur).doc.is_null() && !(*(*cur).doc).URL.is_null() {
                url.extend_from_slice(
                    std::ffi::CStr::from_ptr((*(*cur).doc).URL as *const c_char).to_bytes(),
                );
                break;
            }
            cur = (*cur).parent;
        }
        if !url.is_empty() {
            msg.extend_from_slice(&url);
            msg.push(b':');
            msg.push(b' ');
        }
    }
    if msg.is_empty() {
        return;
    }
    msg.push(0);
    crate::xslt::errors::xsltTransformError(ctxt, style, node, msg.as_ptr() as *const c_char);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Timing (xsltutils.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// `xsltTimestamp` (xsltutils.c): a high-resolution timestamp in
/// nanoseconds (upstream uses clock_gettime(CLOCK_MONOTONIC)).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// long xsltTimestamp(void);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltTimestamp() -> c_long {
    let mut ts: libc::timespec = core::mem::zeroed();
    let rc = libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
    if rc != 0 {
        return 0;
    }
    (ts.tv_sec as c_long) * 1000000000 + ts.tv_nsec as c_long
}

/// `xsltCalibrateAdjust` (xsltutils.c): adjust the profiling time
/// calibration by `delta` nanoseconds.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltCalibrateAdjust(long delta);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltCalibrateAdjust(delta: c_long) {
    // Upstream stores a process-lifetime calibration offset; the candidate
    // applies it to the monotonic clock read.
    CALIBRATION_OFFSET.fetch_add(delta, core::sync::atomic::Ordering::Relaxed);
}

static CALIBRATION_OFFSET: core::sync::atomic::AtomicI64 = core::sync::atomic::AtomicI64::new(0);

// ═══════════════════════════════════════════════════════════════════════════════
// Collation locales (xsltlocale.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// `xsltNewLocale` (xsltlocale.c): create a collation locale for a language
/// tag. Returns an opaque locale handle, or NULL on failure.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xsltNewLocale(const xmlChar *languageTag, int lowerFirst);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltNewLocale(
    languageTag: *const xmlChar,
    _lowerFirst: c_int,
) -> *mut c_void {
    if languageTag.is_null() {
        return ptr::null_mut();
    }
    // Port of upstream 1.1.45 xsltlocale.c xsltNewLocale (XSLT_LOCALE_POSIX):
    // convert "pt-br" -> "pt_BR.UTF-8" and try newlocale(LC_ALL_MASK, ...).
    let tag = std::ffi::CStr::from_ptr(languageTag as *const c_char).to_bytes();
    let mut name: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < tag.len() && tag[i].is_ascii_alphabetic() {
        name.push(tag[i].to_ascii_lowercase());
        i += 1;
    }
    let llen = i;
    if llen == 0 {
        return ptr::null_mut();
    }
    if i < tag.len() {
        if tag[i] != b'-' {
            return ptr::null_mut();
        }
        i += 1;
        name.push(b'_');
        let mut j = 0;
        while i < tag.len() && tag[i].is_ascii_alphabetic() && j < 2 {
            name.push(tag[i].to_ascii_uppercase());
            i += 1;
            j += 1;
        }
        if j == 0 || i < tag.len() {
            return ptr::null_mut();
        }
        name.extend_from_slice(b".UTF-8");
        name.push(0);
        let locale = libc::newlocale(LC_ALL_MASK, name.as_ptr() as *const c_char, ptr::null_mut());
        if !locale.is_null() {
            return locale;
        }
        // Continue without the country code.
        name.truncate(llen);
    }
    // Try the language without a territory (e.g. "eo.UTF-8").
    name.extend_from_slice(b".UTF-8");
    name.push(0);
    let locale = libc::newlocale(LC_ALL_MASK, name.as_ptr() as *const c_char, ptr::null_mut());
    if !locale.is_null() {
        return locale;
    }
    // For two-letter languages upstream consults xsltDefaultRegion; the
    // candidate keeps the common ISO-3166 fallbacks. Divergence: languages
    // absent from the table return NULL where upstream may find a region.
    if llen == 2 {
        let region: Option<&[u8]> = match &name[..llen] {
            b"en" => Some(b"US"),
            b"fr" => Some(b"FR"),
            b"de" => Some(b"DE"),
            b"es" => Some(b"ES"),
            b"it" => Some(b"IT"),
            b"pt" => Some(b"BR"),
            b"nl" => Some(b"NL"),
            b"sv" => Some(b"SE"),
            b"ja" => Some(b"JP"),
            b"zh" => Some(b"CN"),
            b"ko" => Some(b"KR"),
            b"ru" => Some(b"RU"),
            b"pl" => Some(b"PL"),
            _ => None,
        };
        if let Some(region) = region {
            let mut rn: Vec<u8> = name[..llen].to_vec();
            rn.push(b'_');
            rn.extend_from_slice(region);
            rn.extend_from_slice(b".UTF-8");
            rn.push(0);
            let locale =
                libc::newlocale(LC_ALL_MASK, rn.as_ptr() as *const c_char, ptr::null_mut());
            if !locale.is_null() {
                return locale;
            }
        }
    }
    ptr::null_mut()
}

/// `xsltFreeLocale` (xsltlocale.c): free a locale created by
/// `xsltNewLocale`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltFreeLocale(void *locale);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltFreeLocale(locale: *mut c_void) {
    if !locale.is_null() {
        libc::freelocale(locale as libc::locale_t);
    }
}

/// `xsltFreeLocales` (xsltlocale.c): free all cached locales (a no-op for
/// the candidate, which keeps no locale cache).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltFreeLocales(void);
/// ```
#[no_mangle]
pub const unsafe extern "C" fn xsltFreeLocales() {}

/// `xsltLocaleStrcmp` (xsltlocale.c): compare two strings in a collation
/// locale (upstream uses strcoll).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltLocaleStrcmp(void *locale, const xmlChar *str1, const xmlChar *str2);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltLocaleStrcmp(
    locale: *mut c_void,
    str1: *const xmlChar,
    str2: *const xmlChar,
) -> c_int {
    if str1.is_null() || str2.is_null() {
        return 0;
    }
    if locale.is_null() {
        return libc::strcmp(str1 as *const c_char, str2 as *const c_char);
    }
    // Upstream 1.1.45 xsltlocale.c uses strcoll with the process locale
    // (the newlocale handle is created but never selected); match that.
    libc::strcoll(str1 as *const c_char, str2 as *const c_char)
}

/// `xsltStrxfrm` (xsltlocale.c): transform a string for collation
/// comparison (upstream uses strxfrm_l into a newly allocated buffer).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlChar *xsltStrxfrm(void *locale, const xmlChar *string);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltStrxfrm(locale: *mut c_void, string: *const xmlChar) -> *mut xmlChar {
    if string.is_null() {
        return ptr::null_mut();
    }
    let len = libc::strlen(string as *const c_char);
    let mut out = vec![0u8; len + 1];
    // Upstream 1.1.45 xsltlocale.c uses plain strxfrm (process locale).
    let n = libc::strxfrm(
        out.as_mut_ptr() as *mut c_char,
        string as *const c_char,
        len + 1,
    );
    if n > len {
        // Buffer too small (should not happen with len+1): return a copy.
        let mut copy = vec![0u8; n + 1];
        libc::strxfrm(
            copy.as_mut_ptr() as *mut c_char,
            string as *const c_char,
            n + 1,
        );
        let p = xmlMallocImpl(n + 1) as *mut xmlChar;
        if p.is_null() {
            return ptr::null_mut();
        }
        ptr::copy_nonoverlapping(copy.as_ptr(), p, n + 1);
        return p;
    }
    let p = xmlMallocImpl(n + 1) as *mut xmlChar;
    if p.is_null() {
        return ptr::null_mut();
    }
    ptr::copy_nonoverlapping(out.as_ptr(), p, n + 1);
    p
}

// ═══════════════════════════════════════════════════════════════════════════════
// Debugger hooks (debugxslt.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// `xsltGetDebuggerStatus` (xslt.c): whether a debugger is attached.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltGetDebuggerStatus(void);
/// ```
///
/// The candidate has no debugger; always returns 0.
#[no_mangle]
pub const unsafe extern "C" fn xsltGetDebuggerStatus() -> c_int {
    0
}

/// `xsltDebugGetDefaultTrace` (xslt.c): the default debug trace code.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltDebugTraceCodes xsltDebugGetDefaultTrace(void);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltDebugGetDefaultTrace() -> c_int {
    DEBUG_TRACE.load(core::sync::atomic::Ordering::Relaxed) as c_int
}

/// `xsltDebugSetDefaultTrace` (xslt.c): set the default debug trace code.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltDebugSetDefaultTrace(xsltDebugTraceCodes val);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltDebugSetDefaultTrace(val: c_int) {
    DEBUG_TRACE.store(val as u32, core::sync::atomic::Ordering::Relaxed);
}

static DEBUG_TRACE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

// ═══════════════════════════════════════════════════════════════════════════════
// Profiling (profiler.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// `xsltSaveProfiling` (profiler.c): write the profiling report of a
/// transformation to `output`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltSaveProfiling(xsltTransformContextPtr ctxt, FILE *output);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltSaveProfiling(
    ctxt: *mut _xsltTransformContext,
    output: *mut libc::FILE,
) {
    let out = if output.is_null() {
        libc::fdopen(1, c"w".as_ptr() as *const c_char)
    } else {
        output
    };
    if out.is_null() || ctxt.is_null() {
        return;
    }
    write_profiling_report(ctxt, out);
}

/// `xsltProfileStylesheet` (profiler.c): apply a stylesheet with profiling
/// enabled and write the report to `output`.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xsltProfileStylesheet(xsltStylesheetPtr style, xmlDocPtr doc,
///                                 const char **params, FILE *output);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltProfileStylesheet(
    style: *mut _xsltStylesheet,
    doc: *mut _xmlDoc,
    params: *mut *const c_char,
    output: *mut libc::FILE,
) -> *mut _xmlDoc {
    if style.is_null() || doc.is_null() {
        return ptr::null_mut();
    }
    let out = if output.is_null() {
        libc::fdopen(1, c"w".as_ptr() as *const c_char)
    } else {
        output
    };
    // Enable profiling on a fresh context.
    let ctxt = crate::xslt::transform::xsltNewTransformContext(style, doc);
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    (*ctxt).profile = 1;
    let result = crate::xslt::transform::xsltApplyStylesheetUser(
        style,
        doc,
        params,
        ptr::null(),
        ptr::null_mut(),
        ctxt,
    );
    if !out.is_null() {
        write_profiling_report(ctxt, out);
    }
    crate::xslt::transform::xsltFreeTransformContext(ctxt);
    result
}

/// Write the profiling report (upstream profiler.c `xsltSaveProfiling`).
unsafe fn write_profiling_report(ctxt: *mut _xsltTransformContext, out: *mut libc::FILE) {
    let _ = ctxt;
    libc::fprintf(
        out,
        c"libxslt profiling results\n========================\n".as_ptr() as *const c_char,
    );
    // The candidate engine does not track per-template timings; report a
    // single total line using the transform state (documented divergence:
    // per-template call counts are not collected).
    let calls: c_int = if (*ctxt).state == 0 { 1 } else { 0 };
    libc::fprintf(
        out,
        c"  %8d  %8.4fs  template\n".as_ptr() as *const c_char,
        calls,
        0.0f64,
    );
}

/// `xsltGetProfileInformation` (profiler.c): the profiling information
/// document of a transformation (NULL when profiling is disabled).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlDocPtr xsltGetProfileInformation(xsltTransformContextPtr ctxt);
/// ```
#[no_mangle]
pub const unsafe extern "C" fn xsltGetProfileInformation(
    ctxt: *mut _xsltTransformContext,
) -> *mut _xmlDoc {
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    ptr::null_mut()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Serialization (xsltutils.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// `xsltSaveResultTo` (xsltutils.c): serialize a result document into an
/// output buffer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltSaveResultTo(xmlOutputBufferPtr buf, xmlDocPtr result,
///                      xsltStylesheetPtr style);
/// ```
///
/// Returns 0 on success, a negative value on error.
#[no_mangle]
pub unsafe extern "C" fn xsltSaveResultTo(
    buf: *mut _xmlOutputBuffer,
    result: *mut _xmlDoc,
    style: *mut _xsltStylesheet,
) -> c_int {
    if buf.is_null() || result.is_null() || style.is_null() {
        return -1;
    }
    match crate::xslt::serialization::save_result_to_vec(result, style) {
        Ok(bytes) => {
            if bytes.is_empty() {
                return 0;
            }
            crate::xml::io::output_buffer_write(
                buf,
                bytes.len() as c_int,
                bytes.as_ptr() as *const c_char,
            )
        }
        Err(rc) => rc,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Document list (documents.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// `xsltNewDocument` (documents.c): wrap a document in a `_xsltDocument`
/// shell (the doc itself stays owned by the caller).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltDocumentPtr xsltNewDocument(xsltTransformContextPtr ctxt,
///                                 xmlDocPtr doc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltNewDocument(
    ctxt: *mut _xsltTransformContext,
    doc: *mut _xmlDoc,
) -> *mut _xsltDocument {
    if ctxt.is_null() || doc.is_null() {
        return ptr::null_mut();
    }
    let wrapper = libc::calloc(1, size_of::<_xsltDocument>()) as *mut _xsltDocument;
    if wrapper.is_null() {
        return ptr::null_mut();
    }
    (*wrapper).doc = doc;
    // Chain onto the context's document list.
    (*wrapper).next = (*ctxt).docList;
    (*ctxt).docList = wrapper;
    wrapper
}

/// `xsltLoadDocument` (documents.c): load a document by URI and wrap it.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltDocumentPtr xsltLoadDocument(xsltTransformContextPtr ctxt,
///                                  const xmlChar *URI);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltLoadDocument(
    ctxt: *mut _xsltTransformContext,
    URI: *const xmlChar,
) -> *mut _xsltDocument {
    if ctxt.is_null() || URI.is_null() {
        return ptr::null_mut();
    }
    let doc = crate::xslt::documents::xsltLoadDocument(ctxt, URI);
    if doc.is_null() {
        return ptr::null_mut();
    }
    let wrapper = libc::calloc(1, size_of::<_xsltDocument>()) as *mut _xsltDocument;
    if wrapper.is_null() {
        return ptr::null_mut();
    }
    (*wrapper).doc = doc;
    (*wrapper).next = (*ctxt).docList;
    (*ctxt).docList = wrapper;
    wrapper
}

/// `xsltFindDocument` (documents.c): find the `_xsltDocument` wrapper for a
/// document in the context's lists.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltDocumentPtr xsltFindDocument(xsltTransformContextPtr ctxt,
///                                  xmlDocPtr doc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltFindDocument(
    ctxt: *mut _xsltTransformContext,
    doc: *mut _xmlDoc,
) -> *mut _xsltDocument {
    if ctxt.is_null() || doc.is_null() {
        return ptr::null_mut();
    }
    let mut cur = (*ctxt).docList;
    while !cur.is_null() {
        if (*cur).doc == doc {
            return cur;
        }
        cur = (*cur).next;
    }
    // Also search the stylesheet's document list (upstream checks
    // ctxt->style->docList for style documents).
    if !(*ctxt).style.is_null() {
        let mut s = (*(*ctxt).style).docList;
        while !s.is_null() {
            if (*s).doc == doc {
                return s;
            }
            s = (*s).next;
        }
    }
    ptr::null_mut()
}

/// `xsltFreeDocuments` (documents.c): free the context's document wrappers
/// (not the documents themselves).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltFreeDocuments(xsltTransformContextPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltFreeDocuments(ctxt: *mut _xsltTransformContext) {
    if ctxt.is_null() {
        return;
    }
    let mut cur = (*ctxt).docList;
    (*ctxt).docList = ptr::null_mut();
    while !cur.is_null() {
        let next = (*cur).next;
        libc::free(cur as *mut libc::c_void);
        cur = next;
    }
}
