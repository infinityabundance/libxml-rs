//! Exported C data globals — the classic libxml2/libxslt public variables
//! (11.1-G data-ABI closure, residual R-000135).
//!
//! Downstream C code reads and writes these symbols directly
//! (e.g. `xmlDoValidityCheckingDefaultValue = 1;`), so they must exist with
//! upstream names, types and defaults. The parser-default accessors in
//! `src/xml/globals/mod.rs` read and write the SAME statics (single source
//! of truth): a C write is immediately observable by the candidate parser.
//!
//! Defaults match upstream `globals.c` (libxml2 2.15.3) and `xslt.c`
//! (libxslt 1.1.45).
//!
//! # SAFETY
//!
//! `static mut` globals are unsafe to touch from Rust; every access goes
//! through the accessor functions in `crate::xml::globals` (or directly
//! here with an explicit safety note). C accesses are inherently racy in
//! upstream too — upstream documents these globals as deprecated and
//! not thread-safe.
//!
//! # Upstream contract
//!
//! The parity target is upstream `globals.c` (libxml2 2.15.3), `SAX2.c` and
//! `chvalid.c`, plus the libxslt `xslt.c` globals. Every symbol here is
//! exported `#[no_mangle]` as DATA with the upstream name, type and default so
//! downstream C code that reads or writes the classic global variables links
//! and observes the same values (R-000135 data-globals closure).
//!
//! # Conceptual behavior
//!
//! This module implements the exported C data-global surface: parser defaults
//! (`xmlDoValidityCheckingDefaultValue`, `xmlKeepBlanksDefaultValue`, ...),
//! the error handler/context slots (`xmlGenericError`, `xmlStructuredError`,
//! `xmlLastError`), the default SAX handler structs and locator, the version
//! data symbols and the allocator function pointers. The parser-default
//! accessors in `src/xml/globals/mod.rs` read and write the SAME statics
//! (single source of truth), so a C write is immediately observable by the
//! candidate parser.
//!
//! # Ownership & safety invariants
//!
//! `static mut` globals are inherently racy — upstream documents them as
//! deprecated and not thread-safe, and the candidate preserves that C-visible
//! contract. Internal writers are serialized where corruption is possible: the
//! `xmlLastError` mirror deep-copy/free is guarded by `LAST_ERROR_MIRROR_LOCK`
//! (R-000170) and the handler-slot pairs are read and written atomically
//! (R-000171), while C readers keep upstreams documented racy semantics.
//!
//! # Historical quirks & epochs
//!
//! R-000135 (11.1-I): 11 exported data symbols were missing entirely and are
//! now byte-identical with the oracle DSO. R-000161 (11.1-K): `xmlGenericError`
//! / `xsltGenericError` default to the variadic stderr printers
//! (`xmlGenericErrorDefaultFunc` / `xsltGenericErrorDefaultFunc`) via the asm
//! va_list shims, and five default values were wrong before that fix
//! (e.g. `xmlLineNumbersDefaultValue` 0 to 1, `xmlTreeIndentString` NULL to a
//! two-space string). R-000167 (11.1-S): `xsltLibxsltVersion` and friends are
//! exported as data (R) not functions (T). R-000162 (11.1-L): the allocator
//! entry points are DATA function pointers. R-000170/R-000171 (11.1-X): the
//! parallel-suite corruption races.
//!
//! # Deliberate oddities
//!
//! The candidates internal writers are locked while the exported symbols stay
//! racy — a deliberate safe divergence that keeps the C-visible contract
//! identical to upstream while making the Rust internals safe.
//! `xsltDocDefaultLoader` defaults to NULL instead of upstreams variadic
//! default function (stable Rust cannot define variadic extern fns) —
//! documented safe divergence from R-000135.
//!
//! # Proving courts
//!
//! The DATA-GLOBALS-001 differential court (tools/abi/data_globals_probe.py
//! and courts/suites/data-abi/data-globals-probe.c) compiles one probe
//! against the oracle DSO and the candidate and requires byte-identical
//! output; the ABI-DATA, ALLOCATOR, GLOBAL-STATE and THREADING families
//! plus DSO-LOADER (symbol-type parity) and HEADER-COMPILE also cover this
//! module.
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is to keep parser defaults only in Rust atomics
//! and skip the exported C globals — that was the pre-R-000135 state and it
//! breaks every downstream consumer that reads or writes
//! `xmlDoValidityCheckingDefaultValue` directly (the link would fail).
//! Another tempting shortcut is dropping the mirror locks: R-000170 observed
//! glibc `double free or corruption` aborts in about 12% of parallel runs, so
//! the locks must not be removed even though upstream is racy.

use core::ffi::c_void;
use core::ptr;
use std::os::raw::{c_char, c_int, c_uint};

use crate::abi::callbacks::{xmlGenericErrorFunc, xmlStructuredErrorFunc};
use crate::abi::exports_xslt_compile::xsltDocLoaderFunc;
use crate::abi::types::xmlChar;

/// Serializes writes to the exported `xmlLastError` mirror.
///
/// Upstream's `xmlLastError` is a bare racy global (deprecated, documented
/// not thread-safe), and the candidate preserves the C-visible semantics:
/// downstream C consumers read the symbol directly without a lock. The
/// candidate's internal deep-copy/free writers are serialized here so that
/// concurrent error raises on different threads can never free the same
/// mirror string twice or write while another thread is freeing — observed
/// as heap corruption (`double free or corruption (!prev)`) in the parallel
/// lib test suite (xml::errors tests racing with any other raising thread).
static LAST_ERROR_MIRROR_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

// ═══════════════════════════════════════════════════════════════════════════════
// Parser defaults (upstream globals.c)
// ═══════════════════════════════════════════════════════════════════════════════

// ═══════════════════════════════════════════════════════════════════════════════
// Parser-default / error-handler / node-hook / IO-hook globals
// ═══════════════════════════════════════════════════════════════════════════════
//
// Phase 13 (HOSTILE-THREADS): the 18 globals below were moved to
// THREAD-LOCAL storage (`crate::xml::globals::tls`), matching upstream 2.15
// LIBXML_THREAD_ENABLED (`xmlGetThreadLocalStorage`, globals.c). The oracle
// DSO exports only the `__xmlXxx()` accessor FUNCTIONS for them (nm -D: no
// data symbols), so the candidate removed the plain data exports and the
// accessors now return pointers into the per-thread cells; the candidate
// headers use the upstream macro/accessor contract. See tls.rs.
//
// Still exported as plain global data (executed oracle keeps these global):
// xmlParserVersion, xmlParserMaxDepth, xmlDefaultBufferSize,
// xmlBufferAllocScheme, xmlParserDebugEntities, xmlLastError (R-000135
// mirror), the allocator hooks, the SAX handler structs and the character
// tables.

/// `int xmlParserDebugEntities` (default 0) — candidate-only data export
/// (CUSTODIAN_EXTENSION); NOT TLS-era in the executed oracle.
#[no_mangle]
pub static mut xmlParserDebugEntities: c_int = 0;

/// Upstream static `xmlRegisterCallbacks` (tree.c): the gate that arms the
/// node register/deregister hooks once a callback has been registered.
pub static XML_REGISTER_CALLBACKS: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Upstream `xmlRegisterNodeCallback(node)` — invoke the registered node
/// hook, gated by `xmlRegisterCallbacks` (tree.c).
///
/// # SAFETY
///
/// - `node` must be a valid, fully-initialised node or NULL.
pub fn register_node_hook(node: *mut crate::abi::structs::_xmlNode) {
    if !XML_REGISTER_CALLBACKS.load(core::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let hook = crate::xml::globals::get_register_node_default();
    if let Some(h) = hook {
        if !node.is_null() {
            // SAFETY: the hook is a valid C callback registered by the user.
            unsafe { h(node) };
        }
    }
}

/// Upstream `xmlDeregisterNodeCallback(node)` — invoke the registered node
/// deregister hook, gated by `xmlRegisterCallbacks` (tree.c).
///
/// # SAFETY
///
/// - `node` must be a valid node about to be freed, or NULL.
pub fn deregister_node_hook(node: *mut crate::abi::structs::_xmlNode) {
    if !XML_REGISTER_CALLBACKS.load(core::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let hook = crate::xml::globals::get_deregister_node_default();
    if let Some(h) = hook {
        if !node.is_null() {
            // SAFETY: the hook is a valid C callback registered by the user.
            unsafe { h(node) };
        }
    }
}

/// `const char *xmlParserVersion` — matched to the system oracle build
/// (libxml2 2.15.3 GIT build: LIBXML_VERSION_STRING "21503" plus the
/// upstream version extra "-GITv2.15.3").
///
/// SAFETY: the pointed-to string is a static, immutable, null-terminated
/// literal; `static mut` is used because C raw pointers are not `Sync`.
/// Reads/writes of the pointer itself are racy only if C code mutates it
/// (upstream treats it as a constant).
#[no_mangle]
pub static mut xmlParserVersion: *const c_char = {
    static V: [u8; 17] = *b"21503-GITv2.15.3\0";
    V.as_ptr() as *const c_char
};

/// `int xmlParserMaxDepth` (default 256)
#[no_mangle]
pub static mut xmlParserMaxDepth: c_int = 256;

// ═══════════════════════════════════════════════════════════════════════════════
// Buffer globals (upstream tree.h / xmlIO.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// `int xmlDefaultBufferSize` (default 4096)
#[no_mangle]
pub static mut xmlDefaultBufferSize: c_int = 4096;

/// `xmlBufferAllocationScheme xmlBufferAllocScheme` (default XML_BUFFER_ALLOC_EXACT = 1;
/// upstream globals.c `xmlBufferAllocSchemeThrDef = XML_BUFFER_ALLOC_EXACT`)
#[no_mangle]
pub static mut xmlBufferAllocScheme: c_int = 1;

// ═══════════════════════════════════════════════════════════════════════════════
// Error callback globals (upstream xmlerror.h) — THREAD-LOCAL since Phase 13
// ═══════════════════════════════════════════════════════════════════════════════
//
// `xmlGenericError` / `xmlGenericErrorContext` / `xmlStructuredError` /
// `xmlStructuredErrorContext` live in per-thread cells
// (`crate::xml::globals::tls`), matching upstream 2.15
// LIBXML_THREAD_ENABLED: `xmlSetGenericErrorFunc`/`xmlSetStructuredErrorFunc`
// affect only the calling thread, and the oracle DSO exports only the
// `__xmlGenericError` family of accessor FUNCTIONS (no data symbols).
//
// The default `xmlGenericError` value is `xmlGenericErrorDefaultFunc` (the
// R-000161 x86-64 variadic stderr shim), exactly like upstream error.c; the
// shim below reproduces it, so a freshly-initialized library routes errors
// to stderr exactly like upstream.

// ═══════════════════════════════════════════════════════════════════════════════
// Variadic default error handlers (upstream error.c / xsltutils.c)
// ═══════════════════════════════════════════════════════════════════════════════
//
// Upstream `xmlGenericErrorDefaultFunc(void *ctx, const char *msg, ...)`
// prints the formatted message to `xmlGenericErrorContext` (stderr when
// NULL). Stable Rust cannot define a variadic extern fn body, so the ABI
// entry is an x86_64 SysV inline-asm shim that materialises the caller's
// register/stack arguments into a `va_list` and forwards to a non-variadic
// receiver — the same pattern as `xsltTransformError`
// (exports_xslt_util.rs) and the writer's `vfmt_shim!`. Neither default
// function is a dynamic export upstream (both are internal); the exported
// data globals merely point at them.

/// System V AMD64 `__va_list_tag` (24 bytes) — same layout as the writer's
/// shims and `exports_xslt_util.rs`.
#[cfg(target_arch = "x86_64")]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VaListTag {
    gp_offset: c_uint,
    fp_offset: c_uint,
    overflow_arg_area: *mut c_void,
    reg_save_area: *mut c_void,
}

#[cfg(target_arch = "x86_64")]
unsafe extern "C" {
    fn vfprintf(stream: *mut c_void, format: *const c_char, ap: *mut VaListTag) -> c_int;
}

/// The `stderr` FILE* — glibc exports the `stderr` data object, an
/// 8-byte pointer variable whose value is `&_IO_2_1_stderr_`. Upstream
/// `xmlGenericErrorDefaultFunc` defaults the error context to `stderr`;
/// using the real stdio object (unbuffered, fd-2 relative) keeps writes
/// byte-exact and honors fd-2 redirection, unlike a private `fdopen(2)`
/// FILE* which is fully buffered and lands at exit on whatever fd 2 then
/// points to.
#[cfg(target_arch = "x86_64")]
unsafe fn stderr_file() -> *mut c_void {
    extern "C" {
        static stderr: *mut c_void;
    }
    unsafe { stderr }
}

/// Variadic receiver for the `xmlGenericErrorDefaultFunc` shim (upstream
/// error.c semantics: default the context to stderr, then `vfprintf`).
///
/// # SAFETY
///
/// - `_ctx`, `ap` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `msg` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[cfg(target_arch = "x86_64")]
#[no_mangle]
pub unsafe extern "C" fn xmlGenericErrorDefaultFuncV(
    _ctx: *mut c_void,
    msg: *const c_char,
    ap: *mut VaListTag,
) -> c_int {
    unsafe {
        if crate::xml::globals::get_generic_error_ctx().is_null() {
            crate::xml::globals::set_generic_error_ctx(stderr_file());
        }
        let stream = crate::xml::globals::get_generic_error_ctx();
        if msg.is_null() || stream.is_null() {
            return 0;
        }
        vfprintf(stream, msg, ap)
    }
}

/// `xmlGenericErrorDefaultFunc(void *ctx, const char *msg, ...)` — the
/// upstream default generic error handler. Not exported dynamically (matches
/// upstream, where the symbol is internal).
///
/// 2 fixed args (ctx=rdi, msg=rsi) → `gp_offset` 16; the va_list pointer is
/// passed as the 3rd arg (rdx) of the receiver.
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
#[cfg(target_arch = "x86_64")]
pub unsafe extern "C" fn xmlGenericErrorDefaultFunc() -> c_int {
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
            "mov dword ptr [rsp+176], 16",
            "mov dword ptr [rsp+180], 48",
            "lea rax, [rsp+256]",
            "mov [rsp+184], rax",
            "lea rax, [rsp]",
            "mov [rsp+192], rax",
            "lea rdx, [rsp+176]",
            "call xmlGenericErrorDefaultFuncV",
            "add rsp, 240",
            "add rsp, 8",
            "ret",
            options(noreturn),
        );
    }
}

/// Default value of the exported `xmlGenericError` data global.
#[cfg(target_arch = "x86_64")]
#[doc(hidden)]
pub(crate) const XML_GENERIC_ERROR_DEFAULT: xmlGenericErrorFunc = unsafe {
    // SAFETY: the shim and the function-pointer type have identical ABI
    // (a code pointer); the declared arity is a Rust-side fiction required
    // to store a variadic entry in the non-variadic pointer type.
    core::mem::transmute::<
        unsafe extern "C" fn() -> c_int,
        unsafe extern "C" fn(*mut c_void, *const c_char),
    >(xmlGenericErrorDefaultFunc)
};

/// Variadic receiver for the `xsltGenericErrorDefaultFunc` shim (upstream
/// xsltutils.c semantics: default the context to stderr, then `vfprintf`).
///
/// # SAFETY
///
/// - `_ctx`, `ap` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `msg` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[cfg(target_arch = "x86_64")]
#[no_mangle]
pub unsafe extern "C" fn xsltGenericErrorDefaultFuncV(
    _ctx: *mut c_void,
    msg: *const c_char,
    ap: *mut VaListTag,
) -> c_int {
    unsafe {
        if crate::abi::data_globals::xsltGenericErrorContext.is_null() {
            crate::abi::data_globals::xsltGenericErrorContext = stderr_file();
        }
        let stream = crate::abi::data_globals::xsltGenericErrorContext;
        if msg.is_null() || stream.is_null() {
            return 0;
        }
        vfprintf(stream, msg, ap)
    }
}

/// `xsltGenericErrorDefaultFunc(void *ctx, const char *msg, ...)` — the
/// upstream default XSLT error handler (xsltutils.c).
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
#[cfg(target_arch = "x86_64")]
pub unsafe extern "C" fn xsltGenericErrorDefaultFunc() -> c_int {
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
            "mov dword ptr [rsp+176], 16",
            "mov dword ptr [rsp+180], 48",
            "lea rax, [rsp+256]",
            "mov [rsp+184], rax",
            "lea rax, [rsp]",
            "mov [rsp+192], rax",
            "lea rdx, [rsp+176]",
            "call xsltGenericErrorDefaultFuncV",
            "add rsp, 240",
            "add rsp, 8",
            "ret",
            options(noreturn),
        );
    }
}

/// Default value of the exported `xsltGenericError` data global.
#[cfg(target_arch = "x86_64")]
const XSLT_GENERIC_ERROR_DEFAULT: xmlGenericErrorFunc = unsafe {
    // SAFETY: as above — ABI-identical code pointer.
    core::mem::transmute::<
        unsafe extern "C" fn() -> c_int,
        unsafe extern "C" fn(*mut c_void, *const c_char),
    >(xsltGenericErrorDefaultFunc)
};

/// The built-in default generic error handler (upstream
/// `xmlGenericErrorDefaultFunc`), for use when a caller resets the handler
/// with NULL. Only available on x86_64 (the variadic shim is SysV-specific);
/// on other targets there is no default (resets leave the handler unset).
pub fn default_generic_error_func() -> Option<xmlGenericErrorFunc> {
    #[cfg(target_arch = "x86_64")]
    {
        Some(XML_GENERIC_ERROR_DEFAULT)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        None
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Static strings (upstream xmlstring.h / tree.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// `const xmlChar xmlStringText[]` — "text"
#[no_mangle]
pub static xmlStringText: [xmlChar; 5] = [b't', b'e', b'x', b't', 0];

/// `const xmlChar xmlStringTextNoenc[]` — "textnoenc"
#[no_mangle]
pub static xmlStringTextNoenc: [xmlChar; 9] = *b"textnoenc";

/// `const xmlChar xmlStringComment[]` — "comment"
#[no_mangle]
pub static xmlStringComment: [xmlChar; 8] = [b'c', b'o', b'm', b'm', b'e', b'n', b't', 0];

// ═══════════════════════════════════════════════════════════════════════════════
// XPath numeric constants (upstream xpath.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// `double xmlXPathNAN` — NaN
#[no_mangle]
pub static xmlXPathNAN: f64 = f64::NAN;

/// `double xmlXPathPINF` — +infinity
#[no_mangle]
pub static xmlXPathPINF: f64 = f64::INFINITY;

/// `double xmlXPathNINF` — -infinity
#[no_mangle]
pub static xmlXPathNINF: f64 = f64::NEG_INFINITY;

// ═══════════════════════════════════════════════════════════════════════════════
// libxslt globals (upstream xslt.c / xsltutils.c / documents.c / xslt.h)
// ═══════════════════════════════════════════════════════════════════════════════
// xsltMaxDepth / xsltMaxVars are exported from src/xslt/transform/mod.rs
// (they are read by the transform engine).

/// `const int xsltLibxmlVersion` = 21501 — the libxml2 version the system
/// libxslt 1.1.45 was compiled against (upstream xslt.c `LIBXML_VERSION`;
/// the system libxslt was built against libxml2 2.15.1). Byte-parity with
/// the oracle DSO, read directly by `xsltproc -V`.
#[no_mangle]
pub static xsltLibxmlVersion: c_int = 21501;

/// `const int xsltLibxsltVersion` = 10145 — the libxslt version
/// (upstream xslt.c `XSLTPUBVAR const int xsltLibxsltVersion = LIBXSLT_VERSION`;
/// oracle DSO symbol type R). Was previously exported as a function (T)
/// — R-000167.
#[no_mangle]
pub static xsltLibxsltVersion: c_int = 10145;

/// `const char *xsltEngineVersion` = "10145-GITv1.1.45" — the libxslt engine
/// version string (upstream xslt.c `XSLTPUBVAR const char *xsltEngineVersion`;
/// oracle DSO symbol type D). Was previously exported as a function (T)
/// — R-000167. `static mut` follows the xmlParserVersion pattern (raw
/// pointers are not Sync).
#[no_mangle]
pub static mut xsltEngineVersion: *const c_char = {
    static S: [u8; 17] = *b"10145-GITv1.1.45\0";
    S.as_ptr() as *const c_char
};

/// `const char *exsltLibraryVersion` = "825-GITv1.1.45" — the libexslt
/// library version string (upstream exslt.c `EXSLTPUBVAR const char *`;
/// oracle DSO symbol type D). Read by `xsltproc -V`.
#[no_mangle]
pub static mut exsltLibraryVersion: *const c_char = {
    static S: [u8; 15] = *b"825-GITv1.1.45\0";
    S.as_ptr() as *const c_char
};

/// `const int exsltLibexsltVersion` = 825 — the libexslt version
/// (upstream exslt.h `EXSLTPUBVAR const int`; oracle DSO symbol type R).
#[no_mangle]
pub static exsltLibexsltVersion: c_int = 825;

/// `const int exsltLibxmlVersion` = 21501 — the libxml2 version the system
/// libexslt 0.8.25 was compiled against (oracle DSO symbol type R).
#[no_mangle]
pub static exsltLibxmlVersion: c_int = 21501;

/// `const int exsltLibxsltVersion` = 10145 — the libxslt version the system
/// libexslt 0.8.25 was compiled against (oracle DSO symbol type R).
#[no_mangle]
pub static exsltLibxsltVersion: c_int = 10145;

/// `int xslDebugStatus` — the libxslt debugger status (upstream xsltutils.c
/// `XSLTPUBVAR int xslDebugStatus;`, default XSLT_DEBUG_NONE = 0; oracle DSO
/// symbol type B). Written by `xsltSetDebuggerStatus`; read by the transform
/// engine's profiling gates (R-000165 DATA_MISSING closure).
#[no_mangle]
pub static mut xslDebugStatus: c_int = 0;

/// `xmlGenericErrorFunc xsltGenericError` — the libxslt error callback.
/// Upstream defaults to `xsltGenericErrorDefaultFunc` (xsltutils.c, a variadic
/// stderr printer); the candidate's shim below reproduces it (R-000135
/// divergence now closed).
#[cfg(target_arch = "x86_64")]
#[no_mangle]
pub static mut xsltGenericError: Option<xmlGenericErrorFunc> = Some(XSLT_GENERIC_ERROR_DEFAULT);

#[cfg(not(target_arch = "x86_64"))]
#[no_mangle]
pub static mut xsltGenericError: Option<xmlGenericErrorFunc> = None;

/// `void *xsltGenericErrorContext` (default NULL)
#[no_mangle]
pub static mut xsltGenericErrorContext: *mut c_void = core::ptr::null_mut();

/// `void *xsltGenericDebugContext` (default NULL)
#[no_mangle]
pub static mut xsltGenericDebugContext: *mut c_void = core::ptr::null_mut();

/// `xmlGenericErrorFunc xsltGenericDebug` — the libxslt debug callback.
/// Upstream defaults to `xsltGenericDebugDefaultFunc` (xsltutils.c:632), a
/// handler that writes the message to `xsltGenericDebugContext` (a FILE*)
/// when that context is non-NULL. The candidate reproduces the default via
/// `xsltGenericDebugDefaultFunc` below. Was previously exported as a
/// FUNCTION (T) — an R-000167-class ABI type divergence closed in 11.1-Z.1
/// (oracle DSO symbol type D, `nm -D /usr/lib/libxslt.so.1`).
#[no_mangle]
pub static mut xsltGenericDebug: Option<xmlGenericErrorFunc> = Some(XSLT_GENERIC_DEBUG_DEFAULT);

/// Default debug handler reproducing upstream `xsltGenericDebugDefaultFunc`
/// (xsltutils.c:621): no-op unless `xsltGenericDebugContext` is non-NULL,
/// then the (pre-formatted) message is written to that context FILE*.
/// Upstream's handler is variadic; the candidate's callers format the
/// message before dispatch (same documented divergence as xsltGenericError).
///
/// # Safety
///
/// - `msg` must be NULL or a valid NUL-terminated string valid for the
///   call; when `xsltGenericDebugContext` is non-NULL it must be a valid
///   `FILE*` writable via `fwrite` for the message length.
unsafe extern "C" fn xsltGenericDebugDefaultFunc(_ctx: *mut c_void, msg: *const c_char) {
    if msg.is_null() {
        return;
    }
    let dctx = unsafe { xsltGenericDebugContext };
    if dctx.is_null() {
        return;
    }
    let len = unsafe { libc::strlen(msg) };
    unsafe {
        libc::fwrite(msg as *const libc::c_void, 1, len, dctx as *mut libc::FILE);
    }
}

/// Default value of the exported `xsltGenericDebug` data global.
const XSLT_GENERIC_DEBUG_DEFAULT: xmlGenericErrorFunc = xsltGenericDebugDefaultFunc;

/// `const xmlChar xsltExtMarker[]` — empty string used to mark extension
/// nodes (upstream transform.c).
#[no_mangle]
pub static xsltExtMarker: [xmlChar; 1] = [0];

/// Upstream `xsltDocDefaultLoaderFunc` (documents.c): parse the URI as a
/// document with the given parser options — a DIRECT parse that never
/// re-enters the registered loader. Consumers capture the exported data
/// global at load time (lxml's `_xslt_doc_loader` falls back to
/// `XSLT_DOC_DEFAULT_LOADER` when its resolver cannot handle a URI) and
/// must get a real function, not NULL.
///
/// # SAFETY
///
/// - `uri` must be NULL or a valid NUL-terminated string.
unsafe extern "C" fn xslt_doc_default_loader_func(
    uri: *const xmlChar,
    _dict: *mut c_void,
    options: c_int,
    _ctxt: *mut c_void,
    _kind: c_int,
) -> *mut crate::abi::structs::_xmlDoc {
    if uri.is_null() {
        return ptr::null_mut();
    }
    crate::abi::exports_xml2::xmlReadFile(uri as *const c_char, ptr::null(), options)
}

/// `xsltDocLoaderFunc xsltDocDefaultLoader` — the document loader callback.
/// Upstream initializes it to `xsltDocDefaultLoaderFunc`; the candidate
/// mirrors that with the direct-parsing default above (R-000135: the
/// previous NULL default made lxml's import-time capture of this data
/// global jump to address zero when its resolver fell back to the default
/// loader).
#[no_mangle]
pub static mut xsltDocDefaultLoader: Option<xsltDocLoaderFunc> = Some(xslt_doc_default_loader_func);

// ═══════════════════════════════════════════════════════════════════════════════
// I/O filename callback globals (upstream xmlIO.h) — THREAD-LOCAL since
// Phase 13 (see tls.rs); the `xml*Default` accessor functions below read/write
// the per-thread slots.
// ═══════════════════════════════════════════════════════════════════════════════

// ═══════════════════════════════════════════════════════════════════════════════
// Default SAX v1 handler structs + locator (upstream globals.c 2.15.3)
// ═══════════════════════════════════════════════════════════════════════════════
//
// `const xmlSAXHandlerV1 xmlDefaultSAXHandler` and `htmlDefaultSAXHandler`
// (parser.h / HTMLparser.h), plus `const xmlSAXLocator xmlDefaultSAXLocator`.
// The handler instances reproduce the upstream initializer lists exactly
// (globals.c); every referenced xmlSAX2* entry point is a real candidate
// export.

/// `const xmlSAXHandlerV1 xmlDefaultSAXHandler` (globals.c 2.15.3).
///
/// `#[no_mangle]` data symbol that C consumers (PHP's DOM/XML extensions)
/// MUTATE (they overwrite the error/warning callbacks on the shared default
/// handler). Rust places non-`mut` statics in read-only memory; a write to
/// it would segfault. Make it `static mut` so it lives in writable data.
#[no_mangle]
pub static mut xmlDefaultSAXHandler: crate::abi::structs::_xmlSAXHandlerV1 =
    crate::abi::structs::_xmlSAXHandlerV1 {
        internalSubset: Some(crate::abi::exports_xml2::xmlSAX2InternalSubset),
        isStandalone: Some(crate::abi::exports_xml2::xmlSAX2IsStandalone),
        hasInternalSubset: Some(crate::abi::exports_xml2::xmlSAX2HasInternalSubset),
        hasExternalSubset: Some(crate::abi::exports_xml2::xmlSAX2HasExternalSubset),
        resolveEntity: Some(crate::abi::exports_xml2::xmlSAX2ResolveEntity),
        getEntity: Some(crate::abi::exports_xml2::xmlSAX2GetEntity),
        entityDecl: Some(crate::abi::exports_xml2::xmlSAX2EntityDecl),
        notationDecl: Some(crate::abi::exports_xml2::xmlSAX2NotationDecl),
        attributeDecl: Some(crate::abi::exports_xml2::xmlSAX2AttributeDecl),
        elementDecl: Some(crate::abi::exports_xml2::xmlSAX2ElementDecl),
        unparsedEntityDecl: Some(crate::abi::exports_xml2::xmlSAX2UnparsedEntityDecl),
        setDocumentLocator: Some(crate::abi::exports_xml2::xmlSAX2SetDocumentLocator),
        startDocument: Some(crate::abi::exports_xml2::xmlSAX2StartDocument),
        endDocument: Some(crate::abi::exports_xml2::xmlSAX2EndDocument),
        startElement: Some(crate::abi::exports_xml2::xmlSAX2StartElement),
        endElement: Some(crate::abi::exports_xml2::xmlSAX2EndElement),
        reference: Some(crate::abi::exports_xml2::xmlSAX2Reference),
        characters: Some(crate::abi::exports_xml2::xmlSAX2Characters),
        ignorableWhitespace: Some(crate::abi::exports_xml2::xmlSAX2IgnorableWhitespace),
        processingInstruction: Some(crate::abi::exports_xml2::xmlSAX2ProcessingInstruction),
        comment: Some(crate::abi::exports_xml2::xmlSAX2Comment),
        warning: Some(crate::xml::errors::XML_PARSER_WARNING_SAX1),
        error: Some(crate::xml::errors::XML_PARSER_ERROR_SAX1),
        fatalError: Some(crate::xml::errors::XML_PARSER_ERROR_SAX1),
        getParameterEntity: Some(crate::abi::exports_xml2::xmlSAX2GetParameterEntity),
        cdataBlock: Some(crate::abi::exports_xml2::xmlSAX2CDataBlock),
        externalSubset: Some(crate::abi::exports_xml2::xmlSAX2ExternalSubset),
        initialized: 1,
    };

/// `const xmlSAXHandlerV1 htmlDefaultSAXHandler` (globals.c 2.15.3).
///
/// Mutable (see xmlDefaultSAXHandler) so C consumers (PHP) that overwrite
/// the shared default handler's callbacks don't fault on read-only memory.
#[no_mangle]
pub static mut htmlDefaultSAXHandler: crate::abi::structs::_xmlSAXHandlerV1 =
    crate::abi::structs::_xmlSAXHandlerV1 {
        internalSubset: Some(crate::abi::exports_xml2::xmlSAX2InternalSubset),
        isStandalone: None,
        hasInternalSubset: None,
        hasExternalSubset: None,
        resolveEntity: None,
        getEntity: Some(crate::abi::exports_xml2::xmlSAX2GetEntity),
        entityDecl: None,
        notationDecl: None,
        attributeDecl: None,
        elementDecl: None,
        unparsedEntityDecl: None,
        setDocumentLocator: Some(crate::abi::exports_xml2::xmlSAX2SetDocumentLocator),
        startDocument: Some(crate::abi::exports_xml2::xmlSAX2StartDocument),
        endDocument: Some(crate::abi::exports_xml2::xmlSAX2EndDocument),
        startElement: Some(crate::abi::exports_xml2::xmlSAX2StartElement),
        endElement: Some(crate::abi::exports_xml2::xmlSAX2EndElement),
        reference: None,
        characters: Some(crate::abi::exports_xml2::xmlSAX2Characters),
        ignorableWhitespace: Some(crate::abi::exports_xml2::xmlSAX2IgnorableWhitespace),
        processingInstruction: Some(crate::abi::exports_xml2::xmlSAX2ProcessingInstruction),
        comment: Some(crate::abi::exports_xml2::xmlSAX2Comment),
        warning: Some(crate::xml::errors::XML_PARSER_WARNING_SAX1),
        error: Some(crate::xml::errors::XML_PARSER_ERROR_SAX1),
        fatalError: Some(crate::xml::errors::XML_PARSER_ERROR_SAX1),
        getParameterEntity: None,
        cdataBlock: Some(crate::abi::exports_xml2::xmlSAX2CDataBlock),
        externalSubset: None,
        initialized: 1,
    };

/// `const xmlSAXLocator xmlDefaultSAXLocator` (globals.c 2.15.3).
#[no_mangle]
pub static xmlDefaultSAXLocator: crate::abi::callbacks::_xmlSAXLocator =
    crate::abi::callbacks::_xmlSAXLocator {
        getPublicId: Some(crate::abi::exports_xml2::xmlSAX2GetPublicId),
        getSystemId: Some(crate::abi::exports_xml2::xmlSAX2GetSystemId),
        getLineNumber: Some(crate::abi::exports_xml2::xmlSAX2GetLineNumber),
        getColumnNumber: Some(crate::abi::exports_xml2::xmlSAX2GetColumnNumber),
    };

// ═══════════════════════════════════════════════════════════════════════════════
// xmlLastError — the exported C global mirror of the thread-local error state
// ═══════════════════════════════════════════════════════════════════════════════
//
// Upstream `XMLPUBVAR xmlError xmlLastError` (xmlerror.h). The candidate's
// internal error state is thread-local (safe divergence, more correct than
// upstream's racy global); this mirror is deep-copied on every error raise
// and freed on reset, so C consumers observe upstream-equivalent lifetime
// semantics. Residual R-000135.

/// `xmlError xmlLastError` — most recent error (mirror).
#[no_mangle]
pub static mut xmlLastError: crate::abi::structs::_xmlError = crate::abi::structs::_xmlError {
    domain: 0,
    code: 0,
    message: core::ptr::null_mut(),
    level: 0,
    file: core::ptr::null_mut(),
    line: 0,
    str1: core::ptr::null_mut(),
    str2: core::ptr::null_mut(),
    str3: core::ptr::null_mut(),
    int1: 0,
    int2: 0,
    ctxt: core::ptr::null_mut(),
    node: core::ptr::null_mut(),
};

/// Deep-copy `err` into the exported `xmlLastError` global.
///
/// The string fields are copied with `libc::malloc`/`memcpy` so the mirror
/// owns them (previous mirror strings are freed first — upstream
/// xmlResetError semantics).
///
/// # SAFETY
///
/// - `err` must point to a valid `_xmlError` whose string fields are
///   NUL-terminated or NULL.
pub unsafe fn sync_xml_last_error(err: *const crate::abi::structs::_xmlError) {
    if err.is_null() {
        return;
    }
    let _guard = LAST_ERROR_MIRROR_LOCK.lock();
    unsafe { sync_xml_last_error_locked(err) };
}

/// Mirror write helper; caller must hold `LAST_ERROR_MIRROR_LOCK`.
///
/// # Safety
///
/// - `err` must be non-NULL and point to a valid `_xmlError` whose string
///   fields are NULL or NUL-terminated; the caller must hold
///   `LAST_ERROR_MIRROR_LOCK` so the `xmlLastError` global is not read or
///   written concurrently; each field is deep-copied with `dup_cstr`.
unsafe fn sync_xml_last_error_locked(err: *const crate::abi::structs::_xmlError) {
    unsafe {
        reset_xml_last_error_locked();
        let src = &*err;
        let dst = core::ptr::addr_of_mut!(xmlLastError);
        (*dst).domain = src.domain;
        (*dst).code = src.code;
        (*dst).level = src.level;
        (*dst).line = src.line;
        (*dst).int1 = src.int1;
        (*dst).int2 = src.int2;
        (*dst).ctxt = src.ctxt;
        (*dst).node = src.node;
        (*dst).message = dup_cstr(src.message as *const u8);
        (*dst).file = dup_cstr(src.file as *const u8);
        (*dst).str1 = dup_cstr(src.str1 as *const u8);
        (*dst).str2 = dup_cstr(src.str2 as *const u8);
        (*dst).str3 = dup_cstr(src.str3 as *const u8);
    }
}

/// Reset the exported `xmlLastError` global, freeing owned strings
/// (upstream xmlResetError).
///
/// # SAFETY
///
/// Only call while no other thread is reading the global (upstream has the
/// same race; documented).
pub unsafe fn reset_xml_last_error() {
    let _guard = LAST_ERROR_MIRROR_LOCK.lock();
    unsafe { reset_xml_last_error_locked() };
}

/// Mirror reset helper; caller must hold `LAST_ERROR_MIRROR_LOCK`.
unsafe fn reset_xml_last_error_locked() {
    unsafe {
        let dst = core::ptr::addr_of_mut!(xmlLastError);
        if !(*dst).message.is_null() {
            libc::free((*dst).message as *mut libc::c_void);
        }
        if !(*dst).file.is_null() {
            libc::free((*dst).file as *mut libc::c_void);
        }
        if !(*dst).str1.is_null() {
            libc::free((*dst).str1 as *mut libc::c_void);
        }
        if !(*dst).str2.is_null() {
            libc::free((*dst).str2 as *mut libc::c_void);
        }
        if !(*dst).str3.is_null() {
            libc::free((*dst).str3 as *mut libc::c_void);
        }
        *dst = crate::abi::structs::_xmlError {
            domain: 0,
            code: 0,
            message: core::ptr::null_mut(),
            level: 0,
            file: core::ptr::null_mut(),
            line: 0,
            str1: core::ptr::null_mut(),
            str2: core::ptr::null_mut(),
            str3: core::ptr::null_mut(),
            int1: 0,
            int2: 0,
            ctxt: core::ptr::null_mut(),
            node: core::ptr::null_mut(),
        };
    }
}

/// Heap-copy a NUL-terminated string (NULL-safe).
///
/// # Safety
///
/// - `s` must be NULL or a valid NUL-terminated string readable via
///   `strlen` for the duration of the copy; the returned pointer is
///   `libc::malloc`-owned and must be freed with `libc::free`, or is NULL
///   when `s` is NULL or the allocation fails.
unsafe fn dup_cstr(s: *const u8) -> *mut c_char {
    if s.is_null() {
        return core::ptr::null_mut();
    }
    unsafe {
        let len = libc::strlen(s as *const libc::c_char) as usize;
        let p = libc::malloc(len + 1) as *mut u8;
        if p.is_null() {
            return core::ptr::null_mut();
        }
        libc::memcpy(p as *mut libc::c_void, s as *const libc::c_void, len + 1);
        p as *mut c_char
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Default accessor functions (upstream parser.h / tree.h / xmlsave.h)
// ═══════════════════════════════════════════════════════════════════════════════
//
// The deprecated `xmlXxxDefault(v)` accessors set the corresponding global
// when `v != 0` and return the (new) value — upstream semantics (they
// predate the plain globals; the modern behavior is conditional-set-and-
// return, see upstream globals.c / parser.c).

/// Upstream `xmlKeepBlanksDefault(int v)`.
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
pub unsafe extern "C" fn xmlKeepBlanksDefault(v: c_int) -> c_int {
    if v != 0 {
        crate::xml::globals::set_keep_blanks_default(v);
    }
    crate::xml::globals::get_keep_blanks_default()
}

/// Upstream `xmlLineNumbersDefault(int v)`.
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
pub unsafe extern "C" fn xmlLineNumbersDefault(v: c_int) -> c_int {
    if v != 0 {
        crate::xml::globals::set_line_numbers_default(v);
    }
    crate::xml::globals::get_line_numbers_default()
}

/// Upstream `xmlSubstituteEntitiesDefault(int v)`.
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
pub unsafe extern "C" fn xmlSubstituteEntitiesDefault(v: c_int) -> c_int {
    if v != 0 {
        crate::xml::globals::set_substitute_entities_default(v);
    }
    crate::xml::globals::get_substitute_entities_default()
}

/// Upstream `xmlPedanticParserDefault(int v)`.
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
pub unsafe extern "C" fn xmlPedanticParserDefault(v: c_int) -> c_int {
    if v != 0 {
        crate::xml::globals::set_pedantic_parser_default(v);
    }
    crate::xml::globals::get_pedantic_parser_default()
}

/// Upstream `xmlDoValidityCheckingDefaultValue` accessor is the global
/// itself; `xmlGetWarningsDefaultValue` likewise (no accessor functions
/// exist for those in upstream 2.15).
/// Upstream `xmlRegisterNodeDefault(xmlRegisterNodeFunc func)`.
///
/// # SAFETY
///
///
/// - `func` must be a valid callback (or None);
///   the callback is invoked with the documented context pointer and
///   must itself uphold the same pointer invariants.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlRegisterNodeDefault(
    func: Option<unsafe extern "C" fn(*mut crate::abi::structs::_xmlNode)>,
) -> Option<unsafe extern "C" fn(*mut crate::abi::structs::_xmlNode)> {
    // UPSTREAM-PARITY (tree.c): registering any callback arms the
    // xmlRegisterCallbacks gate.
    XML_REGISTER_CALLBACKS.store(true, core::sync::atomic::Ordering::Relaxed);
    if func.is_some() {
        crate::xml::globals::set_register_node_default(func);
    }
    crate::xml::globals::get_register_node_default()
}

/// Upstream `xmlDeregisterNodeDefault(xmlDeregisterNodeFunc func)`.
///
/// # SAFETY
///
///
/// - `func` must be a valid callback (or None);
///   the callback is invoked with the documented context pointer and
///   must itself uphold the same pointer invariants.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlDeregisterNodeDefault(
    func: Option<unsafe extern "C" fn(*mut crate::abi::structs::_xmlNode)>,
) -> Option<unsafe extern "C" fn(*mut crate::abi::structs::_xmlNode)> {
    // UPSTREAM-PARITY (tree.c): registering any callback arms the
    // xmlRegisterCallbacks gate.
    XML_REGISTER_CALLBACKS.store(true, core::sync::atomic::Ordering::Relaxed);
    if func.is_some() {
        crate::xml::globals::set_deregister_node_default(func);
    }
    crate::xml::globals::get_deregister_node_default()
}

/// Upstream `__xmlIndentTreeOutput(void)` (parser.h) — returns a pointer to
/// the `xmlIndentTreeOutput` global.
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
pub unsafe extern "C" fn __xmlIndentTreeOutput() -> *mut c_int {
    crate::xml::globals::indent_tree_output_ptr()
}

/// Upstream `__xmlSaveNoEmptyTags(void)` (parser.h) — returns a pointer to
/// the `xmlSaveNoEmptyTags` global.
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
pub unsafe extern "C" fn __xmlSaveNoEmptyTags() -> *mut c_int {
    crate::xml::globals::save_no_empty_tags_ptr()
}

/// Upstream `__xmlTreeIndentString(void)` (parser.h) — returns a pointer to
/// the `xmlTreeIndentString` global.
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
pub unsafe extern "C" fn __xmlTreeIndentString() -> *mut *const xmlChar {
    crate::xml::globals::tree_indent_string_ptr()
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlThrDef* accessors (upstream threads.c / globals.c)
// ═══════════════════════════════════════════════════════════════════════════════
//
// The deprecated `xmlThrDef*` family reads/writes the public globals with
// the upstream semantics: when `v != 0` the global is set, and the (new)
// value is returned. In upstream these were thread-local definitions before
// the globals became plain variables; the modern behavior is exactly this
// conditional-set-and-return on the global (see upstream globals.c).

/// Upstream `xmlThrDefDoValidityCheckingDefaultValue(int v)`.
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
pub unsafe extern "C" fn xmlThrDefDoValidityCheckingDefaultValue(v: c_int) -> c_int {
    if v != 0 {
        crate::xml::globals::set_validity_checking_default(v);
    }
    crate::xml::globals::get_validity_checking_default()
}

/// Upstream `xmlThrDefGetWarningsDefaultValue(int v)`.
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
pub unsafe extern "C" fn xmlThrDefGetWarningsDefaultValue(v: c_int) -> c_int {
    if v != 0 {
        crate::xml::globals::set_get_warnings_default(v);
    }
    crate::xml::globals::get_get_warnings_default()
}

/// Upstream `xmlThrDefLoadExtDtdDefaultValue(int v)`.
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
pub unsafe extern "C" fn xmlThrDefLoadExtDtdDefaultValue(v: c_int) -> c_int {
    if v != 0 {
        crate::xml::globals::set_load_ext_dtd_default(v);
    }
    crate::xml::globals::get_load_ext_dtd_default()
}

/// Upstream `xmlThrDefPedanticParserDefaultValue(int v)`.
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
pub unsafe extern "C" fn xmlThrDefPedanticParserDefaultValue(v: c_int) -> c_int {
    if v != 0 {
        crate::xml::globals::set_pedantic_parser_default(v);
    }
    crate::xml::globals::get_pedantic_parser_default()
}

/// Upstream `xmlThrDefLineNumbersDefaultValue(int v)`.
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
pub unsafe extern "C" fn xmlThrDefLineNumbersDefaultValue(v: c_int) -> c_int {
    if v != 0 {
        crate::xml::globals::set_line_numbers_default(v);
    }
    crate::xml::globals::get_line_numbers_default()
}

/// Upstream `xmlThrDefKeepBlanksDefaultValue(int v)`.
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
pub unsafe extern "C" fn xmlThrDefKeepBlanksDefaultValue(v: c_int) -> c_int {
    if v != 0 {
        crate::xml::globals::set_keep_blanks_default(v);
    }
    crate::xml::globals::get_keep_blanks_default()
}

/// Upstream `xmlThrDefSubstituteEntitiesDefaultValue(int v)`.
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
pub unsafe extern "C" fn xmlThrDefSubstituteEntitiesDefaultValue(v: c_int) -> c_int {
    if v != 0 {
        crate::xml::globals::set_substitute_entities_default(v);
    }
    crate::xml::globals::get_substitute_entities_default()
}

/// Upstream `xmlThrDefParserDebugEntities(int v)`.
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
pub unsafe extern "C" fn xmlThrDefParserDebugEntities(v: c_int) -> c_int {
    unsafe {
        if v != 0 {
            xmlParserDebugEntities = v;
        }
        xmlParserDebugEntities
    }
}

/// Upstream `xmlThrDefIndentTreeOutput(int v)`.
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
pub unsafe extern "C" fn xmlThrDefIndentTreeOutput(v: c_int) -> c_int {
    if v != 0 {
        crate::xml::globals::set_indent_tree_output(v);
    }
    crate::xml::globals::get_indent_tree_output()
}

/// Upstream `xmlThrDefTreeIndentString(const char *v)` — sets the indent
/// string when non-NULL and returns the current pointer.
///
/// # SAFETY
///
///
/// - `v` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlThrDefTreeIndentString(v: *const c_char) -> *const c_char {
    if !v.is_null() {
        crate::xml::globals::set_tree_indent_string(v as *const crate::abi::types::xmlChar);
    }
    crate::xml::globals::get_tree_indent_string() as *const c_char
}

/// Upstream `xmlThrDefSaveNoEmptyTags(int v)`.
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
pub unsafe extern "C" fn xmlThrDefSaveNoEmptyTags(v: c_int) -> c_int {
    if v != 0 {
        crate::xml::globals::set_save_no_empty_tags(v);
    }
    crate::xml::globals::get_save_no_empty_tags()
}

/// Upstream `xmlThrDefRegisterNodeDefault(xmlRegisterNodeFunc func)`.
///
/// # SAFETY
///
///
/// - `func` must be a valid callback (or None);
///   the callback is invoked with the documented context pointer and
///   must itself uphold the same pointer invariants.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlThrDefRegisterNodeDefault(
    func: Option<unsafe extern "C" fn(*mut crate::abi::structs::_xmlNode)>,
) -> Option<unsafe extern "C" fn(*mut crate::abi::structs::_xmlNode)> {
    if func.is_some() {
        crate::xml::globals::set_register_node_default(func);
    }
    crate::xml::globals::get_register_node_default()
}

/// Upstream `xmlThrDefDeregisterNodeDefault(xmlDeregisterNodeFunc func)`.
///
/// # SAFETY
///
///
/// - `func` must be a valid callback (or None);
///   the callback is invoked with the documented context pointer and
///   must itself uphold the same pointer invariants.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlThrDefDeregisterNodeDefault(
    func: Option<unsafe extern "C" fn(*mut crate::abi::structs::_xmlNode)>,
) -> Option<unsafe extern "C" fn(*mut crate::abi::structs::_xmlNode)> {
    if func.is_some() {
        crate::xml::globals::set_deregister_node_default(func);
    }
    crate::xml::globals::get_deregister_node_default()
}

/// Upstream `xmlThrDefSetGenericErrorFunc(void *ctx, xmlGenericErrorFunc func)`.
///
/// # SAFETY
///
/// - `ctx` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `func` must be a valid callback (or None);
///   the callback is invoked with the documented context pointer and
///   must itself uphold the same pointer invariants.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlThrDefSetGenericErrorFunc(
    ctx: *mut c_void,
    func: Option<xmlGenericErrorFunc>,
) {
    crate::xml::globals::set_generic_error_func(ctx, func);
}

/// Upstream `xmlThrDefSetStructuredErrorFunc(void *ctx, xmlStructuredErrorFunc func)`.
///
/// # SAFETY
///
/// - `ctx` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `func` must be a valid callback (or None);
///   the callback is invoked with the documented context pointer and
///   must itself uphold the same pointer invariants.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlThrDefSetStructuredErrorFunc(
    ctx: *mut c_void,
    func: Option<xmlStructuredErrorFunc>,
) {
    crate::xml::globals::set_structured_error_func(ctx, func);
}

/// Upstream `xmlThrDefDefaultBufferSize(int v)`.
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
pub unsafe extern "C" fn xmlThrDefDefaultBufferSize(v: c_int) -> c_int {
    unsafe {
        if v != 0 {
            xmlDefaultBufferSize = v;
        }
        xmlDefaultBufferSize
    }
}

/// Upstream `xmlThrDefBufferAllocScheme(xmlBufferAllocationScheme v)`.
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
pub unsafe extern "C" fn xmlThrDefBufferAllocScheme(v: c_int) -> c_int {
    unsafe {
        if v != 0 {
            xmlBufferAllocScheme = v;
        }
        xmlBufferAllocScheme
    }
}

/// Upstream `xmlThrDefParserInputBufferCreateFilenameDefault(...)`.
///
/// # SAFETY
///
///
/// - `func` must be a valid callback (or None);
///   the callback is invoked with the documented context pointer and
///   must itself uphold the same pointer invariants.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlThrDefParserInputBufferCreateFilenameDefault(
    func: Option<
        unsafe extern "C" fn(
            *const c_char,
            c_int,
        ) -> *mut crate::abi::structs::_xmlParserInputBuffer,
    >,
) -> Option<
    unsafe extern "C" fn(*const c_char, c_int) -> *mut crate::abi::structs::_xmlParserInputBuffer,
> {
    if func.is_some() {
        crate::xml::globals::set_parser_input_buffer_create_filename_value(func);
    }
    crate::xml::globals::get_parser_input_buffer_create_filename_value()
}

/// Upstream `xmlThrDefOutputBufferCreateFilenameDefault(...)`.
///
/// # SAFETY
///
///
/// - `func` must be a valid callback (or None);
///   the callback is invoked with the documented context pointer and
///   must itself uphold the same pointer invariants.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlThrDefOutputBufferCreateFilenameDefault(
    func: Option<
        unsafe extern "C" fn(
            *const c_char,
            crate::abi::structs::xmlCharEncodingHandlerPtr,
            c_int,
        ) -> *mut crate::abi::structs::_xmlOutputBuffer,
    >,
) -> Option<
    unsafe extern "C" fn(
        *const c_char,
        crate::abi::structs::xmlCharEncodingHandlerPtr,
        c_int,
    ) -> *mut crate::abi::structs::_xmlOutputBuffer,
> {
    if func.is_some() {
        crate::xml::globals::set_output_buffer_create_filename_value(func);
    }
    crate::xml::globals::get_output_buffer_create_filename_value()
}

// ═══════════════════════════════════════════════════════════════════════════════
// __xmlXxx() pointer accessors (upstream threads.c / globals.c)
// ═══════════════════════════════════════════════════════════════════════════════
// The deprecated thread-local API exports one `__xmlXxx(void)` accessor per
// global; each returns a pointer to the global so callers can read/write it.

/// Upstream `__xmlBufferAllocScheme(void)` — returns a pointer to `xmlBufferAllocScheme`.
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
pub unsafe extern "C" fn __xmlBufferAllocScheme() -> *mut c_int {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    core::ptr::addr_of_mut!(xmlBufferAllocScheme)
}

/// Upstream `__xmlDefaultBufferSize(void)` — returns a pointer to `xmlDefaultBufferSize`.
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
pub unsafe extern "C" fn __xmlDefaultBufferSize() -> *mut c_int {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    core::ptr::addr_of_mut!(xmlDefaultBufferSize)
}

/// Upstream `__xmlDeregisterNodeDefaultValue(void)` — returns a pointer to `xmlDeregisterNodeDefaultValue`.
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
pub unsafe extern "C" fn __xmlDeregisterNodeDefaultValue(
) -> *mut Option<unsafe extern "C" fn(*mut crate::abi::structs::_xmlNode)> {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    crate::xml::globals::deregister_node_ptr()
}

/// Upstream `__xmlDoValidityCheckingDefaultValue(void)` — returns a pointer to `xmlDoValidityCheckingDefaultValue`.
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
pub unsafe extern "C" fn __xmlDoValidityCheckingDefaultValue() -> *mut c_int {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    crate::xml::globals::do_validity_ptr()
}

/// Upstream `__xmlGenericError(void)` — returns a pointer to `xmlGenericError`.
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
pub unsafe extern "C" fn __xmlGenericError() -> *mut Option<xmlGenericErrorFunc> {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    crate::xml::globals::generic_error_ptr()
}

/// Upstream `__xmlGenericErrorContext(void)` — returns a pointer to `xmlGenericErrorContext`.
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
pub unsafe extern "C" fn __xmlGenericErrorContext() -> *mut *mut c_void {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    crate::xml::globals::generic_error_ctx_ptr()
}

/// Upstream `__xmlGetWarningsDefaultValue(void)` — returns a pointer to `xmlGetWarningsDefaultValue`.
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
pub unsafe extern "C" fn __xmlGetWarningsDefaultValue() -> *mut c_int {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    crate::xml::globals::get_warnings_ptr()
}

/// Upstream `__xmlKeepBlanksDefaultValue(void)` — returns a pointer to `xmlKeepBlanksDefaultValue`.
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
pub unsafe extern "C" fn __xmlKeepBlanksDefaultValue() -> *mut c_int {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    crate::xml::globals::keep_blanks_ptr()
}

/// Upstream `__xmlLineNumbersDefaultValue(void)` — returns a pointer to `xmlLineNumbersDefaultValue`.
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
pub unsafe extern "C" fn __xmlLineNumbersDefaultValue() -> *mut c_int {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    crate::xml::globals::line_numbers_ptr()
}

/// Upstream `__xmlLoadExtDtdDefaultValue(void)` — returns a pointer to `xmlLoadExtDtdDefaultValue`.
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
pub unsafe extern "C" fn __xmlLoadExtDtdDefaultValue() -> *mut c_int {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    crate::xml::globals::load_ext_dtd_ptr()
}

/// Upstream `__xmlOutputBufferCreateFilenameValue(void)` — returns a pointer to `xmlOutputBufferCreateFilenameValue`.
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
pub unsafe extern "C" fn __xmlOutputBufferCreateFilenameValue() -> *mut Option<
    unsafe extern "C" fn(
        *const c_char,
        crate::abi::structs::xmlCharEncodingHandlerPtr,
        c_int,
    ) -> *mut crate::abi::structs::_xmlOutputBuffer,
> {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    crate::xml::globals::output_create_filename_ptr()
}

/// Upstream `__xmlParserDebugEntities(void)` — returns a pointer to `xmlParserDebugEntities`.
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
pub unsafe extern "C" fn __xmlParserDebugEntities() -> *mut c_int {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    core::ptr::addr_of_mut!(xmlParserDebugEntities)
}

/// Upstream `__xmlParserInputBufferCreateFilenameValue(void)` — returns a pointer to `xmlParserInputBufferCreateFilenameValue`.
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
pub unsafe extern "C" fn __xmlParserInputBufferCreateFilenameValue() -> *mut Option<
    unsafe extern "C" fn(*const c_char, c_int) -> *mut crate::abi::structs::_xmlParserInputBuffer,
> {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    crate::xml::globals::parser_input_create_filename_ptr()
}

/// Upstream `__xmlParserVersion(void)` — returns a pointer to `xmlParserVersion`.
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
pub unsafe extern "C" fn __xmlParserVersion() -> *mut *const c_char {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    core::ptr::addr_of_mut!(xmlParserVersion)
}

/// Upstream `__xmlPedanticParserDefaultValue(void)` — returns a pointer to `xmlPedanticParserDefaultValue`.
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
pub unsafe extern "C" fn __xmlPedanticParserDefaultValue() -> *mut c_int {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    crate::xml::globals::pedantic_ptr()
}

/// Upstream `__xmlRegisterNodeDefaultValue(void)` — returns a pointer to `xmlRegisterNodeDefaultValue`.
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
pub unsafe extern "C" fn __xmlRegisterNodeDefaultValue(
) -> *mut Option<unsafe extern "C" fn(*mut crate::abi::structs::_xmlNode)> {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    crate::xml::globals::register_node_ptr()
}

/// Upstream `__xmlStructuredError(void)` — returns a pointer to `xmlStructuredError`.
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
pub unsafe extern "C" fn __xmlStructuredError() -> *mut Option<xmlStructuredErrorFunc> {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    crate::xml::globals::structured_error_ptr()
}

/// Upstream `__xmlStructuredErrorContext(void)` — returns a pointer to `xmlStructuredErrorContext`.
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
pub unsafe extern "C" fn __xmlStructuredErrorContext() -> *mut *mut c_void {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    crate::xml::globals::structured_error_ctx_ptr()
}

/// Upstream `__xmlSubstituteEntitiesDefaultValue(void)` — returns a pointer to `xmlSubstituteEntitiesDefaultValue`.
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
pub unsafe extern "C" fn __xmlSubstituteEntitiesDefaultValue() -> *mut c_int {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    crate::xml::globals::substitute_entities_ptr()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Regression court — xmlLastError mirror concurrency (11.1-X)
// ═══════════════════════════════════════════════════════════════════════════════
//
// R-000135 discovery during 11.1-X: the exported `xmlLastError` mirror was
// deep-copied and freed without synchronization, so concurrent error raises
// on different threads double-freed the mirror strings. The parallel lib
// test suite observed this as `double free or corruption (!prev)` aborts
// (xml::errors tests racing with any other raising thread). The writers are
// serialized via LAST_ERROR_MIRROR_LOCK; these courts hammer the exact
// interleavings and must complete without crashing.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::allocator::xmlMallocImpl;
    use crate::abi::structs::_xmlError;
    use crate::xml::globals;
    use core::ptr;

    /// Allocate a NUL-terminated C string owned by xmlMallocImpl (the same
    /// allocator the thread-local error slot uses).
    ///
    /// # Safety
    ///
    /// - `s` must be a valid string; the returned pointer is
    ///   allocator-owned, valid for `bytes.len() + 1` bytes, and must be
    ///   freed with `xmlFreeImpl` exactly once.
    unsafe fn alloc_cstr(s: &str) -> *mut c_char {
        let bytes = s.as_bytes();
        let p = unsafe { xmlMallocImpl(bytes.len() + 1) as *mut c_char };
        assert!(!p.is_null(), "alloc_cstr: xmlMallocImpl failed");
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), p as *mut u8, bytes.len());
            *((p as *mut u8).add(bytes.len())) = 0;
        }
        p
    }

    /// Build an owned `_xmlError` with distinct string fields.
    ///
    /// # Safety
    ///
    /// - Every `message`/`file`/`str1` field is an `alloc_cstr`
    ///   allocation that the returned struct owns; the caller must free
    ///   them with `xmlFreeImpl` exactly once (e.g. via
    ///   `reset_last_error`), or the fields leak.
    unsafe fn build_error(tag: &str) -> _xmlError {
        _xmlError {
            domain: 1,
            code: 2,
            message: unsafe { alloc_cstr(&format!("msg {tag}")) },
            level: 3,
            file: unsafe { alloc_cstr(&format!("file {tag}")) },
            line: 4,
            str1: unsafe { alloc_cstr(&format!("str1 {tag}")) },
            str2: ptr::null_mut(),
            str3: ptr::null_mut(),
            int1: 0,
            int2: 0,
            ctxt: ptr::null_mut(),
            node: ptr::null_mut(),
        }
    }

    /// Concurrent sync/reset hammer: one thread raises errors while another
    /// resets. Before the mirror lock this double-freed the shared strings;
    /// the test crashes (SIGABRT) under the old code and passes now.
    ///
    /// # Safety
    ///
    /// - `build_error` allocates owned string fields with `xmlFreeImpl`;
    ///   `set_last_error` deep-copies them into the mirror and the
    ///   thread-local slot, and `reset_last_error` frees each copy exactly
    ///   once; the mirror is internally serialized by `LAST_ERROR_MIRROR_LOCK`.
    #[test]
    fn test_last_error_mirror_concurrent_sync_reset() {
        let sync = std::thread::spawn(|| {
            for i in 0..400 {
                unsafe { globals::set_last_error(build_error(&format!("sync {i}"))) };
            }
        });
        let reset = std::thread::spawn(|| {
            for _ in 0..400 {
                globals::reset_last_error();
            }
        });
        sync.join().unwrap();
        reset.join().unwrap();
        // Leave the mirror in a clean state for later tests. (No thread-local
        // assertion: the harness reuses OS threads across tests, so a prior
        // test's error may legitimately live in this thread's slot.)
        globals::reset_last_error();
    }

    /// Many threads raising concurrently (the full parallel-suite shape that
    /// originally aborted in `test_encode_entities_reentrant_*` victims).
    ///
    /// # Safety
    ///
    /// - Each thread's `build_error` allocations are owned by the
    ///   thread-local last-error slot and the locked mirror; every
    ///   `set_last_error`/`reset_last_error` pair frees owned strings
    ///   exactly once, and the final `reset_last_error` leaves the mirror
    ///   clean.
    #[test]
    fn test_last_error_mirror_many_threads() {
        let mut handles = Vec::new();
        for t in 0..8 {
            handles.push(std::thread::spawn(move || {
                for i in 0..150 {
                    unsafe { globals::set_last_error(build_error(&format!("t{t} i{i}"))) };
                    if i % 7 == 0 {
                        globals::reset_last_error();
                    }
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        globals::reset_last_error();
    }
}
