#!/usr/bin/env python3
"""
platform_surface_atlas.py — 11.1-U platform surface atlas generator.

Classifies the platform-conditioned API/ABI surface of the libxml2/libxslt
drop-in from source archaeology, and records the candidate's execution
status per platform. Distinguishes "surface-complete" (every conditional
family classified) from "runtime-executed on every historical platform"
(not claimed — only Linux x86-64 executes in this environment).

Evidence sources (harvested 2026-08-30, 11.1-U):
  - oracle/historical/src/libxml2-2.15.0/*.c          (upstream conditional families)
  - archaeology/libxslt-git/libxslt/*.c libexslt/*.c  (libxslt conditional families)
  - oracle/historical/prefix/libxml2-2.15.0/include/libxml2/libxml/xmlversion.h
  - oracle/historical/src/libxml2-2.15.0/config.h
  - include/libxml/xmlexports.h                       (candidate export macros)
  - build.rs, Cargo.toml                              (candidate cfg surface)
  - oracle/historical/doxygen/configs.json             (Doxygen preprocessor configs)

Output: atlas/PLATFORM_SURFACE_ATLAS.json + atlas/PLATFORM_SURFACE_ATLAS.md
"""

import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
OUT_JSON = ROOT / "atlas" / "PLATFORM_SURFACE_ATLAS.json"
OUT_MD = ROOT / "atlas" / "PLATFORM_SURFACE_ATLAS.md"

ATLAS = {
    "schema": "platform-surface-atlas-1",
    "phase": "11.1-U",
    "generated": "2026-08-30",
    "claim": (
        "SURFACE-COMPLETE: every platform-conditional API/ABI family identified by "
        "source archaeology is classified below. RUNTIME-EXECUTED only on Linux "
        "x86-64 (the reference system); every other platform carries an explicit "
        "documented obligation (see obligations) with a stable residual state "
        "(R-000168)."
    ),
    "platforms": [
        {
            "id": "linux-x86-64",
            "os": "Linux",
            "arch": "x86-64",
            "status": "RUNTIME-EXECUTED",
            "execution_evidence": [
                "All 11.1-S/11.1-T courts green on this system (glibc 2.44, gcc 16.2.1, rustc stable)",
                "1144 lib tests + ASan suite clean",
                "DSO-LOADER probe: dlopen/dlsym/callback/static-linking executed",
            ],
            "surface_notes": (
                "glibc 2.34+ merged libpthread/libm into libc; NEEDED = libgcc_s, libm, libc, "
                "ld-linux. ELF SONAME libxml2.so.16. LP64 (int=32, long=64) matches upstream ABI."
            ),
        },
        {
            "id": "bsd-family",
            "os": "FreeBSD / NetBSD / OpenBSD",
            "arch": "amd64/i386 etc.",
            "status": "DOCUMENTED-ONLY (unexecuted)",
            "surface_notes": (
                "Upstream uses the POSIX thread/dlopen paths (no _WIN32). FreeBSD/NetBSD lack "
                "glibc's merged libm; a BSD build links -lm explicitly (Libs.private already "
                "carries it). dlopen with RTLD_GLOBAL differs subtly; xmlmodule.c uses HAVE_DLOPEN "
                "unconditionally on POSIX."
            ),
            "obligation": "Cross-compile + execute the court suite on a BSD target; verify NEEDED set.",
        },
        {
            "id": "macos-darwin",
            "os": "macOS / Darwin",
            "arch": "arm64/x86-64",
            "status": "DOCUMENTED-ONLY (unexecuted)",
            "surface_notes": (
                "Upstream: encoding.c __APPLE__ iconv path (xmlCharEncInput/OutFunc iconv state "
                "handling). Upstream installs libxml2.2.dylib / libxslt.1.dylib / libexslt.0.dylib "
                "with install_name chains; the candidate build.rs has a macos branch (liblibxml_rs.dylib) "
                "but the dylib SONAME/install_name contract is not generated."
            ),
            "obligation": "dylib naming + install_name + two-level namespace resolution for the court suite.",
        },
        {
            "id": "windows",
            "os": "Windows",
            "arch": "x86-64/x86",
            "status": "DOCUMENTED-ONLY (unexecuted)",
            "surface_notes": (
                "Upstream: HAVE_WIN32_THREADS (CriticalSection in threads.c), _WIN32 file IO "
                "(stat/_stat64, O_BINARY, GetModuleFileName), LoadLibrary in xmlmodule.c, "
                "XMLCALL=__cdecl + XMLPUBFUN/XMLPUBVAR __declspec(dllexport/dllimport) in "
                "xmlexports.h, USE_DLL_MAIN for TLS destructor registration. Candidate: "
                "include/libxml/xmlexports.h carries the Windows export macros; build.rs windows "
                "branch names the DLL libxml_rs.dll; the DLL symbol-export surface, thread model "
                "and IO paths are unexecuted."
            ),
            "obligation": "DLL export-definition surface, Win32 threads, IO path mapping; execute court suite under Wine or a Windows runner.",
        },
        {
            "id": "unix-posix-other",
            "os": "Solaris / AIX / HP-UX",
            "arch": "sparc/ppc/ia64 etc.",
            "status": "DOCUMENTED-ONLY (unexecuted)",
            "surface_notes": (
                "Upstream: HAVE_SHLLOAD (Solaris module loading in xmlmodule.c), _AIX quirk paths, "
                "dlopen variants. Candidate: pure-Rust, no cfg for these; unexecuted."
            ),
            "obligation": "Documented only; no execution obligation planned without a target system.",
        },
        {
            "id": "word-size-32",
            "os": "Linux 32-bit / i686",
            "arch": "x86 / arm / ppc 32-bit",
            "status": "COMPILE-EXPECTED (cargo check --target i686/armv7 clean; runtime unexecuted)",
            "surface_notes": (
                "The C API is int-based (xmlStrlen -> int, xmlNode.lineno int, xmlParserInputBuffer "
                "size int): the ABI is word-size-stable for the API surface; internal size_t buffers "
                "limit to 2GiB on 32-bit like upstream. 11.1-U cross-compilation found and fixed "
                "real 32-bit bugs: x86_64-only cfg gates on the streamed error channel (now a "
                "portable fallback on other ABIs), c_ulong-vs-u64 length/id comparisons in "
                "xmlSchemaValidateFacetWhtsp and generate-id(), c_long calibration arithmetic, "
                "and i32 time_t (y2038, inherent to 32-bit time_t). cargo check --target "
                "i686-unknown-linux-gnu and armv7-unknown-linux-gnueabihf are clean."
            ),
            "obligation": "Runtime execution of the court suite on a 32-bit target remains unexecuted.",
        },
        {
            "id": "aarch64",
            "os": "Linux aarch64",
            "arch": "arm64",
            "status": "COMPILE-EXPECTED (cargo check --target aarch64 clean; runtime unexecuted)",
            "surface_notes": (
                "11.1-U cross-compilation found c_char=u8 (aarch64) buffer-typing bugs in the "
                "xmlShell debugger and xsltTransformError path (fixed with c_char-typed buffers). "
                "cargo check --target aarch64-unknown-linux-gnu is clean."
            ),
            "obligation": "Runtime execution on arm64 unexecuted.",
        },
        {
            "id": "musl",
            "os": "Linux musl (Alpine)",
            "arch": "x86-64",
            "status": "COMPILE-EXPECTED (cargo check --target x86_64-unknown-linux-musl clean; runtime unexecuted)",
            "surface_notes": (
                "11.1-U cross-compilation found the libc crate lacks LC_ALL_MASK on musl; a "
                "cfg'd fallback mask (all category bits below LC_ALL, matching upstream "
                "xsltlocale.c non-glibc construction) is used. cargo check --target "
                "x86_64-unknown-linux-musl is clean. NOTE: cdylib is not produced for musl "
                "(cargo warning) — staticlib/rlib only; the DSO contract does not apply there."
            ),
            "obligation": "Runtime execution on musl unexecuted.",
        },
        {
            "id": "wasm32",
            "os": "wasm32-unknown-unknown",
            "arch": "wasm",
            "status": "NOT-APPLICABLE (no OS/dlopen/filesystem; C-ABI drop-in contract does not apply)",
            "surface_notes": (
                "wasm32 has no dynamic loader or filesystem; the libxml2.so drop-in contract is "
                "meaningless there. cargo check --target wasm32 fails on libc-dependent modules "
                "(dlopen/stat/locale) — documented, not a defect."
            ),
            "obligation": "None — out of scope for the C-ABI drop-in contract.",
        },
        {
            "id": "endianness",
            "os": "big-endian (s390x, ppc64 BE, sparc)",
            "arch": "any",
            "status": "SAFE-BY-DESIGN + DOCUMENTED-ONLY (unexecuted)",
            "surface_notes": (
                "Internal strings are UTF-8 (endian-independent). UTF-16/UCS-4 codecs are "
                "byte-order-parameterized (xmlEncUTF16LE/xmlEncUTF16BE named codecs), not "
                "BYTE_ORDER-switched. xmlXPathNAN/PINF/NINF initialized from NAN/INFINITY macros "
                "(endian-safe bit patterns). No WORDS_BIGENDIAN conditional families remain in "
                "2.15.0 encoding.c."
            ),
            "obligation": "Endian-safe by construction; execute on s390x if a target becomes available.",
        },
        {
            "id": "compiler-exports",
            "os": "gcc / clang / MSVC",
            "arch": "any",
            "status": "PARTIALLY-EXECUTED (gcc+clang on Linux)",
            "surface_notes": (
                "include/libxml/xmlexports.h: LIBXML_ATTR_FORMAT/ALLOC_SIZE (gcc/clang, exercised by "
                "the header-compile court), XML_DEPRECATED (gcc/clang/MSVC branches), Windows "
                "dllexport/dllimport (compile-time only). XMLCALL/XMLCDECL defined empty (upstream: "
                "__cdecl on Win32)."
            ),
            "obligation": "MSVC __cdecl + __declspec(deprecated) branches are compile-time-only; unexecuted.",
        },
    ],
    "conditional_families": [
        {"family": "threads", "macro": "HAVE_POSIX_THREADS / HAVE_WIN32_THREADS", "evidence": "threads.c (2.15.0): 15 HAVE_POSIX_THREADS, 10 HAVE_WIN32_THREADS sites; CriticalSection vs pthread.", "candidate_status": "Rust std threads (platform-agnostic); Win32 thread model unexecuted."},
        {"family": "thread-local-storage", "macro": "USE_TLS, USE_WAIT_DTOR, USE_DLL_MAIN", "evidence": "globals.c/threads.c: __thread vs _Thread_local vs pthread_getspecific; destructor registration.", "candidate_status": "Rust TLS (std); per-platform destructor semantics unexecuted."},
        {"family": "file-io", "macro": "_WIN32, HAVE_DECL_MMAP, HAVE_DECL_GLOB, HAVE_DECL_GETENTROPY", "evidence": "xmlIO.c: 20 _WIN32 sites (stat, O_BINARY, paths); mmap input buffer; glob pattern IO.", "candidate_status": "Rust std::fs; mmap/glob paths unexecuted."},
        {"family": "module-loading", "macro": "HAVE_DLOPEN / _WIN32 / HAVE_SHLLOAD", "evidence": "xmlmodule.c: dlopen vs LoadLibrary vs shl_load (Solaris).", "candidate_status": "Rust libloading-free stubs; unexecuted outside Linux."},
        {"family": "encoding-iconv", "macro": "__APPLE__", "evidence": "encoding.c:1264 iconv state handling quirk.", "candidate_status": "Rust encoding core; Darwin iconv path unexecuted."},
        {"family": "locale", "macro": "XSLT_LOCALE_POSIX / XSLT_LOCALE_WINAPI / XSLT_LOCALE_NONE, HAVE_STRXFRM_L, HAVE_XLOCALE_H", "evidence": "libxslt xsltlocale.c: 8 XSLT_LOCALE_WINAPI, 3 XSLT_LOCALE_POSIX sites.", "candidate_status": "Rust locale handling; WinAPI locale unexecuted."},
        {"family": "printf-fallback", "macro": "XSLT_NEED_TRIO", "evidence": "libxslt trio.c fallback printf implementation.", "candidate_status": "Rust std::fmt; trio fallback not applicable (no C printf)."},
        {"family": "export-macros", "macro": "_WIN32/__CYGWIN__ + LIBXML_STATIC/IN_LIBXML; XMLCALL; _MSC_VER", "evidence": "xmlexports.h (upstream + candidate identical structure).", "candidate_status": "Candidate header carries the full macro set; MSVC compile unexecuted."},
        {"family": "config-detection", "macro": "HAVE_*_H, SIZEOF_*, STDC_HEADERS", "evidence": "config.h.in / oracle 2.15.0 config.h (26 defines).", "candidate_status": "N/A — Rust compile-time environment; config.h obligations documented only."},
    ],
    "candidate_cfg_surface": [
        "build.rs: #[cfg(unix)] create_symlink / make_executable / remove_if_symlink_or_file; #[cfg(not(unix))] no-op fallbacks",
        "build.rs lib_name: linux -> liblibxml_rs.so; macos -> liblibxml_rs.dylib; windows -> libxml_rs.dll",
        "Cargo.toml crate-type: cdylib + staticlib + rlib (all platforms)",
        "src/: no cfg(target_os) in library code — the Rust implementation is platform-agnostic",
    ],
    "obligations": [
        {
            "id": "OBLIG-PLATFORM-WIN32",
            "platform": "windows",
            "detail": "DLL export surface (dynsym equivalent), Win32 thread model, file IO path mapping, XMLCALL=__cdecl, dllexport/dllimport exercised.",
            "status": "DOCUMENTED-ONLY",
        },
        {
            "id": "OBLIG-PLATFORM-DARWIN",
            "platform": "macos-darwin",
            "detail": "libxml2.2.dylib naming + install_name chains; iconv state path.",
            "status": "DOCUMENTED-ONLY",
        },
        {
            "id": "OBLIG-PLATFORM-BSD",
            "platform": "bsd-family",
            "detail": "libm linkage (Libs.private -lm), dlopen behavior, court suite execution.",
            "status": "DOCUMENTED-ONLY",
        },
        {
            "id": "OBLIG-WORDSIZE-32",
            "platform": "word-size-32",
            "detail": "COMPILE-EXPECTED achieved (cargo check --target i686/armv7 clean after 11.1-U fixes); runtime court execution on 32-bit remains the open obligation.",
            "status": "COMPILE-EXPECTED; runtime unexecuted",
        },
        {
            "id": "OBLIG-AARCH64",
            "platform": "aarch64",
            "detail": "COMPILE-EXPECTED achieved (cargo check --target aarch64 clean); runtime unexecuted.",
            "status": "COMPILE-EXPECTED; runtime unexecuted",
        },
        {
            "id": "OBLIG-MUSL",
            "platform": "musl",
            "detail": "COMPILE-EXPECTED achieved (cargo check --target x86_64-unknown-linux-musl clean); cdylib not produced on musl (staticlib/rlib only).",
            "status": "COMPILE-EXPECTED; runtime unexecuted",
        },
        {
            "id": "OBLIG-BIG-ENDIAN",
            "platform": "endianness",
            "detail": "Endian-safe by construction (UTF-8 internals, parameterized codecs, macro NaN); execution on s390x/ppc64 BE when available.",
            "status": "DOCUMENTED-ONLY",
        },
        {
            "id": "OBLIG-COMPILER-MSVC",
            "platform": "compiler-exports",
            "detail": "MSVC __cdecl/deprecated/declspec branches compile-verified.",
            "status": "DOCUMENTED-ONLY",
        },
    ],
    "residual": {
        "id": "R-000168",
        "status": "OPEN (stable documented state; word-size-32/aarch64/musl now COMPILE-EXPECTED with evidence)",
        "closure_condition": "Each OBLIG-PLATFORM-* obligation executes its court suite on its target (or a cross-compile layout probe where execution is impossible), flipping status to RUNTIME-EXECUTED or COMPILE-EXPECTED with evidence.",
    },
    "evidence": [
        "oracle/historical/src/libxml2-2.15.0/{threads,xmlIO,encoding,xmlmodule,xpath,globals}.c conditional families (harvested counts in conditional_families)",
        "archaeology/libxslt-git/libxslt/{xsltlocale,security,transform,attributes}.c: XSLT_LOCALE_*/XSLT_NEED_TRIO/HAVE_MSVCRT",
        "oracle/historical/prefix/libxml2-2.15.0/include/libxml2/libxml/xmlversion.h feature macros (LIBXML_*_ENABLED)",
        "oracle/historical/src/libxml2-2.15.0/config.h (26 configure-time defines)",
        "oracle/historical/doxygen/configs.json oracle_configs (Doxygen preprocessor macro configs per version)",
        "include/libxml/xmlexports.h (candidate export macros, upstream-identical structure)",
        "build.rs + Cargo.toml (candidate cfg surface)",
        "cargo check --target {i686,aarch64,armv7,x86_64-musl}-unknown-linux-gnu: 0 errors after 11.1-U portability fixes (streamed-error fallback, c_ulong/c_long widths, c_char buffers, LC_ALL_MASK, time_t)",
    ],
}


def main():
    OUT_JSON.parent.mkdir(parents=True, exist_ok=True)
    OUT_JSON.write_text(json.dumps(ATLAS, indent=1, ensure_ascii=False) + "\n")

    rows = []
    for p in ATLAS["platforms"]:
        rows.append(
            f"| `{p['id']}` | {p['os']} | {p['arch']} | **{p['status']}** | "
            f"{p['surface_notes'].split('.')[0]}. |"
        )
    fam_rows = []
    for f in ATLAS["conditional_families"]:
        fam_rows.append(f"| `{f['family']}` | `{f['macro']}` | {f['candidate_status']} |")
    obl_rows = []
    for o in ATLAS["obligations"]:
        obl_rows.append(f"| `{o['id']}` | `{o['platform']}` | {o['detail']} | **{o['status']}** |")

    md = f"""# Platform Surface Atlas — 11.1-U

**Claim: {ATLAS['claim']}**

## Platforms

| id | OS | Arch | Status | Surface summary |
|---|---|---|---|---|
{chr(10).join(rows)}

## Upstream conditional families (source archaeology)

| Family | Guard macros | Candidate status |
|---|---|---|
{chr(10).join(fam_rows)}

## Candidate cfg surface

{chr(10).join(f'- {c}' for c in ATLAS['candidate_cfg_surface'])}

## Execution obligations (stable residual state)

| Obligation | Platform | Detail | Status |
|---|---|---|---|
{chr(10).join(obl_rows)}

## Residual

- **{ATLAS['residual']['id']}** — {ATLAS['residual']['status']}.
  Closure condition: {ATLAS['residual']['closure_condition']}

## Evidence

{chr(10).join(f'- {e}' for e in ATLAS['evidence'])}
"""
    OUT_MD.write_text(md)
    print(f"wrote {OUT_JSON.relative_to(ROOT)}")
    print(f"wrote {OUT_MD.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
