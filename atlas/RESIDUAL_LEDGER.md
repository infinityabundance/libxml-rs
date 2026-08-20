# Residual Ledger

Per §71: every unexplained difference gets an ID (`R-000001`...), and its
history is retained after fixing. This Markdown is generated from
`RESIDUAL_LEDGER.json` (§70 policy: Markdown generated from JSON).

## Current Residuals

**0 open residuals.** All discovered tooling bugs have been fixed.

## Fixed Residuals

### R-000001: `#line` directive mapping uses wrong coordinate space

- **Status:** FIXED
- **Component:** `tools/archaeology/apiatlas.py`
- **Surface:** tooling
- **Root cause:** The `resolve_origin` function used the original source line number from `#line` directives as dict keys. When multiple directives shared the same original line number, they overwrote each other, causing incorrect file attribution. Furthermore, the `#line` mapping approach is fundamentally flawed because `loc.line` from clang's AST is in the **original source file** coordinate space, while `#line` directive positions are in the **preprocessed output** coordinate space — these are different and cannot be compared directly.
- **Fix:** Replaced the `#line` mapping approach entirely. The new `resolve_origin` uses clang's AST location fields directly: (1) `loc.file` for type declarations from included files, (2) `loc.includedFrom` presence to detect function declarations from included files (filtered out — they'll be captured when their own header is processed), (3) absence of both for direct declarations in the main file.
- **Evidence:** 45 system functions leaked into header inventory; most HTML functions (44+) were missing; tree functions were missing.

### R-000002: System header path filtering missing

- **Status:** FIXED
- **Component:** `tools/archaeology/apiatlas.py`
- **Surface:** tooling
- **Root cause:** No explicit filter for system header paths in the `collect()` function. When `resolve_origin` returned `None` (for declarations from included files), the caller didn't handle it.
- **Fix:** Added `None` check for origin, and added a comprehensive system function name denylist (`SYSTEM_FUNCTION_NAMES`) as a secondary defense for declarations that bypass origin-based filtering.
- **Evidence:** 45 system functions (fopen, fprintf, printf, etc.) in header inventory.

### R-000003: Internal `__xml*` functions not classified

- **Status:** FIXED
- **Component:** `tools/archaeology/symbols.py`
- **Surface:** tooling
- **Root cause:** Internal `__xml*` function declarations (the implementations behind public function-pointer variables) were listed alongside potentially missing API functions.
- **Fix:** Added `INTERNAL_FUNCTIONS` set and separate reporting in `internal_functions` field.
- **Evidence:** 6 `__xml*` functions now correctly classified.

### R-000004: SAX1 callback names not classified

- **Status:** FIXED
- **Component:** `tools/archaeology/symbols.py`
- **Surface:** tooling
- **Root cause:** SAX1 callback struct field names appeared in DSO symbol tables as OBJECT type but were listed as undocumented function exports.
- **Fix:** Added `SAX1_CALLBACK_NAMES` set and separate reporting in `sax1_callbacks` field.
- **Evidence:** 24 SAX1 callback names now correctly classified.

### R-000005: `XML_TREE_INTERNALS` not defined when processing tree.h

- **Status:** FIXED
- **Component:** `tools/archaeology/apiatlas.py`
- **Surface:** tooling
- **Root cause:** `tree.h` has a circular dependency workaround: when `XML_TREE_INTERNALS` is not defined, it just includes `parser.h` and hides all tree declarations. Other headers (parser.h, entities.h, valid.h, xmlIO.h) define this before including tree.h, but when processing tree.h directly, the define was missing.
- **Fix:** Added `-DXML_TREE_INTERNALS` to clang include args globally.
- **Evidence:** tree.h showed 0 FunctionDecl declarations; `xmlAddChild` and all tree functions were missing from the API inventory.

## Classification Legend

- `CANDIDATE_BUG` — Bug in the libxml-rs tooling
- `ORACLE_BUG` — Bug in the upstream implementation
- `VERSION_DIFFERENCE` — Difference due to version mismatch
- `INTENTIONAL_SAFE_DIVERGENCE` — Known safe difference
- `UNRESOLVED` — Not yet classified
