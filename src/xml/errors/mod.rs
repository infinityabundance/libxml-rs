//! Error subsystem (§21, §85 Phase 1).
//!
//! Implements the libxml2 error reporting infrastructure:
//!
//! - `xmlError` struct management
//! - Error domain/code registry
//! - Structured error callbacks (thread-local storage)
//! - Generic error callbacks (thread-local storage)
//! - Last-error tracking (thread-local `xmlGetLastError`, `xmlResetLastError`, `xmlCopyError`)
//! - Error message formatting
//! - `xmlRaiseError()` — the central error reporting function
//!
//! # UPSTREAM-PARITY
//!
//! libxml2 has a two-tier error system:
//!
//! 1. **Structured errors** — `xmlStructuredErrorFunc` receives an `xmlErrorPtr`
//!    with all structured fields (domain, code, level, line, etc.)
//!
//! 2. **Generic errors** — `xmlGenericErrorFunc` receives a formatted string
//!    (printf-style). This is the older system, still widely used.
//!
//! Both systems coexist. When both handlers are set, both are called.
//! The last error is stored thread-locally for retrieval via `xmlGetLastError`.
//!
//! # Phase 1 status
//!
//! Complete — all error functions are implemented.
//! Variadic message formatting will be enhanced in Phase 2+.
//!
//! # Upstream contract
//!
//! Mirrors upstream error.c (SRC-LIBXML2-2.15.0-ERROR-C, oracle tree
//! `oracle/historical/src/libxml2-2.15.0/error.c`): xmlRaiseError /
//! xmlVRaiseError routing, xmlFormatError, xmlGenericErrorDefaultFunc,
//! xmlGetLastError / xmlCtxtGetLastError / xmlResetLastError, xmlCopyError
//! and the legacy xmlParserError / xmlParserWarning SAX handlers.
//!
//! # Conceptual behavior
//!
//! Two-tier error system: structured errors (xmlStructuredErrorFunc with the
//! full xmlError) and generic errors (printf-style fragments). The 11.1-M/K
//! rework routes each raise through ONE channel: a structured handler, else
//! the custom SAX channel (channel(data, msg) once), else the legacy default
//! streaming the xmlFormatError fragments (file/line, domain, level, message,
//! source window, caret) as 6 variadic calls (R-000161).
//!
//! # Ownership & safety invariants
//!
//! Ownership: the thread-local last error owns its file/str1-3 copies;
//! raise_error_streamed copies transient CStrings (R-000163: dangling
//! pointers were the bug). The exported xmlLastError mirror is synced under
//! lock (R-000170). SAFETY: the variadic xmlGenericErrorDefaultFunc and
//! xmlParserError legacy handlers are x86_64 SysV inline-asm va_list shims —
//! stable Rust cannot define variadic extern fns.
//!
//! # Historical quirks & epochs
//!
//! E-005: fatal parser errors are reported twice from 2.13.0 (attr-markup-
//! entity); the xmlFormatError fragment stream and the 80-column source
//! window cap are 2.15 semantics (xmlParserInputGetWindow, caret clamp).
//! R-000163 pinned all XML_ERR_* 64-96 constants to upstream numbering
//! (SPACE_REQUIRED 65, NAME_REQUIRED 68, GT_REQUIRED 73, TAG_NAME_MISMATCH
//! 76, TAG_NOT_FINISHED 77 — not the synthetic renumbering).
//!
//! # Deliberate oddities
//!
//! Deliberate oddities: warnings only bump nbWarnings while other levels
//! update errNo / nbErrors / wellFormed per xmlCtxtVErr; the source window
//! replicates the 80-char cap, continuation-byte skip, UTF-8 forward scan and
//! the 2.15 caret clamp (col >= n maps to size-1).
//!
//! # Proving courts
//!
//! ERROR and CALLBACK court families; ERROR-001 (48 deterministic malformed
//! inputs x 4 passes, byte-identical), GLOBALS-THREADING, DATA-GLOBALS-001,
//! and `cargo test --lib` (ASan-clean).
//!
//! # Tempting simplifications that would break parity
//!
//! The tempting simplification is one generic call plus a direct stderr write
//! — a counting handler would observe 1 call instead of the 6 xmlFormatError
//! fragments the oracle emits (R-000161). Do not route errors to stderr when a
//! handler is installed. Never renumber the XML_ERR_* constants (R-000163).

use core::ffi::c_void;
use core::ptr;
use std::os::raw::{c_char, c_int, c_uint};

use crate::abi::callbacks::{errorSAXFunc, xmlGenericErrorFunc, xmlStructuredErrorFunc};
use crate::abi::structs::_xmlError;
use crate::abi::types::xmlErrorLevel::*;
use crate::abi::types::*;
use crate::xml::globals;

// ═══════════════════════════════════════════════════════════════════════════════
// Error Management Functions
// ═══════════════════════════════════════════════════════════════════════════════

/// Set the generic error handler.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSetGenericErrorFunc(void *ctx, xmlGenericErrorFunc handler);
/// ```
///
/// # SAFETY
///
/// - `handler` must be a valid function pointer or NULL (to reset to default).
/// - If non-NULL, the handler may be called at any time with `ctx`.
pub unsafe fn set_generic_error_func(ctx: *mut c_void, handler: Option<xmlGenericErrorFunc>) {
    // SAFETY: Delegates to globals with same safety contract.
    unsafe { globals::set_generic_error_func(ctx, handler) };
}

/// Set the structured error handler.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlSetStructuredErrorFunc(void *ctx, xmlStructuredErrorFunc handler);
/// ```
///
/// # SAFETY
///
/// - `handler` must be a valid function pointer or NULL.
pub unsafe fn set_structured_error_func(ctx: *mut c_void, handler: Option<xmlStructuredErrorFunc>) {
    // SAFETY: Delegates to globals with same safety contract.
    unsafe { globals::set_structured_error_func(ctx, handler) };
}

/// Get the last error for the current thread.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlErrorPtr xmlGetLastError(void);
/// ```
///
/// Returns a pointer to the last error, or NULL if no error occurred.
/// The returned pointer is valid until the next libxml2 call in this thread.
pub fn get_last_error() -> *mut _xmlError {
    globals::get_last_error()
}

/// Copy an error from one location to another.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlErrorPtr xmlCopyError(xmlErrorPtr from, xmlErrorPtr to);
/// ```
///
/// Copies `from` into `to`. Returns 0 on success, -1 on error.
///
/// # SAFETY
///
/// - `from` and `to` must be valid pointers to `_xmlError` structs, or NULL.
pub const unsafe fn copy_error(from: *const _xmlError, to: *mut _xmlError) -> c_int {
    if from.is_null() || to.is_null() {
        return -1;
    }
    // SAFETY: Caller guarantees both pointers are valid.
    unsafe {
        ptr::copy_nonoverlapping(from, to, 1);
    }
    0
}

/// Reset an error structure to its default state.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlResetError(xmlErrorPtr err);
/// ```
///
/// # SAFETY
///
/// - `err` must be a valid pointer to `_xmlError`, or NULL.
pub const unsafe fn reset_error(err: *mut _xmlError) {
    if err.is_null() {
        return;
    }
    // SAFETY: Caller guarantees pointer is valid.
    unsafe {
        ptr::write(
            err,
            _xmlError {
                domain: XML_FROM_NONE,
                code: XML_ERR_OK as c_int,
                message: ptr::null_mut(),
                level: XML_ERR_NONE as c_int,
                file: ptr::null_mut(),
                line: 0,
                str1: ptr::null_mut(),
                str2: ptr::null_mut(),
                str3: ptr::null_mut(),
                int1: 0,
                int2: 0,
                ctxt: ptr::null_mut(),
                node: ptr::null_mut(),
            },
        );
    }
}

/// Reset the last error for the current thread.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlResetLastError(void);
/// ```
pub fn reset_last_error() {
    globals::reset_last_error();
}

/// Format an error message.
///
/// This function creates a formatted error message from the component parts.
/// In Phase 1, this is a basic implementation. In Phase 2+, variadic
/// printf-style formatting will be added.
///
/// Returns a C string pointer (allocated with xmlMalloc) that the caller
/// must free with xmlFreeImpl, or NULL on allocation failure.
///
/// # UPSTREAM-PARITY
///
/// Upstream libxml2 uses `vsnprintf` internally for message formatting.
/// We use a simple formatting approach that produces compatible output
/// for the common error patterns.
///
/// # Safety
///
/// - `msg` must be NULL or a valid NUL-terminated C string; `str1`, `str2`
///   and `str3` must be NULL or valid NUL-terminated C strings; the
///   returned buffer is allocator-owned and must be freed with
///   `xmlFreeImpl`, or is NULL on allocation failure.
pub fn format_error_message(
    _domain: c_int,
    _code: c_int,
    msg: *const c_char,
    str1: *const c_char,
    str2: *const c_char,
    str3: *const c_char,
) -> *mut c_char {
    // Phase 1: basic message construction.
    // If a direct message string is provided, use it.
    if !msg.is_null() {
        // SAFETY: Caller guarantees msg is a valid C string.
        let msg_str = unsafe { crate::abi::allocator::xmlMemStrdupImpl(msg) };
        return msg_str as *mut c_char;
    }

    // Build a message from the component strings.
    // This matches upstream behavior where domain/code are combined
    // with str1/str2/str3 into a diagnostic message.
    let mut buf: [u8; 1024] = [0; 1024];
    let mut pos = 0;

    // Write domain prefix
    let domain_str = match _domain {
        XML_FROM_PARSER => "parser",
        XML_FROM_TREE => "tree",
        XML_FROM_NAMESPACE => "namespace",
        XML_FROM_DTD => "dtd",
        XML_FROM_HTML => "html",
        XML_FROM_MEMORY => "memory",
        XML_FROM_OUTPUT => "output",
        XML_FROM_IO => "io",
        XML_FROM_XPATH => "xpath",
        XML_FROM_XPOINTER => "xpointer",
        XML_FROM_XINCLUDE => "xinclude",
        XML_FROM_CATALOG => "catalog",
        XML_FROM_C14N => "c14n",
        XML_FROM_XSLT => "xslt",
        XML_FROM_VALID => "valid",
        XML_FROM_CHECK => "check",
        XML_FROM_WRITER => "writer",
        XML_FROM_MODULE => "module",
        XML_FROM_I18N => "i18n",
        XML_FROM_SCHEMATRONV => "schematron",
        XML_FROM_BUFFER => "buffer",
        XML_FROM_URI => "uri",
        XML_FROM_NONE => "",
        XML_FROM_FTP => "ftp",
        XML_FROM_HTTP => "http",
        XML_FROM_REGEXP => "regexp",
        XML_FROM_DATATYPE => "datatype",
        XML_FROM_SCHEMASP => "schema parser",
        XML_FROM_SCHEMASV => "schema validator",
        XML_FROM_RELAXNGP => "relaxng parser",
        XML_FROM_RELAXNGV => "relaxng validator",
        _ => "unknown",
    };

    if !domain_str.is_empty() {
        let bytes = domain_str.as_bytes();
        let len = bytes.len().min(buf.len().saturating_sub(pos + 2));
        buf[pos..pos + len].copy_from_slice(&bytes[..len]);
        pos += len;
        buf[pos] = b' ';
        pos += 1;
    }

    // Append str1 if present
    if !str1.is_null() {
        // SAFETY: Caller guarantees str1 is a valid C string.
        let s = unsafe { crate::abi::versioning::c_str_to_bytes(str1).unwrap_or_default() };
        if pos + s.len() + 3 <= buf.len() {
            buf[pos] = b'\'';
            pos += 1;
            buf[pos..pos + s.len()].copy_from_slice(s);
            pos += s.len();
            buf[pos] = b'\'';
            pos += 1;
            buf[pos] = b' ';
            pos += 1;
        }
    }

    // Append str2 if present
    if !str2.is_null() {
        let s = unsafe { crate::abi::versioning::c_str_to_bytes(str2).unwrap_or_default() };
        if pos + s.len() + 3 <= buf.len() {
            buf[pos] = b'\'';
            pos += 1;
            buf[pos..pos + s.len()].copy_from_slice(s);
            pos += s.len();
            buf[pos] = b'\'';
            pos += 1;
            buf[pos] = b' ';
            pos += 1;
        }
    }

    // Append str3 if present
    if !str3.is_null() {
        let s = unsafe { crate::abi::versioning::c_str_to_bytes(str3).unwrap_or_default() };
        if pos + s.len() + 3 <= buf.len() {
            buf[pos] = b'\'';
            pos += 1;
            buf[pos..pos + s.len()].copy_from_slice(s);
            pos += s.len();
            buf[pos] = b'\'';
            pos += 1;
            buf[pos] = b' ';
            pos += 1;
        }
    }

    // Null-terminate
    if pos < buf.len() {
        buf[pos] = 0;
    } else {
        buf[buf.len() - 1] = 0;
    }

    // Allocate and return
    let result = unsafe { crate::abi::allocator::xmlMallocImpl(pos + 1) };
    if result.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::copy_nonoverlapping(buf.as_ptr(), result as *mut u8, pos + 1);
    }
    result as *mut c_char
}

/// Raise an error — the central error reporting function.
///
/// This is called internally when an error occurs. It:
/// 1. Updates the thread-local last error
/// 2. Invokes the structured error handler if one is set
/// 3. Invokes the generic error handler if one is set (for warnings/errors)
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlRaiseError(xmlErrorPtr ctxt,
///                    xmlErrorPtr ctxt2,
///                    xmlErrorPtr ctxt3,
///                    xmlErrorPtr ctxt4,
///                    xmlErrorPtr ctxt5,
///                    int domain,
///                    int code,
///                    xmlErrorLevel level,
///                    const char *file,
///                    int line,
///                    const char *str1,
///                    const char *str2,
///                    const char *str3,
///                    int int1,
///                    int int2,
///                    const char *msg,
///                    ...);
/// ```
///
/// # SAFETY
///
/// - `ctxt` may be NULL (context of the error).
/// - `domain`, `code`, `level`: valid error codes.
/// - `msg` must be a valid C string or NULL.
/// - `file` must be a valid C string or NULL.
/// - `str1`, `str2`, `str3`: error-related strings (may be NULL).
#[allow(clippy::too_many_arguments)]
pub unsafe fn raise_error(
    ctxt: *mut c_void,
    _ctxt2: *mut c_void,
    _ctxt3: *mut c_void,
    _ctxt4: *mut c_void,
    _ctxt5: *mut c_void,
    domain: c_int,
    code: c_int,
    level: c_int,
    file: *const c_char,
    line: c_int,
    str1: *const c_char,
    str2: *const c_char,
    str3: *const c_char,
    int1: c_int,
    _int2: c_int,
    msg: *const c_char,
) {
    // Format the error message
    let formatted_msg = format_error_message(domain, code, msg, str1, str2, str3);

    // UPSTREAM-PARITY (xmlVSetError): string fields are owned copies so the
    // stored error survives transient callers.
    let file_copy = if file.is_null() {
        ptr::null_mut()
    } else {
        crate::abi::allocator::xmlMemStrdupImpl(file) as *mut c_char
    };
    let str1_copy = if str1.is_null() {
        ptr::null_mut()
    } else {
        crate::abi::allocator::xmlMemStrdupImpl(str1) as *mut c_char
    };
    let str2_copy = if str2.is_null() {
        ptr::null_mut()
    } else {
        crate::abi::allocator::xmlMemStrdupImpl(str2) as *mut c_char
    };
    let str3_copy = if str3.is_null() {
        ptr::null_mut()
    } else {
        crate::abi::allocator::xmlMemStrdupImpl(str3) as *mut c_char
    };

    // Store the last error
    let err = _xmlError {
        domain,
        code,
        message: formatted_msg,
        level,
        file: file_copy,
        line,
        str1: str1_copy,
        str2: str2_copy,
        str3: str3_copy,
        int1,
        int2: 0,
        ctxt,
        node: ptr::null_mut(),
    };

    globals::set_last_error(err);

    // UPSTREAM-PARITY (xmlCtxtVErr): the structured handler wins; the
    // generic channel is only used when no structured handler is set. The
    // (handler, ctx) pairs are read atomically (11.1-X), then invoked
    // outside the lock so a handler that re-enters the library cannot
    // deadlock.
    let structured = globals::with_structured_error(|h, c| (h, c));
    if let Some(handler) = structured.0 {
        let err_ref = globals::get_last_error();
        if !err_ref.is_null() {
            handler(structured.1, err_ref as *const _xmlError);
        }
    } else if level != 0 {
        let generic = globals::with_generic_error(|h, c| (h, c));
        if let Some(handler) = generic.0 {
            let ctx = generic.1;
            if !formatted_msg.is_null() {
                handler(ctx, formatted_msg as *const core::ffi::c_char);
            } else if !msg.is_null() {
                handler(ctx, msg);
            }
        }
    }

    // Free the formatted message if it was allocated
    // Note: We keep it as the last error's message, so we don't free it here.
    // The next call to raise_error or reset_error will free the old message.
    // Actually, in Phase 1, we don't free because the message is the last error's.
    // A more complete implementation would free the old message when setting a new one.
}

/// Emit a legacy-format message through the generic error channel.
///
/// Upstream's `xmlGenericErrorDefaultFunc` writes the message to stderr;
/// when a custom generic handler is installed it receives the message. The
/// `level` prefix matches upstream `xmlVFormatLegacyError` (error.c 2.15).
unsafe fn emit_legacy_message(level: &str, msg: *const c_char) {
    if msg.is_null() {
        return;
    }
    let len = libc::strlen(msg) as usize;
    let text = core::slice::from_raw_parts(msg as *const u8, len);
    let mut full = Vec::with_capacity(level.len() + 2 + len);
    full.extend_from_slice(level.as_bytes());
    full.push(b':');
    full.push(b' ');
    full.extend_from_slice(text);
    if let Some(handler) = globals::get_generic_error_func() {
        let ctx = globals::get_generic_error_ctx();
        let mut cmsg = full.clone();
        cmsg.push(0);
        handler(ctx, cmsg.as_ptr() as *const c_char);
    } else {
        // Upstream default (xmlGenericErrorDefaultFunc): stderr.
        let _ = libc::write(2, full.as_ptr() as *const libc::c_void, full.len());
    }
}

/// System V AMD64 `__va_list_tag` (24 bytes) — same layout as the writer's
/// shims and `data_globals.rs`.
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
    /// Format `msg` with the caller's va_list into `s` (libc `vsnprintf`).
    ///
    /// # Safety
    ///
    /// - `s` must point to a writable buffer of at least `n` bytes;
    ///   `format` must be a valid NUL-terminated printf format string;
    ///   `ap` must be a valid va_list matching the format's specifiers.
    fn vsnprintf(s: *mut c_char, n: usize, format: *const c_char, ap: *mut VaListTag) -> c_int;
}

/// Format `msg` with the caller's varargs and emit `level + ": "` + the
/// formatted text through the generic channel (upstream error.c
/// `xmlVFormatLegacyError`: `xmlGenericError(ctx, "%s: ", level)` then the
/// `xmlStrVASPrintf`-formatted message).
///
/// # Safety
///
/// - `msg` must be NULL or a valid NUL-terminated printf format string;
///   `ap` must be a valid va_list matching the format's specifiers; the
///   format is consumed exactly once.
#[cfg(target_arch = "x86_64")]
unsafe fn emit_legacy_message_v(level: &str, msg: *const c_char, ap: *mut VaListTag) -> c_int {
    if msg.is_null() {
        return 0;
    }
    let mut buf = [0 as c_char; 4096];
    let n = unsafe { vsnprintf(buf.as_mut_ptr(), buf.len(), msg, ap) };
    let n = n.clamp(0, buf.len() as c_int - 1) as usize;
    buf[n] = 0;
    unsafe { emit_legacy_message(level, buf.as_ptr()) };
    0
}

/// x86_64 SysV variadic shim: captures the register save area exactly like
/// `va_start` (2 fixed args → `gp_offset` 16), builds the va_list at
/// rsp+176, passes it as the 3rd argument (rdx) to `receiver`, then restores
/// the stack and returns. Same technique as `xmlStrPrintf`
/// (exports_string.rs) and `xsltTransformError` (exports_xslt_util.rs).
///
/// Layout: reg_save_area = rsp+0 (6 GP + 8 SSE slots, 176 bytes); the
/// va_list struct lives at rsp+176; overflow varargs are above the return
/// address; a 240-byte frame keeps the `call` 16-aligned and the overflow
/// area at rsp+256 (= entry_rsp + 8); the alignment push is popped before
/// `ret`.
///
/// # Safety
///
/// - `receiver` must be a valid x86_64 SysV function pointer that accepts
///   `(ctx, msg, ap)` and consumes the va_list built in the fixed stack
///   slots; the frame layout described above must match the ABI.
#[cfg(target_arch = "x86_64")]
unsafe fn legacy_shim(
    receiver: unsafe extern "C" fn(*mut c_void, *const c_char, *mut VaListTag) -> c_int,
) -> c_int {
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
            "call {receiver}",
            "add rsp, 240",
            "add rsp, 8",
            "ret",
            receiver = in(reg) receiver as usize,
            options(noreturn),
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Generic-channel fragment streaming (upstream error.c `xmlFormatError`)
// ═══════════════════════════════════════════════════════════════════════════════
//
// Upstream streams each error through the generic channel as a sequence of
// variadic calls (e.g. `channel(data, "%s:%d: ", file, line)` followed by the
// domain, level, message and source-context fragments). Custom handlers and the
// built-in default (an x86_64 SysV va_list shim, see data_globals.rs) both
// observe the same per-fragment calls. Stable Rust cannot express a variadic
// call, so each fragment goes through a tiny x86_64 trampoline that places the
// fixed arguments in the ABI registers and does an indirect call.

/// `channel(data, fmt)` — no variadic arguments.
#[cfg(target_arch = "x86_64")]
#[inline]
unsafe fn ch_call0(handler: xmlGenericErrorFunc, data: *mut c_void, fmt: *const c_char) {
    // SAFETY: `handler` is a C-compatible generic error callback; per the
    // SysV ABI the callee sees (data, fmt) with no additional registers
    // consumed (rdx/rcx zeroed so a va_list-reading callee finds nothing).
    // The compiler guarantees 16-byte stack alignment at the asm block, so
    // the `call` is correctly aligned.
    unsafe {
        core::arch::asm!(
            "xor edx, edx",
            "xor ecx, ecx",
            "call {h}",
            h = in(reg) handler as usize,
            in("rdi") data,
            in("rsi") fmt,
            out("rdx") _, out("rcx") _,
            lateout("rax") _, lateout("r8") _, lateout("r9") _, lateout("r10") _, lateout("r11") _,
        );
    }
}

/// `channel(data, fmt, a1)` — one pointer-sized variadic argument.
#[cfg(target_arch = "x86_64")]
#[inline]
unsafe fn ch_call1(handler: xmlGenericErrorFunc, data: *mut c_void, fmt: *const c_char, a1: usize) {
    // SAFETY: as ch_call0; `a1` lands in the va_list slot after the two
    // fixed args (rdx).
    unsafe {
        core::arch::asm!(
            "xor ecx, ecx",
            "call {h}",
            h = in(reg) handler as usize,
            in("rdi") data,
            in("rsi") fmt,
            in("rdx") a1,
            out("rcx") _,
            lateout("rax") _, lateout("r8") _, lateout("r9") _, lateout("r10") _, lateout("r11") _,
        );
    }
}

/// `channel(data, fmt, a1, a2)` — two pointer-sized variadic arguments.
#[cfg(target_arch = "x86_64")]
#[inline]
unsafe fn ch_call2(
    handler: xmlGenericErrorFunc,
    data: *mut c_void,
    fmt: *const c_char,
    a1: usize,
    a2: usize,
) {
    // SAFETY: as ch_call0; a1/a2 land in the va_list slots (rdx, rcx).
    unsafe {
        core::arch::asm!(
            "call {h}",
            h = in(reg) handler as usize,
            in("rdi") data,
            in("rsi") fmt,
            in("rdx") a1,
            in("rcx") a2,
            lateout("rax") _, lateout("r8") _, lateout("r9") _, lateout("r10") _, lateout("r11") _,
        );
    }
}

/// Emit one raise through the generic channel with upstream's
/// `xmlFormatError` fragment sequence (error.c 2.15): file/line prefix,
/// domain, level, message, then the source window and caret line.
///
/// `file`/`line` come from the raising site's input; `source_window` is the
/// current input line text plus the 0-based caret column (upstream
/// `xmlParserInputGetWindow`).
///
/// # SAFETY
///
/// - `file` and `message` must be valid C strings or NULL.
/// - `source_window` bytes must be valid for the duration of the call.
#[allow(clippy::too_many_arguments)]
#[cfg(target_arch = "x86_64")]
unsafe fn format_error_streamed(
    domain: c_int,
    code: c_int,
    level: c_int,
    message: *const c_char,
    file: *const c_char,
    line: c_int,
    source_window: Option<(&[u8], usize)>,
    enc_bytes: Option<[u8; 4]>,
) {
    // SAFETY: reads the exported C globals (upstream reads the same).
    let Some(handler) = globals::get_generic_error_func() else {
        return;
    };
    let data = globals::get_generic_error_ctx();

    // 1. File/line prefix (xmlFormatError).
    if !file.is_null() {
        ch_call2(
            handler,
            data,
            c"%s:%d: ".as_ptr() as *const c_char,
            file as usize,
            line as usize,
        );
    } else if line != 0
        && (domain == XML_FROM_PARSER
            || domain == XML_FROM_SCHEMASV
            || domain == XML_FROM_SCHEMASP
            || domain == XML_FROM_DTD
            || domain == XML_FROM_RELAXNGP
            || domain == XML_FROM_RELAXNGV)
    {
        ch_call1(
            handler,
            data,
            c"Entity: line %d: ".as_ptr() as *const c_char,
            line as usize,
        );
    }

    // 2. Domain fragment (xmlFormatError switch).
    let dom: &[u8] = match domain {
        XML_FROM_PARSER => b"parser \0",
        XML_FROM_NAMESPACE => b"namespace \0",
        XML_FROM_DTD | XML_FROM_VALID => b"validity \0",
        XML_FROM_HTML => b"HTML parser \0",
        XML_FROM_MEMORY => b"memory \0",
        XML_FROM_OUTPUT => b"output \0",
        XML_FROM_IO => b"I/O \0",
        XML_FROM_XINCLUDE => b"XInclude \0",
        XML_FROM_XPATH => b"XPath \0",
        XML_FROM_XPOINTER => b"parser \0",
        XML_FROM_REGEXP => b"regexp \0",
        XML_FROM_MODULE => b"module \0",
        XML_FROM_SCHEMASV => b"Schemas validity \0",
        XML_FROM_SCHEMASP => b"Schemas parser \0",
        XML_FROM_RELAXNGP => b"Relax-NG parser \0",
        XML_FROM_RELAXNGV => b"Relax-NG validity \0",
        XML_FROM_CATALOG => b"Catalog \0",
        XML_FROM_C14N => b"C14N \0",
        XML_FROM_XSLT => b"XSLT \0",
        XML_FROM_I18N => b"encoding \0",
        XML_FROM_SCHEMATRONV => b"schematron \0",
        XML_FROM_BUFFER => b"internal buffer \0",
        XML_FROM_URI => b"URI \0",
        _ => b"\0",
    };
    if !dom.is_empty() && dom[0] != 0 {
        ch_call0(handler, data, dom.as_ptr() as *const c_char);
    }

    // 3. Level fragment (xmlFormatError switch).
    let lvl: &[u8] = if level == XML_ERR_NONE as c_int {
        b": \0"
    } else if level == XML_ERR_WARNING as c_int {
        b"warning : \0"
    } else if level == XML_ERR_ERROR as c_int || level == XML_ERR_FATAL as c_int {
        b"error : \0"
    } else {
        b"\0"
    };
    if !lvl.is_empty() && lvl[0] != 0 {
        ch_call0(handler, data, lvl.as_ptr() as *const c_char);
    }

    // 4. Message fragment.
    if !message.is_null() {
        let msg = message as *const u8;
        let mut len = 0usize;
        while unsafe { *msg.add(len) } != 0 {
            len += 1;
        }
        let ends_nl = len > 0 && unsafe { *msg.add(len - 1) } == b'\n';
        let fmt: &[u8] = if ends_nl { b"%s\0" } else { b"%s\n\0" };
        ch_call1(handler, data, fmt.as_ptr() as *const c_char, msg as usize);
    }

    // 4b. Invalid-encoding byte dump (upstream xmlFormatError: the first 4
    // bytes at the error position, only for XML_ERR_INVALID_ENCODING).
    if code == XML_ERR_INVALID_ENCODING {
        if let Some(bytes) = enc_bytes {
            ch_call0(handler, data, c"Bytes:".as_ptr() as *const c_char);
            for b in bytes {
                // " 0x%02X"
                let hex = format!(" 0x{:02X}\0", b);
                ch_call0(handler, data, hex.as_ptr() as *const c_char);
            }
            ch_call0(handler, data, c"\n".as_ptr() as *const c_char);
        }
    }

    // 5. Source window + caret (xmlParserPrintFileContextInternal).
    if let Some((window, caret)) = source_window {
        let mut win = window.to_vec();
        win.push(0);
        ch_call1(
            handler,
            data,
            c"%s\n".as_ptr() as *const c_char,
            win.as_ptr() as usize,
        );
        let mut caret_line = Vec::with_capacity(caret + 2);
        for &b in window.iter().take(caret) {
            caret_line.push(if b == b'\t' { b'\t' } else { b' ' });
        }
        caret_line.push(b'^');
        caret_line.push(0);
        ch_call1(
            handler,
            data,
            c"%s\n".as_ptr() as *const c_char,
            caret_line.as_ptr() as usize,
        );
    }
}

/// How a raise delivers to the generic side of the error system (upstream
/// `xmlVRaiseError` channel selection, error.c 2.15).
#[derive(Clone, Copy, Debug)]
pub enum GenericDelivery {
    /// Custom SAX channel: single call `channel(ctx, msg)`.
    Custom(xmlGenericErrorFunc, *mut c_void),
    /// Legacy/default channel: stream the `xmlFormatError` fragments through
    /// the global generic handler.
    Stream,
    /// No channel (SAX slot NULL): no generic delivery.
    None,
}

/// Raise an error with upstream's full routing (error.c 2.15
/// `xmlVRaiseError`): update the last error, then deliver to the structured
/// handler **or** the selected generic channel — never both.
///
/// `file`/`line`/`source_window` feed the generic fragment stream (the
/// structured handler receives the complete `xmlError` instead). `col` is
/// the 1-based byte column (upstream `input->col` → `err->int2`); `str1`..
/// `str3`/`int1` are the upstream extra fields; `enc_bytes` feeds the
/// `XML_ERR_INVALID_ENCODING` "Bytes:" fragment.
///
/// # UPSTREAM-PARITY (ownership)
///
/// Like upstream `xmlVSetError`, every string field of the stored error is
/// owned (`xmlStrdup`): `file`/`str1`/`str2`/`str3` are heap copies, so the
/// caller may pass transient C strings.
///
/// # SAFETY
///
/// - `ctxt` may be NULL.
/// - `msg`, `file`, `str1`, `str2`, `str3` must be valid C strings or NULL.
/// - `source_window` bytes must be valid for the duration of the call.
#[allow(clippy::too_many_arguments)]
pub unsafe fn raise_error_streamed(
    ctxt: *mut c_void,
    domain: c_int,
    code: c_int,
    level: c_int,
    file: *const c_char,
    line: c_int,
    col: c_int,
    str1: *const c_char,
    str2: *const c_char,
    str3: *const c_char,
    int1: c_int,
    msg: *const c_char,
    source_window: Option<(&[u8], usize)>,
    enc_bytes: Option<[u8; 4]>,
    delivery: GenericDelivery,
) {
    // The streamed generic-error channel below uses an x86_64 SysV va_list
    // trampoline (ch_call0/1/2 — register-based). Other ABIs (i686 cdecl,
    // ARM/aarch64 AAPCS, ...) fall back to the plain raise path; full
    // streamed-fragment parity there is an unexecuted platform obligation
    // (atlas/PLATFORM_SURFACE_ATLAS.md, OBLIG-WORDSIZE-32 / compiler-ABI).
    #[cfg(not(target_arch = "x86_64"))]
    {
        raise_error(
            ctxt,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            domain,
            code,
            level,
            file,
            line,
            str1,
            str2,
            str3,
            int1,
            col,
            msg,
        );
        return;
    }

    #[cfg(target_arch = "x86_64")]
    {
        // UPSTREAM-PARITY (xmlVRaiseError): warnings are suppressed when the
        // global xmlGetWarningsDefaultValue is zero.
        if level == xmlErrorLevel::XML_ERR_WARNING as c_int
            && unsafe { crate::abi::data_globals::xmlGetWarningsDefaultValue } == 0
        {
            return;
        }

        raise_error_streamed_x86_64(
            ctxt,
            domain,
            code,
            level,
            file,
            line,
            col,
            str1,
            str2,
            str3,
            int1,
            msg,
            source_window,
            enc_bytes,
            delivery,
        );
    }
}

/// x86-64 streamed raise (SysV va_list channel). See `raise_error_streamed`.
///
/// # Safety
///
/// - `ctxt` may be NULL; `msg`, `file`, `str1`, `str2`, `str3` must be
///   valid NUL-terminated C strings or NULL; `source_window` bytes must be
///   valid for the duration of the call; every string field is duplicated
///   before being stored in the thread-local last error.
#[allow(clippy::too_many_arguments)]
#[cfg(target_arch = "x86_64")]
unsafe fn raise_error_streamed_x86_64(
    ctxt: *mut c_void,
    domain: c_int,
    code: c_int,
    level: c_int,
    file: *const c_char,
    line: c_int,
    col: c_int,
    str1: *const c_char,
    str2: *const c_char,
    str3: *const c_char,
    int1: c_int,
    msg: *const c_char,
    source_window: Option<(&[u8], usize)>,
    enc_bytes: Option<[u8; 4]>,
    delivery: GenericDelivery,
) {
    // UPSTREAM-PARITY (xmlVRaiseError): warnings are suppressed when the
    // global xmlGetWarningsDefaultValue is zero.
    if level == xmlErrorLevel::XML_ERR_WARNING as c_int
        && unsafe { crate::abi::data_globals::xmlGetWarningsDefaultValue } == 0
    {
        return;
    }

    // Format the error message (same as raise_error).
    let formatted_msg = format_error_message(domain, code, msg, str1, str2, str3);
    let file_copy = if file.is_null() {
        ptr::null_mut()
    } else {
        crate::abi::allocator::xmlMemStrdupImpl(file) as *mut c_char
    };
    let str1_copy = if str1.is_null() {
        ptr::null_mut()
    } else {
        crate::abi::allocator::xmlMemStrdupImpl(str1) as *mut c_char
    };
    let str2_copy = if str2.is_null() {
        ptr::null_mut()
    } else {
        crate::abi::allocator::xmlMemStrdupImpl(str2) as *mut c_char
    };
    let str3_copy = if str3.is_null() {
        ptr::null_mut()
    } else {
        crate::abi::allocator::xmlMemStrdupImpl(str3) as *mut c_char
    };
    let err = _xmlError {
        domain,
        code,
        message: formatted_msg,
        level,
        file: file_copy,
        line,
        str1: str1_copy,
        str2: str2_copy,
        str3: str3_copy,
        int1,
        int2: col,
        ctxt,
        node: ptr::null_mut(),
    };

    globals::set_last_error(err);

    // Structured handler wins (upstream `else if` chain); the (handler, ctx)
    // pair is read atomically and invoked outside the lock (11.1-X).
    let structured = globals::with_structured_error(|h, c| (h, c));
    if let Some(handler) = structured.0 {
        let err_ref = globals::get_last_error();
        if !err_ref.is_null() {
            handler(structured.1, err_ref as *const _xmlError);
        }
        return;
    }

    match delivery {
        GenericDelivery::Custom(channel, ctx) => {
            if !msg.is_null() {
                // SAFETY: the caller provided a valid C callback.
                unsafe { channel(ctx, msg) };
            }
        }
        GenericDelivery::Stream => {
            if globals::get_generic_error_func().is_some() {
                unsafe {
                    format_error_streamed(
                        domain,
                        code,
                        level,
                        formatted_msg,
                        file,
                        line,
                        source_window,
                        enc_bytes,
                    )
                };
            }
        }
        GenericDelivery::None => {}
    }
}

/// Default SAX v1 error handler — upstream `void xmlParserError(void *ctx,
/// const char *msg, ...)`. Variadic x86_64 SysV shim (11.1-Z.2, R-000176:
/// the previous fixed-arity body silently dropped the varargs; upstream
/// error.c formats them via `xmlVFormatLegacyError`).
///
/// 2 fixed args (ctx=rdi, msg=rsi) → `gp_offset` 16; the va_list pointer is
/// passed as the 3rd arg (rdx) to the `xmlParserErrorV` receiver.
///
/// # SAFETY
///
/// - `ctx` may be NULL (unused by the candidate's legacy path).
/// - `msg` must be a valid NUL-terminated printf format string.
#[cfg(target_arch = "x86_64")]
#[no_mangle]
pub unsafe extern "C" fn xmlParserError() -> c_int {
    unsafe { legacy_shim(xmlParserErrorV) }
}

/// Variadic receiver for the `xmlParserError` shim: formats `msg` with the
/// caller's varargs and emits `"error: "` + formatted text through the
/// generic channel (upstream error.c `xmlVFormatLegacyError`).
///
/// # Safety
///
/// - `msg` must be a valid NUL-terminated printf format string, `ap` a
///   valid va_list matching the format, `ctx` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlParserErrorV(
    ctx: *mut c_void,
    msg: *const c_char,
    ap: *mut VaListTag,
) -> c_int {
    let _ = ctx;
    unsafe { emit_legacy_message_v("error", msg, ap) }
}

/// Default SAX v1 warning handler — upstream `void xmlParserWarning(void
/// *ctx, const char *msg, ...)`. Variadic shim as `xmlParserError`.
///
/// # SAFETY
///
/// - `ctx` may be NULL (unused by the candidate's legacy path).
/// - `msg` must be a valid NUL-terminated printf format string.
#[cfg(target_arch = "x86_64")]
#[no_mangle]
pub unsafe extern "C" fn xmlParserWarning() -> c_int {
    unsafe { legacy_shim(xmlParserWarningV) }
}

/// Variadic receiver for the `xmlParserWarning` shim.
///
/// # Safety
///
/// - `msg` must be a valid NUL-terminated printf format string, `ap` a
///   valid va_list matching the format, `ctx` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlParserWarningV(
    ctx: *mut c_void,
    msg: *const c_char,
    ap: *mut VaListTag,
) -> c_int {
    let _ = ctx;
    unsafe { emit_legacy_message_v("warning", msg, ap) }
}

/// Default validity error handler — upstream `void
/// xmlParserValidityError(void *ctx, const char *msg, ...)`. Variadic shim
/// as `xmlParserError`.
///
/// # SAFETY
///
/// - `ctx` may be NULL (unused by the candidate's legacy path).
/// - `msg` must be a valid NUL-terminated printf format string.
#[cfg(target_arch = "x86_64")]
#[no_mangle]
pub unsafe extern "C" fn xmlParserValidityError() -> c_int {
    unsafe { legacy_shim(xmlParserValidityErrorV) }
}

/// Variadic receiver for the `xmlParserValidityError` shim.
///
/// # Safety
///
/// - `msg` must be a valid NUL-terminated printf format string, `ap` a
///   valid va_list matching the format, `ctx` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlParserValidityErrorV(
    ctx: *mut c_void,
    msg: *const c_char,
    ap: *mut VaListTag,
) -> c_int {
    let _ = ctx;
    unsafe { emit_legacy_message_v("validity error", msg, ap) }
}

/// Default validity warning handler — upstream `void
/// xmlParserValidityWarning(void *ctx, const char *msg, ...)`. Variadic
/// shim as `xmlParserError`.
///
/// # SAFETY
///
/// - `ctx` may be NULL (unused by the candidate's legacy path).
/// - `msg` must be a valid NUL-terminated printf format string.
#[cfg(target_arch = "x86_64")]
#[no_mangle]
pub unsafe extern "C" fn xmlParserValidityWarning() -> c_int {
    unsafe { legacy_shim(xmlParserValidityWarningV) }
}

/// Variadic receiver for the `xmlParserValidityWarning` shim.
///
/// # Safety
///
/// - `msg` must be a valid NUL-terminated printf format string, `ap` a
///   valid va_list matching the format, `ctx` may be NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlParserValidityWarningV(
    ctx: *mut c_void,
    msg: *const c_char,
    ap: *mut VaListTag,
) -> c_int {
    let _ = ctx;
    unsafe { emit_legacy_message_v("validity warning", msg, ap) }
}

/// The variadic `xmlParserError` shim, transmuted to the fixed-arity SAX v1
/// callback type. The shim's declared arity is a Rust-side fiction (stable
/// Rust cannot express `c_variadic`); the ABI is a plain code pointer and
/// the C variadic call contract is preserved (11.1-Z.2, R-000176).
pub const XML_PARSER_ERROR_SAX1: errorSAXFunc = unsafe {
    // SAFETY: shim and SAX callback are both plain code pointers (same ABI);
    // the declared arity difference is the documented shim fiction.
    core::mem::transmute::<unsafe extern "C" fn() -> c_int, errorSAXFunc>(xmlParserError)
};

/// The variadic `xmlParserWarning` shim as the SAX v1 callback type.
pub const XML_PARSER_WARNING_SAX1: errorSAXFunc = unsafe {
    // SAFETY: see XML_PARSER_ERROR_SAX1.
    core::mem::transmute::<unsafe extern "C" fn() -> c_int, errorSAXFunc>(xmlParserWarning)
};

/// The variadic `xmlParserValidityError` shim as the validation callback
/// type (`xmlValidityErrorFunc`-compatible fixed-arity pointer).
pub const XML_PARSER_VALIDITY_ERROR_SAX1: unsafe extern "C" fn(*mut c_void, *const c_char) =
    // SAFETY: see XML_PARSER_ERROR_SAX1.
    unsafe {
        core::mem::transmute::<
            unsafe extern "C" fn() -> c_int,
            unsafe extern "C" fn(*mut c_void, *const c_char),
        >(xmlParserValidityError)
    };

/// The variadic `xmlParserValidityWarning` shim as the validation callback
/// type.
pub const XML_PARSER_VALIDITY_WARNING_SAX1: unsafe extern "C" fn(*mut c_void, *const c_char) =
    // SAFETY: see XML_PARSER_ERROR_SAX1.
    unsafe {
        core::mem::transmute::<
            unsafe extern "C" fn() -> c_int,
            unsafe extern "C" fn(*mut c_void, *const c_char),
        >(xmlParserValidityWarning)
    };

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::allocator;
    use core::ffi::c_void;

    /// Reset an error struct and verify the defaults are applied.
    ///
    /// # Safety
    ///
    /// - `err` is a stack `_xmlError` whose fields are all NULL/zero; it
    ///   is valid for `reset_error` to write and for the subsequent reads.
    #[test]
    fn test_error_default_reset() {
        unsafe {
            let mut err = _xmlError {
                domain: XML_FROM_PARSER,
                code: XML_ERR_NO_MEMORY,
                message: ptr::null_mut(),
                level: XML_ERR_ERROR as c_int,
                file: ptr::null_mut(),
                line: 42,
                str1: ptr::null_mut(),
                str2: ptr::null_mut(),
                str3: ptr::null_mut(),
                int1: 0,
                int2: 0,
                ctxt: ptr::null_mut(),
                node: ptr::null_mut(),
            };

            reset_error(&mut err);
            assert_eq!(err.domain, XML_FROM_NONE);
            assert_eq!(err.code, XML_ERR_OK as c_int);
            assert_eq!(err.level, XML_ERR_NONE as c_int);
            assert_eq!(err.line, 0);
        }
    }

    /// Copy one error struct into another and verify the fields.
    ///
    /// # Safety
    ///
    /// - `from` and `to` are stack `_xmlError` structs valid for the
    ///   `copy_error` copy and the subsequent field reads.
    #[test]
    fn test_copy_error() {
        unsafe {
            let from = _xmlError {
                domain: XML_FROM_PARSER,
                code: XML_ERR_NO_MEMORY,
                message: ptr::null_mut(),
                level: XML_ERR_FATAL as c_int,
                file: ptr::null_mut(),
                line: 100,
                str1: ptr::null_mut(),
                str2: ptr::null_mut(),
                str3: ptr::null_mut(),
                int1: 1,
                int2: 2,
                ctxt: ptr::null_mut(),
                node: ptr::null_mut(),
            };
            let mut to = _xmlError {
                domain: XML_FROM_NONE,
                code: XML_ERR_OK as c_int,
                message: ptr::null_mut(),
                level: XML_ERR_NONE as c_int,
                file: ptr::null_mut(),
                line: 0,
                str1: ptr::null_mut(),
                str2: ptr::null_mut(),
                str3: ptr::null_mut(),
                int1: 0,
                int2: 0,
                ctxt: ptr::null_mut(),
                node: ptr::null_mut(),
            };

            let result = copy_error(&from, &mut to);
            assert_eq!(result, 0);
            assert_eq!(to.domain, XML_FROM_PARSER);
            assert_eq!(to.code, XML_ERR_NO_MEMORY);
            assert_eq!(to.level, XML_ERR_FATAL as c_int);
            assert_eq!(to.line, 100);
            assert_eq!(to.int1, 1);
            assert_eq!(to.int2, 2);
        }
    }

    /// Raise an error and verify it is stored as the last error.
    ///
    /// # Safety
    ///
    /// - `file`/`str1` are static NUL-terminated strings valid for the
    ///   raise; `raise_error` duplicates them, so the test's later reads of
    ///   `last` only touch the thread-local copy; `reset_last_error`
    ///   releases the owned strings exactly once.
    #[test]
    fn test_raise_and_get_last_error() {
        unsafe {
            reset_last_error();
            assert!(get_last_error().is_null());

            let file = b"test.xml\0" as *const u8 as *const c_char;
            let str1 = b"element\0" as *const u8 as *const c_char;

            raise_error(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                XML_FROM_PARSER,
                XML_ERR_TAG_NAME_MISMATCH,
                XML_ERR_ERROR as c_int,
                file,
                10,
                str1,
                ptr::null(),
                ptr::null(),
                0,
                0,
                ptr::null(),
            );

            let last = get_last_error();
            assert!(!last.is_null());
            assert_eq!((*last).domain, XML_FROM_PARSER);
            assert_eq!((*last).code, XML_ERR_TAG_NAME_MISMATCH);
            assert_eq!((*last).level, XML_ERR_ERROR as c_int);
            assert_eq!((*last).line, 10);

            // Check file was stored
            let last_file = (*last).file;
            assert!(!last_file.is_null());

            reset_last_error();
            assert!(get_last_error().is_null());
        }
    }

    #[test]
    fn test_structured_error_callback() {
        // Serialized against the handler-slot tests in xml::globals (11.1-X):
        // the structured handler slot is shared global state.
        let _guard = crate::xml::globals::ERROR_HANDLER_TEST_LOCK.lock();
        unsafe {
            reset_last_error();

            // Set up a structured error handler that captures the error
            let mut captured_domain: c_int = 0;
            let captured_ptr = &mut captured_domain as *mut c_int as *mut c_void;

            // SAFETY: The callback writes to captured_ptr which lives on the stack
            // for the duration of this test.
            extern "C" fn test_handler(ctx: *mut c_void, _err: *const _xmlError) {
                // SAFETY: ctx is valid for the test duration.
                unsafe {
                    let captured = &mut *(ctx as *mut c_int);
                    *captured = 42;
                }
            }

            set_structured_error_func(captured_ptr, Some(test_handler as xmlStructuredErrorFunc));

            raise_error(
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                XML_FROM_PARSER,
                XML_ERR_OK as c_int,
                XML_ERR_WARNING as c_int,
                ptr::null(),
                0,
                ptr::null(),
                ptr::null(),
                ptr::null(),
                0,
                0,
                ptr::null(),
            );

            assert_eq!(captured_domain, 42);

            // Reset
            set_structured_error_func(ptr::null_mut(), None);
            reset_last_error();
        }
    }

    /// Format messages with a direct `msg` and with domain plus `str1`.
    ///
    /// # Safety
    ///
    /// - `msg`/`str1` are static NUL-terminated strings valid for the
    ///   calls; each returned buffer is allocator-owned and freed with
    ///   `xmlFreeImpl` exactly once before the test ends.
    #[test]
    fn test_format_error_message() {
        unsafe {
            // Test with direct message
            let msg = b"test error\0" as *const u8 as *const c_char;
            let formatted = format_error_message(
                XML_FROM_NONE,
                XML_ERR_OK as c_int,
                msg,
                ptr::null(),
                ptr::null(),
                ptr::null(),
            );
            assert!(!formatted.is_null());
            let formatted_str = std::ffi::CStr::from_ptr(formatted);
            assert_eq!(formatted_str.to_bytes(), b"test error");

            // Free the allocated message
            allocator::xmlFreeImpl(formatted as *mut c_void);

            // Test with domain and str1
            let str1 = b"foo\0" as *const u8 as *const c_char;
            let formatted2 = format_error_message(
                XML_FROM_PARSER,
                XML_ERR_OK as c_int,
                ptr::null(),
                str1,
                ptr::null(),
                ptr::null(),
            );
            assert!(!formatted2.is_null());
            allocator::xmlFreeImpl(formatted2 as *mut c_void);
        }
    }
}
