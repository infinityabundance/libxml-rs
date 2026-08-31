//! Internal implementation details — NOT part of the public C ABI (§4).
//!
//! This module contains internal Rust semantics that are not exposed through
//! the C-compatible ABI. It exists to keep the public ABI surface clean while
//! allowing safe internal Rust implementation patterns.
//!
//! # Safety
//!
//! The `unsafe` boundary is at the ABI membrane (src/abi). Internal code
//! should be as strongly typed and safe as possible, using the ABI membrane
//! to translate between C-compatible representations and internal Rust types.
//!
//! # Upstream contract
//!
//! Upstream has no "internal" subsystem that ships to consumers: libxml2
//! keeps its private helpers in non-installed headers (private/parser.h,
//! private/tree.h, ...) and in file-scope statics inside globals.c and
//! parser.c. This module is the candidate equivalent: Rust-internal state
//! that backs public ABI entry points (src/abi/exports_xml2.rs calls into
//! `internal::globals`) but is never exported, never declared in include/,
//! and never part of the parity target symbol surface. The concrete
//! upstream mirror for the current content is globals.c
//! (SRC-LIBXML2-GIT, archaeology/libxml2-git); see the `globals` submodule.
//!
//! # Conceptual behavior
//!
//! Internal state that upstream keeps as library-global C statics lives here
//! as Rust modules so it can use safe Rust primitives (thread_local!,
//! parking_lot, atomics) behind the ABI membrane. Where an upstream public
//! struct field is application-owned (for example the parser-context
//! `_private` slot), the candidate stores its internals in side tables keyed
//! by stable native pointers instead of commandeering the field; that side
//! storage is internal-by-construction and never crosses the ABI. The module
//! is deliberately private (`mod internal;` in lib.rs), so it cannot even be
//! named from outside the crate.
//!
//! # Ownership & safety invariants
//!
//! - Nothing in this module is reachable from C: no exported symbol, no
//!   header declaration, no data-global mirror. The ABI membrane (src/abi)
//!   is the only translation point between C and internal state.
//! - Side tables are keyed by stable native pointers and must never be
//!   consulted after the key object is freed; every table entry is removed
//!   in the matching free path (the R-000169 policy moved parser-context
//!   internals out of the application-owned `_private` field into exactly
//!   this kind of side storage).
//! - Initialization/cleanup reference counting lives in
//!   `crate::xml::globals` and is only delegated to here, never duplicated.
//!
//! # Historical quirks & epochs
//!
//! - The 2.12 lazy-init rework (the `GlobalStateInit` capability epoch,
//!   boundary 2.12.0, atlas/COMPATIBILITY_PROFILES.md) is why init paths
//!   here delegate to per-context lazy initialization instead of eager
//!   static constructors.
//! - `xmlInitGlobals`/`xmlCleanupGlobals` are exported ABI relics: the
//!   former is a deprecated alias for `xmlInitParser` and the latter is a
//!   documented no-op in globals.c; the real lifecycle is
//!   init_parser/cleanup_parser, which is exactly what the `globals`
//!   submodule delegates to.
//!
//! # Deliberate oddities
//!
//! - The module is deliberately sparse: it exists so internal helpers have
//!   one home instead of leaking into src/abi or src/xml. Adding
//!   public-facing content here would be a bug, not an enhancement.
//! - The delegation functions (init_parser, cleanup_parser, init_threads)
//!   are thin on purpose: all state lives in the implementing subsystem so
//!   there is exactly one source of truth.
//!
//! # Proving courts
//!
//! Internal state is proven indirectly by the courts that observe its
//! effects: the GLOBAL-STATE, THREADING, ABI-DATA and ALLOCATOR court
//! families, the TREE-001 structural probe (R-000169 regression), and
//! `cargo test` — the parallel error-mirror tests (R-000170/R-000171)
//! hammer the synchronized global state this module delegates to.
//!
//! # Tempting simplifications that would break parity
//!
//! - Moving internal state back into public ABI struct fields (e.g.
//!   reusing `ctxt->_private`) would break the application-owned-field
//!   contract and fail the TREE-001 and ownership probes.
//! - Exporting the helpers of this module as public ABI would grow the DSO
//!   symbol surface beyond the upstream parity target and fail the ABI
//!   census.
//! - Duplicating the init reference counting here instead of delegating to
//!   `crate::xml::globals` would create two owners of one lifecycle and
//!   corrupt the cleanup order.
//!
//! # Phase status
//!
//! The `globals` submodule is fully implemented; further internal helpers
//! are added as each subsystem is implemented per §85 phases.

pub mod globals;
