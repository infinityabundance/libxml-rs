# Parity Matrix

Generated from `PARITY_MATRIX.json` (§70). Last updated: 2026-08-20.

## API Completeness

### libxml2 2.15.3

| Surface | Headers | DSO | Both | H-only | DSO-only | Status |
|---------|---------|-----|------|--------|----------|--------|
| Functions | 1403 | 1658 | **1344** | 53 | 290 | 🟢 81% |
| Globals | 17 | 89 | **17** | 0 | 72 | 🟢 100% of captured |
| System leaks | — | — | — | **0** | — | ✅ Clean |
| Internal `__xml*` | 6 | — | — | 6 | — | ✅ Classified |
| SAX1 callbacks | — | 24 | — | — | 24 | ✅ Classified |
| Typedefs | 312 | — | — | — | — | 📋 Cataloged |
| Records (struct/union) | 76 | — | — | — | — | 📋 Cataloged |
| Enums | 39 | — | — | — | — | 📋 Cataloged |
| Enumerators | 1131 | — | — | — | — | 📋 Cataloged |
| Callbacks | 86 | — | — | — | — | 📋 Cataloged |
| Headers processed | 45 | — | — | — | — | ✅ Complete |

### libxslt 1.1.45

| Surface | Headers | DSO | Both | H-only | DSO-only | Status |
|---------|---------|-----|------|--------|----------|--------|
| Functions | 231 | 232 | **225** | 6 | 7 | 🟢 97% |
| Globals | 12 | 39 | **12** | 0 | 27 | 🟢 100% of captured |
| System leaks | — | — | — | **0** | — | ✅ Clean |
| Typedefs | 63 | — | — | — | — | 📋 Cataloged |
| Records | 16 | — | — | — | — | 📋 Cataloged |
| Enums | 7 | — | — | — | — | 📋 Cataloged |
| Enumerators | 65 | — | — | — | — | 📋 Cataloged |
| Callbacks | 17 | — | — | — | — | 📋 Cataloged |
| Headers processed | 23 | — | — | — | — | ✅ Complete |

## Tools Status

| Tool | Status | Notes |
|------|--------|-------|
| `manifest.py` | ✅ Working | 183 libxml2 + 92 libxslt releases cataloged |
| `profileconfig.py` | ✅ Working | Generates xmlversion.h/xsltconfig.h for distro profile |
| `apiatlas.py` | ✅ Working | Clang-AST-based public API extractor. Fixes applied: (1) origin resolution uses loc.file/loc.includedFrom instead of buggy #line mapping, (2) XML_TREE_INTERNALS define for tree.h circular dependency, (3) system function name denylist |
| `symbols.py` | ✅ Working | ABI ground-truth comparison via readelf. Classifies SAX1 callbacks, internal __xml* functions, system leaks |
| `delta.py` | ✅ Built | Ready for multi-version API diffing |
| `court runner` | ✅ Built | Ready for differential testing (needs Docker oracle) |

## Infrastructure Status

| Component | Status | Notes |
|-----------|--------|-------|
| Docker oracle | 🟡 Scaffolded | Dockerfile and build script created, not yet built |
| Court receipts | 🟡 Scaffolded | Schema and runner created, no cases executed yet |
| Historical API matrix | 🟡 Partial | Only 2.15.3 + 1.1.45 snapshots taken |
| Historical deltas | 🔴 Not started | Need at least 2 versions per project |
| Downstream consumers | 🔴 Not started | |
