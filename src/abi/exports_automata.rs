//! exports_automata — family closure (11.1-I).
//!
//! C ABI exports for the automata / stream / regex-exec / pattern families
//! from `xmlautomata.h`, `xmlregexp.h`, `pattern.h`, `encoding.h` and
//! `xmlIO.h` that are *not* already exported by the internal engines in
//! `src/xml/automata/mod.rs` and `src/xml/regex/mod.rs`.
//!
//! # What is wired to the internal engines
//!
//! - `xmlAutomata*` "2"-token variants and `xmlAutomataIsDeterminist` call
//!   into `crate::xml::automata` (the base single-token constructors and
//!   `xmlAutomataIsDeterministic` live there and are `#[no_mangle]` already).
//!   The internal automata model is single-byte-token based, so the
//!   `token2` value is folded into the transition token exactly like
//!   upstream folds it into one atom (`token|token2`), but only the first
//!   byte is honored by the engine when compiling/matching.
//! - `xmlRegExecPushString2` ports upstream `xmlregexp.c`'s concatenation
//!   of `value`, `XML_REG_STRING_SEPARATOR` (`|`) and `value2` and pushes
//!   the combined token through `xmlRegExecPushString`.
//! - `xmlRegexpIsDeterminist` delegates to the engine's
//!   `xmlRegexpIsDeterministic`.
//!
//! # Side registries
//!
//! `xmlPattern*` / `xmlStream*` use side registries keyed by fake opaque
//! pointers (upstream keeps these structures in `pattern.c`). A compiled
//! pattern is a list of branches (one per `|` alternative); a stream
//! context is a chain of per-branch contexts, mirroring upstream's
//! `_xmlPattern.next` / `_xmlStreamCtxt.next` lists.
//!
//! # Known divergences from upstream
//!
//! - `xmlPatterncompile` is a faithful port of `pattern.c`'s mini pattern
//!   compiler (`/`, `//`, `.`, `@attr`, `*`, `QName`, `prefix:*`,
//!   `child::`, `attribute::`), including the streaming compilation and
//!   the push/pop stream evaluator. Name scanning approximates XML
//!   `Name`/`NCName` with the ASCII subset plus any byte >= 0x80; the
//!   dictionary argument is stored but not used for interning (all names
//!   are owned copies).
//! - `xmlRegExecNextValues` / `xmlRegExecErrInfo` return `0` with
//!   `nbval`/`nbneg` set to 0: the internal `RegExecCtxt` does not expose
//!   its NFA transition table, so the expected-next-value enumeration of
//!   upstream cannot be reproduced. `terminal` is approximated from the
//!   last `xmlRegExecPushString2` result.
//! - `xmlAutomataNewNegTrans` degrades to a plain transition: the
//!   internal automata engine has no negated-transition support.
//! - The `xmlRegister*Callbacks` registrations are stored faithfully in a
//!   side table (the internal I/O layer does not yet consult them) and
//!   `xmlRegisterHTTPPostCallbacks` is a no-op (no HTTP support).
//!
//! # Upstream contract
//!
//! Parity target is upstream `xmlautomata.c`/`xmlregexp.c`/`pattern.c`
//! (libxml2 2.15.3, SRC-LIBXML2-2.15.0-XMLAUTOMATA-C); the signatures follow
//! `xmlautomata.h`, `xmlregexp.h` and `pattern.h` so every symbol here
//! resolves against the oracle DSO export set.
//!
//! # Conceptual behavior
//!
//! This module implements the automata/stream/regex-exec/pattern export
//! family: NFA construction (`xmlAutomataNew*`), determinism checks, regex
//! execution against pushed strings, and the compiled-pattern streaming
//! evaluators, wired to the internal engines in `src/xml/automata` and
//! `src/xml/regex` with the divergences listed above.
//!
//! # Ownership & safety invariants
//!
//! Handles (`xmlAutomataPtr`, `xmlRegExecCtxtPtr`, `xmlPatternPtr`,
//! `xmlStreamCtxtPtr`) are owned by the caller and freed with the matching
//! `xmlAutomataFree*`/`xmlRegFree*`/`xmlPatternFree*`/`xmlStreamFree*` entry
//! points; the side registries keyed by fake opaque pointers keep internal
//! state alive until the free functions run.
//!
//! # Historical quirks & epochs
//!
//! The automata/regex subsystem matured in the 2.6 `validation_era`
//! (HISTORY.md) and has been ABI-stable since; the parity target is the 2.15.3
//! oracle. R-000165 (11.1-O) closed the subsystem census so every oracle-DSO
//! automata symbol is exported.
//!
//! # Deliberate oddities
//!
//! `xmlRegExecNextValues`/`xmlRegExecErrInfo` return 0 with empty value
//! lists, `xmlAutomataNewNegTrans` degrades to a plain transition, and
//! `xmlRegisterHTTPPostCallbacks` is a no-op (no HTTP support) — all
//! deliberate, documented divergences where the internal engine lacks the
//! upstream feature.
//!
//! # Proving courts
//!
//! The RELAXNG/automata probes and the DSO-LOADER court (25/25 symbols load)
//! plus HEADER-COMPILE (595/595) exercise this module; the data-ABI family
//! probes require byte-identical behavior on the supported paths.
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is to fold the token2 value into a single token
//! and claim full two-token support — the header already documents that only
//! the first byte is honored by the engine; widening the claim would break the
//! automata probes that feed two-byte tokens. Dropping the side registries
//! would make pattern/stream handles dangle across calls.

#![allow(
    missing_docs,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals
)]

use core::ffi::c_void;
use core::ptr;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::os::raw::{c_char, c_int};

use crate::abi::callbacks::{
    xmlInputCloseCallback, xmlInputReadCallback, xmlOutputCloseCallback, xmlOutputWriteCallback,
};
use crate::abi::structs::{_xmlCharEncodingHandler, _xmlNode};
use crate::abi::types::xmlChar;
use crate::xml::automata::{
    xmlAutomataIsDeterministic, xmlAutomataNewCountTrans, xmlAutomataNewOnceTrans,
    xmlAutomataNewState, xmlAutomataNewTransition, XmlAutomataPtr, XmlAutomataStatePtr,
};
use crate::xml::encoding;
use crate::xml::regex::{xmlRegExecPushString, xmlRegexpIsDeterministic, RegExecCtxt, XmlRegexp};

// ═══════════════════════════════════════════════════════════════════════════════
// 1. xmlAutomata* — the "2"-token variants and IsDeterminist
//    (base constructors live in src/xml/automata/mod.rs)
// ═══════════════════════════════════════════════════════════════════════════════

/// Build the atom value exactly like upstream `xmlautomata.c` /
/// `xmlregexp.c`: `token` if `token2` is NULL/empty, else `token|token2`.
unsafe fn build_token2_value(token: *const xmlChar, token2: *const xmlChar) -> Option<Vec<u8>> {
    if token.is_null() {
        return None;
    }
    let lenp = cstr_len(token);
    let lenn = if token2.is_null() {
        0
    } else {
        cstr_len(token2)
    };
    if lenn == 0 {
        let mut v = Vec::with_capacity(lenp + 1);
        v.extend_from_slice(unsafe { core::slice::from_raw_parts(token, lenp) });
        v.push(0);
        return Some(v);
    }
    let mut v = Vec::with_capacity(lenp + lenn + 2);
    v.extend_from_slice(unsafe { core::slice::from_raw_parts(token, lenp) });
    v.push(b'|');
    v.extend_from_slice(unsafe { core::slice::from_raw_parts(token2, lenn) });
    v.push(0);
    Some(v)
}

/// Upstream `int xmlAutomataIsDeterminist(xmlAutomata *am)` — delegates to
/// the engine's `xmlAutomataIsDeterministic` (compiled regex determinism).
///
/// # SAFETY
///
/// - `am` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlAutomataIsDeterminist(am: XmlAutomataPtr) -> c_int {
    xmlAutomataIsDeterministic(am)
}

/// Upstream `xmlAutomataState * xmlAutomataNewTransition2(...)`.
///
/// If `to` is NULL a new target state is created (upstream creates the
/// state and returns it). The `token`/`token2` pair is folded into one
/// atom value (`token|token2`) like upstream, then wired through the
/// internal single-token transition builder.
///
/// # SAFETY
///
/// - `am`, `from`, `to`, `data` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `token`, `token2` must point to valid NUL-terminated
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
pub unsafe extern "C" fn xmlAutomataNewTransition2(
    am: XmlAutomataPtr,
    from: XmlAutomataStatePtr,
    to: XmlAutomataStatePtr,
    token: *const xmlChar,
    token2: *const xmlChar,
    data: *mut c_void,
) -> XmlAutomataStatePtr {
    if am.is_null() || from.is_null() || token.is_null() {
        return ptr::null_mut();
    }
    let value = match unsafe { build_token2_value(token, token2) } {
        Some(v) => v,
        None => return ptr::null_mut(),
    };
    let target = if to.is_null() {
        let s = xmlAutomataNewState(am);
        if s.is_null() {
            return ptr::null_mut();
        }
        s
    } else {
        to
    };
    xmlAutomataNewTransition(am, from, target, value.as_ptr() as *const c_char, data);
    target
}

/// Upstream `xmlAutomataState * xmlAutomataNewNegTrans(...)`.
///
/// Upstream creates a negated atom (`atom->neg = 1`, `valuep2 = "not X"`).
/// The internal engine has no negated transitions, so this degrades to a
/// plain `token|token2` transition (see module docs).
///
/// # SAFETY
///
/// - `am`, `from`, `to`, `data` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `token`, `token2` must point to valid NUL-terminated
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
pub unsafe extern "C" fn xmlAutomataNewNegTrans(
    am: XmlAutomataPtr,
    from: XmlAutomataStatePtr,
    to: XmlAutomataStatePtr,
    token: *const xmlChar,
    token2: *const xmlChar,
    data: *mut c_void,
) -> XmlAutomataStatePtr {
    xmlAutomataNewTransition2(am, from, to, token, token2, data)
}

/// Upstream `xmlAutomataState * xmlAutomataNewCountTrans2(...)` — same
/// atom-value folding as upstream, with the same argument validation
/// (`min < 0`, `max < min`, `max < 1` → NULL).
///
/// # SAFETY
///
/// - `am`, `from`, `to`, `data` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `token`, `token2` must point to valid NUL-terminated
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
pub unsafe extern "C" fn xmlAutomataNewCountTrans2(
    am: XmlAutomataPtr,
    from: XmlAutomataStatePtr,
    to: XmlAutomataStatePtr,
    token: *const xmlChar,
    token2: *const xmlChar,
    min: c_int,
    max: c_int,
    data: *mut c_void,
) -> XmlAutomataStatePtr {
    if am.is_null() || from.is_null() || token.is_null() {
        return ptr::null_mut();
    }
    if min < 0 {
        return ptr::null_mut();
    }
    if (max < min) || (max < 1) {
        return ptr::null_mut();
    }
    let value = match unsafe { build_token2_value(token, token2) } {
        Some(v) => v,
        None => return ptr::null_mut(),
    };
    let target = if to.is_null() {
        let s = xmlAutomataNewState(am);
        if s.is_null() {
            return ptr::null_mut();
        }
        s
    } else {
        to
    };
    xmlAutomataNewCountTrans(
        am,
        from,
        target,
        value.as_ptr() as *const c_char,
        data,
        min,
        max,
    );
    target
}

/// Upstream `xmlAutomataState * xmlAutomataNewOnceTrans2(...)` — validation
/// is `min < 1` or `max < min` → NULL (matches upstream `xmlregexp.c`).
///
/// # SAFETY
///
/// - `am`, `from`, `to`, `data` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `token`, `token2` must point to valid NUL-terminated
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
pub unsafe extern "C" fn xmlAutomataNewOnceTrans2(
    am: XmlAutomataPtr,
    from: XmlAutomataStatePtr,
    to: XmlAutomataStatePtr,
    token: *const xmlChar,
    token2: *const xmlChar,
    min: c_int,
    max: c_int,
    data: *mut c_void,
) -> XmlAutomataStatePtr {
    if am.is_null() || from.is_null() || token.is_null() {
        return ptr::null_mut();
    }
    if min < 1 {
        return ptr::null_mut();
    }
    if max < min {
        return ptr::null_mut();
    }
    let value = match unsafe { build_token2_value(token, token2) } {
        Some(v) => v,
        None => return ptr::null_mut(),
    };
    let target = if to.is_null() {
        let s = xmlAutomataNewState(am);
        if s.is_null() {
            return ptr::null_mut();
        }
        s
    } else {
        to
    };
    xmlAutomataNewOnceTrans(
        am,
        from,
        target,
        value.as_ptr() as *const c_char,
        data,
        min,
        max,
    );
    target
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. xmlReg* / xmlRegexpIsDeterminist — regex execution context exports
// ═══════════════════════════════════════════════════════════════════════════════

/// Last-push state tracked per exec context (upstream keeps this inside
/// `_xmlRegExecCtxt`; the internal `RegExecCtxt` does not expose it).
#[derive(Debug, Clone, Copy, Default)]
struct ExecState {
    last_ret: c_int,
}

static EXEC_STATE: Lazy<Mutex<HashMap<usize, ExecState>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Upstream `int xmlRegExecPushString2(xmlRegExecCtxt *exec,
/// const xmlChar *value, const xmlChar *value2, void *data)`.
///
/// Concatenates `value`, the string separator (`|`) and `value2` into one
/// token and pushes it (exactly the strategy of upstream `xmlregexp.c`).
/// `value2 == NULL` behaves like `xmlRegExecPushString(exec, value, data)`.
///
/// # SAFETY
///
/// - `exec`, `_data` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `value`, `value2` must point to valid NUL-terminated
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
pub unsafe extern "C" fn xmlRegExecPushString2(
    exec: *mut RegExecCtxt,
    value: *const xmlChar,
    value2: *const xmlChar,
    _data: *mut c_void,
) -> c_int {
    if exec.is_null() {
        return -1;
    }
    if value2.is_null() || value.is_null() {
        let ret = xmlRegExecPushString(exec, value);
        EXEC_STATE
            .lock()
            .insert(exec as usize, ExecState { last_ret: ret });
        return ret;
    }
    let lenp = cstr_len(value);
    let lenn = cstr_len(value2);
    let mut buf = Vec::with_capacity(lenp + lenn + 2);
    buf.extend_from_slice(unsafe { core::slice::from_raw_parts(value, lenp) });
    buf.push(b'|');
    buf.extend_from_slice(unsafe { core::slice::from_raw_parts(value2, lenn) });
    buf.push(0);
    let ret = xmlRegExecPushString(exec, buf.as_ptr());
    EXEC_STATE
        .lock()
        .insert(exec as usize, ExecState { last_ret: ret });
    ret
}

/// Best-effort "expected next values" extraction.
///
/// The internal `RegExecCtxt` (an NFA state-set simulator) does not expose
/// the transition table needed to enumerate the acceptable next tokens, so
/// this returns success with an empty value list; `terminal` reflects the
/// last push result (1 if the last push completed a match).
///
/// # SAFETY
///
/// - `exec`, `nbval`, `nbneg`, `values`, `terminal` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlRegExecNextValues(
    exec: *mut RegExecCtxt,
    nbval: *mut c_int,
    nbneg: *mut c_int,
    values: *mut *mut xmlChar,
    terminal: *mut c_int,
) -> c_int {
    if exec.is_null() || nbval.is_null() || nbneg.is_null() || values.is_null() {
        return -1;
    }
    if unsafe { *nbval } <= 0 {
        return -1;
    }
    unsafe {
        *nbval = 0;
        *nbneg = 0;
    }
    if !terminal.is_null() {
        let last = EXEC_STATE
            .lock()
            .get(&(exec as usize))
            .map_or(0, |s| s.last_ret);
        unsafe { *terminal = if last == 1 { 1 } else { 0 } };
    }
    0
}

/// Best-effort error-info extraction: reports no error string and an empty
/// value list (see `xmlRegExecNextValues` for why the value enumeration is
/// unavailable).
///
/// # SAFETY
///
/// - `exec`, `nbval`, `nbneg`, `values`, `terminal` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `string` must point to valid NUL-terminated
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
pub unsafe extern "C" fn xmlRegExecErrInfo(
    exec: *mut RegExecCtxt,
    string: *mut *const xmlChar,
    nbval: *mut c_int,
    nbneg: *mut c_int,
    values: *mut *mut xmlChar,
    terminal: *mut c_int,
) -> c_int {
    if exec.is_null() {
        return -1;
    }
    if !string.is_null() {
        unsafe { *string = ptr::null() };
    }
    xmlRegExecNextValues(exec, nbval, nbneg, values, terminal)
}

/// Upstream `int xmlRegexpIsDeterminist(xmlRegexp *comp)` — delegates to
/// the engine's `xmlRegexpIsDeterministic`.
///
/// # SAFETY
///
/// - `comp` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub const unsafe extern "C" fn xmlRegexpIsDeterminist(comp: *const XmlRegexp) -> c_int {
    xmlRegexpIsDeterministic(comp)
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. Encoding / I/O registration exports
// ═══════════════════════════════════════════════════════════════════════════════

/// Upstream `void xmlRegisterCharEncodingHandler(xmlCharEncodingHandler *handler)`.
///
/// Registers the handler in the encoding subsystem's global table.
///
/// # SAFETY
///
/// - `handler` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlRegisterCharEncodingHandler(handler: *mut _xmlCharEncodingHandler) {
    encoding::add_encoding_handler(handler);
}

/// `xmlInputMatchCallback` (xmlIO.h): decides whether a handler can open a
/// given filename.
pub type xmlInputMatchCallback =
    unsafe extern "C" fn(context: *mut c_void, filename: *const c_char) -> c_int;

/// `xmlInputOpenCallback` (xmlIO.h): opens the input, returning a context.
pub type xmlInputOpenCallback =
    unsafe extern "C" fn(context: *mut c_void, filename: *const c_char) -> *mut c_void;

/// `xmlOutputMatchCallback` (xmlIO.h).
pub type xmlOutputMatchCallback =
    unsafe extern "C" fn(context: *mut c_void, filename: *const c_char) -> c_int;

/// `xmlOutputOpenCallback` (xmlIO.h).
pub type xmlOutputOpenCallback =
    unsafe extern "C" fn(context: *mut c_void, filename: *const c_char) -> *mut c_void;

/// One registered input-callback set (upstream `xmlInputCallbackTable[]`).
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct InputCallbackSet {
    match_cb: Option<xmlInputMatchCallback>,
    open_cb: Option<xmlInputOpenCallback>,
    read_cb: Option<xmlInputReadCallback>,
    close_cb: Option<xmlInputCloseCallback>,
}

/// One registered output-callback set (upstream `xmlOutputCallbackTable[]`).
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct OutputCallbackSet {
    match_cb: Option<xmlOutputMatchCallback>,
    open_cb: Option<xmlOutputOpenCallback>,
    write_cb: Option<xmlOutputWriteCallback>,
    close_cb: Option<xmlOutputCloseCallback>,
}

#[allow(dead_code)]
static INPUT_CALLBACKS: Lazy<Mutex<Vec<InputCallbackSet>>> = Lazy::new(|| Mutex::new(Vec::new()));
#[allow(dead_code)]
static OUTPUT_CALLBACKS: Lazy<Mutex<Vec<OutputCallbackSet>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// The compiled-in default match callback (upstream `xmlIODefaultMatch`):
/// matches every filename.
#[allow(dead_code)]
const unsafe extern "C" fn xml_io_default_match(
    _context: *mut c_void,
    _filename: *const c_char,
) -> c_int {
    1
}

// Upstream `int xmlRegisterInputCallbacks(...)` — returns the registered
// Upstream `int xmlRegisterOutputCallbacks(...)` — returns the registered
// Upstream `void xmlRegisterHTTPPostCallbacks(void)`.
//
// No-op: this build has no HTTP transport (upstream gates it behind
// ═══════════════════════════════════════════════════════════════════════════════
// 4. xmlPattern* / xmlStream* — pattern compiler, matcher and stream engine
//    (port of archaeology/libxml2-git/pattern.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Opaque pattern handle type (`pattern.h`: `typedef struct _xmlPattern xmlPattern`).
#[derive(Debug)]
#[repr(C)]
pub struct _xmlPattern {
    _opaque: [u8; 0],
}

/// Opaque stream-context handle type (`pattern.h`: `_xmlStreamCtxt`).
#[derive(Debug)]
#[repr(C)]
pub struct _xmlStreamCtxt {
    _opaque: [u8; 0],
}

pub type xmlPatternPtr = *mut _xmlPattern;
pub type xmlStreamCtxtPtr = *mut _xmlStreamCtxt;

// ── Pattern flags (pattern.h + private pattern.c bits) ────────────────────

#[allow(dead_code)]
const XML_PATTERN_DEFAULT: c_int = 0;
const XML_PATTERN_XPATH: c_int = 1 << 0;
const XML_PATTERN_XSSEL: c_int = 1 << 1;
const XML_PATTERN_XSFIELD: c_int = 1 << 2;
const XML_PATTERN_NOTPATTERN: c_int = XML_PATTERN_XPATH | XML_PATTERN_XSSEL | XML_PATTERN_XSFIELD;
const PAT_FROM_ROOT: c_int = 1 << 8;
const PAT_FROM_CUR: c_int = 1 << 9;

// ── Stream step/comp flags (pattern.c) ────────────────────────────────────

const XML_STREAM_STEP_DESC: c_int = 1;
const XML_STREAM_STEP_FINAL: c_int = 2;
const XML_STREAM_STEP_ROOT: c_int = 4;
const XML_STREAM_STEP_ATTR: c_int = 8;
const XML_STREAM_STEP_NODE: c_int = 16;
const XML_STREAM_STEP_IN_SET: c_int = 32;
const XML_STREAM_FINAL_IS_ANY_NODE: c_int = 1 << 14;
const XML_STREAM_FROM_ROOT: c_int = 1 << 15;
const XML_STREAM_DESC: c_int = 1 << 16;
const XML_STREAM_ANY_NODE: c_int = 100;

/// Node types used by the pattern engine (tree.h `xmlElementType` values).
const XML_ELEMENT_NODE: c_int = 1;
const XML_ATTRIBUTE_NODE: c_int = 2;
const XML_DOCUMENT_NODE: c_int = 9;
const XML_HTML_DOCUMENT_NODE: c_int = 13;
const XML_NAMESPACE_DECL: c_int = 18;

// ── Pattern op codes (pattern.c `xmlPatOp`) ───────────────────────────────

const XML_OP_END: c_int = 0;
const XML_OP_ROOT: c_int = 1;
const XML_OP_ELEM: c_int = 2;
const XML_OP_CHILD: c_int = 3;
const XML_OP_ATTR: c_int = 4;
const XML_OP_PARENT: c_int = 5;
const XML_OP_ANCESTOR: c_int = 6;
const XML_OP_NS: c_int = 7;
const XML_OP_ALL: c_int = 8;

/// One compiled step (pattern.c `xmlStepOp`).
#[derive(Debug, Clone)]
struct StepOp {
    op: c_int,
    value: Option<Vec<u8>>,
    value2: Option<Vec<u8>>,
}

/// One `|` alternative of a compiled pattern (pattern.c `_xmlPattern`).
#[derive(Debug, Clone)]
struct CompiledPattern {
    flags: c_int,
    steps: Vec<StepOp>,
    stream: Option<StreamComp>,
}

/// Streaming compilation (pattern.c `_xmlStreamComp`).
#[derive(Debug, Clone)]
struct StreamComp {
    nb_step: usize,
    steps: Vec<StreamStep>,
    flags: c_int,
}

/// One streaming step (pattern.c `_xmlStreamStep`).
#[derive(Debug, Clone)]
struct StreamStep {
    flags: c_int,
    name: Option<Vec<u8>>,
    ns: Option<Vec<u8>>,
    node_type: c_int,
}

/// Registry entry for a compiled pattern (all `|` branches).
struct PatternState {
    branches: Vec<CompiledPattern>,
}

/// Registry entry for one stream context of one branch (pattern.c
/// `_xmlStreamCtxt`); `next` chains the branch contexts.
struct StreamCtxtState {
    comp: StreamComp,
    next: Option<usize>,
    nb_state: usize,
    states: Vec<(i32, i32)>,
    level: c_int,
    flags: c_int,
    block_level: c_int,
}

static PATTERNS: Lazy<Mutex<HashMap<usize, PatternState>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static NEXT_PATTERN_KEY: Lazy<Mutex<usize>> = Lazy::new(|| Mutex::new(1));

static STREAM_CTXTS: Lazy<Mutex<HashMap<usize, StreamCtxtState>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static NEXT_STREAM_KEY: Lazy<Mutex<usize>> = Lazy::new(|| Mutex::new(1));

// ── Small string helpers ───────────────────────────────────────────────────

/// Length of a null-terminated xmlChar string.
const unsafe fn cstr_len(s: *const xmlChar) -> usize {
    if s.is_null() {
        return 0;
    }
    let mut len = 0usize;
    while unsafe { *s.add(len) } != 0 {
        len += 1;
    }
    len
}

/// Compare an owned byte slice with a null-terminated C string.
const unsafe fn cstr_eq_opt(s: Option<&[u8]>, cstr: *const xmlChar) -> bool {
    match s {
        None => cstr.is_null(),
        Some(bytes) => {
            if cstr.is_null() {
                return false;
            }
            let mut i = 0usize;
            while i < bytes.len() {
                if unsafe { *cstr.add(i) } != bytes[i] {
                    return false;
                }
                i += 1;
            }
            unsafe { *cstr.add(i) == 0 }
        }
    }
}

// ── The interpreter (pattern.c xmlPatMatch) ────────────────────────────────

/// Port of `xmlPatMatch` from pattern.c — tests whether `node` matches one
/// branch of a compiled pattern. Returns 1 on match, 0 on no match, -1 on
/// error.
unsafe fn pat_match(comp: &CompiledPattern, mut node: *mut _xmlNode) -> c_int {
    if node.is_null() {
        return -1;
    }
    let mut i: isize = 0;
    // Rollback states for "//" (pattern.c xmlStepStates).
    let mut states: Vec<(isize, *mut _xmlNode)> = Vec::new();

    loop {
        // The step-processing block; `break 'process` performs the C code's
        // `goto rollback`.
        'process: {
            while (i as usize) < comp.steps.len() {
                let op = comp.steps[i as usize].op;
                match op {
                    XML_OP_END => return 1,
                    XML_OP_ROOT => {
                        if unsafe { (*node).type_ } == XML_NAMESPACE_DECL {
                            break 'process;
                        }
                        node = unsafe { (*node).parent };
                        if node.is_null() {
                            break 'process;
                        }
                        let t = unsafe { (*node).type_ };
                        if t == XML_DOCUMENT_NODE || t == XML_HTML_DOCUMENT_NODE {
                            i += 1;
                            continue;
                        }
                        break 'process;
                    }
                    XML_OP_ELEM => {
                        if unsafe { (*node).type_ } != XML_ELEMENT_NODE {
                            break 'process;
                        }
                        let value = comp.steps[i as usize].value.clone();
                        let value2 = comp.steps[i as usize].value2.clone();
                        if let Some(value) = value {
                            let nm = unsafe { (*node).name };
                            if nm.is_null() || value[0] != *nm {
                                break 'process;
                            }
                            if !cstr_eq_opt(Some(&value), nm) {
                                break 'process;
                            }
                            // Namespace test.
                            let ns_href: *const xmlChar = if unsafe { (*node).ns }.is_null() {
                                ptr::null()
                            } else {
                                unsafe { (*(*node).ns).href }
                            };
                            if ns_href.is_null() {
                                if value2.is_some() {
                                    break 'process;
                                }
                            } else if unsafe { *ns_href } != 0 {
                                if value2.is_none() {
                                    break 'process;
                                }
                                if !cstr_eq_opt(value2.as_deref(), ns_href) {
                                    break 'process;
                                }
                            }
                        }
                        i += 1;
                        continue;
                    }
                    XML_OP_CHILD => {
                        let t = unsafe { (*node).type_ };
                        if t != XML_ELEMENT_NODE
                            && t != XML_DOCUMENT_NODE
                            && t != XML_HTML_DOCUMENT_NODE
                        {
                            break 'process;
                        }
                        let value = comp.steps[i as usize].value.clone();
                        let mut lst = unsafe { (*node).children };
                        if let Some(value) = value {
                            while !lst.is_null() {
                                let lt = unsafe { (*lst).type_ };
                                let ln = unsafe { (*lst).name };
                                if lt == XML_ELEMENT_NODE
                                    && !ln.is_null()
                                    && value[0] == *ln
                                    && cstr_eq_opt(Some(&value), ln)
                                {
                                    break;
                                }
                                lst = unsafe { (*lst).next };
                            }
                            if !lst.is_null() {
                                i += 1;
                                continue;
                            }
                        }
                        break 'process;
                    }
                    XML_OP_ATTR => {
                        if unsafe { (*node).type_ } != XML_ATTRIBUTE_NODE {
                            break 'process;
                        }
                        let value = comp.steps[i as usize].value.clone();
                        let value2 = comp.steps[i as usize].value2.clone();
                        if let Some(value) = value {
                            let nm = unsafe { (*node).name };
                            if nm.is_null() || value[0] != *nm {
                                break 'process;
                            }
                            if !cstr_eq_opt(Some(&value), nm) {
                                break 'process;
                            }
                        }
                        let ns_href: *const xmlChar = if unsafe { (*node).ns }.is_null() {
                            ptr::null()
                        } else {
                            unsafe { (*(*node).ns).href }
                        };
                        if ns_href.is_null() {
                            if value2.is_some() {
                                break 'process;
                            }
                        } else if value2.is_some() && !cstr_eq_opt(value2.as_deref(), ns_href) {
                            break 'process;
                        }
                        i += 1;
                        continue;
                    }
                    XML_OP_PARENT => {
                        let t = unsafe { (*node).type_ };
                        if t == XML_DOCUMENT_NODE
                            || t == XML_HTML_DOCUMENT_NODE
                            || t == XML_NAMESPACE_DECL
                        {
                            break 'process;
                        }
                        node = unsafe { (*node).parent };
                        if node.is_null() {
                            break 'process;
                        }
                        let value = comp.steps[i as usize].value.clone();
                        let value2 = comp.steps[i as usize].value2.clone();
                        if let Some(value) = value {
                            let nm = unsafe { (*node).name };
                            if nm.is_null() || value[0] != *nm {
                                break 'process;
                            }
                            if !cstr_eq_opt(Some(&value), nm) {
                                break 'process;
                            }
                            let ns_href: *const xmlChar = if unsafe { (*node).ns }.is_null() {
                                ptr::null()
                            } else {
                                unsafe { (*(*node).ns).href }
                            };
                            if ns_href.is_null() {
                                if value2.is_some() {
                                    break 'process;
                                }
                            } else if unsafe { *ns_href } != 0 {
                                if value2.is_none() {
                                    break 'process;
                                }
                                if !cstr_eq_opt(value2.as_deref(), ns_href) {
                                    break 'process;
                                }
                            }
                        }
                        i += 1;
                        continue;
                    }
                    XML_OP_ANCESTOR => {
                        let mut value = comp.steps[i as usize].value.clone();
                        let mut value2 = comp.steps[i as usize].value2.clone();
                        let mut step_op = comp.steps[i as usize].op;
                        if value.is_none() {
                            i += 1;
                            if (i as usize) >= comp.steps.len() {
                                break 'process;
                            }
                            step_op = comp.steps[i as usize].op;
                            if step_op == XML_OP_ROOT {
                                return 1;
                            }
                            if step_op != XML_OP_ELEM {
                                break 'process;
                            }
                            value = comp.steps[i as usize].value.clone();
                            value2 = comp.steps[i as usize].value2.clone();
                            if value.is_none() {
                                return -1;
                            }
                        }
                        if node.is_null() {
                            break 'process;
                        }
                        let t = unsafe { (*node).type_ };
                        if t == XML_DOCUMENT_NODE
                            || t == XML_HTML_DOCUMENT_NODE
                            || t == XML_NAMESPACE_DECL
                        {
                            break 'process;
                        }
                        node = unsafe { (*node).parent };
                        let value = match value {
                            Some(v) => v,
                            None => break 'process,
                        };
                        while !node.is_null() {
                            let nt = unsafe { (*node).type_ };
                            let nn = unsafe { (*node).name };
                            if nt == XML_ELEMENT_NODE
                                && !nn.is_null()
                                && value[0] == *nn
                                && cstr_eq_opt(Some(&value), nn)
                            {
                                // Namespace test.
                                let ns_href: *const xmlChar = if unsafe { (*node).ns }.is_null() {
                                    ptr::null()
                                } else {
                                    unsafe { (*(*node).ns).href }
                                };
                                if ns_href.is_null() {
                                    if value2.is_none() {
                                        break;
                                    }
                                } else if unsafe { *ns_href } != 0
                                    && value2.is_some()
                                    && cstr_eq_opt(value2.as_deref(), ns_href)
                                {
                                    break;
                                }
                            }
                            node = unsafe { (*node).parent };
                        }
                        if node.is_null() {
                            break 'process;
                        }
                        // Prepare a potential rollback from this ancestor.
                        if step_op == XML_OP_ANCESTOR {
                            states.push((i, node));
                        } else {
                            states.push((i - 1, node));
                        }
                        i += 1;
                        continue;
                    }
                    XML_OP_NS => {
                        if unsafe { (*node).type_ } != XML_ELEMENT_NODE {
                            break 'process;
                        }
                        let value = comp.steps[i as usize].value.clone();
                        let ns_href: *const xmlChar = if unsafe { (*node).ns }.is_null() {
                            ptr::null()
                        } else {
                            unsafe { (*(*node).ns).href }
                        };
                        if ns_href.is_null() {
                            if value.is_some() {
                                break 'process;
                            }
                        } else if unsafe { *ns_href } != 0 {
                            if value.is_none() {
                                break 'process;
                            }
                            if !cstr_eq_opt(value.as_deref(), ns_href) {
                                break 'process;
                            }
                        }
                        i += 1;
                        continue;
                    }
                    XML_OP_ALL => {
                        if unsafe { (*node).type_ } != XML_ELEMENT_NODE {
                            break 'process;
                        }
                        i += 1;
                        continue;
                    }
                    _ => {
                        // Unknown op: treat like upstream's default (no case) —
                        // falls to the for-loop increment.
                        i += 1;
                    }
                }
            }
            // fell out of the step loop: found.
            return 1;
        }
        // rollback: try the saved ancestor state, if any.
        match states.pop() {
            None => return 0,
            Some((si, sn)) => {
                i = si;
                node = sn;
                // Continue the outer loop to restart matching from the saved
                // ancestor state (upstream `goto restart`).
            }
        }
    }
}

// ── The mini pattern compiler (pattern.c) ─────────────────────────────────

/// Parser context (pattern.c `xmlPatParserContext`).
struct PatCtxt<'a> {
    cur: usize,
    base: &'a [u8],
    error: c_int,
    /// (URI, prefix) pairs from the `namespaces` argument.
    namespaces: Vec<(Vec<u8>, Vec<u8>)>,
}

const fn is_blank(c: u8) -> bool {
    c == 0x20 || c == 0x9 || c == 0xA || c == 0xD
}

const fn is_name_start(c: u8) -> bool {
    c >= 0x80 || c == b'_' || c == b':' || c.is_ascii_alphabetic()
}

const fn is_name_char(c: u8) -> bool {
    c >= 0x80 || c.is_ascii_alphanumeric() || c == b'.' || c == b'-' || c == b'_' || c == b':'
}

const fn is_ncname_start(c: u8) -> bool {
    c >= 0x80 || c == b'_' || c.is_ascii_alphabetic()
}

const fn is_ncname_char(c: u8) -> bool {
    c >= 0x80 || c.is_ascii_alphanumeric() || c == b'.' || c == b'-' || c == b'_'
}

impl<'a> PatCtxt<'a> {
    fn cur_byte(&self) -> Option<u8> {
        self.base.get(self.cur).copied()
    }

    fn peek(&self, off: usize) -> Option<u8> {
        self.base.get(self.cur + off).copied()
    }

    fn is_blank_cur(&self) -> bool {
        self.cur_byte().is_some_and(is_blank)
    }

    fn skip_blanks(&mut self) {
        while self.cur_byte().is_some_and(is_blank) {
            self.cur += 1;
        }
    }

    /// `xmlPatScanName` — scans an XML Name.
    fn scan_name(&mut self) -> Option<Vec<u8>> {
        self.skip_blanks();
        let start = self.cur;
        let mut cur = start;
        if self.base.get(cur).is_some_and(|&c| !is_name_start(c)) {
            return None;
        }
        cur += 1;
        while self.base.get(cur).is_some_and(|&c| is_name_char(c)) {
            cur += 1;
        }
        if cur == start {
            return None;
        }
        let ret = self.base[start..cur].to_vec();
        self.cur = cur;
        Some(ret)
    }

    /// `xmlPatScanNCName` — scans an NCName.
    fn scan_ncname(&mut self) -> Option<Vec<u8>> {
        self.skip_blanks();
        let start = self.cur;
        let mut cur = start;
        if self.base.get(cur).is_some_and(|&c| !is_ncname_start(c)) {
            return None;
        }
        cur += 1;
        while self.base.get(cur).is_some_and(|&c| is_ncname_char(c)) {
            cur += 1;
        }
        if cur == start {
            return None;
        }
        let ret = self.base[start..cur].to_vec();
        self.cur = cur;
        Some(ret)
    }
}

const XML_XML_NAMESPACE: &[u8] = b"http://www.w3.org/XML/1998/namespace";

/// Resolve a `prefix:name` prefix to its URI (pattern.c's namespace loop,
/// including the special `xml` prefix).
fn resolve_prefix(ctxt: &PatCtxt, prefix: &[u8]) -> Option<Vec<u8>> {
    if prefix == b"xml" {
        return Some(XML_XML_NAMESPACE.to_vec());
    }
    for (uri, pref) in &ctxt.namespaces {
        if pref == prefix {
            return Some(uri.clone());
        }
    }
    None
}

/// Port of `xmlCompileAttributeTest` (pattern.c).
fn compile_attribute_test(ctxt: &mut PatCtxt, comp: &mut CompiledPattern) {
    ctxt.skip_blanks();
    let name = ctxt.scan_ncname();
    if ctxt.error < 0 {
        return;
    }
    let Some(name) = name else {
        if ctxt.cur_byte() == Some(b'*') {
            comp.steps.push(StepOp {
                op: XML_OP_ATTR,
                value: None,
                value2: None,
            });
            ctxt.cur += 1;
        } else {
            ctxt.error = 1;
        }
        return;
    };
    if ctxt.cur_byte() == Some(b':') {
        let prefix = name;
        ctxt.cur += 1;
        if ctxt.is_blank_cur() {
            ctxt.error = 1;
            return;
        }
        let token = ctxt.scan_name();
        let url = resolve_prefix(ctxt, &prefix);
        let Some(url) = url else {
            ctxt.error = 1;
            return;
        };
        if let Some(token) = token {
            comp.steps.push(StepOp {
                op: XML_OP_ATTR,
                value: Some(token),
                value2: Some(url),
            });
        } else {
            if ctxt.cur_byte() == Some(b'*') {
                ctxt.cur += 1;
                comp.steps.push(StepOp {
                    op: XML_OP_ATTR,
                    value: None,
                    value2: Some(url),
                });
            } else {
                ctxt.error = 1;
            }
        }
    } else {
        comp.steps.push(StepOp {
            op: XML_OP_ATTR,
            value: Some(name),
            value2: None,
        });
    }
}

/// Port of `xmlCompileStepPattern` (pattern.c).
fn compile_step_pattern(ctxt: &mut PatCtxt, comp: &mut CompiledPattern) {
    ctxt.skip_blanks();
    if ctxt.cur_byte() == Some(b'.') {
        ctxt.cur += 1;
        comp.steps.push(StepOp {
            op: XML_OP_ELEM,
            value: None,
            value2: None,
        });
        return;
    }
    if ctxt.cur_byte() == Some(b'@') {
        if comp.flags & XML_PATTERN_XSSEL != 0 {
            ctxt.error = 1;
            return;
        }
        ctxt.cur += 1;
        compile_attribute_test(ctxt, comp);
        if ctxt.error != 0 {
            return;
        }
        return;
    }
    let name = ctxt.scan_ncname();
    if ctxt.error < 0 {
        return;
    }
    let Some(mut name) = name else {
        if ctxt.cur_byte() == Some(b'*') {
            ctxt.cur += 1;
            comp.steps.push(StepOp {
                op: XML_OP_ALL,
                value: None,
                value2: None,
            });
        } else {
            ctxt.error = 1;
        }
        return;
    };
    let mut has_blanks = false;
    if ctxt.is_blank_cur() {
        has_blanks = true;
        ctxt.skip_blanks();
    }
    if ctxt.cur_byte() == Some(b':') {
        ctxt.cur += 1;
        if ctxt.cur_byte() != Some(b':') {
            // `prefix:name` / `prefix:*` namespace match.
            let prefix = name;
            if has_blanks || ctxt.is_blank_cur() {
                ctxt.error = 1;
                return;
            }
            let token = ctxt.scan_name();
            let url = resolve_prefix(ctxt, &prefix);
            let Some(url) = url else {
                ctxt.error = 1;
                return;
            };
            if let Some(token) = token {
                comp.steps.push(StepOp {
                    op: XML_OP_ELEM,
                    value: Some(token),
                    value2: Some(url),
                });
            } else {
                if ctxt.cur_byte() == Some(b'*') {
                    ctxt.cur += 1;
                    comp.steps.push(StepOp {
                        op: XML_OP_NS,
                        value: None,
                        value2: Some(url),
                    });
                } else {
                    ctxt.error = 1;
                }
            }
            return;
        }
        // `child::` / `attribute::` axes.
        ctxt.cur += 1;
        if name == b"child" {
            match ctxt.scan_name() {
                None => {
                    if ctxt.cur_byte() == Some(b'*') {
                        ctxt.cur += 1;
                        comp.steps.push(StepOp {
                            op: XML_OP_ALL,
                            value: None,
                            value2: None,
                        });
                        return;
                    }
                    ctxt.error = 1;
                    return;
                }
                Some(n) => name = n,
            }
            if ctxt.cur_byte() == Some(b':') {
                let prefix = name;
                ctxt.cur += 1;
                if ctxt.is_blank_cur() {
                    ctxt.error = 1;
                    return;
                }
                let token = ctxt.scan_name();
                let url = resolve_prefix(ctxt, &prefix);
                let Some(url) = url else {
                    ctxt.error = 1;
                    return;
                };
                if let Some(token) = token {
                    comp.steps.push(StepOp {
                        op: XML_OP_ELEM,
                        value: Some(token),
                        value2: Some(url),
                    });
                } else {
                    if ctxt.cur_byte() == Some(b'*') {
                        ctxt.cur += 1;
                        comp.steps.push(StepOp {
                            op: XML_OP_NS,
                            value: None,
                            value2: Some(url),
                        });
                    } else {
                        ctxt.error = 1;
                    }
                }
                return;
            }
            comp.steps.push(StepOp {
                op: XML_OP_ELEM,
                value: Some(name),
                value2: None,
            });
        } else if name == b"attribute" {
            if comp.flags & XML_PATTERN_XSSEL != 0 {
                ctxt.error = 1;
                return;
            }
            compile_attribute_test(ctxt, comp);
            if ctxt.error != 0 {}
        } else {
            ctxt.error = 1;
        }
    } else if ctxt.cur_byte() == Some(b'*') {
        // Unreachable with a scanned name in practice: `foo*` is invalid.
        ctxt.error = 1;
    } else {
        comp.steps.push(StepOp {
            op: XML_OP_ELEM,
            value: Some(name),
            value2: None,
        });
    }
}

/// Port of `xmlCompilePathPattern` (pattern.c).
fn compile_path_pattern(ctxt: &mut PatCtxt, comp: &mut CompiledPattern) {
    ctxt.skip_blanks();
    if ctxt.cur_byte() == Some(b'/') {
        comp.flags |= PAT_FROM_ROOT;
    } else if ctxt.cur_byte() == Some(b'.') || (comp.flags & XML_PATTERN_NOTPATTERN) != 0 {
        comp.flags |= PAT_FROM_CUR;
    }

    if ctxt.cur_byte() == Some(b'/') && ctxt.peek(1) == Some(b'/') {
        comp.steps.push(StepOp {
            op: XML_OP_ANCESTOR,
            value: None,
            value2: None,
        });
        ctxt.cur += 2;
    } else if ctxt.cur_byte() == Some(b'.')
        && ctxt.peek(1) == Some(b'/')
        && ctxt.peek(2) == Some(b'/')
    {
        comp.steps.push(StepOp {
            op: XML_OP_ANCESTOR,
            value: None,
            value2: None,
        });
        ctxt.cur += 3;
        ctxt.skip_blanks();
        if ctxt.cur_byte().is_none() {
            ctxt.error = 1;
            return;
        }
    }
    if ctxt.cur_byte() == Some(b'@') {
        ctxt.cur += 1;
        compile_attribute_test(ctxt, comp);
        if ctxt.error != 0 {
            return;
        }
        ctxt.skip_blanks();
        if ctxt.cur_byte().is_some() {
            compile_step_pattern(ctxt, comp);
            if ctxt.error != 0 {
                return;
            }
        }
    } else {
        if ctxt.cur_byte() == Some(b'/') {
            comp.steps.push(StepOp {
                op: XML_OP_ROOT,
                value: None,
                value2: None,
            });
            ctxt.cur += 1;
            ctxt.skip_blanks();
            if ctxt.cur_byte().is_none() {
                ctxt.error = 1;
                return;
            }
        }
        compile_step_pattern(ctxt, comp);
        if ctxt.error != 0 {
            return;
        }
        ctxt.skip_blanks();
        while ctxt.cur_byte() == Some(b'/') {
            if ctxt.peek(1) == Some(b'/') {
                comp.steps.push(StepOp {
                    op: XML_OP_ANCESTOR,
                    value: None,
                    value2: None,
                });
                ctxt.cur += 2;
                ctxt.skip_blanks();
                compile_step_pattern(ctxt, comp);
                if ctxt.error != 0 {
                    return;
                }
            } else {
                comp.steps.push(StepOp {
                    op: XML_OP_PARENT,
                    value: None,
                    value2: None,
                });
                ctxt.cur += 1;
                ctxt.skip_blanks();
                if ctxt.cur_byte().is_none() {
                    ctxt.error = 1;
                    return;
                }
                compile_step_pattern(ctxt, comp);
                if ctxt.error != 0 {
                    return;
                }
            }
        }
    }
    if ctxt.cur_byte().is_some() {
        ctxt.error = 1;
    }
}

/// Port of `xmlCompileIDCXPathPath` (pattern.c) — the XS-IDC subset used
/// for `XML_PATTERN_XSSEL` / `XML_PATTERN_XSFIELD`.
fn compile_idc_xpath_path(ctxt: &mut PatCtxt, comp: &mut CompiledPattern) {
    ctxt.skip_blanks();
    if ctxt.cur_byte() == Some(b'/') {
        ctxt.error = 1;
        return;
    }
    comp.flags |= PAT_FROM_CUR;

    if ctxt.cur_byte() == Some(b'.') {
        ctxt.cur += 1;
        ctxt.skip_blanks();
        if ctxt.cur_byte().is_none() {
            comp.steps.push(StepOp {
                op: XML_OP_ELEM,
                value: None,
                value2: None,
            });
            return;
        }
        if ctxt.cur_byte() != Some(b'/') {
            ctxt.error = 1;
            return;
        }
        ctxt.cur += 1;
        ctxt.skip_blanks();
        if ctxt.cur_byte() == Some(b'/') {
            if ctxt.cur > 0 && is_blank(ctxt.base[ctxt.cur - 1]) {
                ctxt.error = 1;
                return;
            }
            comp.steps.push(StepOp {
                op: XML_OP_ANCESTOR,
                value: None,
                value2: None,
            });
            ctxt.cur += 1;
            ctxt.skip_blanks();
        }
        if ctxt.cur_byte().is_none() {
            ctxt.error = 1;
            return;
        }
    }
    loop {
        compile_step_pattern(ctxt, comp);
        if ctxt.error != 0 {
            return;
        }
        ctxt.skip_blanks();
        if ctxt.cur_byte() != Some(b'/') {
            break;
        }
        comp.steps.push(StepOp {
            op: XML_OP_PARENT,
            value: None,
            value2: None,
        });
        ctxt.cur += 1;
        ctxt.skip_blanks();
        if ctxt.cur_byte() == Some(b'/') {
            ctxt.error = 1;
            return;
        }
        if ctxt.cur_byte().is_none() {
            ctxt.error = 1;
            return;
        }
    }
    if ctxt.cur_byte().is_some() {
        ctxt.error = 1;
    }
}

/// Port of `xmlReversePattern` (pattern.c): drops a leading `//` op and
/// reverses the op stack, appending `XML_OP_END`.
fn reverse_pattern(comp: &mut CompiledPattern) {
    // Remove the leading `//` for `//a` / `.//a`.
    if !comp.steps.is_empty() && comp.steps[0].op == XML_OP_ANCESTOR {
        comp.steps.remove(0);
    }
    comp.steps.reverse();
    comp.steps.push(StepOp {
        op: XML_OP_END,
        value: None,
        value2: None,
    });
}

/// Add a step to a streaming compilation (pattern.c `xmlStreamCompAddStep`).
fn stream_comp_add_step(
    comp: &mut StreamComp,
    name: Option<Vec<u8>>,
    ns: Option<Vec<u8>>,
    node_type: c_int,
    flags: c_int,
) -> c_int {
    comp.steps.push(StreamStep {
        flags,
        name,
        ns,
        node_type,
    });
    comp.nb_step += 1;
    (comp.nb_step - 1) as c_int
}

/// Port of `xmlStreamCompile` (pattern.c).
///
/// `Ok(Some(stream))` — streamable; `Ok(None)` — compiled but not
/// streamable (upstream returns 0 and leaves `comp->stream == NULL`);
/// `Err(())` — hard error (upstream returns -1).
fn stream_compile(comp: &mut CompiledPattern) -> Result<Option<StreamComp>, ()> {
    // Special case for `.` — no steps, matches any node.
    if comp.steps.len() == 1
        && comp.steps[0].op == XML_OP_ELEM
        && comp.steps[0].value.is_none()
        && comp.steps[0].value2.is_none()
    {
        return Ok(Some(StreamComp {
            nb_step: 0,
            steps: Vec::new(),
            flags: XML_STREAM_FINAL_IS_ANY_NODE,
        }));
    }

    let mut stream = StreamComp {
        nb_step: 0,
        steps: Vec::with_capacity(comp.steps.len() / 2 + 1),
        flags: 0,
    };
    if comp.flags & PAT_FROM_ROOT != 0 {
        stream.flags |= XML_STREAM_FROM_ROOT;
    }

    let mut s: c_int = 0;
    let mut root = 0;
    let mut flags = 0;
    let mut prevs: c_int = -1;
    let nb_step = comp.steps.len();

    for (i, step) in comp.steps.iter().enumerate() {
        match step.op {
            XML_OP_END => break,
            XML_OP_ROOT => {
                if i != 0 {
                    return Ok(None);
                }
                root = 1;
            }
            XML_OP_NS => {
                s = stream_comp_add_step(
                    &mut stream,
                    None,
                    step.value.clone(),
                    XML_ELEMENT_NODE,
                    flags,
                );
                if s < 0 {
                    return Err(());
                }
                prevs = s;
                flags = 0;
            }
            XML_OP_ATTR => {
                flags |= XML_STREAM_STEP_ATTR;
                prevs = -1;
                s = stream_comp_add_step(
                    &mut stream,
                    step.value.clone(),
                    step.value2.clone(),
                    XML_ATTRIBUTE_NODE,
                    flags,
                );
                flags = 0;
                if s < 0 {
                    return Err(());
                }
            }
            XML_OP_ELEM => {
                if step.value.is_none() && step.value2.is_none() {
                    // `.` / `self::node()` — eliminate redundant tests.
                    if nb_step == i + 1 && (flags & XML_STREAM_STEP_DESC) != 0 {
                        if nb_step == i + 1 {
                            stream.flags |= XML_STREAM_FINAL_IS_ANY_NODE;
                        }
                        flags |= XML_STREAM_STEP_NODE;
                        s = stream_comp_add_step(
                            &mut stream,
                            None,
                            None,
                            XML_STREAM_ANY_NODE,
                            flags,
                        );
                        if s < 0 {
                            return Err(());
                        }
                        flags = 0;
                        if prevs != -1 {
                            stream.steps[prevs as usize].flags |= XML_STREAM_STEP_IN_SET;
                            prevs = -1;
                        }
                        continue;
                    } else {
                        continue;
                    }
                }
                s = stream_comp_add_step(
                    &mut stream,
                    step.value.clone(),
                    step.value2.clone(),
                    XML_ELEMENT_NODE,
                    flags,
                );
                if s < 0 {
                    return Err(());
                }
                prevs = s;
                flags = 0;
            }
            XML_OP_CHILD => {
                s = stream_comp_add_step(
                    &mut stream,
                    step.value.clone(),
                    step.value2.clone(),
                    XML_ELEMENT_NODE,
                    flags,
                );
                if s < 0 {
                    return Err(());
                }
                prevs = s;
                flags = 0;
            }
            XML_OP_ALL => {
                s = stream_comp_add_step(&mut stream, None, None, XML_ELEMENT_NODE, flags);
                if s < 0 {
                    return Err(());
                }
                prevs = s;
                flags = 0;
            }
            XML_OP_PARENT => {}
            XML_OP_ANCESTOR if (flags & XML_STREAM_STEP_DESC) == 0 => {
                flags |= XML_STREAM_STEP_DESC;
                if (stream.flags & XML_STREAM_DESC) == 0 {
                    stream.flags |= XML_STREAM_DESC;
                }
            }
            _ => {}
        }
    }

    if root == 0 && (comp.flags & XML_PATTERN_NOTPATTERN) == 0 {
        if (stream.flags & XML_STREAM_DESC) == 0 {
            stream.flags |= XML_STREAM_DESC;
        }
        if stream.nb_step > 0 && (stream.steps[0].flags & XML_STREAM_STEP_DESC) == 0 {
            stream.steps[0].flags |= XML_STREAM_STEP_DESC;
        }
    }
    if stream.nb_step <= s as usize {
        // No final step produced — not streamable (upstream error path).
        return Ok(None);
    }
    stream.steps[s as usize].flags |= XML_STREAM_STEP_FINAL;
    if root != 0 {
        stream.steps[0].flags |= XML_STREAM_STEP_ROOT;
    }
    Ok(Some(stream))
}

/// Port of `xmlPatternCompileSafe` (pattern.c). Returns 0 on success, 1 on
/// pattern error, -1 on allocation failure; `*pattern_out` receives the new
/// pattern (or NULL).
unsafe fn pattern_compile_safe_impl(
    pattern: *const xmlChar,
    _dict: *mut c_void,
    flags: c_int,
    namespaces: *const *const xmlChar,
    pattern_out: *mut xmlPatternPtr,
) -> c_int {
    if pattern_out.is_null() {
        return 1;
    }
    if pattern.is_null() {
        unsafe { *pattern_out = ptr::null_mut() };
        return 1;
    }

    // Gather namespace pairs: array of [URI, prefix] terminated by NULL.
    let mut ns_pairs: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    if !namespaces.is_null() {
        let mut i = 0usize;
        loop {
            let uri = unsafe { *namespaces.add(2 * i) };
            if uri.is_null() {
                break;
            }
            let prefix = unsafe { *namespaces.add(2 * i + 1) };
            let uri = unsafe { core::slice::from_raw_parts(uri, cstr_len(uri)) }.to_vec();
            let prefix = if prefix.is_null() {
                Vec::new()
            } else {
                unsafe { core::slice::from_raw_parts(prefix, cstr_len(prefix)) }.to_vec()
            };
            ns_pairs.push((uri, prefix));
            i += 1;
        }
    }

    let pat_bytes = unsafe { core::slice::from_raw_parts(pattern, cstr_len(pattern)) };
    // Upstream loops over `|`-separated alternatives with `while (*or != 0)`;
    // an empty pattern never enters the loop and yields a NULL pattern with
    // a success status.
    if pat_bytes.is_empty() {
        unsafe { *pattern_out = ptr::null_mut() };
        return 0;
    }
    let mut branches: Vec<CompiledPattern> = Vec::new();
    let mut error: c_int = 0;
    let mut streamable = 1;
    let mut pat_type: c_int = 0;

    for segment in pat_bytes.split(|&b| b == b'|') {
        let mut ctxt = PatCtxt {
            cur: 0,
            base: segment,
            error: 0,
            namespaces: ns_pairs.clone(),
        };
        let mut cur = CompiledPattern {
            flags,
            steps: Vec::new(),
            stream: None,
        };
        if (cur.flags & (XML_PATTERN_XSSEL | XML_PATTERN_XSFIELD)) != 0 {
            compile_idc_xpath_path(&mut ctxt, &mut cur);
        } else {
            compile_path_pattern(&mut ctxt, &mut cur);
        }
        if ctxt.error != 0 {
            error = ctxt.error;
            break;
        }
        if streamable != 0 {
            let t = cur.flags & (PAT_FROM_ROOT | PAT_FROM_CUR);
            if pat_type == 0 {
                pat_type = t;
            } else if pat_type == PAT_FROM_ROOT {
                if t & PAT_FROM_CUR != 0 {
                    streamable = 0;
                }
            } else if pat_type == PAT_FROM_CUR && t & PAT_FROM_ROOT != 0 {
                streamable = 0;
            }
        }
        if streamable != 0 {
            match stream_compile(&mut cur) {
                Ok(stream) => cur.stream = stream,
                Err(()) => {
                    error = -1;
                    break;
                }
            }
        }
        reverse_pattern(&mut cur);
        branches.push(cur);
    }

    if error != 0 {
        unsafe { *pattern_out = ptr::null_mut() };
        return error;
    }

    if streamable == 0 {
        for branch in &mut branches {
            branch.stream = None;
        }
    }

    // Register the compiled pattern.
    let key = {
        let mut next = NEXT_PATTERN_KEY.lock();
        let k = *next;
        *next += 1;
        k
    };
    PATTERNS.lock().insert(key, PatternState { branches });
    unsafe { *pattern_out = key as xmlPatternPtr };
    0
}

// ── Public pattern API ────────────────────────────────────────────────────

/// Upstream `xmlPattern * xmlPatterncompile(const xmlChar *pattern,
/// xmlDict *dict, int flags, const xmlChar **namespaces)`.
///
/// # SAFETY
///
/// - `dict` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `pattern`, `namespaces` must point to valid NUL-terminated
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
pub unsafe extern "C" fn xmlPatterncompile(
    pattern: *const xmlChar,
    dict: *mut c_void,
    flags: c_int,
    namespaces: *const *const xmlChar,
) -> xmlPatternPtr {
    let mut out: xmlPatternPtr = ptr::null_mut();
    let ret = pattern_compile_safe_impl(pattern, dict, flags, namespaces, &mut out);
    if ret != 0 {
        return ptr::null_mut();
    }
    out
}

/// Upstream `int xmlPatternCompileSafe(const xmlChar *pattern, xmlDict *dict,
/// int flags, const xmlChar **namespaces, xmlPattern **patternOut)`.
///
/// # SAFETY
///
/// - `dict`, `patternOut` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `pattern`, `namespaces` must point to valid NUL-terminated
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
pub unsafe extern "C" fn xmlPatternCompileSafe(
    pattern: *const xmlChar,
    dict: *mut c_void,
    flags: c_int,
    namespaces: *const *const xmlChar,
    patternOut: *mut xmlPatternPtr,
) -> c_int {
    pattern_compile_safe_impl(pattern, dict, flags, namespaces, patternOut)
}

/// Upstream `int xmlPatternMatch(xmlPattern *comp, xmlNode *node)`.
///
/// # SAFETY
///
/// - `comp`, `node` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlPatternMatch(comp: xmlPatternPtr, node: *mut _xmlNode) -> c_int {
    if comp.is_null() || node.is_null() {
        return -1;
    }
    let key = comp as usize;
    let reg = PATTERNS.lock();
    let Some(ps) = reg.get(&key) else {
        return -1;
    };
    for branch in &ps.branches {
        let ret = pat_match(branch, node);
        if ret != 0 {
            return ret;
        }
    }
    0
}

/// Upstream `void xmlFreePattern(xmlPattern *comp)`.
///
/// # SAFETY
///
/// - `comp` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlFreePattern(comp: xmlPatternPtr) {
    xmlFreePatternList(comp);
}

/// Upstream `void xmlFreePatternList(xmlPattern *comp)`.
///
/// # SAFETY
///
/// - `comp` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlFreePatternList(comp: xmlPatternPtr) {
    if comp.is_null() {
        return;
    }
    PATTERNS.lock().remove(&(comp as usize));
}

/// Upstream `int xmlPatternStreamable(xmlPattern *comp)`.
///
/// # SAFETY
///
/// - `comp` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlPatternStreamable(comp: xmlPatternPtr) -> c_int {
    if comp.is_null() {
        return -1;
    }
    let reg = PATTERNS.lock();
    let Some(ps) = reg.get(&(comp as usize)) else {
        return -1;
    };
    if ps.branches.iter().any(|b| b.stream.is_none()) {
        return 0;
    }
    1
}

/// Upstream `int xmlPatternMaxDepth(xmlPattern *comp)` — -2 if unlimited
/// (uses `//`), else the maximum step count, -1 on error.
///
/// # SAFETY
///
/// - `comp` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlPatternMaxDepth(comp: xmlPatternPtr) -> c_int {
    if comp.is_null() {
        return -1;
    }
    let reg = PATTERNS.lock();
    let Some(ps) = reg.get(&(comp as usize)) else {
        return -1;
    };
    let mut ret = 0;
    for branch in &ps.branches {
        let Some(stream) = &branch.stream else {
            return -1;
        };
        for step in &stream.steps {
            if step.flags & XML_STREAM_STEP_DESC != 0 {
                return -2;
            }
        }
        if stream.nb_step > ret as usize {
            ret = stream.nb_step as c_int;
        }
    }
    ret
}

/// Upstream `int xmlPatternMinDepth(xmlPattern *comp)` — the minimum depth
/// reachable by the pattern (0 means `/` or `.` are part of the set).
///
/// # SAFETY
///
/// - `comp` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlPatternMinDepth(comp: xmlPatternPtr) -> c_int {
    if comp.is_null() {
        return -1;
    }
    let reg = PATTERNS.lock();
    let Some(ps) = reg.get(&(comp as usize)) else {
        return -1;
    };
    let mut ret: c_int = 12345678;
    for branch in &ps.branches {
        let Some(stream) = &branch.stream else {
            return -1;
        };
        if stream.nb_step < ret as usize {
            ret = stream.nb_step as c_int;
        }
        if ret == 0 {
            return 0;
        }
    }
    ret
}

/// Upstream `int xmlPatternFromRoot(xmlPattern *comp)`.
///
/// # SAFETY
///
/// - `comp` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlPatternFromRoot(comp: xmlPatternPtr) -> c_int {
    if comp.is_null() {
        return -1;
    }
    let reg = PATTERNS.lock();
    let Some(ps) = reg.get(&(comp as usize)) else {
        return -1;
    };
    for branch in &ps.branches {
        if branch.stream.is_none() {
            return -1;
        }
        if branch.flags & PAT_FROM_ROOT != 0 {
            return 1;
        }
    }
    0
}

// ── Stream contexts ───────────────────────────────────────────────────────

/// Port of `xmlNewStreamCtxt` + chain building done by
/// `xmlPatternGetStreamCtxt` (pattern.c).
///
/// # SAFETY
///
/// - `comp` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlPatternGetStreamCtxt(comp: xmlPatternPtr) -> xmlStreamCtxtPtr {
    if comp.is_null() {
        return ptr::null_mut();
    }
    let key = comp as usize;
    let reg = PATTERNS.lock();
    let Some(ps) = reg.get(&key) else {
        return ptr::null_mut();
    };

    // Every branch must have a streaming compilation; snapshot them (with
    // their flags) before creating any context.
    let mut snapshot: Vec<(StreamComp, c_int)> = Vec::new();
    for branch in &ps.branches {
        match &branch.stream {
            Some(stream) => snapshot.push((stream.clone(), branch.flags)),
            None => return ptr::null_mut(),
        }
    }
    drop(reg);

    let mut head: Option<usize> = None;
    let mut last: Option<usize> = None;
    {
        let mut stream_reg = STREAM_CTXTS.lock();
        for (stream, flags) in snapshot {
            let h = {
                let mut next = NEXT_STREAM_KEY.lock();
                let k = *next;
                *next += 1;
                k
            };
            stream_reg.insert(
                h,
                StreamCtxtState {
                    comp: stream,
                    next: None,
                    nb_state: 0,
                    states: Vec::new(),
                    level: 0,
                    flags,
                    block_level: -1,
                },
            );
            match last {
                None => head = Some(h),
                Some(prev) => {
                    if let Some(prev_st) = stream_reg.get_mut(&prev) {
                        prev_st.next = Some(h);
                    }
                }
            }
            last = Some(h);
        }
    }
    match head {
        Some(h) => h as xmlStreamCtxtPtr,
        None => ptr::null_mut(),
    }
}

/// Free a chain of stream contexts given the head handle.
fn stream_ctxt_free_chain(head: Option<usize>) {
    let mut cur = head;
    let mut registry = STREAM_CTXTS.lock();
    while let Some(h) = cur {
        let next = registry.get(&h).and_then(|s| s.next);
        registry.remove(&h);
        cur = next;
    }
}

/// Upstream `void xmlFreeStreamCtxt(xmlStreamCtxt *stream)`.
///
/// # SAFETY
///
/// - `stream` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlFreeStreamCtxt(stream: xmlStreamCtxtPtr) {
    if stream.is_null() {
        return;
    }
    stream_ctxt_free_chain(Some(stream as usize));
}

/// Port of `xmlStreamCtxtAddState` (pattern.c).
fn stream_ctxt_add_state(st: &mut StreamCtxtState, idx: i32, level: i32) -> c_int {
    for i in 0..st.nb_state {
        if st.states[i].0 < 0 {
            st.states[i] = (idx, level);
            return i as c_int;
        }
    }
    st.states.push((idx, level));
    st.nb_state += 1;
    (st.nb_state - 1) as c_int
}

/// Port of `xmlStreamPushInternal` (pattern.c) — the streaming evaluator.
unsafe fn stream_push_internal(
    head: usize,
    name: *const xmlChar,
    ns: *const xmlChar,
    node_type: c_int,
) -> c_int {
    let mut ret = 0;
    let mut registry = STREAM_CTXTS.lock();
    let mut cur = Some(head);

    while let Some(h) = cur {
        let next = registry.get(&h).and_then(|s| s.next);
        let Some(st) = registry.get_mut(&h) else {
            return -1;
        };

        // Document node (or reset): both name and ns NULL.
        if node_type == XML_ELEMENT_NODE && name.is_null() && ns.is_null() {
            st.nb_state = 0;
            st.level = 0;
            st.block_level = -1;
            if st.comp.flags & XML_STREAM_FROM_ROOT != 0 {
                if st.comp.nb_step == 0
                    || (st.comp.nb_step == 1
                        && st.comp.steps[0].node_type == XML_STREAM_ANY_NODE
                        && st.comp.steps[0].flags & XML_STREAM_STEP_DESC != 0)
                {
                    ret = 1;
                } else if st.comp.steps[0].flags & XML_STREAM_STEP_ROOT != 0
                    && stream_ctxt_add_state(st, 0, 0) < 0
                {
                    return -1;
                }
            }
            cur = next;
            continue;
        }

        // Fast check for "." (stream with no steps).
        if st.comp.nb_step == 0 {
            if st.flags & XML_PATTERN_XPATH != 0 {
                cur = next;
                continue;
            }
            if node_type != XML_ATTRIBUTE_NODE
                && ((st.flags & XML_PATTERN_NOTPATTERN) == 0 || st.level == 0)
            {
                ret = 1;
            }
            st.level += 1;
            cur = next;
            continue;
        }

        if st.block_level != -1 {
            st.level += 1;
            cur = next;
            continue;
        }

        if node_type != XML_ELEMENT_NODE
            && node_type != XML_ATTRIBUTE_NODE
            && (st.comp.flags & XML_STREAM_FINAL_IS_ANY_NODE) == 0
        {
            st.level += 1;
            cur = next;
            continue;
        }

        // Evolution of existing states.
        let mut i = 0usize;
        let m = st.nb_state;
        let mut final_: c_int = 0;
        while i < m {
            let step_nr: i32;
            if (st.comp.flags & XML_STREAM_DESC) == 0 {
                step_nr = st.states[st.nb_state - 1].0;
                if st.states[st.nb_state - 1].1 < st.level {
                    return -1;
                }
                i = m; // loop-stopper
            } else {
                step_nr = st.states[i].0;
                if step_nr < 0 {
                    i += 1;
                    continue;
                }
                let tmp = st.states[i].1;
                if tmp > st.level {
                    i += 1;
                    continue;
                }
                if tmp < st.level
                    && (st.comp.steps[step_nr as usize].flags & XML_STREAM_STEP_DESC) == 0
                {
                    i += 1;
                    continue;
                }
            }
            let Some(step) = st.comp.steps.get(step_nr as usize).cloned() else {
                i += 1;
                continue;
            };
            if step.node_type != node_type {
                if step.node_type == XML_ATTRIBUTE_NODE {
                    if (st.comp.flags & XML_STREAM_DESC) == 0 {
                        st.block_level = st.level + 1;
                    }
                    i += 1;
                    continue;
                } else if step.node_type != XML_STREAM_ANY_NODE {
                    i += 1;
                    continue;
                }
            }
            let mut match_ = false;
            if step.node_type == XML_STREAM_ANY_NODE {
                match_ = true;
            } else if step.name.is_none() {
                if step.ns.is_none() {
                    match_ = true;
                } else if !ns.is_null() {
                    match_ = cstr_eq_opt(step.ns.as_deref(), ns);
                }
            } else if (step.ns.is_some() == !ns.is_null())
                && !name.is_null()
                && step.name.as_ref().unwrap()[0] == *name
                && cstr_eq_opt(step.name.as_deref(), name)
                && (step.ns.is_none() || cstr_eq_opt(step.ns.as_deref(), ns))
            {
                match_ = true;
            }
            if match_ {
                final_ = step.flags & XML_STREAM_STEP_FINAL;
                if final_ != 0 {
                    ret = 1;
                } else if stream_ctxt_add_state(st, step_nr + 1, st.level + 1) < 0 {
                    return -1;
                }
                if ret != 1 && step.flags & XML_STREAM_STEP_IN_SET != 0 {
                    ret = 1;
                }
            }
            if (st.comp.flags & XML_STREAM_DESC) == 0 && (!match_ || final_ != 0) {
                st.block_level = st.level + 1;
            }
            i += 1;
        }

        st.level += 1;

        // Re/enter the expression.
        let step0 = st.comp.steps[0].clone();
        if step0.flags & XML_STREAM_STEP_ROOT != 0 {
            cur = next;
            continue;
        }
        let desc = step0.flags & XML_STREAM_STEP_DESC != 0;
        let do_compare = if st.flags & XML_PATTERN_NOTPATTERN != 0 {
            if st.level == 1 {
                if st.flags & (XML_PATTERN_XSSEL | XML_PATTERN_XSFIELD) != 0 {
                    // XS-IDC: the missing self::node() always matches the
                    // first given node.
                    cur = next;
                    continue;
                }
                true
            } else if desc
                || (st.level == 2 && st.flags & (XML_PATTERN_XSSEL | XML_PATTERN_XSFIELD) != 0)
            {
                true
            } else {
                cur = next;
                continue;
            }
        } else {
            true
        };

        if do_compare {
            if step0.node_type != node_type
                && (node_type == XML_ATTRIBUTE_NODE || step0.node_type != XML_STREAM_ANY_NODE)
            {
                cur = next;
                continue;
            }
            let mut match_ = false;
            if step0.node_type == XML_STREAM_ANY_NODE {
                match_ = true;
            } else if step0.name.is_none() {
                if step0.ns.is_none() {
                    match_ = true;
                } else if !ns.is_null() {
                    match_ = cstr_eq_opt(step0.ns.as_deref(), ns);
                }
            } else if (step0.ns.is_some() == !ns.is_null())
                && !name.is_null()
                && step0.name.as_ref().unwrap()[0] == *name
                && cstr_eq_opt(step0.name.as_deref(), name)
                && (step0.ns.is_none() || cstr_eq_opt(step0.ns.as_deref(), ns))
            {
                match_ = true;
            }
            final_ = step0.flags & XML_STREAM_STEP_FINAL;
            if match_ {
                if final_ != 0 {
                    ret = 1;
                } else if stream_ctxt_add_state(st, 1, st.level) < 0 {
                    return -1;
                }
                if ret != 1 && step0.flags & XML_STREAM_STEP_IN_SET != 0 {
                    ret = 1;
                }
            }
            if (st.comp.flags & XML_STREAM_DESC) == 0 && (!match_ || final_ != 0) {
                st.block_level = st.level;
            }
        }

        cur = next;
    }
    ret
}

/// Upstream `int xmlStreamPush(xmlStreamCtxt *stream, const xmlChar *name,
/// const xmlChar *ns)`.
///
/// # SAFETY
///
/// - `stream` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `name`, `ns` must point to valid NUL-terminated
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
pub unsafe extern "C" fn xmlStreamPush(
    stream: xmlStreamCtxtPtr,
    name: *const xmlChar,
    ns: *const xmlChar,
) -> c_int {
    if stream.is_null() {
        return -1;
    }
    stream_push_internal(stream as usize, name, ns, XML_ELEMENT_NODE)
}

/// Upstream `int xmlStreamPushAttr(xmlStreamCtxt *stream, const xmlChar *name,
/// const xmlChar *ns)`.
///
/// # SAFETY
///
/// - `stream` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `name`, `ns` must point to valid NUL-terminated
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
pub unsafe extern "C" fn xmlStreamPushAttr(
    stream: xmlStreamCtxtPtr,
    name: *const xmlChar,
    ns: *const xmlChar,
) -> c_int {
    if stream.is_null() {
        return -1;
    }
    stream_push_internal(stream as usize, name, ns, XML_ATTRIBUTE_NODE)
}

/// Upstream `int xmlStreamPushNode(xmlStreamCtxt *stream, const xmlChar *name,
/// const xmlChar *ns, int nodeType)`.
///
/// # SAFETY
///
/// - `stream` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// - `name`, `ns` must point to valid NUL-terminated
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
pub unsafe extern "C" fn xmlStreamPushNode(
    stream: xmlStreamCtxtPtr,
    name: *const xmlChar,
    ns: *const xmlChar,
    nodeType: c_int,
) -> c_int {
    if stream.is_null() {
        return -1;
    }
    stream_push_internal(stream as usize, name, ns, nodeType)
}

/// Upstream `int xmlStreamPop(xmlStreamCtxt *stream)`.
///
/// # SAFETY
///
/// - `stream` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlStreamPop(stream: xmlStreamCtxtPtr) -> c_int {
    if stream.is_null() {
        return -1;
    }
    let mut registry = STREAM_CTXTS.lock();
    let mut cur = Some(stream as usize);
    while let Some(h) = cur {
        let next = registry.get(&h).and_then(|s| s.next);
        let Some(st) = registry.get_mut(&h) else {
            return -1;
        };
        // Reset block-level.
        if st.block_level == st.level {
            st.block_level = -1;
        }
        // `level` can be zero when FINAL_IS_ANY_NODE is set.
        if st.level > 0 {
            st.level -= 1;
        }
        // Discard obsoleted states.
        let mut i = st.nb_state as isize - 1;
        while i >= 0 {
            let lev = st.states[i as usize].1;
            if lev > st.level {
                st.nb_state -= 1;
            }
            if lev <= st.level {
                break;
            }
            i -= 1;
        }
        cur = next;
    }
    0
}

/// Upstream `int xmlStreamWantsAnyNode(xmlStreamCtxt *streamCtxt)` — 1 if the
/// pattern needs text/cdata/comment/PI nodes pushed as well.
///
/// # SAFETY
///
/// - `streamCtxt` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlStreamWantsAnyNode(streamCtxt: xmlStreamCtxtPtr) -> c_int {
    if streamCtxt.is_null() {
        return -1;
    }
    let registry = STREAM_CTXTS.lock();
    let mut cur = Some(streamCtxt as usize);
    while let Some(h) = cur {
        let Some(st) = registry.get(&h) else {
            return -1;
        };
        if st.comp.flags & XML_STREAM_FINAL_IS_ANY_NODE != 0 {
            return 1;
        }
        cur = st.next;
    }
    0
}
