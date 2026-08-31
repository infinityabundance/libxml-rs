//! EXSLT implementation — native Rust (§35).
//!
//! EXSLT is a community-driven set of extensions to XSLT 1.0. libxslt ships
//! implementations of the following modules:
//!
//! - `exsl:` — Common (exsl:node-set, exsl:object-type, exsl:document)
//! - `math:` — Math (math:max, math:min, math:sin, math:cos, ...)
//! - `set:` — Sets (set:difference, set:distinct, set:intersection, ...)
//! - `str:` — Strings (str:concat, str:padding, str:split, ...)
//! - `dyn:` — Dynamic (dyn:element, dyn:attribute, dyn:evaluate, ...)
//! - `func:` — Functions (func:function, func:result, func:script)
//! - `date:` — Dates and Times (date:date, date:format-date, ...)
//!
//! # Registration model
//!
//! Upstream libxslt requires an explicit `exsltRegisterAll()` call (usually
//! from the host application; `xsltproc` calls it at startup) before EXSLT
//! functions are available. This module mirrors that model: a process-wide
//! registry of EXSLT functions keyed by their full QName (e.g. `"math:max"`).
//! `exsltRegisterAll()` populates the registry; each new transform context
//! copies the registered functions into its XPath context (§31 integration).
//!
//! # EXSLT namespaces
//!
//! | Prefix | URI |
//! |--------|-----|
//! | exsl | `http://exslt.org/common` |
//! | math | `http://exslt.org/math` |
//! | set | `http://exslt.org/sets` |
//! | str | `http://exslt.org/strings` |
//! | dyn | `http://exslt.org/dynamic` |
//! | func | `http://exslt.org/functions` |
//! | date | `http://exslt.org/dates-and-times` |
//!
//! # Phase 9 status
//!
//! Complete: all seven modules implemented and registered.
//!
//! # Upstream contract
//!
//! Parity target: upstream libexslt (the EXSLT support library shipped
//! inside the libxslt distribution) 1.1.45, with per-module sources
//! `SRC-LIBXSLT-1.1.42-libexslt/<module>.c` (exslt.c, common.c, date.c,
//! dynamic.c, functions.c, math.c, saxon.c, sets.c, strings.c) under
//! `oracle/historical/src/libxslt-1.1.42/libexslt/`. EXSLT 1.0 semantics per
//! the EXSLT-COMMON/MATH/SETS/STRINGS/DYNAMIC/DATES standards registry
//! (atlas/SOURCES.md). The C ABI surface is the per-module registration
//! functions (`exsltCommonRegister`, `exsltMathRegister`, ...), `exsltRegisterAll`,
//! and the version data symbols — closed in R-000165/R-000167 and verified
//! by DSO-LOADER.
//!
//! # Conceptual behavior
//!
//! Upstream requires an explicit `exsltRegisterAll()` call (usually from
//! the host application; `xsltproc` calls it at startup) before EXSLT
//! functions are available. The candidate mirrors that model with a
//! process-wide registry keyed by full QName; each new transform context
//! copies the registered functions into its XPath context (§31
//! integration).
//!
//! # Ownership & safety invariants
//!
//! The registry owns its entries. Registered closures are `Box::leak`-ed
//! (see the `register` function below): they live for the process lifetime
//! because the registry is never cleared — a deliberate, bounded leak. The
//! leaked references are `Send + Sync`; lookups clone the reference out of
//! the mutex-guarded map, so no entry is ever invalidated while in use.
//!
//! # Historical quirks & epochs
//!
//! EXSLT is part of the libxslt 1.1 series (since 1.1.0, 2004-12-15;
//! atlas/HISTORY.md section 2). E-008 (atlas/SEMANTIC_EPOCHS.md) shows the
//! XSLT/xsltproc output epoch frozen since 2009, which includes the EXSLT
//! function results exercised by the xsltproc corpus. R-000112 fixed the
//! dates module no-argument defaults; R-000165 added the missing per-module
//! registration exports. R-000168 (platform surface) remains OPEN and
//! touches this module and src/exslt/dates.
//!
//! # Deliberate oddities
//!
//! - `exsltCryptoRegister` is a deliberate no-op export: the candidate has
//!   no crypto module and upstream returns void.
//! - The `*XpathCtxtRegister` variants register globally (the registry is
//!   process-wide) and return 0, an intentional divergence from upstream
//!   per-context registration.
//! - saxon has no candidate module of its own beyond the registry entries;
//!   `exsltSaxonRegister` still exists (upstream exslt.c calls it from
//!   `exsltRegisterAll`).
//!
//! # Proving courts
//!
//! EXSLT, CLI-XSLTPROC (exslt-using corpus stylesheets), DSO-LOADER
//! (per-module registration exports, R-000165), and the in-crate `cargo
//! test` suites (e.g. test_register_all_populates).
//!
//! # Tempting simplifications that would break parity
//!
//! - Making the registry per-transform instead of process-wide would break
//!   the upstream registration model (`exsltRegisterAll` before
//!   `xsltApplyStylesheet`) and the ABI registration functions.
//! - Replacing the leak with scoped lifetimes would either require
//!   invalidating live lookups or copying every function per call; the
//!   leak is the parity-preserving choice.
//! - Dropping the marker registrations (`func:function` et al.) would
//!   break `function-available` and `element-available` reporting.

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::ffi::c_void;
use std::os::raw::c_int;

use crate::abi::types::xmlChar;
use crate::xml::xpath::context::XPathContext;
use crate::xml::xpath::types::XPathValue;

pub mod common;
pub mod dates;
pub mod dynamic;
pub mod functions;
pub mod math;
pub mod saxon;
pub mod sets;
pub mod strings;

/// EXSLT namespace URIs.
pub const EXSLT_NS_COMMON: &str = "http://exslt.org/common";
/// Math module namespace (`math:max`, `math:min`, `math:sin`, ...).
pub const EXSLT_NS_MATH: &str = "http://exslt.org/math";
/// Sets module namespace (`set:difference`, `set:distinct`, ...).
pub const EXSLT_NS_SETS: &str = "http://exslt.org/sets";
/// Strings module namespace (`str:concat`, `str:padding`, ...).
pub const EXSLT_NS_STRINGS: &str = "http://exslt.org/strings";
/// Dynamic module namespace (`dyn:element`, `dyn:evaluate`, ...).
pub const EXSLT_NS_DYNAMIC: &str = "http://exslt.org/dynamic";
/// Functions module namespace (`func:function`, `func:result`, ...).
pub const EXSLT_NS_FUNCTIONS: &str = "http://exslt.org/functions";
/// Dates-and-times module namespace (`date:date`, `date:format-date`, ...).
pub const EXSLT_NS_DATES: &str = "http://exslt.org/dates-and-times";

/// The XPath function signature used throughout the EXSLT modules.
pub type ExsltFunction = fn(&mut XPathContext, &[XPathValue]) -> Result<XPathValue, String>;

/// A capture-capable EXSLT function (used by `func:function` bodies).
///
/// Stored as a leaked `&'static` reference so entries are `Copy` and the
/// registry can hand out clones. The process-wide registry lives for the
/// lifetime of the program, so leaking is intentional and bounded.
pub type ExsltClosure =
    &'static (dyn Fn(&mut XPathContext, &[XPathValue]) -> Result<XPathValue, String> + Send + Sync);

/// Process-wide EXSLT function registry, keyed by full QName
/// (e.g. `"math:max"`). Populated by `exsltRegisterAll()`.
static REGISTRY: Lazy<Mutex<HashMap<String, ExsltClosure>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Register a single EXSLT function under its full QName.
pub fn register<F>(name: &str, f: F)
where
    F: Fn(&mut XPathContext, &[XPathValue]) -> Result<XPathValue, String> + Send + Sync + 'static,
{
    // SAFETY: the boxed closure is leaked; it lives for the process lifetime
    // (the registry is never cleared) so the resulting 'static reference is
    // sound.
    let leaked: &'static (dyn Fn(&mut XPathContext, &[XPathValue]) -> Result<XPathValue, String>
                  + Send
                  + Sync) = Box::leak(Box::new(f));
    REGISTRY.lock().insert(name.to_string(), leaked);
}

/// Look up a registered EXSLT function by full QName.
pub fn lookup(name: &str) -> Option<ExsltClosure> {
    REGISTRY.lock().get(name).copied()
}

/// Iterate over all registered EXSLT functions.
pub fn iter_functions() -> Vec<(String, ExsltClosure)> {
    REGISTRY
        .lock()
        .iter()
        .map(|(k, v)| (k.clone(), *v))
        .collect()
}

/// Whether any EXSLT functions have been registered.
pub fn is_registered() -> bool {
    !REGISTRY.lock().is_empty()
}

/// Register every EXSLT module (mirrors upstream `exsltRegisterAll`).
pub fn register_all() {
    common::register_all();
    math::register_all();
    sets::register_all();
    strings::register_all();
    dynamic::register_all();
    dates::register_all();
    functions::register_all();
    saxon::register_all();
}

/// The C ABI entry point: register all EXSLT modules.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void exsltRegisterAll(void);
/// ```
///
/// Oracle behavior: registers every EXSLT function so it becomes available
/// to subsequently created transform contexts. Calling it twice is a no-op
/// (re-registration overwrites identical entries).
#[no_mangle]
pub extern "C" fn exsltRegisterAll() {
    register_all();
}

// ═══════════════════════════════════════════════════════════════════════════════
// Per-module registration entry points (11.1-X R-000165 closure)
// ═══════════════════════════════════════════════════════════════════════════════
//
// Upstream libexslt exposes one register function per module (exslt.c). The
// candidate's registry is keyed by "prefix:name"; each module's register_all
// populates it. saxon/crypto have no candidate module — their register
// functions are exported no-ops (upstream contracts: void / int 0).

/// `exsltCommonRegister` — register the EXSLT common module.
#[no_mangle]
pub extern "C" fn exsltCommonRegister() {
    common::register_all();
}

/// `exsltMathRegister` — register the EXSLT math module.
#[no_mangle]
pub extern "C" fn exsltMathRegister() {
    math::register_all();
}

/// `exsltSetsRegister` — register the EXSLT sets module.
#[no_mangle]
pub extern "C" fn exsltSetsRegister() {
    sets::register_all();
}

/// `exsltFuncRegister` — register the EXSLT functions module.
#[no_mangle]
pub extern "C" fn exsltFuncRegister() {
    functions::register_all();
}

/// `exsltStrRegister` — register the EXSLT strings module.
#[no_mangle]
pub extern "C" fn exsltStrRegister() {
    strings::register_all();
}

/// `exsltDateRegister` — register the EXSLT dates module.
#[no_mangle]
pub extern "C" fn exsltDateRegister() {
    dates::register_all();
}

/// `exsltSaxonRegister` — register the EXSLT Saxon extensions
/// (upstream exslt.c calls this from `exsltRegisterAll`).
#[no_mangle]
pub extern "C" fn exsltSaxonRegister() {
    saxon::register_all();
}

/// `exsltDynRegister` — register the EXSLT dynamic module.
#[no_mangle]
pub extern "C" fn exsltDynRegister() {
    dynamic::register_all();
}

/// `exsltCryptoRegister` — register the EXSLT crypto module. The candidate
/// has no crypto module; upstream returns void, so this is a no-op.
#[no_mangle]
pub const extern "C" fn exsltCryptoRegister() {}

/// `exsltDateXpathCtxtRegister(ctxt, prefix)` — register the dates module
/// on a specific XPath context (upstream date.c). The candidate's registry
/// is global; registration is performed for all contexts.
#[no_mangle]
pub extern "C" fn exsltDateXpathCtxtRegister(_ctxt: *mut c_void, _prefix: *const xmlChar) -> c_int {
    dates::register_all();
    0
}

/// `exsltMathXpathCtxtRegister(ctxt, prefix)` — math module (see above).
#[no_mangle]
pub extern "C" fn exsltMathXpathCtxtRegister(_ctxt: *mut c_void, _prefix: *const xmlChar) -> c_int {
    math::register_all();
    0
}

/// `exsltSetsXpathCtxtRegister(ctxt, prefix)` — sets module (see above).
#[no_mangle]
pub extern "C" fn exsltSetsXpathCtxtRegister(_ctxt: *mut c_void, _prefix: *const xmlChar) -> c_int {
    sets::register_all();
    0
}

/// `exsltStrXpathCtxtRegister(ctxt, prefix)` — strings module (see above).
#[no_mangle]
pub extern "C" fn exsltStrXpathCtxtRegister(_ctxt: *mut c_void, _prefix: *const xmlChar) -> c_int {
    strings::register_all();
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_lookup() {
        fn my_func(_ctx: &mut XPathContext, _args: &[XPathValue]) -> Result<XPathValue, String> {
            Ok(XPathValue::String("hello".to_string()))
        }
        register("test:myfunc", my_func);
        let f = lookup("test:myfunc");
        assert!(f.is_some());
        let (names, _) = iter_functions()
            .into_iter()
            .find(|(n, _)| n == "test:myfunc")
            .unwrap();
        assert_eq!(names, "test:myfunc");
    }

    #[test]
    fn test_lookup_missing() {
        assert!(lookup("nonexistent:fn").is_none());
    }

    #[test]
    fn test_register_all_populates() {
        // Register everything; all module functions must be present.
        register_all();
        for name in [
            "exsl:node-set",
            "exsl:object-type",
            "math:max",
            "math:sin",
            "math:constant",
            "set:difference",
            "set:distinct",
            "str:tokenize",
            "str:padding",
            "dyn:evaluate",
            "date:date",
            "date:date-time",
        ] {
            assert!(lookup(name).is_some(), "missing EXSLT function {}", name);
        }
    }
}
