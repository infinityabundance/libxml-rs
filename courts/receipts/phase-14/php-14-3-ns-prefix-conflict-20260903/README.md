# php-14.3 KEY-5 namespace prefix-conflict resolution (2026-09-03)

Full suite **158 → 148 failed**, ZERO regressions (name-level diff vs the
158 baseline `phpbuild-c:/out/xpe-six11.log`: 0 new). Log:
`phpbuild-c:/out/xpe-six12.log`. dom 89 → 79 (10 flipped).

## Root cause 1 — xmlNewNs allowed duplicate prefixes on one node (~7 tests)
Upstream tree.c xmlNewNs rejects a declaration whose prefix is ALREADY used
on the SAME node's own nsDef chain with a non-NULL href (returns NULL after
freeing). PHP's setAttributeNS/createAttributeNS conflict resolution depends
on that NULL: dom_get_ns_unchecked falls back to
dom_get_ns_resolve_prefix_conflict, allocating a fresh prefix
(`xmlns:default`, `xmlns:default1`, …). The candidate appended the duplicate
unconditionally, so `<container xmlns:foo="ns1">` + setAttributeNS(ns2,
foo:hello) produced a second `xmlns:foo="ns2"` with both attrs bound to foo.
`new_ns` now mirrors upstream (first-element check + walk; xmlStrEqual NULL==
NULL semantics; existing href must be non-NULL to conflict).
Flipped: createAttributeNS_prefix_conflicts ×6 (setAttribute{,NS}_with/without/
mixed_prefix), DOMElement_setAttributeNS_prefix_conflict,
DOMDocument_importNode_attribute_prefix_conflict, DOMElement_prefix_empty.

## Root cause 2 — xmlHasProp wrongly required a NULL namespace (~3 tests)
Upstream xmlHasProp matches the attribute's LOCAL NAME only (the NULL-ns
restriction belongs to xmlHasNsProp). The candidate required attr.ns == NULL,
so php's setAttributeNode replacement lookup (xmlHasProp(node, localname))
never found a namespaced attribute: re-setting foo:hello left the stale
attribute AND the new one (duplicate hello attrs on one element). has_prop
now matches by name; the DTD default-decl fallback of upstream xmlHasProp is
still pending (no current member needs it).
Flipped: the same setAttribute* prefix-conflict phpt, plus the
crash-guard delayed_freeing/namespace_definition_crash_in_attribute and
import_attribute_namespace.

## Probe kept (consumers/, candidate == oracle)
- nsprop-replace-probe.c — createAttributeNS + setAttributeNode call
  sequence; xmlHasProp visibility of namespaced attrs.

## Residuals next (dom 79)
clone_attribute_namespace_01/02 (reconcile-on-insert missing the xmlns
decl on the target), modern Element_setAttributeNS (y:foo vs x:foo prefix
retention — php 8.5 ns-mapper), attribute_renaming_conflict (DTD #FIXED
decl serialization), schemaValidate/relaxNGValidate (~13, several roots),
xsl 52, xmlreader 15.

Validation: cargo test --lib 1239 pass, clippy at the 4 pre-existing
warnings, fmt clean. Full six-gate `phpbuild-c:/out/xpe-six12.log`:
1291 / **148 failed** / 40 skipped.
