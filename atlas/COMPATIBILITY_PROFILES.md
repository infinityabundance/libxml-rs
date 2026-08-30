# Compatibility Profiles — 11.1-R

The candidate models historical behavior through **capability epochs** (src/compatibility/profiles/mod.rs), never scattered `if version == ...` branches. A `CompatibilityProfile` resolves every capability for a target upstream version pair.

## Capability epochs

| Capability | Evidence | Boundary | Change |
|---|---|---|---|
| `XPathNodeSetSerialization` | E-001 | 2.9.10 | xmllint --xpath node-set output: concatenated -> newline-separated (commit da35eeae, documented breaking change) |
| `ParserDiagnostic` | E-002 | 2.9.10 / 2.12.x | second parse-error diagnostic: present -> 2.9.10 regression variant -> dropped in the 2.12 error-handling rework |
| `EntityCompactStorage` | E-004 | 2.13.0 | entity content debug node: TEXT -> TEXT compact (commit 8d04f0ee) |
| `ValidationExit` | E-005 | 2.13.0 | xmllint parse-error/undeclared exit 1 -> 4; valid-invalid 4 -> 3 |
| `XpathAttrEmptyExit` | E-003 | 2.11.0 / 2.12.6 | empty node-set exit code 10 -> 0 (e85f9b98) -> 11 (387a952b) |
| `HtmlSerializer` | E-007 | 2.15.0 | HTML dump: newline-per-element -> single line (newline writes removed from HTMLtree.c) |
| `ValidationNoDtdExit` | E-006 | 2.15.0 | --valid without DTD: exit 3 -> 0 (xmllint refactor) |
| `GlobalStateInit` | 2.12 rework | 2.12.0 | eager static initialisation -> lazy per-context initialisation |
| `XslTransform` | E-008 | stable since <=1.1.26 | libxslt transform output byte-identical 1.1.26..1.1.45 |

## Version -> profile mapping (resolver rules)

```text
capabilities_for_libxml2(version):
  XPathNodeSetSerialization : (maj,min) > (2,9) or (2,9,pat>=10) ? NewlineSeparated : Concatenated
  ParserDiagnostic         : [2.9.10,2.9.11) Regression; >=2.12 Single; else Dual
  EntityCompactStorage     : >=2.13 Compact : Plain
  ValidationExit           : >=2.13 Reworked : Legacy
  XpathAttrEmptyExit       : <2.11 Legacy; <2.12.6 NoError; else Error11
  HtmlSerializer           : >=2.15 SingleLine : Formatted
  ValidationNoDtdExit      : >=2.15 Ok0 : Error3
  GlobalStateInit          : >=2.12 Lazy : Eager
  XslTransform             : Stable (all versions)
```

## Reconciliation with the surface delta engine (11.1-Q)

The Q engine (`atlas/HISTORICAL_SURFACE_EPOCHS.json`) tracks entity-level surface transitions; the table below shows the libxml2 boundaries where the surface moved, and whether the behavioral capability is represented.

### libxml2

| boundary | added | removed | changed |
|---|---|---|---|
| 2.7.8->2.8.0 | 6 | 0 | 5 |
| 2.8.0->2.9.4 | 24 | 0 | 42 |
| 2.9.4->2.9.10 | 5 | 28 | 6 |
| 2.9.10->2.9.14 | 9 | 2 | 48 |
| 2.9.14->2.10.4 | 5 | 74 | 5 |
| 2.10.4->2.11.5 | 11 | 4 | 2 |
| 2.11.5->2.12.6 | 34 | 28 | 65 |
| 2.12.6->2.13.0 | 30 | 10 | 17 |
| 2.13.0->2.13.5 | 5 | 0 | 0 |
| 2.13.5->2.14.1 | 83 | 225 | 14 |
| 2.14.1->2.15.0 | 8 | 155 | 978 |

### libxslt

| boundary | added | removed | changed |
|---|---|---|---|
| 1.1.26->1.1.32 | 20 | 4 | 6 |
| 1.1.32->1.1.35 | 6 | 1 | 30 |
| 1.1.35->1.1.38 | 11 | 2 | 22 |
| 1.1.38->1.1.42 | 0 | 0 | 4 |

## Policy

- The candidate's current profile targets the system oracle (libxml2 2.15.3 / libxslt 1.1.45).
- New historical differences must be added as capability epochs with evidence-backed boundaries, not version branches.
- `CompatibilityProfile::for_libxml2` refuses versions newer than the system oracle (no unverifiable epochs).

