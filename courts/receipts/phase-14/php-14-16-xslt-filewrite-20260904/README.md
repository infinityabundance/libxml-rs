# php-14.16 — xsltSaveResultToFilename routed through the core output-buffer DSO (2026-09-04)

Full six-gate **28 → 26 failed**, ZERO regressions (name-level diff vs the
`phpbuild-c:/out/xpe-six31.log` baseline: **NEW_ONLY=0, FIXED=2**). Log:
`phpbuild-c:/out/xpe-six32.log`. xsl 9 → 7 | dom 10 | xmlreader 8 |
xmlwriter 1. Commit on main (Phase 14.16).

## Root cause — xsltSaveResultToFilename opened destinations with raw fopen (~2 tests)
`xsltSaveResultToFilename` did `libc::fopen(URL, "wb")`, so
`transformToURI($xsldoc, 'php://output')` failed to open (`php:` has no such
dir) and returned `-1`, and `transformToURI` writing `out.xml` through the
test's `file://`… URI also failed — `xslt007.phpt` then saw no file at all.
Upstream xsltutils.c opens the destination with
`xmlOutputBufferCreateFilename`, which under PHP dispatches to the stream
loader PHP registered (php://output works; plain paths and file URIs open as
PHP streams); `xmlOutputBufferClose` yields the total byte count (int(56)).

The XSLT code runs inside the `libxslt.so.1` whole-archive facade, whose
private copy of the `xmlOutputBufferCreateFilenameDefault` static is never
populated by PHP (PHP hooks the core `libxml2.so.16` copy — the R-000177
cross-DSO partition). FIX: resolve the core's exported
`xmlOutputBufferCreateFilename`/`xmlOutputBufferWrite`/`xmlOutputBufferClose`
with `dlsym(handle, …)` on `libxml2.so.16` (RTLD_NOLOAD — always already
loaded through the facade's NEEDED chain), serialize via the existing
`xsltSaveResultToString`, write, and return the core close byte count. A plain
`fopen` fallback covers staticlib/CLI builds where no core DSO is loaded.

## Guards / validation
- cargo test --lib 1241 pass / 1 ignored; clippy clean; fmt clean.
- Targeted phpt green: xslt007, xsltprocessor_transformToURI (batch rc=0).

## Residuals next (26)
dom 10 | xsl 7 (bug54446 pair = saxon output security dispatch; bug71571_a/b
= recursion depth/vars diagnostics; gh21357_2 = xmlns-ns attr copy;
req30622 = namespaced params; xinclude doXInclude heap crash) | xmlreader 8
(schema set/relaxNG + reader state) | xmlwriter 1 (SHIFT_JIS output
transcoding).
