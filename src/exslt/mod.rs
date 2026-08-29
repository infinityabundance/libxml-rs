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

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap;

use crate::xml::xpath::context::XPathContext;
use crate::xml::xpath::types::XPathValue;

pub mod common;
pub mod dates;
pub mod dynamic;
pub mod functions;
pub mod math;
pub mod sets;
pub mod strings;

/// EXSLT namespace URIs.
pub const EXSLT_NS_COMMON: &str = "http://exslt.org/common";
pub const EXSLT_NS_MATH: &str = "http://exslt.org/math";
pub const EXSLT_NS_SETS: &str = "http://exslt.org/sets";
pub const EXSLT_NS_STRINGS: &str = "http://exslt.org/strings";
pub const EXSLT_NS_DYNAMIC: &str = "http://exslt.org/dynamic";
pub const EXSLT_NS_FUNCTIONS: &str = "http://exslt.org/functions";
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
