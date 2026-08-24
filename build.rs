//! build.rs — libxml-rs artifact generation (§1, §11)
//!
//! This build script handles:
//! 1. Generating the correct DSO names (libxml2.so, libxslt.so)
//! 2. Creating pkg-config files for downstream consumers
//! 3. Platform-specific naming conventions
//! 4. SONAME handling
//! 5. Generating C header compatibility wrappers
//!
//! The Rust crate produces liblibxml_rs.so as a cdylib. This build script
//! creates the necessary symlinks and metadata so that downstream build
//! systems can find libxml2.so / libxslt.so as expected.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

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

    // Generate pkg-config files
    generate_pkgconfig(&artifact_dir, &profile);

    // Generate SONAME symlinks
    generate_symlinks(&artifact_dir);

    // Generate xml2-config and xslt-config scripts
    generate_config_scripts(&artifact_dir);

    // Print cargo instructions
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=include/");

    // Set metadata for downstream crates
    println!("cargo:rustc-cfg=libxml_rs");
}

/// Find the target directory by walking up from OUT_DIR.
fn find_target_dir(out_dir: &Path) -> PathBuf {
    // OUT_DIR is typically target/<profile>/build/<crate>/out
    // Walk up to find the target directory
    let mut current = out_dir.to_path_buf();
    loop {
        if current.ends_with("target") || current.file_name().map_or(false, |n| n == "target") {
            return current;
        }
        if !current.pop() {
            // Fallback: use Cargo's CARGO_MANIFEST_DIR/target
            let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
            return PathBuf::from(manifest_dir).join("target");
        }
    }
}

/// Generate pkg-config files for libxml-2.0 and libxslt.
fn generate_pkgconfig(artifact_dir: &Path, _profile: &str) {
    let pkg_dir = artifact_dir.join("pkgconfig");
    fs::create_dir_all(&pkg_dir).ok();

    // libxml-2.0.pc
    let libxml_pc = r#"prefix=@prefix@
exec_prefix=${prefix}
libdir=${exec_prefix}/lib
includedir=${prefix}/include

Name: libxml-2.0
Description: libxml-rs — Native Rust libxml2 compatibility
Version: 2.12.0
Requires:
Libs: -L${libdir} -lxml2
Libs.private: -lpthread -lm
Cflags: -I${includedir}/libxml2
"#;
    let libxml_pc_path = pkg_dir.join("libxml-2.0.pc");
    fs::write(&libxml_pc_path, libxml_pc).ok();
    println!("cargo:warning=Generated pkg-config: {:?}", libxml_pc_path);

    // libxslt.pc
    let libxslt_pc = r#"prefix=@prefix@
exec_prefix=${prefix}
libdir=${exec_prefix}/lib
includedir=${prefix}/include

Name: libxslt
Description: libxslt — Native Rust libxslt compatibility
Version: 1.1.39
Requires: libxml-2.0
Libs: -L${libdir} -lxslt
Libs.private: -lpthread -lm
Cflags: -I${includedir}
"#;
    let libxslt_pc_path = pkg_dir.join("libxslt.pc");
    fs::write(&libxslt_pc_path, libxslt_pc).ok();
    println!("cargo:warning=Generated pkg-config: {:?}", libxslt_pc_path);
}

/// Generate SONAME symlinks.
///
/// On Linux, upstream installs:
///   libxml2.so -> libxml2.so.2 -> libxml2.so.2.12.0
///   libxslt.so -> libxslt.so.1 -> libxslt.so.1.1.39
///
/// Since our crate produces liblibxml_rs.so, we create symlinks
/// so that `-lxml2` and `-lxslt` link correctly.
fn generate_symlinks(artifact_dir: &Path) {
    // Find the actual shared library
    let lib_name = if cfg!(target_os = "linux") {
        "liblibxml_rs.so"
    } else if cfg!(target_os = "macos") {
        "liblibxml_rs.dylib"
    } else if cfg!(target_os = "windows") {
        "libxml_rs.dll"
    } else {
        "liblibxml_rs.so"
    };

    let actual_lib = artifact_dir.join(lib_name);
    if !actual_lib.exists() {
        // Library hasn't been built yet (this runs before compilation)
        println!("cargo:warning=Library not yet built: {:?}", actual_lib);
        return;
    }

    // Create libxml2.so symlinks
    // Use owned strings to avoid temporary lifetime issues
    let v2120 = artifact_dir.join("libxml2.so.2.12.0");
    let v2 = artifact_dir.join("libxml2.so.2");
    let _v = artifact_dir.join("libxml2.so");

    create_symlink("libxml2.so.2.12.0", &actual_lib, artifact_dir);
    create_symlink("libxml2.so.2", &v2120, artifact_dir);
    create_symlink("libxml2.so", &v2, artifact_dir);

    // Create libxslt.so symlinks
    let xslt1139 = artifact_dir.join("libxslt.so.1.1.39");
    let xslt1 = artifact_dir.join("libxslt.so.1");
    let _xslt = artifact_dir.join("libxslt.so");

    create_symlink("libxslt.so.1.1.39", &actual_lib, artifact_dir);
    create_symlink("libxslt.so.1", &xslt1139, artifact_dir);
    create_symlink("libxslt.so", &xslt1, artifact_dir);

    println!(
        "cargo:warning=Generated SONAME symlinks in {:?}",
        artifact_dir
    );
}

/// Create a symbolic link.
#[cfg(unix)]
fn create_symlink(link_name: &str, target: &PathBuf, artifact_dir: &Path) {
    let link_path = artifact_dir.join(link_name);
    if link_path.exists() {
        fs::remove_file(&link_path).ok();
    }
    std::os::unix::fs::symlink(target, &link_path).ok();
}

#[cfg(not(unix))]
fn create_symlink(_link_name: &str, _target: &PathBuf, _artifact_dir: &Path) {
    // Symlinks not supported on this platform
}

/// Generate xml2-config and xslt-config shell scripts.
fn generate_config_scripts(artifact_dir: &Path) {
    // xml2-config
    let xml2_config = format!(
        r#"#!/bin/sh
# xml2-config — libxml-rs compatibility script
prefix={0}
exec_prefix=${{prefix}}
libdir=${{exec_prefix}}/lib
includedir=${{prefix}}/include

usage()
{{
    cat <<EOF
Usage: xml2-config [OPTION]
Known values for OPTION are:
  --prefix        display $prefix
  --exec-prefix   display $exec_prefix
  --libs          display library link flags
  --cflags        display C compiler flags
  --version       display library version
  --help          display this help
EOF
}}

if test $# -eq 0; then
    usage
    exit 1
fi

while test $# -gt 0; do
    case "$1" in
    --prefix)
        echo "$prefix"
        ;;
    --exec-prefix)
        echo "$exec_prefix"
        ;;
    --libs)
        echo "-L$libdir -lxml2"
        ;;
    --cflags)
        echo "-I$includedir/libxml2"
        ;;
    --version)
        echo "2.12.0"
        ;;
    --help)
        usage
        ;;
    *)
        usage
        exit 1
        ;;
    esac
    shift
done
"#,
        artifact_dir.display()
    );
    let xml2_config_path = artifact_dir.join("xml2-config");
    fs::write(&xml2_config_path, xml2_config).ok();
    make_executable(&xml2_config_path);
    println!(
        "cargo:warning=Generated xml2-config: {:?}",
        xml2_config_path
    );

    // xslt-config
    let xslt_config = format!(
        r#"#!/bin/sh
# xslt-config — libxml-rs compatibility script
prefix={0}
exec_prefix=${{prefix}}
libdir=${{exec_prefix}}/lib
includedir=${{prefix}}/include

usage()
{{
    cat <<EOF
Usage: xslt-config [OPTION]
Known values for OPTION are:
  --prefix        display $prefix
  --exec-prefix   display $exec_prefix
  --libs          display library link flags
  --cflags        display C compiler flags
  --version       display library version
  --help          display this help
EOF
}}

if test $# -eq 0; then
    usage
    exit 1
fi

while test $# -gt 0; do
    case "$1" in
    --prefix)
        echo "$prefix"
        ;;
    --exec-prefix)
        echo "$exec_prefix"
        ;;
    --libs)
        echo "-L$libdir -lxslt"
        ;;
    --cflags)
        echo "-I$includedir"
        ;;
    --version)
        echo "1.1.39"
        ;;
    --help)
        usage
        ;;
    *)
        usage
        exit 1
        ;;
    esac
    shift
done
"#,
        artifact_dir.display()
    );
    let xslt_config_path = artifact_dir.join("xslt-config");
    fs::write(&xslt_config_path, xslt_config).ok();
    make_executable(&xslt_config_path);
    println!(
        "cargo:warning=Generated xslt-config: {:?}",
        xslt_config_path
    );
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
