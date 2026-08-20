//! EXSLT implementation — native Rust (§35).
//!
//! EXSLT is a community-driven set of extensions to XSLT 1.0. libxslt ships
//! implementations of the following modules:
//!
//! - `exsl:` — Common (exsl:document, exsl:node-set, etc.)
//! - `math:` — Math (math:abs, math:max, math:min, math:sin, etc.)
//! - `set:` — Sets (set:difference, set:distinct, set:intersection, etc.)
//! - `str:` — Strings (str:concat, str:padding, str:split, etc.)
//! - `dyn:` — Dynamic (dyn:element, dyn:attribute, etc.)
//! - `func:` — Functions (func:function, func:result, func:script)
//! - `date:` — Dates and Times (date:date, date:format-date, etc.)
//!
//! # Phase 0 status
//!
//! All sub-modules are scaffolded. Implementation begins in Phase 9 (§85).
//!
//! See `atlas/standards/STANDARDS.md` for EXSLT standards mapping.

pub mod common;
pub mod dates;
pub mod dynamic;
pub mod functions;
pub mod math;
pub mod sets;
pub mod strings;
