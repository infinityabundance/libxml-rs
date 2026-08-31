//! libxslt implementation — native Rust (§1, §3, §31–§34).
//!
//! This module contains the complete native-Rust implementation of libxslt's
//! observable behavior: stylesheet compilation, transform engine, templates,
//! patterns, variables, parameters, keys, attributes, namespace aliasing,
//! whitespace stripping, sorting, numbering, documents, imports, extensions,
//! serialization, security, errors.
//!
//! The XSLT engine operates exclusively on the Rust libxml implementation
//! (src/xml), never on upstream C libxml2 (§31).
//!
//! # Phase 8 status
//!
//! Complete: the full XSLT 1.0 engine is implemented and court-tested.
//! - Phase 8: libxslt core (stylesheet compilation, patterns, templates,
//!   imports/includes, variables, parameters, keys, sorting, numbering,
//!   output, extensions, transform runtime)
//! - Phase 9: EXSLT + xsltproc
//!
//! See `atlas/PARITY_MATRIX.md` for current status.
//!
//! # Upstream contract
//!
//! Parity target: upstream libxslt 1.1.45 (system oracle DSO libxslt.so.1)
//! verified against the 1.1.42 source tree under
//! `oracle/historical/src/libxslt-1.1.42/`; per-file source identifiers are
//! `SRC-LIBXSLT-1.1.42-<module>.c` (atlas/SOURCES.md). Submodules mirror the
//! upstream file layout: transform.c, xslt.c, preproc.c, templates.c,
//! patterns.c, variables.c, params.c, keys.c, numbers.c, sort.c,
//! attributes.c, imports.c, documents.c, extensions.c, namespaces.c,
//! security.c, xsltutils.c. The C ABI drop-in surface covers the public
//! libxslt headers (xslt.h, xsltInternals.h, transform.h, xsltutils.h,
//! security.h, ...), byte-checked by the header-compile and dso-loader
//! courts.
//!
//! # Conceptual behavior
//!
//! Two-phase engine, both per XSLT 1.0 (W3C-XSLT-1.0): (1) compilation
//! walks the stylesheet document into a `_xsltStylesheet` — templates
//! priority-ordered, keys, global variables/params, attribute sets,
//! namespace aliases, strip/preserve rules, output definition — and
//! (2) `xsltApplyStylesheet` runs the transform: the root template is
//! applied to the source document, instructions execute against an
//! XPath context (`src/xml/xpath` §31) and build the result tree, which
//! is returned as a fresh document the caller owns.
//!
//! # Ownership & safety invariants
//!
//! Per atlas/OWNERSHIP_ATLAS.md section 4: the stylesheet owns its style
//! documents and compiled definitions; the transform borrows the source
//! document (caller keeps it); the result document is caller-owned
//! (`xmlFreeDoc`, alias `xsltFreeTransformResult`); RVT documents are owned
//! by the transform context and freed at teardown. Every `unsafe` entry
//! point requires pointers from the matching constructor/owner, freed
//! exactly once with the documented `*Free*` pair.
//!
//! # Historical quirks & epochs
//!
//! E-008: xsltproc basic/num/empty output is byte-identical from libxslt
//! 1.1.26 (2009) through 1.1.45 — a fully stable epoch, frozen for fifteen
//! years (atlas/SEMANTIC_EPOCHS.md). libxslt was born 2001-01-07; EXSLT
//! arrived with the 1.1 series (1.1.0, 2004-12-15); 1.1.45 is dated
//! 2025-07-15 (atlas/HISTORY.md section 2). A modern residual is a
//! candidate bug, never an epoch difference.
//!
//! # Deliberate oddities
//!
//! - R-000160: exports whose upstream 1.1.45 bodies are literally trivial
//!   (`xsltSecurityAllow` returns 1, `xsltSecurityForbid` returns 0) are
//!   intentional no-ops with exact semantics, not placeholder stubs.
//! - R-000167: version symbols (`xsltLibxsltVersion` et al.) are exported
//!   as read-only data symbols (R/D), matching the oracle DSO, not as
//!   functions.
//! - Other documented divergences are annotated `UPSTREAM-PARITY` inline at
//!   the exact site.
//!
//! # Proving courts
//!
//! CLI-XSLTPROC-0001..0057 (xsltproc differential corpus, byte-identical
//! receipts), XSLT-001 (xslt-family differential probe), EXSLT, DSO-LOADER,
//! HEADER-COMPILE, HIST-EPOCH-0001..0008 (E-008), plus the in-crate
//! `cargo test` suites under src/xslt.
//!
//! # Tempting simplifications that would break parity
//!
//! - Fixed-count variable pops instead of pop-back-to-saved-depth break
//!   xsl:call-template with defaulted parameters (R-000158).
//! - Treating position() as a function of the node alone breaks
//!   `//book[position() <= 2]` (R-000159).
//! - Delegating format-number()/number() to a formatting crate breaks
//!   byte parity (R-000163, R-000166).
//! - Collapsing the priority-ordered template list without the import-depth
//!   tiebreak breaks import precedence.
//!   See atlas/RESIDUAL_LEDGER.md for the full lesson set.

pub mod attributes;
pub mod compiler;
pub mod documents;
pub mod errors;
pub mod extensions;
pub mod imports;
pub mod keys;
pub mod namespace_alias;
pub mod numbering;
pub mod parameters;
pub mod patterns;
pub mod security;
pub mod serialization;
pub mod sorting;
pub mod stylesheet;
pub mod templates;
pub mod transform;
pub mod variables;
pub mod whitespace;
