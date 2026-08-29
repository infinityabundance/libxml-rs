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
use std::os::raw::c_char;
use std::os::raw::c_int;

use crate::abi::callbacks::{xmlGenericErrorFunc, xmlStructuredErrorFunc};
use crate::abi::types::xmlChar;

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

/// `int xmlLineNumbersDefaultValue` (default 0)
#[no_mangle]
pub static mut xmlLineNumbersDefaultValue: c_int = 0;

/// `int xmlKeepBlanksDefaultValue` (default 1)
#[no_mangle]
pub static mut xmlKeepBlanksDefaultValue: c_int = 1;

/// `int xmlSubstituteEntitiesDefaultValue` (default 0)
#[no_mangle]
pub static mut xmlSubstituteEntitiesDefaultValue: c_int = 0;

/// `int xmlParserDebugEntities` (default 0)
#[no_mangle]
pub static mut xmlParserDebugEntities: c_int = 0;

/// `int xmlIndentTreeOutput` (default 0)
#[no_mangle]
pub static mut xmlIndentTreeOutput: c_int = 0;

/// `const xmlChar *xmlTreeIndentString` (default NULL)
#[no_mangle]
pub static mut xmlTreeIndentString: *const xmlChar = core::ptr::null();

/// `int xmlSaveNoEmptyTags` (default 0)
#[no_mangle]
pub static mut xmlSaveNoEmptyTags: c_int = 0;

/// `xmlRegisterNodeFunc xmlRegisterNodeDefaultValue` (default NULL)
#[no_mangle]
pub static mut xmlRegisterNodeDefaultValue: Option<
    unsafe extern "C" fn(*mut crate::abi::structs::_xmlNode),
> = None;

/// `xmlDeregisterNodeFunc xmlDeregisterNodeDefaultValue` (default NULL)
#[no_mangle]
pub static mut xmlDeregisterNodeDefaultValue: Option<
    unsafe extern "C" fn(*mut crate::abi::structs::_xmlNode),
> = None;

/// `const char *xmlParserVersion` (default "21503" — upstream LIBXML_VERSION_STRING)
///
/// SAFETY: the pointed-to string is a static, immutable, null-terminated
/// literal; `static mut` is used because C raw pointers are not `Sync`.
/// Reads/writes of the pointer itself are racy only if C code mutates it
/// (upstream treats it as a constant).
#[no_mangle]
pub static mut xmlParserVersion: *const c_char = {
    const S: &[u8] = b"21503\0";
    S.as_ptr() as *const c_char
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

/// `xmlBufferAllocationScheme xmlBufferAllocScheme` (default XML_BUFFER_ALLOC_EXACT = 0)
#[no_mangle]
pub static mut xmlBufferAllocScheme: c_int = 0;

// ═══════════════════════════════════════════════════════════════════════════════
// Error callback globals (upstream xmlerror.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// `xmlGenericErrorFunc xmlGenericError` — the generic error callback.
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

/// `const int xsltLibxmlVersion` = LIBXML_VERSION (21503) — the libxml2
/// version libxslt was built against (upstream xslt.c).
#[no_mangle]
pub static xsltLibxmlVersion: c_int = 21503;

/// `xmlGenericErrorFunc xsltGenericError` — the libxslt error callback.
/// Upstream defaults to `xsltGenericErrorDefaultFunc` (a variadic stderr
/// printer); the candidate defaults to NULL and the XSLT error paths fall
/// back to the generic error handler — documented safe divergence
/// (residual R-000135), since stable Rust cannot define variadic extern
/// functions.
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
