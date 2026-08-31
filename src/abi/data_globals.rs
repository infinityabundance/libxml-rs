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

use core::ffi::c_void;
use std::os::raw::{c_char, c_int, c_uint};

use crate::abi::callbacks::{xmlGenericErrorFunc, xmlStructuredErrorFunc};
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

/// `int xmlDoValidityCheckingDefaultValue` (default 0)
#[no_mangle]
pub static mut xmlDoValidityCheckingDefaultValue: c_int = 0;

/// `int xmlGetWarningsDefaultValue` (default 1)
#[no_mangle]
pub static mut xmlGetWarningsDefaultValue: c_int = 1;

/// `int xmlLoadExtDtdDefaultValue` (default 0)
#[no_mangle]
pub static mut xmlLoadExtDtdDefaultValue: c_int = 0;

/// `int xmlPedanticParserDefaultValue` (default 0)
#[no_mangle]
pub static mut xmlPedanticParserDefaultValue: c_int = 0;

/// `int xmlLineNumbersDefaultValue` (default 1 — upstream globals.c
/// `xmlLineNumbersDefaultValueThrDef = 1`)
#[no_mangle]
pub static mut xmlLineNumbersDefaultValue: c_int = 1;

/// `int xmlKeepBlanksDefaultValue` (default 1)
#[no_mangle]
pub static mut xmlKeepBlanksDefaultValue: c_int = 1;

/// `int xmlSubstituteEntitiesDefaultValue` (default 0)
#[no_mangle]
pub static mut xmlSubstituteEntitiesDefaultValue: c_int = 0;

/// `int xmlParserDebugEntities` (default 0)
#[no_mangle]
pub static mut xmlParserDebugEntities: c_int = 0;

/// `int xmlIndentTreeOutput` (default 1 — upstream globals.c
/// `xmlIndentTreeOutputThrDef = 1`)
#[no_mangle]
pub static mut xmlIndentTreeOutput: c_int = 1;

/// `const xmlChar *xmlTreeIndentString` (default "  " — upstream globals.c
/// `xmlTreeIndentStringThrDef = "  "`)
#[no_mangle]
pub static mut xmlTreeIndentString: *const xmlChar = {
    static S: [u8; 3] = *b"  \0";
    S.as_ptr()
};

/// `int xmlSaveNoEmptyTags` (default 0)
#[no_mangle]
pub static mut xmlSaveNoEmptyTags: c_int = 0;

/// Upstream `xmlRegisterNodeFunc xmlRegisterNodeDefaultValue` (default NULL)
#[no_mangle]
pub static mut xmlRegisterNodeDefaultValue: Option<
    unsafe extern "C" fn(*mut crate::abi::structs::_xmlNode),
> = None;

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
    let hook = unsafe { xmlRegisterNodeDefaultValue };
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
    let hook = unsafe { xmlDeregisterNodeDefaultValue };
    if let Some(h) = hook {
        if !node.is_null() {
            // SAFETY: the hook is a valid C callback registered by the user.
            unsafe { h(node) };
        }
    }
}

/// `xmlDeregisterNodeFunc xmlDeregisterNodeDefaultValue` (default NULL)
#[no_mangle]
pub static mut xmlDeregisterNodeDefaultValue: Option<
    unsafe extern "C" fn(*mut crate::abi::structs::_xmlNode),
> = None;

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
// Error callback globals (upstream xmlerror.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// `xmlGenericErrorFunc xmlGenericError` — the generic error callback.
///
/// Upstream defaults to `xmlGenericErrorDefaultFunc` (a variadic stderr
/// printer, error.c); the candidate's shim below reproduces it, so a
/// freshly-initialized library routes errors to stderr exactly like upstream.
#[cfg(target_arch = "x86_64")]
#[no_mangle]
pub static mut xmlGenericError: Option<xmlGenericErrorFunc> = Some(XML_GENERIC_ERROR_DEFAULT);

#[cfg(not(target_arch = "x86_64"))]
#[no_mangle]
pub static mut xmlGenericError: Option<xmlGenericErrorFunc> = None;

/// `void *xmlGenericErrorContext` — context for the generic error callback.
#[no_mangle]
pub static mut xmlGenericErrorContext: *mut c_void = core::ptr::null_mut();

/// `xmlStructuredErrorFunc xmlStructuredError` — the structured error callback.
#[no_mangle]
pub static mut xmlStructuredError: Option<xmlStructuredErrorFunc> = None;

/// `void *xmlStructuredErrorContext` — context for the structured callback.
#[no_mangle]
pub static mut xmlStructuredErrorContext: *mut c_void = core::ptr::null_mut();

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
#[derive(Clone, Copy)]
struct VaListTag {
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
#[cfg(target_arch = "x86_64")]
#[no_mangle]
pub unsafe extern "C" fn xmlGenericErrorDefaultFuncV(
    _ctx: *mut c_void,
    msg: *const c_char,
    ap: *mut VaListTag,
) -> c_int {
    unsafe {
        if crate::abi::data_globals::xmlGenericErrorContext.is_null() {
            crate::abi::data_globals::xmlGenericErrorContext = stderr_file();
        }
        let stream = crate::abi::data_globals::xmlGenericErrorContext;
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
const XML_GENERIC_ERROR_DEFAULT: xmlGenericErrorFunc = unsafe {
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
pub static xmlStringTextNoenc: [xmlChar; 9] =
    [b't', b'e', b'x', b't', b'n', b'o', b'e', b'n', b'c'];

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

/// `const xmlChar xsltExtMarker[]` — empty string used to mark extension
/// nodes (upstream transform.c).
#[no_mangle]
pub static xsltExtMarker: [xmlChar; 1] = [0];

/// `xsltDocLoaderFunc xsltDocDefaultLoader` — the document loader callback.
/// Upstream defaults to `xsltDocDefaultLoaderFunc`; the candidate defaults
/// to NULL and its internal loader path is used — documented safe
/// divergence (residual R-000135).
#[no_mangle]
pub static mut xsltDocDefaultLoader: Option<
    unsafe extern "C" fn(
        *const xmlChar,
        *mut c_void,
        c_int,
        *mut crate::abi::structs::_xsltStylesheet,
        *mut crate::abi::structs::_xsltTransformContext,
    ) -> *mut crate::abi::structs::_xmlDoc,
> = None;

// ═══════════════════════════════════════════════════════════════════════════════
// I/O filename callback globals (upstream xmlIO.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// `xmlParserInputBufferCreateFilenameFunc xmlParserInputBufferCreateFilenameValue`
#[no_mangle]
pub static mut xmlParserInputBufferCreateFilenameValue: Option<
    unsafe extern "C" fn(*const c_char, c_int) -> *mut crate::abi::structs::_xmlParserInputBuffer,
> = None;

/// `xmlOutputBufferCreateFilenameFunc xmlOutputBufferCreateFilenameValue`
#[no_mangle]
pub static mut xmlOutputBufferCreateFilenameValue: Option<
    unsafe extern "C" fn(
        *const c_char,
        crate::abi::structs::xmlCharEncodingHandlerPtr,
        c_int,
    ) -> *mut crate::abi::structs::_xmlOutputBuffer,
> = None;

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
#[no_mangle]
pub static xmlDefaultSAXHandler: crate::abi::structs::_xmlSAXHandlerV1 =
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
        warning: Some(crate::xml::errors::xmlParserWarning),
        error: Some(crate::xml::errors::xmlParserError),
        fatalError: Some(crate::xml::errors::xmlParserError),
        getParameterEntity: Some(crate::abi::exports_xml2::xmlSAX2GetParameterEntity),
        cdataBlock: Some(crate::abi::exports_xml2::xmlSAX2CDataBlock),
        externalSubset: Some(crate::abi::exports_xml2::xmlSAX2ExternalSubset),
        initialized: 1,
    };

/// `const xmlSAXHandlerV1 htmlDefaultSAXHandler` (globals.c 2.15.3).
#[no_mangle]
pub static htmlDefaultSAXHandler: crate::abi::structs::_xmlSAXHandlerV1 =
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
        warning: Some(crate::xml::errors::xmlParserWarning),
        error: Some(crate::xml::errors::xmlParserError),
        fatalError: Some(crate::xml::errors::xmlParserError),
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
#[no_mangle]
pub unsafe extern "C" fn xmlKeepBlanksDefault(v: c_int) -> c_int {
    unsafe {
        if v != 0 {
            xmlKeepBlanksDefaultValue = v;
        }
        xmlKeepBlanksDefaultValue
    }
}

/// Upstream `xmlLineNumbersDefault(int v)`.
#[no_mangle]
pub unsafe extern "C" fn xmlLineNumbersDefault(v: c_int) -> c_int {
    unsafe {
        if v != 0 {
            xmlLineNumbersDefaultValue = v;
        }
        xmlLineNumbersDefaultValue
    }
}

/// Upstream `xmlSubstituteEntitiesDefault(int v)`.
#[no_mangle]
pub unsafe extern "C" fn xmlSubstituteEntitiesDefault(v: c_int) -> c_int {
    unsafe {
        if v != 0 {
            xmlSubstituteEntitiesDefaultValue = v;
        }
        xmlSubstituteEntitiesDefaultValue
    }
}

/// Upstream `xmlPedanticParserDefault(int v)`.
#[no_mangle]
pub unsafe extern "C" fn xmlPedanticParserDefault(v: c_int) -> c_int {
    unsafe {
        if v != 0 {
            xmlPedanticParserDefaultValue = v;
        }
        xmlPedanticParserDefaultValue
    }
}

/// Upstream `xmlDoValidityCheckingDefaultValue` accessor is the global
/// itself; `xmlGetWarningsDefaultValue` likewise (no accessor functions
/// exist for those in upstream 2.15).

/// Upstream `xmlRegisterNodeDefault(xmlRegisterNodeFunc func)`.
#[no_mangle]
pub unsafe extern "C" fn xmlRegisterNodeDefault(
    func: Option<unsafe extern "C" fn(*mut crate::abi::structs::_xmlNode)>,
) -> Option<unsafe extern "C" fn(*mut crate::abi::structs::_xmlNode)> {
    unsafe {
        // UPSTREAM-PARITY (tree.c): registering any callback arms the
        // xmlRegisterCallbacks gate.
        XML_REGISTER_CALLBACKS.store(true, core::sync::atomic::Ordering::Relaxed);
        if func.is_some() {
            xmlRegisterNodeDefaultValue = func;
        }
        xmlRegisterNodeDefaultValue
    }
}

/// Upstream `xmlDeregisterNodeDefault(xmlDeregisterNodeFunc func)`.
#[no_mangle]
pub unsafe extern "C" fn xmlDeregisterNodeDefault(
    func: Option<unsafe extern "C" fn(*mut crate::abi::structs::_xmlNode)>,
) -> Option<unsafe extern "C" fn(*mut crate::abi::structs::_xmlNode)> {
    unsafe {
        // UPSTREAM-PARITY (tree.c): registering any callback arms the
        // xmlRegisterCallbacks gate.
        XML_REGISTER_CALLBACKS.store(true, core::sync::atomic::Ordering::Relaxed);
        if func.is_some() {
            xmlDeregisterNodeDefaultValue = func;
        }
        xmlDeregisterNodeDefaultValue
    }
}

/// Upstream `__xmlIndentTreeOutput(void)` (parser.h) — returns a pointer to
/// the `xmlIndentTreeOutput` global.
#[no_mangle]
pub unsafe extern "C" fn __xmlIndentTreeOutput() -> *mut c_int {
    unsafe { core::ptr::addr_of_mut!(xmlIndentTreeOutput) }
}

/// Upstream `__xmlSaveNoEmptyTags(void)` (parser.h) — returns a pointer to
/// the `xmlSaveNoEmptyTags` global.
#[no_mangle]
pub unsafe extern "C" fn __xmlSaveNoEmptyTags() -> *mut c_int {
    unsafe { core::ptr::addr_of_mut!(xmlSaveNoEmptyTags) }
}

/// Upstream `__xmlTreeIndentString(void)` (parser.h) — returns a pointer to
/// the `xmlTreeIndentString` global.
#[no_mangle]
pub unsafe extern "C" fn __xmlTreeIndentString() -> *mut *const xmlChar {
    unsafe { core::ptr::addr_of_mut!(xmlTreeIndentString) }
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
#[no_mangle]
pub unsafe extern "C" fn xmlThrDefDoValidityCheckingDefaultValue(v: c_int) -> c_int {
    unsafe {
        if v != 0 {
            xmlDoValidityCheckingDefaultValue = v;
        }
        xmlDoValidityCheckingDefaultValue
    }
}

/// Upstream `xmlThrDefGetWarningsDefaultValue(int v)`.
#[no_mangle]
pub unsafe extern "C" fn xmlThrDefGetWarningsDefaultValue(v: c_int) -> c_int {
    unsafe {
        if v != 0 {
            xmlGetWarningsDefaultValue = v;
        }
        xmlGetWarningsDefaultValue
    }
}

/// Upstream `xmlThrDefLoadExtDtdDefaultValue(int v)`.
#[no_mangle]
pub unsafe extern "C" fn xmlThrDefLoadExtDtdDefaultValue(v: c_int) -> c_int {
    unsafe {
        if v != 0 {
            xmlLoadExtDtdDefaultValue = v;
        }
        xmlLoadExtDtdDefaultValue
    }
}

/// Upstream `xmlThrDefPedanticParserDefaultValue(int v)`.
#[no_mangle]
pub unsafe extern "C" fn xmlThrDefPedanticParserDefaultValue(v: c_int) -> c_int {
    unsafe {
        if v != 0 {
            xmlPedanticParserDefaultValue = v;
        }
        xmlPedanticParserDefaultValue
    }
}

/// Upstream `xmlThrDefLineNumbersDefaultValue(int v)`.
#[no_mangle]
pub unsafe extern "C" fn xmlThrDefLineNumbersDefaultValue(v: c_int) -> c_int {
    unsafe {
        if v != 0 {
            xmlLineNumbersDefaultValue = v;
        }
        xmlLineNumbersDefaultValue
    }
}

/// Upstream `xmlThrDefKeepBlanksDefaultValue(int v)`.
#[no_mangle]
pub unsafe extern "C" fn xmlThrDefKeepBlanksDefaultValue(v: c_int) -> c_int {
    unsafe {
        if v != 0 {
            xmlKeepBlanksDefaultValue = v;
        }
        xmlKeepBlanksDefaultValue
    }
}

/// Upstream `xmlThrDefSubstituteEntitiesDefaultValue(int v)`.
#[no_mangle]
pub unsafe extern "C" fn xmlThrDefSubstituteEntitiesDefaultValue(v: c_int) -> c_int {
    unsafe {
        if v != 0 {
            xmlSubstituteEntitiesDefaultValue = v;
        }
        xmlSubstituteEntitiesDefaultValue
    }
}

/// Upstream `xmlThrDefParserDebugEntities(int v)`.
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
#[no_mangle]
pub unsafe extern "C" fn xmlThrDefIndentTreeOutput(v: c_int) -> c_int {
    unsafe {
        if v != 0 {
            xmlIndentTreeOutput = v;
        }
        xmlIndentTreeOutput
    }
}

/// Upstream `xmlThrDefTreeIndentString(const char *v)` — sets the indent
/// string when non-NULL and returns the current pointer.
#[no_mangle]
pub unsafe extern "C" fn xmlThrDefTreeIndentString(v: *const c_char) -> *const c_char {
    unsafe {
        if !v.is_null() {
            xmlTreeIndentString = v as *const xmlChar;
        }
        xmlTreeIndentString as *const c_char
    }
}

/// Upstream `xmlThrDefSaveNoEmptyTags(int v)`.
#[no_mangle]
pub unsafe extern "C" fn xmlThrDefSaveNoEmptyTags(v: c_int) -> c_int {
    unsafe {
        if v != 0 {
            xmlSaveNoEmptyTags = v;
        }
        xmlSaveNoEmptyTags
    }
}

/// Upstream `xmlThrDefRegisterNodeDefault(xmlRegisterNodeFunc func)`.
#[no_mangle]
pub unsafe extern "C" fn xmlThrDefRegisterNodeDefault(
    func: Option<unsafe extern "C" fn(*mut crate::abi::structs::_xmlNode)>,
) -> Option<unsafe extern "C" fn(*mut crate::abi::structs::_xmlNode)> {
    unsafe {
        if func.is_some() {
            xmlRegisterNodeDefaultValue = func;
        }
        xmlRegisterNodeDefaultValue
    }
}

/// Upstream `xmlThrDefDeregisterNodeDefault(xmlDeregisterNodeFunc func)`.
#[no_mangle]
pub unsafe extern "C" fn xmlThrDefDeregisterNodeDefault(
    func: Option<unsafe extern "C" fn(*mut crate::abi::structs::_xmlNode)>,
) -> Option<unsafe extern "C" fn(*mut crate::abi::structs::_xmlNode)> {
    unsafe {
        if func.is_some() {
            xmlDeregisterNodeDefaultValue = func;
        }
        xmlDeregisterNodeDefaultValue
    }
}

/// Upstream `xmlThrDefSetGenericErrorFunc(void *ctx, xmlGenericErrorFunc func)`.
#[no_mangle]
pub unsafe extern "C" fn xmlThrDefSetGenericErrorFunc(
    ctx: *mut c_void,
    func: Option<xmlGenericErrorFunc>,
) {
    unsafe {
        xmlGenericErrorContext = ctx;
        xmlGenericError = func;
    }
}

/// Upstream `xmlThrDefSetStructuredErrorFunc(void *ctx, xmlStructuredErrorFunc func)`.
#[no_mangle]
pub unsafe extern "C" fn xmlThrDefSetStructuredErrorFunc(
    ctx: *mut c_void,
    func: Option<xmlStructuredErrorFunc>,
) {
    unsafe {
        xmlStructuredErrorContext = ctx;
        xmlStructuredError = func;
    }
}

/// Upstream `xmlThrDefDefaultBufferSize(int v)`.
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
    unsafe {
        if func.is_some() {
            xmlParserInputBufferCreateFilenameValue = func;
        }
        xmlParserInputBufferCreateFilenameValue
    }
}

/// Upstream `xmlThrDefOutputBufferCreateFilenameDefault(...)`.
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
    unsafe {
        if func.is_some() {
            xmlOutputBufferCreateFilenameValue = func;
        }
        xmlOutputBufferCreateFilenameValue
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// __xmlXxx() pointer accessors (upstream threads.c / globals.c)
// ═══════════════════════════════════════════════════════════════════════════════
// The deprecated thread-local API exports one `__xmlXxx(void)` accessor per
// global; each returns a pointer to the global so callers can read/write it.

/// Upstream `__xmlBufferAllocScheme(void)` — returns a pointer to `xmlBufferAllocScheme`.
#[no_mangle]
pub unsafe extern "C" fn __xmlBufferAllocScheme() -> *mut c_int {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    unsafe { core::ptr::addr_of_mut!(xmlBufferAllocScheme) }
}

/// Upstream `__xmlDefaultBufferSize(void)` — returns a pointer to `xmlDefaultBufferSize`.
#[no_mangle]
pub unsafe extern "C" fn __xmlDefaultBufferSize() -> *mut c_int {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    unsafe { core::ptr::addr_of_mut!(xmlDefaultBufferSize) }
}

/// Upstream `__xmlDeregisterNodeDefaultValue(void)` — returns a pointer to `xmlDeregisterNodeDefaultValue`.
#[no_mangle]
pub unsafe extern "C" fn __xmlDeregisterNodeDefaultValue(
) -> *mut Option<unsafe extern "C" fn(*mut crate::abi::structs::_xmlNode)> {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    unsafe { core::ptr::addr_of_mut!(xmlDeregisterNodeDefaultValue) }
}

/// Upstream `__xmlDoValidityCheckingDefaultValue(void)` — returns a pointer to `xmlDoValidityCheckingDefaultValue`.
#[no_mangle]
pub unsafe extern "C" fn __xmlDoValidityCheckingDefaultValue() -> *mut c_int {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    unsafe { core::ptr::addr_of_mut!(xmlDoValidityCheckingDefaultValue) }
}

/// Upstream `__xmlGenericError(void)` — returns a pointer to `xmlGenericError`.
#[no_mangle]
pub unsafe extern "C" fn __xmlGenericError() -> *mut Option<xmlGenericErrorFunc> {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    unsafe { core::ptr::addr_of_mut!(xmlGenericError) }
}

/// Upstream `__xmlGenericErrorContext(void)` — returns a pointer to `xmlGenericErrorContext`.
#[no_mangle]
pub unsafe extern "C" fn __xmlGenericErrorContext() -> *mut *mut c_void {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    unsafe { core::ptr::addr_of_mut!(xmlGenericErrorContext) }
}

/// Upstream `__xmlGetWarningsDefaultValue(void)` — returns a pointer to `xmlGetWarningsDefaultValue`.
#[no_mangle]
pub unsafe extern "C" fn __xmlGetWarningsDefaultValue() -> *mut c_int {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    unsafe { core::ptr::addr_of_mut!(xmlGetWarningsDefaultValue) }
}

/// Upstream `__xmlKeepBlanksDefaultValue(void)` — returns a pointer to `xmlKeepBlanksDefaultValue`.
#[no_mangle]
pub unsafe extern "C" fn __xmlKeepBlanksDefaultValue() -> *mut c_int {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    unsafe { core::ptr::addr_of_mut!(xmlKeepBlanksDefaultValue) }
}

/// Upstream `__xmlLineNumbersDefaultValue(void)` — returns a pointer to `xmlLineNumbersDefaultValue`.
#[no_mangle]
pub unsafe extern "C" fn __xmlLineNumbersDefaultValue() -> *mut c_int {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    unsafe { core::ptr::addr_of_mut!(xmlLineNumbersDefaultValue) }
}

/// Upstream `__xmlLoadExtDtdDefaultValue(void)` — returns a pointer to `xmlLoadExtDtdDefaultValue`.
#[no_mangle]
pub unsafe extern "C" fn __xmlLoadExtDtdDefaultValue() -> *mut c_int {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    unsafe { core::ptr::addr_of_mut!(xmlLoadExtDtdDefaultValue) }
}

/// Upstream `__xmlOutputBufferCreateFilenameValue(void)` — returns a pointer to `xmlOutputBufferCreateFilenameValue`.
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
    unsafe { core::ptr::addr_of_mut!(xmlOutputBufferCreateFilenameValue) }
}

/// Upstream `__xmlParserDebugEntities(void)` — returns a pointer to `xmlParserDebugEntities`.
#[no_mangle]
pub unsafe extern "C" fn __xmlParserDebugEntities() -> *mut c_int {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    unsafe { core::ptr::addr_of_mut!(xmlParserDebugEntities) }
}

/// Upstream `__xmlParserInputBufferCreateFilenameValue(void)` — returns a pointer to `xmlParserInputBufferCreateFilenameValue`.
#[no_mangle]
pub unsafe extern "C" fn __xmlParserInputBufferCreateFilenameValue() -> *mut Option<
    unsafe extern "C" fn(*const c_char, c_int) -> *mut crate::abi::structs::_xmlParserInputBuffer,
> {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    unsafe { core::ptr::addr_of_mut!(xmlParserInputBufferCreateFilenameValue) }
}

/// Upstream `__xmlParserVersion(void)` — returns a pointer to `xmlParserVersion`.
#[no_mangle]
pub unsafe extern "C" fn __xmlParserVersion() -> *mut *const c_char {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    unsafe { core::ptr::addr_of_mut!(xmlParserVersion) }
}

/// Upstream `__xmlPedanticParserDefaultValue(void)` — returns a pointer to `xmlPedanticParserDefaultValue`.
#[no_mangle]
pub unsafe extern "C" fn __xmlPedanticParserDefaultValue() -> *mut c_int {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    unsafe { core::ptr::addr_of_mut!(xmlPedanticParserDefaultValue) }
}

/// Upstream `__xmlRegisterNodeDefaultValue(void)` — returns a pointer to `xmlRegisterNodeDefaultValue`.
#[no_mangle]
pub unsafe extern "C" fn __xmlRegisterNodeDefaultValue(
) -> *mut Option<unsafe extern "C" fn(*mut crate::abi::structs::_xmlNode)> {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    unsafe { core::ptr::addr_of_mut!(xmlRegisterNodeDefaultValue) }
}

/// Upstream `__xmlStructuredError(void)` — returns a pointer to `xmlStructuredError`.
#[no_mangle]
pub unsafe extern "C" fn __xmlStructuredError() -> *mut Option<xmlStructuredErrorFunc> {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    unsafe { core::ptr::addr_of_mut!(xmlStructuredError) }
}

/// Upstream `__xmlStructuredErrorContext(void)` — returns a pointer to `xmlStructuredErrorContext`.
#[no_mangle]
pub unsafe extern "C" fn __xmlStructuredErrorContext() -> *mut *mut c_void {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    unsafe { core::ptr::addr_of_mut!(xmlStructuredErrorContext) }
}

/// Upstream `__xmlSubstituteEntitiesDefaultValue(void)` — returns a pointer to `xmlSubstituteEntitiesDefaultValue`.
#[no_mangle]
pub unsafe extern "C" fn __xmlSubstituteEntitiesDefaultValue() -> *mut c_int {
    // SAFETY: returning a pointer to an exported static; the caller may
    // read/write it exactly as with upstream's deprecated accessor.
    unsafe { core::ptr::addr_of_mut!(xmlSubstituteEntitiesDefaultValue) }
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
