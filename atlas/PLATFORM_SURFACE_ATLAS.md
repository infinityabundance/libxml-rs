# Platform Surface Atlas — 11.1-U

**Claim: SURFACE-COMPLETE: every platform-conditional API/ABI family identified by source archaeology is classified below. RUNTIME-EXECUTED only on Linux x86-64 (the reference system); every other platform carries an explicit documented obligation (see obligations) with a stable residual state (R-000168).**

## Platforms

| id | OS | Arch | Status | Surface summary |
|---|---|---|---|---|
| `linux-x86-64` | Linux | x86-64 | **RUNTIME-EXECUTED** | glibc 2. |
| `bsd-family` | FreeBSD / NetBSD / OpenBSD | amd64/i386 etc. | **DOCUMENTED-ONLY (unexecuted)** | Upstream uses the POSIX thread/dlopen paths (no _WIN32). |
| `macos-darwin` | macOS / Darwin | arm64/x86-64 | **DOCUMENTED-ONLY (unexecuted)** | Upstream: encoding. |
| `windows` | Windows | x86-64/x86 | **DOCUMENTED-ONLY (unexecuted)** | Upstream: HAVE_WIN32_THREADS (CriticalSection in threads. |
| `unix-posix-other` | Solaris / AIX / HP-UX | sparc/ppc/ia64 etc. | **DOCUMENTED-ONLY (unexecuted)** | Upstream: HAVE_SHLLOAD (Solaris module loading in xmlmodule. |
| `word-size-32` | Linux 32-bit / i686 | x86 / arm / ppc 32-bit | **COMPILE-EXPECTED (cargo check --target i686/armv7 clean; runtime unexecuted)** | The C API is int-based (xmlStrlen -> int, xmlNode. |
| `aarch64` | Linux aarch64 | arm64 | **COMPILE-EXPECTED (cargo check --target aarch64 clean; runtime unexecuted)** | 11. |
| `musl` | Linux musl (Alpine) | x86-64 | **COMPILE-EXPECTED (cargo check --target x86_64-unknown-linux-musl clean; runtime unexecuted)** | 11. |
| `wasm32` | wasm32-unknown-unknown | wasm | **NOT-APPLICABLE (no OS/dlopen/filesystem; C-ABI drop-in contract does not apply)** | wasm32 has no dynamic loader or filesystem; the libxml2. |
| `endianness` | big-endian (s390x, ppc64 BE, sparc) | any | **SAFE-BY-DESIGN + DOCUMENTED-ONLY (unexecuted)** | Internal strings are UTF-8 (endian-independent). |
| `compiler-exports` | gcc / clang / MSVC | any | **PARTIALLY-EXECUTED (gcc+clang on Linux)** | include/libxml/xmlexports. |

## Upstream conditional families (source archaeology)

| Family | Guard macros | Candidate status |
|---|---|---|
| `threads` | `HAVE_POSIX_THREADS / HAVE_WIN32_THREADS` | Rust std threads (platform-agnostic); Win32 thread model unexecuted. |
| `thread-local-storage` | `USE_TLS, USE_WAIT_DTOR, USE_DLL_MAIN` | Rust TLS (std); per-platform destructor semantics unexecuted. |
| `file-io` | `_WIN32, HAVE_DECL_MMAP, HAVE_DECL_GLOB, HAVE_DECL_GETENTROPY` | Rust std::fs; mmap/glob paths unexecuted. |
| `module-loading` | `HAVE_DLOPEN / _WIN32 / HAVE_SHLLOAD` | Rust libloading-free stubs; unexecuted outside Linux. |
| `encoding-iconv` | `__APPLE__` | Rust encoding core; Darwin iconv path unexecuted. |
| `locale` | `XSLT_LOCALE_POSIX / XSLT_LOCALE_WINAPI / XSLT_LOCALE_NONE, HAVE_STRXFRM_L, HAVE_XLOCALE_H` | Rust locale handling; WinAPI locale unexecuted. |
| `printf-fallback` | `XSLT_NEED_TRIO` | Rust std::fmt; trio fallback not applicable (no C printf). |
| `export-macros` | `_WIN32/__CYGWIN__ + LIBXML_STATIC/IN_LIBXML; XMLCALL; _MSC_VER` | Candidate header carries the full macro set; MSVC compile unexecuted. |
| `config-detection` | `HAVE_*_H, SIZEOF_*, STDC_HEADERS` | N/A — Rust compile-time environment; config.h obligations documented only. |

## Candidate cfg surface

- build.rs: #[cfg(unix)] create_symlink / make_executable / remove_if_symlink_or_file; #[cfg(not(unix))] no-op fallbacks
- build.rs lib_name: linux -> liblibxml_rs.so; macos -> liblibxml_rs.dylib; windows -> libxml_rs.dll
- Cargo.toml crate-type: cdylib + staticlib + rlib (all platforms)
- src/: no cfg(target_os) in library code — the Rust implementation is platform-agnostic

## Execution obligations (stable residual state)

| Obligation | Platform | Detail | Status |
|---|---|---|---|
| `OBLIG-PLATFORM-WIN32` | `windows` | DLL export surface (dynsym equivalent), Win32 thread model, file IO path mapping, XMLCALL=__cdecl, dllexport/dllimport exercised. | **DOCUMENTED-ONLY** |
| `OBLIG-PLATFORM-DARWIN` | `macos-darwin` | libxml2.2.dylib naming + install_name chains; iconv state path. | **DOCUMENTED-ONLY** |
| `OBLIG-PLATFORM-BSD` | `bsd-family` | libm linkage (Libs.private -lm), dlopen behavior, court suite execution. | **DOCUMENTED-ONLY** |
| `OBLIG-WORDSIZE-32` | `word-size-32` | COMPILE-EXPECTED achieved (cargo check --target i686/armv7 clean after 11.1-U fixes); runtime court execution on 32-bit remains the open obligation. | **COMPILE-EXPECTED; runtime unexecuted** |
| `OBLIG-AARCH64` | `aarch64` | COMPILE-EXPECTED achieved (cargo check --target aarch64 clean); runtime unexecuted. | **COMPILE-EXPECTED; runtime unexecuted** |
| `OBLIG-MUSL` | `musl` | COMPILE-EXPECTED achieved (cargo check --target x86_64-unknown-linux-musl clean); cdylib not produced on musl (staticlib/rlib only). | **COMPILE-EXPECTED; runtime unexecuted** |
| `OBLIG-BIG-ENDIAN` | `endianness` | Endian-safe by construction (UTF-8 internals, parameterized codecs, macro NaN); execution on s390x/ppc64 BE when available. | **DOCUMENTED-ONLY** |
| `OBLIG-COMPILER-MSVC` | `compiler-exports` | MSVC __cdecl/deprecated/declspec branches compile-verified. | **DOCUMENTED-ONLY** |

## Residual

- **R-000168** — OPEN (stable documented state; word-size-32/aarch64/musl now COMPILE-EXPECTED with evidence).
  Closure condition: Each OBLIG-PLATFORM-* obligation executes its court suite on its target (or a cross-compile layout probe where execution is impossible), flipping status to RUNTIME-EXECUTED or COMPILE-EXPECTED with evidence.

## Evidence

- oracle/historical/src/libxml2-2.15.0/{threads,xmlIO,encoding,xmlmodule,xpath,globals}.c conditional families (harvested counts in conditional_families)
- archaeology/libxslt-git/libxslt/{xsltlocale,security,transform,attributes}.c: XSLT_LOCALE_*/XSLT_NEED_TRIO/HAVE_MSVCRT
- oracle/historical/prefix/libxml2-2.15.0/include/libxml2/libxml/xmlversion.h feature macros (LIBXML_*_ENABLED)
- oracle/historical/src/libxml2-2.15.0/config.h (26 configure-time defines)
- oracle/historical/doxygen/configs.json oracle_configs (Doxygen preprocessor macro configs per version)
- include/libxml/xmlexports.h (candidate export macros, upstream-identical structure)
- build.rs + Cargo.toml (candidate cfg surface)
- cargo check --target {i686,aarch64,armv7,x86_64-musl}-unknown-linux-gnu: 0 errors after 11.1-U portability fixes (streamed-error fallback, c_ulong/c_long widths, c_char buffers, LC_ALL_MASK, time_t)
