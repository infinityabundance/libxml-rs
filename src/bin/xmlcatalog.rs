//! xmlcatalog — XML catalog manipulation tool (§36, §85 Phase 10).
//!
//! Faithful port of upstream libxml2's `xmlcatalog` command-line tool
//! (xmlcatalog.c, libxml2 2.12 target):
//!
//! ```text
//! xmlcatalog [options] catalogfile entities...
//! ```
//!
//! Exit codes: 0 success, 1 usage/unknown option.
//!
//! # UPSTREAM-PARITY
//!
//! - `--create` builds a new empty catalog; with `--noout` it is written to
//!   the file, otherwise dumped to stdout.
//! - `--add 'type' 'orig' 'replace'` adds an XML entry; `--del 'values'`
//!   removes entries. With `--noout` the changes are saved back to the
//!   catalog file; without it the catalog is dumped to stdout (not saved).
//! - `--shell` runs an interactive query shell (`public`, `system`,
//!   `resolve`, `add`, `del`, `dump`, `debug`, `quiet`, `exit`).
//! - The dump format matches upstream: XML declaration, the OASIS catalog
//!   DOCTYPE, and a `<catalog xmlns="urn:oasis:names:tc:entity:xmlns:xml:catalog">`
//!   root with two-space-indented entries.

use std::ffi::c_void;
use std::os::raw::{c_char, c_int};
use std::ptr;

use libxml_rs::abi::exports_xml2::*;
use libxml_rs::abi::types::*;

#[derive(Default)]
struct Cli {
    sgml: bool,
    shell: bool,
    create: bool,
    noout: bool,
    no_super_update: bool,
    verbose: bool,
    add: Vec<(Vec<u8>, Vec<u8>, Vec<u8>)>, // (type, orig, replace); SGML add uses 1 arg
    del: Vec<Vec<u8>>,
    entities: Vec<String>,
}

fn usage() {
    let argv0 = std::env::args()
        .next()
        .unwrap_or_else(|| "xmlcatalog".to_string());
    println!("Usage : {} [options] catalogfile entities...", argv0);
    println!("\tParse the catalog file (void specification possibly expressed as \"\"");
    println!("\tappoints the default system one) and query it for the entities");
    println!("\t--sgml : handle SGML Super catalogs for --add and --del");
    println!("\t--shell : run a shell allowing interactive queries");
    println!("\t--create : create a new catalog");
    println!("\t--add 'type' 'orig' 'replace' : add an XML entry");
    println!("\t--add 'entry' : add an SGML entry");
    println!("\t--del 'values' : remove values");
    println!("\t--noout: avoid dumping the result on stdout");
    println!("\t         used with --add or --del, it saves the catalog changes");
    println!("\t         and with --sgml it automatically updates the super catalog");
    println!("\t--no-super-update: do not update the SGML super catalog");
    println!("\t-v --verbose : provide debug information");
}

unsafe fn cstr_alloc(s: &[u8]) -> *mut c_char {
    let p = libc::malloc(s.len() + 1) as *mut c_char;
    if p.is_null() {
        return p;
    }
    libc::memcpy(
        p as *mut libc::c_void,
        s.as_ptr() as *const libc::c_void,
        s.len(),
    );
    *p.add(s.len()) = 0;
    p
}

unsafe fn free_cstr(p: *mut c_char) {
    if !p.is_null() {
        libc::free(p as *mut libc::c_void);
    }
}

/// Serialize the catalog to stdout (upstream xmlCatalogDump format).
unsafe fn dump_catalog() {
    let fp = libc::fdopen(1, c"w".as_ptr() as *const c_char);
    if !fp.is_null() {
        xmlCatalogDump(fp as *mut c_void, ptr::null_mut());
        // Flush so the output is not delayed past subsequent shell prompts.
        libc::fflush(fp);
    }
}

/// Save the catalog to the given file.
unsafe fn save_catalog(path: &str) {
    let cpath = cstr_alloc(path.as_bytes());
    xmlCatalogSave(cpath as *const c_char);
    free_cstr(cpath);
}

/// Load a catalog file into the catalog state. Missing files are ignored
/// (upstream prints a warning via the loader and continues).
unsafe fn load_catalog_file(path: &str) {
    let cpath = cstr_alloc(path.as_bytes());
    xmlCatalogLoad(cpath as *const c_char);
    free_cstr(cpath);
}

/// Resolve and print a public-ID query.
unsafe fn shell_public(id: &str) {
    let cid = cstr_alloc(id.as_bytes());
    let res = xmlCatalogResolvePublic(cid as *const xmlChar);
    if res.is_null() {
        println!("No entry for PUBLIC {}", id);
    } else {
        let len = libc::strlen(res as *const c_char);
        libc::write(1, res as *const c_void, len);
        libc::write(1, b"\n".as_ptr() as *const c_void, 1);
        libxml_rs::abi::allocator::xmlFreeImpl(res as *mut c_void);
    }
    free_cstr(cid);
}

/// Resolve and print a system-ID query.
unsafe fn shell_system(id: &str) {
    let cid = cstr_alloc(id.as_bytes());
    let res = xmlCatalogResolveSystem(cid as *const xmlChar);
    if res.is_null() {
        println!("No entry for SYSTEM {}", id);
    } else {
        let len = libc::strlen(res as *const c_char);
        libc::write(1, res as *const c_void, len);
        libc::write(1, b"\n".as_ptr() as *const c_void, 1);
        libxml_rs::abi::allocator::xmlFreeImpl(res as *mut c_void);
    }
    free_cstr(cid);
}

/// Full resolver: public ID first, then system ID, then URI.
unsafe fn shell_resolve(pub_id: &str, sys_id: &str) {
    let cpub = cstr_alloc(pub_id.as_bytes());
    let res = xmlCatalogResolvePublic(cpub as *const xmlChar);
    free_cstr(cpub);
    if res.is_null() {
        let csys = cstr_alloc(sys_id.as_bytes());
        let res2 = xmlCatalogResolveSystem(csys as *const xmlChar);
        free_cstr(csys);
        if res2.is_null() {
            println!("Resolver failed to find an answer");
            return;
        }
        let len = libc::strlen(res2 as *const c_char);
        libc::write(1, res2 as *const c_void, len);
        libc::write(1, b"\n".as_ptr() as *const c_void, 1);
        libxml_rs::abi::allocator::xmlFreeImpl(res2 as *mut c_void);
        return;
    }
    let len = libc::strlen(res as *const c_char);
    libc::write(1, res as *const c_void, len);
    libc::write(1, b"\n".as_ptr() as *const c_void, 1);
    libxml_rs::abi::allocator::xmlFreeImpl(res as *mut c_void);
}

unsafe fn shell_add(args: &[&str]) {
    // Upstream: `add 'type' 'orig' 'replace'` (3 args) or the SGML form
    // `add 'entry'` (2 args).
    let ret = if args.len() >= 3 {
        let type_c = cstr_alloc(args[0].as_bytes());
        let orig_c = cstr_alloc(args[1].as_bytes());
        let repl_c = cstr_alloc(args[2].as_bytes());
        let r = xmlCatalogAdd(
            type_c as *const xmlChar,
            orig_c as *const xmlChar,
            repl_c as *const xmlChar,
        );
        free_cstr(type_c);
        free_cstr(orig_c);
        free_cstr(repl_c);
        r
    } else if args.len() == 2 {
        let type_c = cstr_alloc(args[0].as_bytes());
        let repl_c = cstr_alloc(args[1].as_bytes());
        let r = xmlCatalogAdd(
            type_c as *const xmlChar,
            ptr::null(),
            repl_c as *const xmlChar,
        );
        free_cstr(type_c);
        free_cstr(repl_c);
        r
    } else {
        -1
    };
    if ret != 0 {
        println!("add command failed");
    }
}

unsafe fn shell_del(value: &str) {
    let cval = cstr_alloc(value.as_bytes());
    xmlCatalogRemove(cval as *const xmlChar);
    free_cstr(cval);
    // UPSTREAM-PARITY: xmlHashRemoveEntry returns 0 on success (and -1 when
    // missing), so the upstream shell prints "del command failed" for every
    // removal.
    println!("del command failed");
}

/// Shell tokenizer matching upstream xmlcatalog.c usershell(): whitespace
/// separates tokens; single and double quotes group characters into one
/// token (the quotes are consumed). Unbalanced quotes terminate the token at
/// end-of-line, exactly like upstream's pointer walk.
fn shell_tokens(line: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut chars = line.chars().peekable();
    let mut cur = String::new();
    let mut in_token = false;
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' => {
                if in_token {
                    tokens.push(std::mem::take(&mut cur));
                    in_token = false;
                }
            }
            '\'' | '"' => {
                in_token = true;
                // consume until the matching quote or end of input
                for c2 in chars.by_ref() {
                    if c2 == c {
                        break;
                    }
                    cur.push(c2);
                }
            }
            _ => {
                in_token = true;
                cur.push(c);
            }
        }
    }
    if in_token {
        tokens.push(cur);
    }
    tokens
}

/// Run the interactive shell.
unsafe fn shell_loop() {
    use std::io::Write;
    loop {
        print!("> ");
        std::io::stdout().flush().ok();
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let trimmed = line.trim().to_string();
        if trimmed.is_empty() {
            continue;
        }
        let parts: Vec<String> = shell_tokens(&trimmed);
        match parts[0].as_str() {
            "quit" | "exit" | "bye" | "q" => break,
            "public" => {
                // UPSTREAM-PARITY: exactly one argument (nbargs != 1 fails)
                if parts.len() != 2 {
                    println!("public requires 1 arguments");
                } else {
                    shell_public(&parts[1]);
                }
            }
            "system" => {
                if parts.len() != 2 {
                    println!("system requires 1 arguments");
                } else {
                    shell_system(&parts[1]);
                }
            }
            "resolve" => {
                if parts.len() != 3 {
                    println!("resolve requires 2 arguments");
                } else {
                    shell_resolve(&parts[1], &parts[2]);
                }
            }
            "add" => {
                if parts.len() < 3 || parts.len() > 4 {
                    println!("add requires 2 or 3 arguments");
                } else {
                    let refs: Vec<&str> = parts[1..].iter().map(String::as_str).collect();
                    shell_add(&refs);
                }
            }
            "del" => {
                if parts.len() != 2 {
                    println!("del requires 1");
                } else {
                    shell_del(&parts[1]);
                }
            }
            "dump" => {
                if parts.len() != 1 {
                    println!("dump has no arguments");
                } else {
                    dump_catalog();
                }
            }
            "debug" => {
                if parts.len() != 1 {
                    println!("debug has no arguments");
                } else {
                    // Verbosity increase: no-op in this implementation.
                }
            }
            "quiet" => {
                if parts.len() != 1 {
                    println!("quiet has no arguments");
                } else {
                    // Verbosity decrease: no-op.
                }
            }
            _ => {
                // UPSTREAM-PARITY: "help" prints only the command list.
                if parts[0] != "help" {
                    println!("Unrecognized command {}", parts[0]);
                }
                println!("Commands available:");
                println!("\tpublic PublicID: make a PUBLIC identifier lookup");
                println!("\tsystem SystemID: make a SYSTEM identifier lookup");
                println!("\tresolve PublicID SystemID: do a full resolver lookup");
                println!("\tadd 'type' 'orig' 'replace' : add an entry");
                println!("\tdel 'values' : remove values");
                println!("\tdump: print the current catalog state");
                println!("\tdebug: increase the verbosity level");
                println!("\tquiet: decrease the verbosity level");
                println!("\texit:  quit the shell");
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut cli = Cli::default();

    let mut positionals: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--sgml" => cli.sgml = true,
            "--shell" => cli.shell = true,
            "--create" => cli.create = true,
            "--noout" => cli.noout = true,
            "--no-super-update" => cli.no_super_update = true,
            "-v" | "--verbose" => cli.verbose = true,
            "--add" => {
                // --add takes 3 args (XML) or 1 (SGML); consume as many as
                // remain, up to 3.
                let mut vals: Vec<String> = Vec::new();
                let mut j = i + 1;
                while j < args.len() && vals.len() < 3 {
                    vals.push(args[j].clone());
                    j += 1;
                }
                if vals.is_empty() {
                    usage();
                    std::process::exit(1);
                }
                // If only one value, SGML entry (type-less); the CLI form
                // requires three.
                if vals.len() < 3 {
                    eprintln!("--add requires 'type' 'orig' 'replace'");
                    std::process::exit(1);
                }
                cli.add.push((
                    vals[0].as_bytes().to_vec(),
                    vals[1].as_bytes().to_vec(),
                    vals[2].as_bytes().to_vec(),
                ));
                i += 3;
            }
            "--del" => {
                if i + 1 >= args.len() {
                    usage();
                    std::process::exit(1);
                }
                cli.del.push(args[i + 1].as_bytes().to_vec());
                i += 1;
            }
            _ => {
                if arg.starts_with('-') && arg.len() > 1 {
                    eprintln!("Unknown option {}", arg);
                    usage();
                    std::process::exit(1);
                }
                positionals.push(arg.to_string());
                // UPSTREAM-PARITY: the option loop stops at the first
                // non-option argument; the remainder are positional entities.
                for a in &args[i + 1..] {
                    positionals.push(a.clone());
                }
                break;
            }
        }
        i += 1;
    }

    if positionals.is_empty() {
        usage();
        std::process::exit(1);
    }
    let catalog_file = positionals[0].clone();
    cli.entities = positionals[1..].to_vec();

    unsafe {
        // Load the catalog file when it exists. UPSTREAM-PARITY: this happens
        // regardless of --create (xmlcatalog.c loads the filename and only
        // falls back to seeding a fresh "catalog" entry when the load fails
        // AND --create is set) — so `--create` on an existing catalog loads
        // and dumps it (R-000122 family).
        if !catalog_file.is_empty() {
            load_catalog_file(&catalog_file);
        }

        // Apply add/del operations.
        let mut modified = false;
        let mut exit_value: c_int = 0;
        for (t, o, r) in &cli.add {
            let type_c = cstr_alloc(t);
            let orig_c = cstr_alloc(o);
            let repl_c = cstr_alloc(r);
            let ret = xmlCatalogAdd(
                type_c as *const xmlChar,
                orig_c as *const xmlChar,
                repl_c as *const xmlChar,
            );
            free_cstr(type_c);
            free_cstr(orig_c);
            free_cstr(repl_c);
            if ret != 0 {
                // UPSTREAM-PARITY: xmlcatalog prints "add command failed" and
                // sets exit_value = 3 for an unrecognized type.
                println!("add command failed");
                exit_value = 3;
            } else {
                modified = true;
            }
        }
        for v in &cli.del {
            let val_c = cstr_alloc(v);
            let ret = xmlCatalogRemove(val_c as *const xmlChar);
            free_cstr(val_c);
            if ret < 0 {
                // UPSTREAM-PARITY: "Failed to remove entry" to stderr, exit 1.
                eprintln!("Failed to remove entry {}", String::from_utf8_lossy(v));
                exit_value = 1;
            } else {
                modified = true;
            }
        }

        if cli.shell {
            shell_loop();
        } else if !cli.entities.is_empty() && !modified {
            // UPSTREAM-PARITY: query mode — every positional argument after the
            // catalog file is resolved: strings that are not parseable URIs go
            // through the PUBLIC path, everything else via SYSTEM then URI
            // (exit 4 on failure). Upstream runs this loop regardless of
            // --create (xmlcatalog.c: the `else` branch of add/del/shell), so
            // trailing options after the first non-option argument are resolved
            // as entities against the freshly created catalog (R-000122).
            for id in &cli.entities {
                let cid = cstr_alloc(id.as_bytes());
                let uri = xmlParseURI(cid as *const c_char);
                let has_space = id.bytes().any(|b| b.is_ascii_whitespace());
                let is_uri = !uri.is_null() && !has_space;
                if !uri.is_null() {
                    xmlFreeURI(uri);
                }
                if !is_uri {
                    let res = xmlCatalogResolvePublic(cid as *const xmlChar);
                    if res.is_null() {
                        println!("No entry for PUBLIC {}", id);
                        exit_value = 4;
                    } else {
                        let len = libc::strlen(res as *const c_char);
                        libc::write(1, res as *const c_void, len);
                        libc::write(1, b"\n".as_ptr() as *const c_void, 1);
                        libxml_rs::abi::allocator::xmlFreeImpl(res as *mut c_void);
                    }
                } else {
                    let res = xmlCatalogResolveSystem(cid as *const xmlChar);
                    if res.is_null() {
                        println!("No entry for SYSTEM {}", id);
                        let res2 = xmlCatalogResolveURI(cid as *const xmlChar);
                        if res2.is_null() {
                            println!("No entry for URI {}", id);
                            exit_value = 4;
                        } else {
                            let len = libc::strlen(res2 as *const c_char);
                            libc::write(1, res2 as *const c_void, len);
                            libc::write(1, b"\n".as_ptr() as *const c_void, 1);
                            libxml_rs::abi::allocator::xmlFreeImpl(res2 as *mut c_void);
                        }
                    } else {
                        let len = libc::strlen(res as *const c_char);
                        libc::write(1, res as *const c_void, len);
                        libc::write(1, b"\n".as_ptr() as *const c_void, 1);
                        libxml_rs::abi::allocator::xmlFreeImpl(res as *mut c_void);
                    }
                }
                free_cstr(cid);
            }
        }
        // UPSTREAM-PARITY: the dump/save runs AFTER the query loop, and
        // independently of it (xmlcatalog.c: `if (add || del || create)` at
        // the end of main) — so `--create FILE --noout` both resolves the
        // trailing --noout as an entity (exit 4) AND dumps the new catalog.
        if modified || cli.create {
            if cli.noout {
                // Save the catalog (only when something changed or created).
                save_catalog(&catalog_file);
            } else {
                // Dump to stdout.
                dump_catalog();
            }
        }
        std::process::exit(exit_value);
    }
}
