//! build.rs — libxml-rs artifact generation (§1, §11)
//!
//! This build script handles:
//! 1. Generating the correct DSO names (libxml2.so, libxslt.so, libexslt.so)
//! 2. Creating pkg-config files for downstream consumers
//! 3. Platform-specific naming conventions
//! 4. SONAME handling
//! 5. Generating C header compatibility wrappers
//! 6. Installing the header tree (libxml/, libxslt/, libexslt/, libxml2/libxml)
//!
//! The Rust crate produces liblibxml_rs.so as a cdylib (the single combined
//! core, SONAME libxml2.so.16) and liblibxml_rs.a as a staticlib. This build
//! script creates the installation contract — an oracle-faithful
//! `lib/ include/ bin/` layout under the artifact directory — so that
//! downstream build systems find `libxml2.so` / `libxslt.so` / `libexslt.so`
//! as expected.
//!
//! THE THREE-DSO CONTRACT (11.1-Z.1): upstream ships three DSOs with three
//! distinct SONAMEs and a dependency chain (libexslt.so.0 NEEDs libxslt.so.1
//! and libxml2.so.16; libxslt.so.1 NEEDs libxml2.so.16). Cargo can only emit
//! one cdylib per link, so the two facade DSOs are generated POST-LINK from
//! the freshly linked staticlib by `facade-gen.sh` (invoked through a linker
//! wrapper installed via `cargo:linker` once the staticlib exists — i.e. at
//! the first bin/test/example link of every build):
//!
//!   lib/libxslt.so.1.1.45   real ELF DSO, SONAME libxslt.so.1, exports the
//!                           xslt* surface (version-script filtered), NEEDs
//!                           libxml2.so.16 (upstream-faithful dependency).
//!   lib/libexslt.so.0.8.25  real ELF DSO, SONAME libexslt.so.0, exports the
//!                           exslt* surface, NEEDs libxslt.so.1 + libxml2.so.16.
//!
//! The facades are whole-core re-links (--whole-archive of the staticlib), so
//! every xslt/exslt export is a real definition — consumers link with plain
//! `-lxslt`/`-lexslt` exactly as with upstream. The version scripts give each
//! facade the oracle's per-DSO export surface instead of leaking the whole
//! combined core. build.rs also regenerates the facades synchronously at its
//! start when the artifacts exist and the facades are stale (covers rebuilds
//! that do not re-link a bin). On non-Linux targets, or when the staticlib is
//! not yet present (first `--lib`-only build), the symlink fallback keeps the
//! names resolvable to the core until a full build produces the facades.
//!
//! DISTRIBUTION CONTRACT (11.1-Z.2, R-000177): the full Git repository
//! (with `.cargo/config.toml` and `tools/packaging/`) is the C-custodian
//! distribution — it builds the real three-DSO contract above. The crates.io
//! package excludes `tools/` and `.cargo/` (see `Cargo.toml` `exclude`), so a
//! published-crate build uses the default linker and the symlink fallback:
//! every SONAME name resolves to the single core `liblibxml_rs.so` instance.
//! The whole-archive facades are the only consumer-linkable construction
//! (thin re-export facades fail the consumer static link and the core cannot
//! satisfy the facades' references to the crate's internal symbols), which
//! carries a documented bounded divergence: the facades hold private copies
//! of the core state, so hooks/globals installed through one DSO are not
//! observed by the others. The DSO-STATE-COHERENCE court pins this exact
//! partition profile; the DSO-LOADER court verifies the ELF identities.
//!
//! The layout mirrors the upstream libtool installation (see
//! `oracle/historical/prefix/*/oracle-manifest.json` and the `lib/*.la` files):
//!
//! ```text
//! <artifact>/
//!   lib/
//!     libxml2.so.16.1.3 -> ../liblibxml_rs.so   (real core DSO; 17:3:1)
//!     libxml2.so.16     -> libxml2.so.16.1.3
//!     libxml2.so        -> libxml2.so.16
//!     libxml2.a         -> ../liblibxml_rs.a
//!     libxml2.la        (libtool metadata)
//!     libxslt.so.1.1.45 (REAL facade DSO, SONAME libxslt.so.1; 2:45:1)
//!     libxslt.so.1      -> libxslt.so.1.1.45
//!     libxslt.so        -> libxslt.so.1
//!     libxslt.a         -> ../liblibxml_rs.a
//!     libxslt.la
//!     libexslt.so.0.8.25 (REAL facade DSO, SONAME libexslt.so.0; 8:25:8)
//!     libexslt.so.0     -> libexslt.so.0.8.25
//!     libexslt.so       -> libexslt.so.0.8.25
//!     libexslt.a        -> ../liblibxml_rs.a
//!     libexslt.la
//!     xsltConf.sh
//!     libxslt-plugins/
//!     pkgconfig/libxml-2.0.pc libxslt.pc libexslt.pc
//!   include/
//!     libxml/*.h  libxslt/*.h  libexslt/*.h
//!     libxml2/libxml/*.h   (upstream -I${includedir}/libxml2 hierarchy)
//!   bin/
//!     xml2-config xslt-config
//!     xmllint -> ../xmllint   xmlcatalog -> ../xmlcatalog
//!     xsltproc -> ../xsltproc
//! ```
//!
//! Version targets (evidence: `/usr/lib/libxml2.so.16.1.3` = 2.15.3,
//! `/usr/lib/libxslt.so.1.1.45` = 1.1.45; archaeology `configure.ac`
//! libtool version-info math: SONAME = current − age).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

// ═══════════════════════════════════════════════════════════════════════════════
// Target version constants (single source of truth for the packaging contract)
// ═══════════════════════════════════════════════════════════════════════════════

/// libxml2 target: 2.15.3 → libtool version-info 17:3:1 → SONAME libxml2.so.16
const LIBXML2_VERSION: &str = "2.15.3";
const LIBXML2_CURRENT: &str = "17";
const LIBXML2_REVISION: &str = "3";
const LIBXML2_AGE: &str = "1";
const LIBXML2_SO_FILE: &str = "libxml2.so.16.1.3";
const LIBXML2_SONAME: &str = "libxml2.so.16";

/// libxslt target: 1.1.45 → version-info 2:45:1 → SONAME libxslt.so.1
const LIBXSLT_VERSION: &str = "1.1.45";
const LIBXSLT_CURRENT: &str = "2";
const LIBXSLT_REVISION: &str = "45";
const LIBXSLT_AGE: &str = "1";
const LIBXSLT_SO_FILE: &str = "libxslt.so.1.1.45";
const LIBXSLT_SONAME: &str = "libxslt.so.1";

/// libexslt target: 0.8.25 → version-info 8:25:8 → SONAME libexslt.so.0
const LIBEXSLT_VERSION: &str = "0.8.25";
const LIBEXSLT_CURRENT: &str = "8";
const LIBEXSLT_REVISION: &str = "25";
const LIBEXSLT_AGE: &str = "8";
const LIBEXSLT_SO_FILE: &str = "libexslt.so.0.8.25";
const LIBEXSLT_SONAME: &str = "libexslt.so.0";

/// The actual shared library produced by cargo.
const CANDIDATE_SO: &str = "liblibxml_rs.so";
/// The actual static library produced by cargo.
const CANDIDATE_A: &str = "liblibxml_rs.a";

fn main() {
    // Only run in release mode or when explicitly enabled
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));

    // Determine the target directory
    let target_dir = find_target_dir(&out_dir);

    // Determine artifact directory (where final libraries will be placed)
    let artifact_dir = if profile == "release" {
        target_dir.join("release")
    } else {
        target_dir.join("debug")
    };

    // Generate pkg-config files (canonical lib/pkgconfig + compat pkgconfig/)
    generate_pkgconfig(&artifact_dir);

    // Whether the three-DSO facade contract is active on this build (Linux
    // native target).
    let three_dso_active = three_dso_active();

    // Generate SONAME symlinks (canonical lib/ chains + top-level compat).
    // When the facade contract is active, stale symlinks at the facade paths
    // are cleared here so facade-gen.sh owns them.
    generate_symlinks(&artifact_dir, three_dso_active);

    // Install the three-DSO machinery and synchronously regenerate the
    // libxslt/libexslt facade DSOs whenever the core artifacts already exist
    // (rebuilds). Must run AFTER generate_symlinks so the cleared paths are
    // re-filled before any consumer could look at them. On a fresh build the
    // staticlib does not exist yet and the linker wrapper (see
    // tools/packaging/linker-wrapper.sh + .cargo/config.toml) performs the
    // generation at the first bin/test/example link.
    install_three_dso_machinery(&artifact_dir, three_dso_active);

    // Generate xml2-config and xslt-config scripts (bin/ + top-level compat)
    generate_config_scripts(&artifact_dir);

    // Generate libtool metadata: .la archives, xsltConf.sh, plugins dir
    generate_libtool_files(&artifact_dir);

    // Install the C header tree into include/
    install_headers(&artifact_dir);

    // The cdylib (the combined core) carries the upstream libxml2 SONAME
    // (libxml2.so.16) so that consumers record the same NEEDED entry as
    // binaries linked against the oracle (readelf -d
    // /usr/lib/libxml2.so.16.1.3 -> SONAME libxml2.so.16). The libxslt and
    // libexslt SONAMEs come from the post-link facade DSOs (see
    // install_three_dso_machinery). The lib/ chains resolve every SONAME at
    // runtime.
    println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libxml2.so.16");

    // Phase 12 (EXPORT-SURFACE-DISPOSITION): the core gets an explicit
    // version-script export map (tools/packaging/libxml2.syms) so that only
    // the dispositioned surface is dynamic — every #[no_mangle] Rust helper
    // not in the current-oracle / historical-compat / extension planes is
    // hidden (local: *). The map carries the upstream LIBXML2_2.x
    // named-version chain (libxml2-2.13.5/libxml2.syms) plus the terminal
    // LIBXML2_2.15.0 node, so both unversioned (executed-oracle) consumers
    // and versioned-distro binaries bind. The published-crate fallback (no
    // tools/) keeps the symlink layout without the map, as documented.
    if three_dso_active {
        let syms = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap_or_default())
            .join("tools/packaging/libxml2.syms");
        if syms.is_file() {
            println!(
                "cargo:rustc-cdylib-link-arg=-Wl,--version-script={}",
                syms.display()
            );
            println!("cargo:rerun-if-changed=tools/packaging/libxml2.syms");
        }
    }

    // Print cargo instructions
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=include/");

    // Set metadata for downstream crates
    println!("cargo:rustc-cfg=libxml_rs");
}

/// Whether the three-DSO facade contract is active on this build (Linux
/// native target). Cross builds and non-Linux hosts keep the symlink
/// fallback in generate_symlinks.
#[cfg(unix)]
fn three_dso_active() -> bool {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let host = env::var("HOST").unwrap_or_default();
    let target = env::var("TARGET").unwrap_or_default();
    target_os == "linux" && target == host
}

#[cfg(not(unix))]
fn three_dso_active() -> bool {
    false
}

/// Install the three-DSO machinery.
///
/// Cargo links the package's cdylib BEFORE it archives the staticlib, so no
/// build-script step can see both artifacts at once. The facade DSOs are
/// therefore generated by `tools/packaging/facade-gen.sh` (single source of
/// truth, committed), driven by the custom linker installed through
/// `.cargo/config.toml` (`tools/packaging/linker-wrapper.sh`): the first
/// link where the staticlib exists (a bin/test/example link — bins are
/// linked only after the lib target completes) re-links the whole core into
/// the two facade DSOs with their own SONAMEs, the oracle's per-DSO export
/// surface (version scripts) and the upstream NEEDED chain:
///
///   libxslt.so.1  NEEDs libxml2.so.16
///   libexslt.so.0 NEEDs libxslt.so.1, libxml2.so.16
///
/// This function additionally performs a SYNCHRONOUS regeneration at the
/// start of the build whenever the artifacts already exist (rebuilds,
/// including `cargo build --lib`), so the facades converge without a bin
/// link. On non-Linux or cross builds the symlink fallback keeps the names
/// resolvable to the core.
#[cfg(unix)]
fn install_three_dso_machinery(artifact_dir: &Path, three_dso_active: bool) {
    if !three_dso_active {
        return;
    }

    // Synchronous regeneration on rebuilds: when the artifacts already exist
    // (e.g. a second `cargo build` after a previous full build), refresh the
    // facades right now instead of waiting for the next bin link. On a fresh
    // build the staticlib does not exist yet and the linker wrapper handles
    // it. tools/ is excluded from the crates.io package, so on published-
    // crate builds this script is absent and the symlink fallback applies.
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let facade_gen = PathBuf::from(&manifest_dir)
        .join("tools")
        .join("packaging")
        .join("facade-gen.sh");
    let core = artifact_dir.join(CANDIDATE_SO);
    let archive = artifact_dir.join(CANDIDATE_A);
    if facade_gen.exists() && core.exists() && archive.exists() {
        let _ = std::process::Command::new("sh")
            .arg(&facade_gen)
            .arg(artifact_dir)
            .output();
    }
}

#[cfg(not(unix))]
fn install_three_dso_machinery(_artifact_dir: &Path, _three_dso_active: bool) {}

/// Find the target directory by walking up from OUT_DIR.
fn find_target_dir(out_dir: &Path) -> PathBuf {
    // OUT_DIR is typically target/<profile>/build/<crate>/out
    // Walk up to find the target directory
    let mut current = out_dir.to_path_buf();
    loop {
        if current.ends_with("target") || current.file_name().is_some_and(|n| n == "target") {
            return current;
        }
        if !current.pop() {
            // Fallback: use Cargo's CARGO_MANIFEST_DIR/target
            let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
            return PathBuf::from(manifest_dir).join("target");
        }
    }
}

/// Write `content` to `path`, creating parent directories.
fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    fs::write(path, content).ok();
    println!("cargo:warning=Generated {:?}", path);
}

/// Generate pkg-config files for libxml-2.0, libxslt and libexslt.
///
/// Contract reference: the libtool-generated files captured from upstream
/// `oracle/historical/prefix/libxml2-2.15.0/lib/pkgconfig/` and
/// `oracle/historical/prefix/libxslt-1.1.42/lib/pkgconfig/`. Field names,
/// `Name:`/`Description:`/`Requires:` relationships and `Libs:` content
/// mirror upstream; only `Version:` tracks the candidate target versions and
/// `prefix`/`libdir` resolve to the artifact directory.
///
/// Both the canonical `lib/pkgconfig/` location (upstream layout) and a
/// top-level `pkgconfig/` copy (used by earlier-phase probe tooling) are
/// written.
fn generate_pkgconfig(artifact_dir: &Path) {
    let prefix = artifact_dir.display().to_string();

    // libxml-2.0.pc — oracle 2.15.0 shape: Name libXML, modules=1,
    // Libs carries -lm (upstream libxml2 has undefined libm references).
    let libxml_pc = format!(
        r#"prefix={prefix}
exec_prefix=${{prefix}}
libdir=${{exec_prefix}}/lib
includedir=${{prefix}}/include
modules=1

Name: libXML
Version: {LIBXML2_VERSION}
Description: libXML library version2.
Requires:
Libs: -L${{libdir}} -lxml2     -lm
Cflags: -I${{includedir}}/libxml2
"#
    );

    // libxslt.pc — oracle 1.1.42 shape.
    let libxslt_pc = format!(
        r#"prefix={prefix}
exec_prefix=${{prefix}}
libdir=${{exec_prefix}}/lib
includedir=${{prefix}}/include

Name: libxslt
Version: {LIBXSLT_VERSION}
Description: XSLT library version 2.
Requires: libxml-2.0
Cflags: -I${{includedir}}
Libs: -L${{libdir}} -lxslt
Libs.private: -lm
"#
    );

    // libexslt.pc — oracle 1.1.42 shape (EXSLT has its own version line).
    let libexslt_pc = format!(
        r#"prefix={prefix}
exec_prefix=${{prefix}}
libdir=${{exec_prefix}}/lib
includedir=${{prefix}}/include

Name: libexslt
Version: {LIBEXSLT_VERSION}
Description: EXSLT Extension library
Requires: libxml-2.0, libxslt
Cflags: -I${{includedir}}
Libs: -L${{libdir}} -lexslt
Libs.private: -lm
"#
    );

    for dir in [
        artifact_dir.join("lib").join("pkgconfig"),
        artifact_dir.join("pkgconfig"),
    ] {
        write_file(&dir.join("libxml-2.0.pc"), &libxml_pc);
        write_file(&dir.join("libxslt.pc"), &libxslt_pc);
        write_file(&dir.join("libexslt.pc"), &libexslt_pc);
    }
}

/// Create a symbolic link, replacing any existing path (including broken
/// symlinks — `Path::exists()` follows links and returns false for dangling
/// ones, so we must probe with `symlink_metadata`).
#[cfg(unix)]
fn create_symlink(link_name: &str, target: &str, dir: &Path) {
    let link_path = dir.join(link_name);
    if fs::symlink_metadata(&link_path).is_ok() {
        fs::remove_file(&link_path).ok();
    }
    std::os::unix::fs::symlink(target, &link_path).ok();
}

#[cfg(not(unix))]
fn create_symlink(_link_name: &str, _target: &str, _dir: &Path) {
    // Symlinks not supported on this platform
}

/// Remove a legacy artifact path if it exists (used to clean up names from
/// earlier build script versions that no longer belong to the contract).
#[cfg(unix)]
fn remove_if_symlink_or_file(path: &Path) {
    if fs::symlink_metadata(path).is_ok() {
        fs::remove_file(path).ok();
    }
}

#[cfg(not(unix))]
fn remove_if_symlink_or_file(_path: &Path) {}

/// Generate SONAME symlink chains.
///
/// On Linux, upstream installs (2.13+ / 1.1.x):
///   libxml2.so -> libxml2.so.16 -> libxml2.so.16.1.3
///   libxslt.so -> libxslt.so.1 -> libxslt.so.1.1.45
///   libexslt.so -> libexslt.so.0 -> libexslt.so.0.8.25
///
/// The canonical chains live in `<artifact>/lib/` and point (via relative
/// symlinks) at the real DSO in the artifact root. Top-level compat links
/// (`libxml2.so` etc. in the artifact root) preserve the flat layout that
/// earlier-phase probe tooling links against with `-L target/debug`.
fn generate_symlinks(artifact_dir: &Path, three_dso_active: bool) {
    let lib_dir = artifact_dir.join("lib");
    fs::create_dir_all(&lib_dir).ok();

    // When the three-DSO machinery is active, the libxslt/libexslt facade
    // paths must hold REAL DSOs written by facade-gen.sh, never symlinks.
    // Remove any leftover symlink from earlier build-script versions or from
    // a run where the machinery was inactive, so the facade generator owns
    // the path.
    if three_dso_active {
        remove_if_symlink_or_file(&lib_dir.join(LIBXSLT_SO_FILE));
        remove_if_symlink_or_file(&lib_dir.join(LIBEXSLT_SO_FILE));
    }

    let actual_lib = artifact_dir.join(CANDIDATE_SO);
    if !actual_lib.exists() {
        // Library hasn't been built yet (this runs before compilation)
        println!(
            "cargo:warning=Library not yet built: {:?} — symlinks will be (re)created on a later build",
            actual_lib
        );
    }

    // ── libxml2 2.15.3 chain (SONAME libxml2.so.16) ──────────────────────
    // The core cdylib itself carries SONAME libxml2.so.16; the chain resolves
    // the runtime name to the real DSO in the artifact root.
    create_symlink(LIBXML2_SO_FILE, &format!("../{CANDIDATE_SO}"), &lib_dir);
    create_symlink(LIBXML2_SONAME, LIBXML2_SO_FILE, &lib_dir);
    create_symlink("libxml2.so", LIBXML2_SONAME, &lib_dir);

    // ── libxslt 1.1.45 chain (SONAME libxslt.so.1) ────────────────────────
    // libxslt.so.1.1.45 is a REAL facade DSO written post-link by
    // facade-gen.sh (see install_three_dso_machinery); when the machinery is
    // active that path was cleared above and the links below are created
    // before the facade exists (dangling until it lands, exactly like the
    // libxml2 chain points at the not-yet-linked core). When the machinery is
    // inactive (non-Linux/cross), fall back to pointing the chain at the
    // combined core so the names stay resolvable.
    if three_dso_active {
        create_symlink(LIBXSLT_SONAME, LIBXSLT_SO_FILE, &lib_dir);
        create_symlink("libxslt.so", LIBXSLT_SONAME, &lib_dir);
    } else {
        create_symlink(LIBXSLT_SO_FILE, &format!("../{CANDIDATE_SO}"), &lib_dir);
        create_symlink(LIBXSLT_SONAME, LIBXSLT_SO_FILE, &lib_dir);
        create_symlink("libxslt.so", LIBXSLT_SONAME, &lib_dir);
    }

    // ── libexslt 0.8.25 chain (SONAME libexslt.so.0) ──────────────────────
    // Upstream: libexslt.so -> libexslt.so.0 -> libexslt.so.0.8.25.
    if three_dso_active {
        create_symlink(LIBEXSLT_SONAME, LIBEXSLT_SO_FILE, &lib_dir);
        create_symlink("libexslt.so", LIBEXSLT_SO_FILE, &lib_dir);
    } else {
        create_symlink(LIBEXSLT_SO_FILE, &format!("../{CANDIDATE_SO}"), &lib_dir);
        create_symlink(LIBEXSLT_SONAME, LIBEXSLT_SO_FILE, &lib_dir);
        create_symlink("libexslt.so", LIBEXSLT_SO_FILE, &lib_dir);
    }

    // ── Static library names (upstream installs libxml2.a/libxslt.a/libexslt.a)
    let static_lib = artifact_dir.join(CANDIDATE_A);
    if static_lib.exists() {
        create_symlink("libxml2.a", &format!("../{CANDIDATE_A}"), &lib_dir);
        create_symlink("libxslt.a", &format!("../{CANDIDATE_A}"), &lib_dir);
        create_symlink("libexslt.a", &format!("../{CANDIDATE_A}"), &lib_dir);
    }

    // ── Top-level compat links for `-L target/debug -lxml2` (probe tooling)
    create_symlink("libxml2.so", "lib/libxml2.so", artifact_dir);
    create_symlink("libxslt.so", "lib/libxslt.so", artifact_dir);
    create_symlink("libexslt.so", "lib/libexslt.so", artifact_dir);

    // Top-level VERSIONED compat links: the DSO carries the upstream SONAME
    // libxml2.so.16, so a consumer NEEDs that name at runtime. The canonical
    // chains live in lib/; these top-level links let `LD_LIBRARY_PATH=<artifact>`
    // resolve the versioned names too (without them the loader would silently
    // fall back to a system libxml2 — contamination, caught by the 11.1-T
    // DSO-LOADER court).
    create_symlink("libxml2.so.16", "lib/libxml2.so.16", artifact_dir);
    create_symlink("libxslt.so.1", "lib/libxslt.so.1", artifact_dir);
    create_symlink("libexslt.so.0", "lib/libexslt.so.0", artifact_dir);

    // ── Legacy cleanup: names created by earlier build script versions that
    // are no longer part of the contract. Remove the top-level chains and the
    // old wrong-version names so audits see exactly the current contract.
    // (libxslt.so.1 / libexslt.so.0 / libxml2.so.16 top-level links are the
    // current SONAME compat links, created above — not legacy.)
    for legacy in [
        "libxml2.so.2",
        "libxml2.so.2.12.0",
        "libxml2.so.2.15.3",
        "libxslt.so.1.1.39",
        "libxslt.so.1.1.47",
        "libexslt.so.0.1.1.47",
        "libexslt.so.0.8.25",
        "libxml2.a",
        "libxslt.a",
        "libexslt.a",
        "libxml2.la",
        "libxslt.la",
        "libexslt.la",
        "xsltConf.sh",
    ] {
        remove_if_symlink_or_file(&artifact_dir.join(legacy));
    }

    println!("cargo:warning=Generated SONAME symlinks in {:?}", lib_dir);
}

/// Install the C header tree into `<artifact>/include/`.
///
/// Upstream layout: `include/libxml2/libxml/*.h` (libxml2),
/// `include/libxslt/*.h` and `include/libexslt/*.h` (libxslt). The candidate
/// keeps its own headers flat under `include/libxml/` and provides the
/// upstream `include/libxml2/libxml/` hierarchy as real copies so both
/// `-I$includedir` and `-I$includedir/libxml2` resolve `<libxml/...>`.
fn install_headers(artifact_dir: &Path) {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let src_include = PathBuf::from(&manifest_dir).join("include");
    let dst_include = artifact_dir.join("include");
    fs::create_dir_all(&dst_include).ok();

    // Flat copies: include/libxml, include/libxslt, include/libexslt
    for sub in ["libxml", "libxslt", "libexslt"] {
        copy_dir(&src_include.join(sub), &dst_include.join(sub));
    }

    // Upstream hierarchy: include/libxml2/libxml (real copies, no symlinks so
    // the tree is self-contained when copied by consumers).
    copy_dir(
        &src_include.join("libxml"),
        &dst_include.join("libxml2").join("libxml"),
    );

    println!("cargo:warning=Installed headers into {:?}", dst_include);
}

/// Recursively copy a directory tree (files only).
fn copy_dir(src: &Path, dst: &Path) {
    let Ok(entries) = fs::read_dir(src) else {
        return;
    };
    fs::create_dir_all(dst).ok();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            copy_dir(&path, &dst.join(entry.file_name()));
        } else {
            let Ok(content) = fs::read(&path) else {
                continue;
            };
            fs::write(dst.join(entry.file_name()), content).ok();
        }
    }
}

/// Generate xml2-config and xslt-config shell scripts.
///
/// These are structural copies of the upstream autoconf-generated scripts
/// (see `oracle/historical/prefix/libxml2-2.15.0/bin/xml2-config` and
/// `oracle/historical/prefix/libxslt-1.1.42/bin/xslt-config`): same option
/// set, same `--libs`/`--cflags` output shape (including the `-lm` that
/// upstream emits), with the libdir pointing at `<artifact>/lib`.
///
/// Canonical location is `bin/`; top-level compat copies are symlinks.
fn generate_config_scripts(artifact_dir: &Path) {
    let prefix = artifact_dir.display().to_string();
    let bin_dir = artifact_dir.join("bin");
    fs::create_dir_all(&bin_dir).ok();

    // xml2-config
    let xml2_config = format!(
        r#"#! /bin/sh

prefix={prefix}
exec_prefix=${{prefix}}
includedir=${{prefix}}/include
libdir=${{exec_prefix}}/lib
cflags=
libs=

usage()
{{
    cat <<EOF
Usage: xml2-config [OPTION]

Known values for OPTION are:

  --prefix=DIR		change libxml prefix [default $prefix]
  --exec-prefix=DIR	change libxml exec prefix [default $exec_prefix]
  --libs		print library linking information
                        add --dynamic to print only shared libraries
  --cflags		print pre-processor and compiler flags
  --modules		module support enabled
  --help		display this help and exit
  --version		output version information
EOF

    exit $1
}}

if test $# -eq 0; then
    usage 1
fi

while test $# -gt 0; do
    case "$1" in
    -*=*) optarg=`echo "$1" | sed 's/[-_a-zA-Z0-9]*=//'` ;;
    *) optarg= ;;
    esac

    case "$1" in
    --prefix=*)
	prefix=$optarg
	includedir=$prefix/include
	libdir=$prefix/lib
	;;

    --prefix)
	echo $prefix
	;;

    --exec-prefix=*)
      exec_prefix=$optarg
      libdir=$exec_prefix/lib
      ;;

    --exec-prefix)
      echo $exec_prefix
      ;;

    --version)
	echo {LIBXML2_VERSION}
	exit 0
	;;

    --help)
	usage 0
	;;

    --cflags)
        cflags="-I${{includedir}}/libxml2 "
       	;;

    --libtool-libs)
	if [ -r ${{libdir}}/libxml2.la ]
	then
	    echo ${{libdir}}/libxml2.la
	fi
        ;;

    --modules)
       	echo 1
       	;;

    --libs)
        if [ "$2" = "--dynamic" ]; then
            shift
            libs="-lxml2     -lm"
        else
            libs="-lxml2     -lm "
        fi

        if [ "${{exec_prefix}}/lib" != "/usr/lib" -a "${{exec_prefix}}/lib" != "/usr/lib64" ]; then
            libs="-L${{libdir}} $libs"
        fi
        ;;

    *)
	usage 1
	;;
    esac
    shift
done

if test -n "$cflags$libs"; then
    echo $cflags $libs
fi

exit 0
"#
    );
    write_file(&bin_dir.join("xml2-config"), &xml2_config);
    make_executable(&bin_dir.join("xml2-config"));

    // xslt-config
    let xslt_config = format!(
        r#"#! /bin/sh

prefix={prefix}
exec_prefix=${{prefix}}
exec_prefix_set=no
includedir=${{prefix}}/include
libdir=${{exec_prefix}}/lib

usage()
{{
    cat <<EOF
Usage: xslt-config [OPTION]...

Known values for OPTION are:

  --prefix=DIR		change XSLT prefix [default $prefix]
  --exec-prefix=DIR	change XSLT executable prefix [default $exec_prefix]
  --libs		print library linking information
                        add --dynamic to print only shared libraries
  --cflags		print pre-processor and compiler flags
  --plugins		print plugin directory
  --help		display this help and exit
  --version		output version information
EOF

    exit $1
}}

if test $# -eq 0; then
    usage 1
fi

while test $# -gt 0; do
    case "$1" in
    -*=*) optarg=`echo "$1" | sed 's/[-_a-zA-Z0-9]*=//'` ;;
    *) optarg= ;;
    esac

    case "$1" in
    --prefix=*)
	prefix=$optarg
        includedir=${{prefix}}/include
        libdir=${{prefix}}/lib
	if test $exec_prefix_set = no ; then
	    exec_prefix=$optarg
	fi
	;;

    --prefix)
	echo $prefix
	;;

    --exec-prefix=*)
	exec_prefix=$optarg
	exec_prefix_set=yes
	;;

    --exec-prefix)
	echo $exec_prefix
	;;

    --version)
	echo {LIBXSLT_VERSION}
	exit 0
	;;

    --plugins)
	echo ${{libdir}}/libxslt-plugins
	exit 0
	;;

    --help)
	usage 0
	;;

    --cflags)
        cflags="-I${{includedir}} "
       	;;

    --libs)
        if [ "$2" = "--dynamic" ]; then
            shift
            libs="-lxslt -lxml2 -lm"
        else
            libs="-lxslt -lxml2 -lm "
        fi

        if [ "-L${{libdir}}" != "-L/usr/lib" -a "-L${{libdir}}" != "-L/usr/lib64" ]; then
            libs="-L${{libdir}} $libs"
        fi

        libs="$libs "
       	;;

    *)
	usage
	exit 1
	;;
    esac
    shift
done

all_flags="$cflags $libs"

if test -z "$all_flags" || test "x$all_flags" = "x "; then
    exit 1
fi

# Straight out any possible duplicates, but be careful to
# get `-lfoo -lbar -lbaz' for `-lfoo -lbaz -lbar -lbaz'
other_flags=
rev_libs=
for i in $all_flags; do
    case "$i" in
    # a library, save it for later, in reverse order
    -l*) rev_libs="$i $rev_libs" ;;
    *)
	case " $other_flags " in
	*\ $i\ *) ;;				# already there
	*) other_flags="$other_flags $i" ;;	# add it to output
        esac ;;
    esac
done

ord_libs=
for i in $rev_libs; do
    case " $ord_libs " in
    *\ $i\ *) ;;			# already there
    *) ord_libs="$i $ord_libs" ;;	# add it to output in reverse order
    esac
done

echo $other_flags $ord_libs

exit 0
"#
    );
    write_file(&bin_dir.join("xslt-config"), &xslt_config);
    make_executable(&bin_dir.join("xslt-config"));

    // Top-level compat copies (earlier-phase probe tooling invokes
    // `target/debug/xml2-config` directly).
    create_symlink("xml2-config", "bin/xml2-config", artifact_dir);
    create_symlink("xslt-config", "bin/xslt-config", artifact_dir);

    // Tool executable links in bin/ (cargo builds them into the artifact
    // root; a dangling link briefly until the next build step completes).
    create_symlink("xmllint", "../xmllint", &bin_dir);
    create_symlink("xmlcatalog", "../xmlcatalog", &bin_dir);
    create_symlink("xsltproc", "../xsltproc", &bin_dir);
}

/// Generate libtool metadata files: `.la` archives, `xsltConf.sh` and the
/// `libxslt-plugins` directory.
///
/// The `.la` files mirror the upstream libtool output (see
/// `oracle/historical/prefix/libxml2-2.15.0/lib/libxml2.la` and the
/// libxslt-1.1.42 equivalents): the static archive is `libxml2.a` etc., the
/// version fields record the candidate target version-info, and `libdir`
/// points at the artifact `lib/`.
fn generate_libtool_files(artifact_dir: &Path) {
    let lib_dir = artifact_dir.join("lib");
    fs::create_dir_all(&lib_dir).ok();
    let libdir = lib_dir.display().to_string();

    let la = |name: &str,
              proj: &str,
              current: &str,
              revision: &str,
              age: &str,
              deps: &str,
              dlname: &str,
              library_names: &str| {
        format!(
            r#"# {name}.la - a libtool library file
# Generated by libtool (GNU libtool) 2.4.7
#
# Please DO NOT delete this file!
# It is necessary for linking the library.

# The name that we can dlopen(3).
dlname='{dlname}'

# Names of this library.
library_names='{library_names}'

# The name of the static archive.
old_library='{name}.a'

# Linker flags that cannot go in dependency_libs.
inherited_linker_flags=''

# Libraries that this one depends upon.
dependency_libs='{deps}'

# Names of additional weak libraries provided by this library
weak_library_names=''

# Version information for {proj}.
current={current}
age={age}
revision={revision}

# Is this an already installed library?
installed=yes

# Should we warn about portability when linking against -modules?
shouldnotlink=no

# Files to dlopen/dlpreopen
dlopen=''
dlpreopen=''

# Directory that this library needs to be installed in:
libdir='{libdir}'
"#
        )
    };

    // The .la archives list the real facade DSOs so libtool consumers (PHP's
    // build) resolve -lxml2/-lxslt/-lexslt through them. The soname and full
    // versioned names match the facade DSOs generated by cargo rustc.
    write_file(
        &lib_dir.join("libxml2.la"),
        &la(
            "libxml2",
            "libxml2",
            LIBXML2_CURRENT,
            LIBXML2_REVISION,
            LIBXML2_AGE,
            " -lm",
            "libxml2.so.16",
            "libxml2.so.16.1.3 libxml2.so.16 libxml2.so",
        ),
    );
    write_file(
        &lib_dir.join("libxslt.la"),
        &la(
            "libxslt",
            "libxslt",
            LIBXSLT_CURRENT,
            LIBXSLT_REVISION,
            LIBXSLT_AGE,
            " -lm",
            "libxslt.so.1",
            "libxslt.so.1.1.45 libxslt.so.1 libxslt.so",
        ),
    );
    write_file(
        &lib_dir.join("libexslt.la"),
        &la(
            "libexslt",
            "libexslt",
            LIBEXSLT_CURRENT,
            LIBEXSLT_REVISION,
            LIBEXSLT_AGE,
            " -lm",
            "libexslt.so.0",
            "libexslt.so.0.8.25 libexslt.so.0 libexslt.so",
        ),
    );

    // xsltConf.sh — upstream ships this beside libxslt.la.
    let xslt_conf = format!(
        r#"#
# Configuration file for using the xslt library
#
XSLT_LIBDIR="-L{libdir}"
XSLT_LIBS="-lxslt -L{libdir} -lxml2 -lm "
XSLT_PRIVATE_LIBS="-lm"
XSLT_INCLUDEDIR="-I{prefix}/include"
MODULE_VERSION="xslt-{LIBXSLT_VERSION}"
"#,
        prefix = artifact_dir.display()
    );
    write_file(&lib_dir.join("xsltConf.sh"), &xslt_conf);

    // libxslt-plugins directory (xslt-config --plugins contract).
    fs::create_dir_all(lib_dir.join("libxslt-plugins")).ok();
}

/// Make a file executable.
#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(metadata) = fs::metadata(path) {
        let mut perms = metadata.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).ok();
    }
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}
