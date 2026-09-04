# php-14.14 — C14N subset visibility + prefix-list lifetime + doc-oldNs alias (2026-09-04)

Full six-gate **36 → 33 failed**, ZERO regressions (name-level diff vs the
`phpbuild-c:/out/cand-37.log` baseline: **NEW_ONLY=0, FIXED=3**). Log:
`phpbuild-c:/out/xpe-six30.log`. dom 14 → 10 | xsl 13 | xmlreader 8 |
simplexml 1 | xmlwriter 1. Commit on main (Phase 14.14).

## Root cause 1 — dangling InclusiveNamespaces PrefixList pointers (~3 tests)
`xmlC14NExecute` and `xmlC14NDocDumpMemory` joined the caller's
NULL-terminated `xmlChar **` prefix array into a comma-separated Rust
`Vec<u8>` *inside* the `if` branch and passed `.as_ptr()` — the Vec dropped at
the end of the branch expression, leaving the delegated call a dangling
pointer. `parse_inclusive_prefixes` then read freed memory (empty/garbage set)
so `Dom\XMLDocument::C14N(exclusive, …, ['test'])` never rendered the
requested `xmlns:test`. The exclusive element-namespace step had the same bug
class (a `format!("{}\0", …)` temporary taken `.as_ptr()` inside the loop).
FIX: the joined Vec (xmlC14NDocDumpMemory / xmlC14NExecute) and the per-prefix
C-string copies (c14n_collect_namespaces) are owned across the whole call.
`xmlC14NDocSaveTo` already kept its Option<Vec> alive.

## Root cause 2 — subset canonicalization treated namespaces as never visible (~2 tests)
`C14nContext::is_visible_ns()` returned `visible_set.is_none()`, so with any
node-set (the `xmlC14NDocSaveTo` path PHP uses for `//namespace::*` xpath
subsets) every namespace declaration was skipped: `<contain>` lost
`xmlns="…"`/`xmlns:test="…"` (canonicalization.phpt check #5/#6,
DOMNode_C14N_references). Upstream c14n.c `xmlC14NIsNodeInNodeset` makes a
stack copy of the ns, links `next` to the owning element and defers to
`xmlXPathNodeSetContains`, whose NAMESPACE_DECL arm matches a node-set entry
by **owner element + prefix** (xpath.c); node-set ns entries are the
synthesized `_xmlNs` copies from the namespace axis (`next` = owner).
FIX: `ns_visible(ns, owner)` replicates that: with a visibility callback it
consults the callback (upstream behaviour), with a node-set it scans for a
NAMESPACE_DECL entry whose `next` == owner with an empty-tolerant prefix
equality, and with no subset everything is visible. Inclusive and exclusive
collection now gate per-namespace exactly like upstream
(`if(visible && IsVisible(ns, cur))` for inserts; stack-add when visible;
attr namespaces gated on attribute visibility only).

## Root cause 3 — inclusive axis walked the document node as an element (~1 test)
The inclusive ancestor walk continued `cur = cur.parent` past the root element
into the DOCUMENT node and read its `nsDef` field — which aliases
`xmlDoc.oldNs` at the same struct offset — collecting leftover namespaces of
the parsed document (e.g. `xmlns:x` from `<x:Child/>`) as declarations of the
root element. gh21544's parsed `Dom\XMLDocument` thus rendered
`<env:Root … xmlns:x="urn:child"><x:Child>` instead of keeping the
declaration on `x:Child`. Upstream filters those with its
`xmlSearchNs(doc, cur, prefix) == ns` effective-binding test. FIX: skip
non-element ancestors in the inclusive walk (equivalent outcome).

## Guards / validation
- cargo test --lib 1241 pass / 1 ignored; clippy clean; fmt clean.
- Probe: nine-mode C14N probe (inclusive/exclusive × no-xpath/xpath-subset ×
  prefix-list) byte-identical candidate vs oracle (`phporacle-c`), plus the
  gh21544 parsed-vs-DOM-built matrix identical across all four modes.
