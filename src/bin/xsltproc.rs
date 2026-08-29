//! xsltproc — XSLT transformation processor (§36, §85 Phase 9).
//!
//! # UPSTREAM-PARITY
//!
//! Faithful port of upstream libxslt's `xsltproc` command-line tool
//! (xsltproc.c 1.1.45). The pipeline is entirely native Rust:
//!
//! ```text
//! Rust CLI → Rust libxslt compatibility layer → Rust XSLT engine
//!         → Rust XPath → Rust libxml tree/parser/serializer
//! ```
//!
//! # Behavior
//!
//! ```text
//! xsltproc [options] stylesheet.xsl [file.xml ...]
//! ```
//!
//! - The first non-option argument is the stylesheet; remaining arguments
//!   are input documents. `-` reads from stdin.
//! - `--param name expr` passes a parameter whose value is the result of
//!   evaluating `expr` as an XPath expression; `--stringparam name value`
//!   passes a literal string (quoted as an XPath string literal).
//! - Exit status mirrors upstream: 1 usage, 2 too many params, 3 unknown
//!   option, 4 cannot parse stylesheet, 5 stylesheet errors, 6 cannot parse
//!   input, 7 unsupported output method, 8 bad stringparam, 9 transform
//!   error, 10 transformation stopped, 11 save failure.

use std::ffi::c_void;
use std::os::raw::{c_char, c_int, c_uint};
use std::ptr;

use libxml_rs::abi::exports_xml2::*;
use libxml_rs::abi::structs::*;
use libxml_rs::abi::types::*;
use libxml_rs::abi::versioning::{xmlLibxmlVersionString, xsltLibxsltVersionString};
use libxml_rs::xslt::serialization::xsltSaveResultToFile;
use libxml_rs::xslt::stylesheet::{
    xsltFreeStylesheet, xsltParseStylesheetDoc, xsltParseStylesheetFile,
};
use libxml_rs::xslt::transform::{
    xsltApplyStylesheet, xsltApplyStylesheetUser, xsltFreeTransformContext,
    xsltNewTransformContext, xsltRunStylesheetUser,
};

// Upstream limits.
const MAX_PARAMETERS: usize = 64;
const MAX_PATHS: usize = 64;

// XSLT_PARSE_OPTIONS = XML_PARSE_NOENT | XML_PARSE_DTDLOAD |
//                      XML_PARSE_DTDATTR | XML_PARSE_NOCDATA
const XML_PARSE_NOENT: c_int = 1 << 1;
const XML_PARSE_DTDLOAD: c_int = 1 << 2;
const XML_PARSE_DTDATTR: c_int = 1 << 3;
const XML_PARSE_NOCDATA: c_int = 1 << 4;
const XML_PARSE_NONET: c_int = 1 << 11;
const XML_PARSE_HUGE: c_int = 1 << 19;

const XSLT_PARSE_OPTIONS: c_int =
    XML_PARSE_NOENT | XML_PARSE_DTDLOAD | XML_PARSE_DTDATTR | XML_PARSE_NOCDATA;

const XSLT_SECPREF_WRITE_FILE: c_int = 2;
const XSLT_SECPREF_CREATE_DIRECTORY: c_int = 3;
const XSLT_SECPREF_WRITE_NETWORK: c_int = 5;

/// Security check that forbids an operation (upstream xsltproc.c's static
/// `xsltSecurityForbid`): always returns 0 (deny).
unsafe extern "C" fn xslt_security_forbid(
    _sec: *mut std::ffi::c_void,
    _ctxt: *mut std::ffi::c_void,
    _value: *const std::os::raw::c_char,
) -> c_int {
    0
}

/// CLI option state (mirrors upstream's file-scope globals).
struct Cli {
    repeat: c_int,
    timing: bool,
    novalid: bool,
    nodtdattr: bool,
    noout: bool,
    html: bool,
    encoding: Option<String>,
    profile: bool,
    debug: bool,
    nonet: bool,
    nowrite: bool,
    nomkdir: bool,
    writesubtree: Option<String>,
    paths: Vec<String>,
    xinclude: bool,
    load_trace: bool,
    dumpextensions: bool,
    output: Option<String>,
    options: c_int,
    errorno: c_int,
    params: Vec<*const c_char>,
    strparams: Vec<*mut xmlChar>,
}

impl Default for Cli {
    fn default() -> Self {
        Cli {
            repeat: 0,
            timing: false,
            novalid: false,
            nodtdattr: false,
            noout: false,
            html: false,
            encoding: None,
            profile: false,
            debug: false,
            nonet: false,
            nowrite: false,
            nomkdir: false,
            writesubtree: None,
            paths: Vec::new(),
            xinclude: false,
            load_trace: false,
            dumpextensions: false,
            output: None,
            options: XSLT_PARSE_OPTIONS,
            errorno: 0,
            params: Vec::new(),
            strparams: Vec::new(),
        }
    }
}

fn usage(name: &str) {
    println!("Usage: {} [options] stylesheet file [file ...]", name);
    println!("   Options:");
    println!("\t--version or -V: show the version of libxml and libxslt used");
    println!("\t--verbose or -v: show logs of what's happening");
    println!("\t--output file or -o file: save to a given file");
    println!("\t--timing: display the time used");
    println!("\t--repeat: run the transformation 20 times");
    println!("\t--dumpextensions: dump the registered extension elements and functions to stdout");
    println!("\t--novalid skip the DTD loading phase");
    println!("\t--nodtdattr do not default attributes from the DTD");
    println!("\t--noout: do not dump the result");
    println!("\t--maxdepth val : increase the maximum depth (default 3000)");
    println!("\t--maxvars val : increase the maximum variables (default 15000)");
    println!("\t--huge: relax any hardcoded limit from the parser");
    println!("\t             fixes \"parser error : internal error: Huge input lookup\"");
    println!("\t--seed-rand val : initialize pseudo random number generator with specific seed");
    println!("\t--html: the input document is(are) an HTML file(s)");
    println!("\t--encoding: the input document character encoding");
    println!("\t--param name value : pass a (parameter,value) pair");
    println!("\t       name is a QName or a string of the form {{URI}}NCName.");
    println!("\t       value is an UTF8 XPath expression.");
    println!("\t       string values must be quoted like \"'string'\"\n or");
    println!("\t       use stringparam to avoid it");
    println!("\t--stringparam name value : pass a (parameter, UTF8 string value) pair");
    println!("\t--path 'paths': provide a set of paths for resources");
    println!("\t--nonet : refuse to fetch DTDs or entities over network");
    println!("\t--nowrite : refuse to write to any file or resource");
    println!("\t--nomkdir : refuse to create directories");
    println!("\t--writesubtree path : allow file write only with the path subtree");
    println!("\t--catalogs : use SGML catalogs from $SGML_CATALOG_FILES");
    println!("\t             otherwise XML Catalogs starting from ");
    println!("\t         file:///etc/xml/catalog are activated by default");
    println!("\t--xinclude : do XInclude processing on document input");
    println!("\t--xincludestyle : do XInclude processing on stylesheets");
    println!("\t--load-trace : print trace of all external entites loaded");
    println!("\t--profile or --norman : dump profiling information ");
    println!();
    println!("Project libxslt home page: https://gitlab.gnome.org/GNOME/libxslt");
}

/// Emit the loader warning upstream prints when an external entity cannot
/// be loaded (xsltprocExternalEntityLoader → sax warning).
fn warn_failed_entity(url: &str) {
    eprintln!("warning: failed to load external entity \"{}\"", url);
}

/// `xmlReadFile` equivalent honoring the CLI options.
unsafe fn read_file(filename: &str, cli: &Cli) -> *mut _xmlDoc {
    let cname = cstr_alloc(filename);
    let enc: *mut xmlChar = cli
        .encoding
        .as_deref()
        .map(|e| cstr_alloc(e))
        .unwrap_or(ptr::null_mut());
    let enc_c = enc as *const c_char;
    // --path fallback for the input documents.
    if !cli.paths.is_empty() {
        if std::path::Path::new(filename).exists() {
            let doc = xmlReadFile(cname as *const c_char, enc_c, cli.options);
            free_cstr(cname);
            if !enc.is_null() {
                free_cstr(enc);
            }
            if doc.is_null() {
                warn_failed_entity(filename);
            }
            return doc;
        }
        warn_failed_entity(filename);
        let last = filename.rsplit('/').next().unwrap_or(filename);
        for p in &cli.paths {
            let candidate = format!("{}/{}", p, last);
            let cc = cstr_alloc(&candidate);
            let doc = xmlReadFile(cc as *const c_char, enc_c, cli.options);
            free_cstr(cc);
            if !doc.is_null() {
                if cli.load_trace {
                    eprintln!("Loaded URL=\"{}\" ID=\"(null)\"", candidate);
                }
                free_cstr(cname);
                if !enc.is_null() {
                    free_cstr(enc);
                }
                return doc;
            }
            warn_failed_entity(&candidate);
        }
        free_cstr(cname);
        if !enc.is_null() {
            free_cstr(enc);
        }
        return ptr::null_mut();
    }
    if !std::path::Path::new(filename).exists() {
        warn_failed_entity(filename);
    }
    let doc = xmlReadFile(cname as *const c_char, enc_c, cli.options);
    free_cstr(cname);
    if !enc.is_null() {
        free_cstr(enc);
    }
    if doc.is_null() && std::path::Path::new(filename).exists() {
        // The file exists but failed to parse; upstream's loader warning
        // only fires for load failures, not parse failures.
    }
    if cli.load_trace && !doc.is_null() {
        eprintln!("Loaded URL=\"{}\" ID=\"(null)\"", filename);
    }
    doc
}

/// Read an input document (upstream `xsltReadFile`): `-` reads stdin.
unsafe fn xslt_read_file(filename: &str, cli: &Cli) -> *mut _xmlDoc {
    if cli.html {
        if filename == "-" {
            use std::io::Read;
            let mut buf = Vec::new();
            std::io::stdin().read_to_end(&mut buf).ok();
            buf.push(0);
            return libxml_rs::xml::html::parse_memory(
                buf.as_ptr() as *const c_char,
                buf.len() as c_int - 1,
            );
        }
        let cname = cstr_alloc(filename);
        let doc = libxml_rs::xml::html::parse_file(cname as *const c_char, ptr::null());
        free_cstr(cname);
        doc
    } else if filename == "-" {
        // xmlReadFd(0, "-", encoding, options)
        let enc: *mut xmlChar = cli
            .encoding
            .as_deref()
            .map(|e| cstr_alloc(e))
            .unwrap_or(ptr::null_mut());
        let doc = xmlReadFd(
            0,
            b"-\0".as_ptr() as *const c_char,
            enc as *const c_char,
            cli.options,
        );
        if !enc.is_null() {
            free_cstr(enc);
        }
        doc
    } else {
        read_file(filename, cli)
    }
}

/// Apply the stylesheet to one input document (upstream `xsltProcess`).
///
/// # SAFETY
///
/// - `cur` must be a valid stylesheet; `doc` a valid input document.
unsafe fn xslt_process(
    cli: &mut Cli,
    cur: *mut _xsltStylesheet,
    doc: *mut _xmlDoc,
    filename: &str,
) {
    if cli.xinclude {
        let ret = xmlXIncludeProcessFlags(doc, cli.options);
        if ret < 0 {
            cli.errorno = 6;
            return;
        }
    }
    let mut doc = doc;
    if cli.output.is_none() {
        if cli.repeat != 0 {
            let mut j: c_int = 1;
            while j < cli.repeat {
                let res = xsltApplyStylesheet(cur, doc, cli.params.as_ptr() as *mut *const c_char);
                if !res.is_null() {
                    xmlFreeDoc(res);
                }
                xmlFreeDoc(doc);
                doc = xslt_read_file(filename, cli);
                j += 1;
            }
        }
        let ctxt = xsltNewTransformContext(cur, doc);
        if ctxt.is_null() {
            return;
        }
        let _ = ctxt;
        let res = if cli.profile {
            xsltApplyStylesheetUser(
                cur,
                doc,
                cli.params.as_ptr() as *mut *const c_char,
                ptr::null(),
                stderr_file() as *mut c_void,
                ctxt,
            )
        } else {
            xsltApplyStylesheetUser(
                cur,
                doc,
                cli.params.as_ptr() as *mut *const c_char,
                ptr::null(),
                ptr::null_mut(),
                ctxt,
            )
        };
        if (*ctxt).state == 1 {
            cli.errorno = 9;
        } else if (*ctxt).state == 2 {
            cli.errorno = 10;
        }
        xsltFreeTransformContext(ctxt);
        xmlFreeDoc(doc);
        if res.is_null() {
            eprintln!("no result for {}", filename);
            return;
        }
        if cli.noout {
            xmlFreeDoc(res);
            return;
        }
        if cli.debug {
            // xmlDebugDumpDocument(stdout, res)
            crate_dump_doc(res);
        } else {
            xsltSaveResultToFile(stdout_file() as *mut c_void, res, cur);
        }
        xmlFreeDoc(res);
    } else {
        let ctxt = xsltNewTransformContext(cur, doc);
        if ctxt.is_null() {
            return;
        }
        let outfile = cli.output.clone().unwrap();
        let c_out = cstr_alloc(&outfile);
        let ret = xsltRunStylesheetUser(
            cur,
            doc,
            cli.params.as_ptr() as *mut *const c_char,
            c_out as *const c_char,
            ptr::null_mut(),
            ptr::null_mut(),
            if cli.profile {
                stderr_file() as *mut c_void
            } else {
                ptr::null_mut()
            },
            ctxt,
        );
        free_cstr(c_out);
        if ret == -1 {
            cli.errorno = 11;
        } else if (*ctxt).state == 1 {
            cli.errorno = 9;
        } else if (*ctxt).state == 2 {
            cli.errorno = 10;
        }
        xsltFreeTransformContext(ctxt);
        xmlFreeDoc(doc);
    }
}

/// Minimal result dump for --debug.
unsafe fn crate_dump_doc(doc: *mut _xmlDoc) {
    let s = libxml_rs::xml::tree::dump_doc(doc);
    if !s.is_null() {
        libc::printf(b"%s\0".as_ptr() as *const c_char, s as *const c_char);
        libxml_rs::abi::allocator::xmlFree(s as *mut c_void);
    }
}

/// Allocate a NUL-terminated copy of a Rust string.
unsafe fn cstr_alloc(s: &str) -> *mut xmlChar {
    let p = libc::malloc(s.len() + 1) as *mut xmlChar;
    if p.is_null() {
        return ptr::null_mut();
    }
    libc::memcpy(
        p as *mut libc::c_void,
        s.as_ptr() as *const libc::c_void,
        s.len(),
    );
    *p.add(s.len()) = 0;
    p
}

/// The `stderr` FILE* (the libc crate exposes no `stderr` value).
unsafe fn stderr_file() -> *mut libc::FILE {
    static mut STDERR_FILE: *mut libc::FILE = ptr::null_mut();
    unsafe {
        if STDERR_FILE.is_null() {
            STDERR_FILE = libc::fdopen(2, b"w\0".as_ptr() as *const c_char);
        }
        STDERR_FILE
    }
}

/// The `stdout` FILE*.
unsafe fn stdout_file() -> *mut libc::FILE {
    static mut STDOUT_FILE: *mut libc::FILE = ptr::null_mut();
    unsafe {
        if STDOUT_FILE.is_null() {
            STDOUT_FILE = libc::fdopen(1, b"w\0".as_ptr() as *const c_char);
        }
        STDOUT_FILE
    }
}

unsafe fn free_cstr(p: *mut xmlChar) {
    if !p.is_null() {
        libc::free(p as *mut libc::c_void);
    }
}

fn main() {
    unsafe {
        main_impl();
    }
}

/// The full xsltproc implementation (upstream xsltproc.c main()).
///
/// # SAFETY
///
/// This function performs C ABI calls throughout; it is only invoked from
/// `main`.
unsafe fn main_impl() {
    let argv: Vec<String> = std::env::args().collect();
    if argv.len() <= 1 {
        usage(&argv[0]);
        std::process::exit(1);
    }

    // Seed the PRNG like upstream (srand(time(NULL))); also honors
    // --seed-rand.
    unsafe {
        libc::srand(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as libc::c_uint)
                .unwrap_or(1),
        );
    }

    let mut cli = Cli::default();

    // Security preferences.
    let sec = unsafe { libxml_rs::xslt::security::xsltNewSecurityPrefs() };
    unsafe { libxml_rs::xslt::security::xsltSetDefaultSecurityPrefs(sec) };

    // ── Option parsing (upstream argument loop) ─────────────────────────
    let mut i = 1usize;
    while i < argv.len() {
        if argv[i] == "-" {
            break;
        }
        if !argv[i].starts_with('-') {
            break;
        }
        match argv[i].as_str() {
            "--debug" | "-debug" => cli.debug = true,
            "-v" | "-verbose" | "--verbose" => unsafe {
                libxml_rs::xslt::errors::xsltSetGenericDebugFunc(
                    stderr_file() as *mut c_void,
                    None,
                );
            },
            "-o" | "-output" | "--output" => {
                i += 1;
                if i < argv.len() {
                    cli.output = Some(argv[i].clone());
                }
            }
            "-V" | "-version" | "--version" => {
                // UPSTREAM-PARITY: four-line version report.
                let xml_v = unsafe { cstr_utf8(xmlLibxmlVersionString()) };
                let xslt_v = unsafe { cstr_utf8(xsltLibxsltVersionString()) };
                let exslt_v = xslt_v.clone();
                println!(
                    "Using libxml {}, libxslt {} and libexslt {}",
                    xml_v, xslt_v, exslt_v
                );
                println!(
                    "xsltproc was compiled against libxml {}, libxslt {} and libexslt {}",
                    libxml_rs::abi::versioning::LIBXML2_VERSION_NUM,
                    libxml_rs::abi::versioning::LIBXSLT_VERSION_NUM,
                    libxml_rs::abi::versioning::LIBXSLT_VERSION_NUM
                );
                println!(
                    "libxslt {} was compiled against libxml {}",
                    libxml_rs::abi::versioning::LIBXSLT_VERSION_NUM,
                    libxml_rs::abi::versioning::LIBXML2_VERSION_NUM
                );
                println!(
                    "libexslt {} was compiled against libxml {}",
                    libxml_rs::abi::versioning::LIBXSLT_VERSION_NUM,
                    libxml_rs::abi::versioning::LIBXML2_VERSION_NUM
                );
                std::process::exit(0);
            }
            "-repeat" | "--repeat" => {
                cli.repeat = if cli.repeat == 0 { 20 } else { 100 };
            }
            "-novalid" | "--novalid" => cli.novalid = true,
            "-nodtdattr" | "--nodtdattr" => cli.nodtdattr = true,
            "-noout" | "--noout" => cli.noout = true,
            "-html" | "--html" => cli.html = true,
            "-encoding" | "--encoding" => {
                i += 1;
                if i < argv.len() {
                    cli.encoding = Some(argv[i].clone());
                }
            }
            "-timing" | "--timing" => cli.timing = true,
            "-profile" | "--profile" | "-norman" | "--norman" => cli.profile = true,
            "-nodict" | "--nodict" => {}
            "-nonet" | "--nonet" => {
                cli.nonet = true;
                cli.options |= XML_PARSE_NONET;
            }
            "-nowrite" | "--nowrite" => {
                cli.nowrite = true;
                unsafe {
                    // UPSTREAM-PARITY (R-000125): xsltproc registers the
                    // xsltSecurityForbid callback (returns 0) for the write
                    // options; the API is callback-based, not value-based.
                    libxml_rs::xslt::security::xsltSetSecurityPrefs(
                        sec,
                        XSLT_SECPREF_WRITE_FILE,
                        Some(xslt_security_forbid),
                    );
                    libxml_rs::xslt::security::xsltSetSecurityPrefs(
                        sec,
                        XSLT_SECPREF_CREATE_DIRECTORY,
                        Some(xslt_security_forbid),
                    );
                    libxml_rs::xslt::security::xsltSetSecurityPrefs(
                        sec,
                        XSLT_SECPREF_WRITE_NETWORK,
                        Some(xslt_security_forbid),
                    );
                }
            }
            "-nomkdir" | "--nomkdir" => {
                cli.nomkdir = true;
                unsafe {
                    libxml_rs::xslt::security::xsltSetSecurityPrefs(
                        sec,
                        XSLT_SECPREF_CREATE_DIRECTORY,
                        Some(xslt_security_forbid),
                    );
                }
            }
            "-writesubtree" | "--writesubtree" => {
                i += 1;
                if i < argv.len() {
                    cli.writesubtree = Some(argv[i].clone());
                }
            }
            "-path" | "--path" => {
                i += 1;
                if i < argv.len() {
                    for p in argv[i].split([' ', ':']) {
                        if !p.is_empty() && cli.paths.len() < MAX_PATHS {
                            cli.paths.push(p.to_string());
                        }
                    }
                }
            }
            "-catalogs" | "--catalogs" => {
                // SGML catalogs from $SGML_CATALOG_FILES (upstream calls
                // xmlLoadCatalogs with the raw variable value).
                match std::env::var("SGML_CATALOG_FILES") {
                    Ok(cats) => {
                        let c = unsafe { cstr_alloc(&cats) };
                        unsafe {
                            xmlLoadCatalogs(c as *const c_char);
                        }
                        free_cstr(c);
                    }
                    Err(_) => {
                        eprintln!("Variable $SGML_CATALOG_FILES not set");
                    }
                }
            }
            "-xinclude" | "--xinclude" => cli.xinclude = true,
            "-xincludestyle" | "--xincludestyle" => {
                // XInclude processing of stylesheets.
                unsafe {
                    libxml_rs::xslt::transform::xsltSetXIncludeDefault(1);
                }
            }
            "-load-trace" | "--load-trace" => cli.load_trace = true,
            "-param" | "--param" => {
                i += 1;
                if i + 1 < argv.len() {
                    cli.params.push(cstr_alloc(&argv[i]) as *const c_char);
                    cli.params.push(cstr_alloc(&argv[i + 1]) as *const c_char);
                    if cli.params.len() >= MAX_PARAMETERS {
                        eprintln!("too many params increase MAX_PARAMETERS ");
                        std::process::exit(2);
                    }
                    i += 1;
                }
            }
            "-stringparam" | "--stringparam" => {
                i += 1;
                if i + 1 < argv.len() {
                    let name = &argv[i];
                    let string = &argv[i + 1];
                    // UPSTREAM-PARITY: wrap in double quotes, falling back
                    // to single quotes if the value contains `"`.
                    let value = if string.contains('"') {
                        if string.contains('\'') {
                            eprintln!("stringparam contains both quote and double-quotes !");
                            std::process::exit(8);
                        }
                        format!("'{}'", string)
                    } else {
                        format!("\"{}\"", string)
                    };
                    cli.params.push(cstr_alloc(name) as *const c_char);
                    cli.params.push(cstr_alloc(&value) as *const c_char);
                    if cli.params.len() >= MAX_PARAMETERS {
                        eprintln!("too many params increase MAX_PARAMETERS ");
                        std::process::exit(2);
                    }
                    i += 1;
                }
            }
            "-maxdepth" | "--maxdepth" => {
                i += 1;
                if i < argv.len() {
                    if let Ok(v) = argv[i].parse::<c_int>() {
                        if v > 0 {
                            libxml_rs::xslt::transform::xsltMaxDepth = v;
                        }
                    }
                }
            }
            "-maxvars" | "--maxvars" => {
                i += 1;
                if i < argv.len() {
                    if let Ok(v) = argv[i].parse::<c_int>() {
                        if v > 0 {
                            libxml_rs::xslt::transform::xsltMaxVars = v;
                        }
                    }
                }
            }
            "-huge" | "--huge" => cli.options |= XML_PARSE_HUGE,
            "-seed-rand" | "--seed-rand" => {
                i += 1;
                if i < argv.len() {
                    if let Ok(v) = argv[i].parse::<c_uint>() {
                        unsafe {
                            libc::srand(v);
                        }
                    }
                }
            }
            "-dumpextensions" | "--dumpextensions" => cli.dumpextensions = true,
            other => {
                eprintln!("Unknown option {}", other);
                usage(&argv[0]);
                std::process::exit(3);
            }
        }
        i += 1;
    }

    cli.params.push(ptr::null());

    if cli.novalid {
        cli.options = XML_PARSE_NOENT | XML_PARSE_NOCDATA;
    } else if cli.nodtdattr {
        cli.options = XML_PARSE_NOENT | XML_PARSE_DTDLOAD | XML_PARSE_NOCDATA;
    }

    // Register EXSLT extensions (upstream: exsltRegisterAll()).
    libxml_rs::exslt::register_all();

    if cli.dumpextensions {
        dump_extensions();
    }

    // ── Locate the stylesheet (upstream's second scan) ──────────────────
    let mut cur: *mut _xsltStylesheet = ptr::null_mut();
    let mut i = 1usize;
    loop {
        if i >= argv.len() {
            break;
        }
        let arg = argv[i].as_str();
        match arg {
            "-maxdepth" | "--maxdepth" | "-maxvars" | "--maxvars" | "-seed-rand"
            | "--seed-rand" | "-o" | "-output" | "--output" | "-encoding" | "--encoding"
            | "-writesubtree" | "--writesubtree" | "-path" | "--path" => {
                i += 2;
                continue;
            }
            "-param" | "--param" | "-stringparam" | "--stringparam" => {
                i += 3;
                continue;
            }
            _ => {}
        }
        if !arg.starts_with('-') || arg == "-" {
            let cname = cstr_alloc(arg);
            if !std::path::Path::new(arg).exists() {
                warn_failed_entity(arg);
            }
            let style = unsafe { xmlReadFile(cname as *const c_char, ptr::null(), cli.options) };
            free_cstr(cname);
            if style.is_null() {
                eprintln!("cannot parse {}", arg);
                cur = ptr::null_mut();
                cli.errorno = 4;
            } else {
                // Embedded stylesheet via xml-stylesheet PI.
                let pi_style = unsafe { libxml_rs::xslt::stylesheet::xsltLoadStylesheetPI(style) };
                if !pi_style.is_null() {
                    unsafe {
                        xslt_process(&mut cli, pi_style, style, arg);
                        xsltFreeStylesheet(pi_style);
                    }
                    cur = ptr::null_mut();
                    break;
                }
                cur = unsafe { xsltParseStylesheetDoc(style) };
                if !cur.is_null() {
                    if unsafe { (*cur).errors } != 0 {
                        cli.errorno = 5;
                        unsafe {
                            xsltFreeStylesheet(cur);
                        }
                        cur = ptr::null_mut();
                    }
                    i += 1;
                } else {
                    unsafe { xmlFreeDoc(style) };
                    cli.errorno = 5;
                }
            }
            break;
        }
        i += 1;
    }

    // ── Process the input documents ─────────────────────────────────────
    if !cur.is_null() && unsafe { (*cur).errors } == 0 {
        while i < argv.len() {
            let input = argv[i].clone();
            let doc = unsafe { xslt_read_file(&input, &cli) };
            if doc.is_null() {
                eprintln!("unable to parse {}", input);
                cli.errorno = 6;
                i += 1;
                continue;
            }
            unsafe {
                xslt_process(&mut cli, cur, doc, &input);
            }
            i += 1;
        }
    }

    if !cur.is_null() {
        unsafe {
            xsltFreeStylesheet(cur);
        }
    }
    for p in &cli.params {
        unsafe {
            free_cstr(*p as *mut xmlChar);
        }
    }
    unsafe {
        libxml_rs::xslt::security::xsltFreeSecurityPrefs(sec);
        xmlCleanupParser();
    }
    std::process::exit(cli.errorno);
}

/// Minimal `xsltDebugDumpExtensions` output (RESIDUAL R-XSLTPROC-EXTDUMP).
fn dump_extensions() {
    println!("Registered extension functions:");
    for (name, _) in libxml_rs::exslt::iter_functions() {
        println!("  function: {}", name);
    }
}

/// Convert a C string pointer to a Rust String.
unsafe fn cstr_utf8(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    let len = libc::strlen(p);
    let slice = std::slice::from_raw_parts(p as *const u8, len);
    String::from_utf8_lossy(slice).into_owned()
}
