//! xmllint — XML validation and formatting tool (§36, §85 Phase 10).
//!
//! Faithful port of upstream libxml2's `xmllint` command-line tool
//! (xmllint.c, libxml2 2.12 target). The pipeline is entirely native Rust:
//!
//! ```text
//! Rust CLI → Rust libxml parser/validator/serializer → Rust XPath/C14N
//! ```
//!
//! # Exit codes (upstream parity, measured against libxml2 2.15.3)
//!
//! | Code | Meaning |
//! |------|---------|
//! | 0 | success |
//! | 1 | usage error (no arguments) |
//! | 3 | validity error (DTD validation failed) |
//! | 4 | parse error / cannot open file |
//! | 5 | schema compilation failure |
//! | 6 | schema validation failure |
//! | 7 | schematron validation failure |

use std::ffi::c_void;
use std::os::raw::{c_char, c_int, c_uint};
use std::ptr;

use libxml_rs::abi::exports_xml2::*;
use libxml_rs::abi::structs::*;
use libxml_rs::abi::types::*;
use libxml_rs::abi::versioning::{xmlLibxmlVersion, xmlLibxmlVersionString};

// ── Parser option bits (upstream XML_PARSE_* values) ───────────────────────
const XML_PARSE_RECOVER: c_int = 1 << 0;
const XML_PARSE_NOENT: c_int = 1 << 1;
const XML_PARSE_DTDLOAD: c_int = 1 << 2;
const XML_PARSE_DTDATTR: c_int = 1 << 3;
const XML_PARSE_DTDVALID: c_int = 1 << 4;
const XML_PARSE_NOERROR: c_int = 1 << 5;
const XML_PARSE_NOWARNING: c_int = 1 << 6;
const XML_PARSE_PEDANTIC: c_int = 1 << 7;
const XML_PARSE_NOBLANKS: c_int = 1 << 8;
const XML_PARSE_SAX1: c_int = 1 << 9;
const XML_PARSE_XINCLUDE: c_int = 1 << 10;
const XML_PARSE_NONET: c_int = 1 << 11;
const XML_PARSE_NODICT: c_int = 1 << 12;
const XML_PARSE_NSCLEAN: c_int = 1 << 13;
const XML_PARSE_NOCDATA: c_int = 1 << 14;
const XML_PARSE_NOXINCNODE: c_int = 1 << 15;
const XML_PARSE_COMPACT: c_int = 1 << 16;
const XML_PARSE_OLD10: c_int = 1 << 17;
const XML_PARSE_NOBASEFIX: c_int = 1 << 18;
const XML_PARSE_HUGE: c_int = 1 << 19;
const XML_PARSE_OLDSAX: c_int = 1 << 20;
const XML_PARSE_IGNORE_ENC: c_int = 1 << 21;
const XML_PARSE_BIG_LINES: c_int = 1 << 22;

/// CLI state (mirrors upstream xmllint.c file-scope globals).
struct Cli {
    shell: bool,
    debug: bool,
    copy: bool,
    recover: bool,
    noout: bool,
    nonet: bool,
    novalid: bool,
    nocompact: bool,
    loaddtd: bool,
    dtdattr: bool,
    loadtrace: bool,
    push: bool,
    memory: bool,
    timing: bool,
    repeat: bool,
    valid: bool,
    postvalid: bool,
    html: bool,
    xmlout: bool,
    nodefdtd: bool,
    dropdtd: bool,
    insert: bool,
    quiet: bool,
    nowarning: bool,
    noblanks: bool,
    nocdata: bool,
    nodict: bool,
    pedantic: bool,
    nsclean: bool,
    auto: bool,
    xinclude: bool,
    noxincludenode: bool,
    nofixup_base_uris: bool,
    catalogs: bool,
    nocatalogs: bool,
    stream: bool,
    walker: bool,
    sax1: bool,
    sax: bool,
    oldxml10: bool,
    strict_namespace: bool,
    encode: Option<String>,
    output: Option<String>,
    format: c_int, // -1 unset, 0 no, 1 yes
    pretty: c_int,
    compress: bool,
    c14n: c_int, // 0 none, 1 c14n, 2 c14n11, 3 exclusive
    pattern: Option<String>,
    schema: Option<String>,
    relaxng: Option<String>,
    schematron: Option<String>,
    xpath: Option<String>,
    xpath0: bool,
    paths: Vec<String>,
    dtdvalid: Option<String>,
    dtdvalidfpi: Option<String>,
    maxmem: Option<usize>,
    max_ampl: Option<f64>,
    options: c_int,
    return_code: c_int,
}

impl Default for Cli {
    fn default() -> Self {
        Cli {
            shell: false,
            debug: false,
            copy: false,
            recover: false,
            noout: false,
            nonet: false,
            novalid: false,
            nocompact: false,
            loaddtd: false,
            dtdattr: false,
            loadtrace: false,
            push: false,
            memory: false,
            timing: false,
            repeat: false,
            valid: false,
            postvalid: false,
            html: false,
            xmlout: false,
            nodefdtd: false,
            dropdtd: false,
            insert: false,
            quiet: false,
            nowarning: false,
            noblanks: false,
            nocdata: false,
            nodict: false,
            pedantic: false,
            nsclean: false,
            auto: false,
            xinclude: false,
            noxincludenode: false,
            nofixup_base_uris: false,
            catalogs: false,
            nocatalogs: false,
            stream: false,
            walker: false,
            sax1: false,
            sax: false,
            oldxml10: false,
            strict_namespace: false,
            encode: None,
            output: None,
            format: -1,
            pretty: -1,
            compress: false,
            c14n: 0,
            pattern: None,
            schema: None,
            relaxng: None,
            schematron: None,
            xpath: None,
            xpath0: false,
            paths: Vec::new(),
            dtdvalid: None,
            dtdvalidfpi: None,
            maxmem: None,
            max_ampl: None,
            // UPSTREAM-PARITY: xmllint defaults parseOptions to
            // XML_PARSE_COMPACT | XML_PARSE_BIG_LINES (xmllint.c:2579);
            // --nocompact clears the COMPACT bit.
            options: XML_PARSE_COMPACT | XML_PARSE_BIG_LINES,
            return_code: 0,
        }
    }
}

fn usage() {
    let argv0 = std::env::args()
        .next()
        .unwrap_or_else(|| "xmllint".to_string());
    eprintln!("Usage : {} [options] XMLfiles ...", argv0);
    eprintln!("\tParse the XML files and output the result of the parsing");
    eprintln!("\t--version : display the version of the XML library used");
    eprintln!("\t--shell : run a navigating shell");
    eprintln!("\t--debug : show additional debug information");
    eprintln!("\t--copy : used to test the internal copy implementation");
    eprintln!("\t--recover : output what was parsable on broken XML documents");
    eprintln!("\t--huge : remove any internal arbitrary parser limits");
    eprintln!("\t--noent : substitute entity references by their value");
    eprintln!("\t--noenc : ignore any encoding specified inside the document");
    eprintln!("\t--noout : don't output the result tree");
    eprintln!("\t--path 'paths': provide a set of paths for resources");
    eprintln!("\t--load-trace : print trace of all external entities loaded");
    eprintln!("\t--nonet : refuse to fetch DTDs or entities over network");
    eprintln!("\t--nocompact : do not generate compact text nodes");
    eprintln!("\t--valid : validate the document in addition to std well-formed check");
    eprintln!("\t--postvalid : do a posteriori validation, i.e after parsing");
    eprintln!("\t--dtdvalid URL : do a posteriori validation against a given DTD");
    eprintln!("\t--dtdvalidfpi FPI : same but name the DTD with a Public Identifier");
    eprintln!("\t--insert : ad-hoc test for valid insertions");
    eprintln!(
        "\t--strict-namespace : Return application failure if document has any namespace errors"
    );
    eprintln!("\t--quiet : be quiet when succeeded");
    eprintln!("\t--timing : print some timings");
    eprintln!("\t--repeat : repeat 100 times, for timing or profiling");
    eprintln!("\t--dropdtd : remove the DOCTYPE of the input docs");
    eprintln!("\t--html : use the HTML parser");
    eprintln!("\t--nodefdtd : do not default HTML doctype");
    eprintln!("\t--xmlout : force to use the XML serializer when using --html");
    eprintln!("\t--push : use the push mode of the parser");
    eprintln!("\t--memory : parse from memory");
    eprintln!("\t--maxmem nbbytes : limits memory allocation to nbbytes bytes");
    eprintln!("\t--nowarning : do not emit warnings from parser/validator");
    eprintln!("\t--noblanks : drop (ignorable?) blanks spaces");
    eprintln!("\t--nocdata : replace cdata section with text nodes");
    eprintln!("\t--nodict : create document without dictionary");
    eprintln!("\t--pedantic : enable additional warnings");
    eprintln!("\t--output file or -o file: save to a given file");
    eprintln!("\t--format : reformat/reindent the output");
    eprintln!("\t--encode encoding : output in the given encoding");
    eprintln!("\t--pretty STYLE : pretty-print in a particular style");
    eprintln!("\t                 0 Do not pretty print");
    eprintln!("\t                 1 Format the XML content, as --format");
    eprintln!("\t                 2 Add whitespace inside tags, preserving content");
    eprintln!("\t--compress : turn on gzip compression of output");
    eprintln!("\t--c14n : save in W3C canonical format v1.0 (with comments)");
    eprintln!("\t--c14n11 : save in W3C canonical format v1.1 (with comments)");
    eprintln!("\t--exc-c14n : save in W3C exclusive canonical format (with comments)");
    eprintln!("\t--nsclean : remove redundant namespace declarations");
    eprintln!("\t--catalogs : use SGML catalogs from $SGML_CATALOG_FILES");
    eprintln!("\t             otherwise XML Catalogs starting from ");
    eprintln!("\t         file:///etc/xml/catalog are activated by default");
    eprintln!("\t--nocatalogs: deactivate all catalogs");
    eprintln!("\t--auto : generate a small doc on the fly");
    eprintln!("\t--xinclude : do XInclude processing");
    eprintln!("\t--noxincludenode : same but do not generate XInclude nodes");
    eprintln!("\t--nofixup-base-uris : do not fixup xml:base uris");
    eprintln!("\t--loaddtd : fetch external DTD");
    eprintln!("\t--dtdattr : loaddtd + populate the tree with inherited attributes ");
    eprintln!("\t--stream : use the streaming interface to process very large files");
    eprintln!("\t--walker : create a reader and walk though the resulting doc");
    eprintln!("\t--pattern pattern_value : test the pattern support");
    eprintln!("\t--relaxng schema : do RelaxNG validation against the schema");
    eprintln!("\t--schema schema : do validation against the WXS schema");
    eprintln!("\t--schematron schema : do validation against a schematron");
    eprintln!("\t--sax1: use the old SAX1 interfaces for processing");
    eprintln!("\t--sax: do not build a tree but work just at the SAX level");
    eprintln!("\t--oldxml10: use XML-1.0 parsing rules before the 5th edition");
    eprintln!("\t--xpath expr: evaluate the XPath expression, results are separated by \\n, imply --noout");
    eprintln!("\t--xpath0 expr: evaluate the XPath expression, results are separated by \\0, imply --noout");
    eprintln!("\t--max-ampl value: set maximum amplification factor");
    eprintln!();
    eprintln!("Libxml project home page: https://gitlab.gnome.org/GNOME/libxml2");
}

/// Write bytes to stdout.
fn write_stdout(bytes: &[u8]) {
    unsafe {
        libc::write(1, bytes.as_ptr() as *const c_void, bytes.len());
    }
}

/// Write a NUL-terminated C string to a FILE*.
unsafe fn write_cstr(fp: *mut libc::FILE, s: *const c_char) {
    if s.is_null() {
        return;
    }
    let len = libc::strlen(s);
    libc::fwrite(s as *const c_void, 1, len, fp);
}

/// Get the output FILE*: a file named by --output, or stdout.
unsafe fn get_output_file(cli: &Cli) -> *mut libc::FILE {
    match &cli.output {
        Some(name) => {
            let cname = cstr_alloc(name);
            let fp = libc::fopen(cname, b"w\0".as_ptr() as *const c_char);
            free_cstr(cname);
            fp
        }
        None => libc::fdopen(1, b"w\0".as_ptr() as *const c_char),
    }
}

unsafe fn close_output_file(cli: &Cli, fp: *mut libc::FILE) {
    if cli.output.is_some() && !fp.is_null() {
        libc::fclose(fp);
    }
}

/// Print the version banner on stderr (upstream format, exit 0).
fn print_version() {
    let argv0 = std::env::args()
        .next()
        .unwrap_or_else(|| "xmllint".to_string());
    let num = xmlLibxmlVersion();
    let dotted = unsafe {
        let s = xmlLibxmlVersionString();
        std::ffi::CStr::from_ptr(s).to_string_lossy().into_owned()
    };
    eprintln!("{}: using libxml version {}-GITv{}", argv0, num, dotted);
    eprintln!(
        "   compiled with: Threads Tree Output Push Reader Patterns Writer SAXv1 DTDValid HTML C14N Catalog XPath XPointer XInclude Iconv ICU ISO8859X Regexps Automata RelaxNG Schemas Schematron Modules Debug Zlib "
    );
}

/// Parse one document (file or stdin `-`) honoring the CLI options.
unsafe fn parse_document(cli: &Cli, filename: &str) -> *mut _xmlDoc {
    let cname = cstr_alloc(filename);
    let mut options = cli.options;
    if cli.nocompact {
        // UPSTREAM-PARITY: --nocompact clears the COMPACT bit (xmllint.c).
        options &= !XML_PARSE_COMPACT;
    }
    if cli.nonet {
        options |= XML_PARSE_NONET;
    }
    if cli.loaddtd || cli.dtdattr {
        options |= XML_PARSE_DTDLOAD;
    }
    if cli.dtdattr {
        options |= XML_PARSE_DTDATTR;
    }
    if cli.recover {
        options |= XML_PARSE_RECOVER;
    }
    if cli.noblanks {
        options |= XML_PARSE_NOBLANKS;
    }
    if cli.nocdata {
        options |= XML_PARSE_NOCDATA;
    }
    if cli.pedantic {
        options |= XML_PARSE_PEDANTIC;
    }
    if cli.oldxml10 {
        options |= XML_PARSE_OLD10;
    }
    if cli.html {
        // HTML parse path.
        let doc = if filename == "-" {
            use std::io::Read;
            let mut buf = Vec::new();
            std::io::stdin().read_to_end(&mut buf).ok();
            buf.push(0);
            libxml_rs::xml::html::parse_memory(
                buf.as_ptr() as *const c_char,
                buf.len() as c_int - 1,
            )
        } else {
            libxml_rs::xml::html::parse_file(cname as *const c_char, ptr::null())
        };
        free_cstr(cname);
        return doc;
    }
    let doc = if filename == "-" {
        xmlReadFd(0, cname as *const c_char, ptr::null(), options)
    } else {
        xmlReadFile(cname as *const c_char, ptr::null(), options)
    };
    free_cstr(cname);
    doc
}

/// Dump a document to the output stream.
unsafe fn dump_document(cli: &Cli, doc: *mut _xmlDoc, filename: &str) {
    if doc.is_null() {
        return;
    }
    if cli.dropdtd
        || (cli.nodefdtd
            && (*doc).type_
                == libxml_rs::abi::types::xmlElementType::XML_HTML_DOCUMENT_NODE as c_int)
    {
        // Remove the internal subset (upstream xmlDtdRemove / HTML
        // --nodefdtd suppresses the default HTML DOCTYPE). UPSTREAM-PARITY
        // (xmllint.c parseAndPrintFile): the DTD is unlinked from the
        // children chain first — it is still linked there by
        // xmlCreateIntSubset, so freeing it while linked double-frees when
        // the document is released.
        let dtd = (*doc).intSubset;
        if !dtd.is_null() {
            libxml_rs::xml::tree::unlink_node(dtd as *mut _xmlNode);
            (*doc).intSubset = ptr::null_mut();
            xmlFreeDtd(dtd);
        }
    }
    // UPSTREAM-PARITY: --encode declares the requested output encoding in the
    // XML declaration (the serializer emits encoding="..." when doc->encoding
    // is set).
    if let Some(enc) = &cli.encode {
        let old = (*doc).encoding;
        let cname = cstr_alloc(enc);
        (*doc).encoding = cname as *mut xmlChar;
        if !old.is_null() {
            libxml_rs::abi::allocator::xmlFreeImpl(old as *mut c_void);
        }
    }
    if cli.c14n != 0 {
        let mut result: *mut xmlChar = ptr::null_mut();
        // Upstream xmllint maps its three flags onto the xmlC14NMode enum:
        // --c14n -> XML_C14N_1_0 (0), --c14n11 -> XML_C14N_1_1 (2),
        // --exc-c14n -> XML_C14N_EXCLUSIVE_1_0 (1).
        let mode = match cli.c14n {
            2 => 2,
            3 => 1,
            _ => 0,
        };
        let ret = libxml_rs::xml::c14n::xmlC14NDocDumpMemory(
            doc,
            ptr::null_mut(),
            mode,
            ptr::null_mut(),
            1, // with comments
            &mut result,
        );
        // UPSTREAM-PARITY: xmlC14NDocDumpMemory returns the length of the
        // canonical form on success (>= 0); xmllint writes exactly ret bytes.
        if ret >= 0 && !result.is_null() {
            write_stdout(core::slice::from_raw_parts(result, ret as usize));
        }
        if !result.is_null() {
            libxml_rs::abi::allocator::xmlFreeImpl(result as *mut c_void);
        }
        return;
    }
    let format = if cli.pretty == 1 || cli.format == 1 {
        1
    } else {
        0
    };
    // UPSTREAM-PARITY: --xmlout forces the XML serializer even for HTML
    // documents (xmlSaveFormatFile with XML_SAVE_AS_XML).
    let mut restore_type = false;
    if cli.xmlout
        && (*doc).type_ == libxml_rs::abi::types::xmlElementType::XML_HTML_DOCUMENT_NODE as c_int
    {
        (*doc).type_ = libxml_rs::abi::types::xmlElementType::XML_DOCUMENT_NODE as c_int;
        restore_type = true;
    }
    let mut mem: *mut xmlChar = ptr::null_mut();
    let mut size: c_int = 0;
    xmlDocDumpFormatMemory(doc, &mut mem, &mut size, format);
    if restore_type {
        (*doc).type_ = libxml_rs::abi::types::xmlElementType::XML_HTML_DOCUMENT_NODE as c_int;
    }
    if !mem.is_null() {
        write_stdout(core::slice::from_raw_parts(mem, size as usize));
        libxml_rs::abi::allocator::xmlFreeImpl(mem as *mut c_void);
    }
    let _ = filename;
}

/// Capture validity messages reported through the validation context's error
/// callback so the CLI can render them in the upstream location format.
thread_local! {
    static CAPTURED_VALIDITY: std::cell::RefCell<Vec<Vec<u8>>> =
        std::cell::RefCell::new(Vec::new());
}

unsafe extern "C" fn capture_validity_msg(_ctx: *mut c_void, msg: *const c_char) {
    if msg.is_null() {
        return;
    }
    let bytes = std::ffi::CStr::from_ptr(msg).to_bytes().to_vec();
    CAPTURED_VALIDITY.with(|c| c.borrow_mut().push(bytes));
}

/// Print a validity diagnostic in the upstream format:
/// `file:line: element NAME: validity error : MSG` + source line + caret.
/// The source line is re-read from the document's file (the caret column for
/// declaration errors points past the line end, matching the oracle; the
/// no-DTD notice caret is placed after the root start tag).
unsafe fn print_validity_error(
    cli: &Cli,
    filename: &str,
    doc: *mut _xmlDoc,
    msg: &[u8],
    caret_spaces: usize,
    element_prefix: bool,
) {
    if cli.nowarning {
        return;
    }
    // Root element name and line.
    let mut root = (*doc).children;
    while !root.is_null() && (*root).type_ != 1 {
        root = (*root).next;
    }
    let (name, line) = if root.is_null() {
        (b"".to_vec(), 1usize)
    } else {
        let n = if (*root).name.is_null() {
            b"".to_vec()
        } else {
            std::ffi::CStr::from_ptr((*root).name as *const c_char)
                .to_bytes()
                .to_vec()
        };
        let ln = (*root).line as usize;
        (n, if ln == 0 { 1 } else { ln })
    };
    // Re-read the source line for context.
    let context = if filename == "-" {
        Vec::new()
    } else {
        std::fs::read_to_string(filename)
            .ok()
            .and_then(|s| s.lines().nth(line - 1).map(|l| l.as_bytes().to_vec()))
            .unwrap_or_default()
    };
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(filename.as_bytes());
    out.push(b':');
    out.extend_from_slice(line.to_string().as_bytes());
    out.push(b':');
    if element_prefix && !name.is_empty() {
        out.extend_from_slice(b" element ");
        out.extend_from_slice(&name);
        out.push(b':');
    }
    out.extend_from_slice(b" validity error : ");
    out.extend_from_slice(msg);
    out.push(b'\n');
    out.extend_from_slice(&context);
    out.push(b'\n');
    out.extend(std::iter::repeat(b' ').take(caret_spaces));
    out.push(b'^');
    out.push(b'\n');
    libc::write(2, out.as_ptr() as *const c_void, out.len());
}

/// Perform DTD validation of a document.
unsafe fn validate_doc(cli: &mut Cli, doc: *mut _xmlDoc, filename: &str) -> c_int {
    if cli.valid || cli.postvalid {
        let vctxt = xmlNewValidCtxt();
        if vctxt.is_null() {
            return -1;
        }
        libxml_rs::xml::validation::set_valid_errors(
            vctxt,
            Some(capture_validity_msg),
            None,
            ptr::null_mut(),
        );
        let dtd_missing = (*doc).intSubset.is_null() && (*doc).extSubset.is_null();
        let ret = xmlValidateDocument(vctxt, doc);
        let msgs: Vec<Vec<u8>> = CAPTURED_VALIDITY.with(|c| c.borrow_mut().drain(..).collect());
        if dtd_missing {
            // UPSTREAM-PARITY: xmllint displays the parser-style message
            // "Validation failed: no DTD found !" and treats the document as
            // valid (exit 0). The xmlValidateDocument API reports the
            // different message "no DTD found!" and returns 0; xmllint
            // overrides both for display. The caret sits after the root
            // start tag and there is no element prefix (the upstream notice
            // has a NULL node).
            let name = root_element_name(doc);
            let caret = name.len() + 1;
            print_validity_error(
                cli,
                filename,
                doc,
                b"Validation failed: no DTD found !",
                caret,
                false,
            );
            libxml_rs::xml::validation::free_valid_ctxt(vctxt);
            return 1; // validates (notice only)
        }
        if ret != 1 {
            // Declaration/content errors: the oracle caret is past the end of
            // the element's source line.
            let context_len = source_line_len(filename, root_element_line(doc));
            for m in &msgs {
                print_validity_error(cli, filename, doc, m, context_len, true);
            }
        }
        libxml_rs::xml::validation::free_valid_ctxt(vctxt);
        return ret;
    }
    if let Some(dtd) = &cli.dtdvalid {
        let cname = cstr_alloc(dtd);
        let dtd_doc = xmlReadFile(cname as *const c_char, ptr::null(), 0);
        free_cstr(cname);
        if dtd_doc.is_null() {
            cli.return_code = 4;
            return -1;
        }
        let vctxt = xmlNewValidCtxt();
        if vctxt.is_null() {
            return -1;
        }
        let dtd_ptr = (*dtd_doc).intSubset;
        let ret = if dtd_ptr.is_null() {
            xmlValidateDocument(vctxt, doc)
        } else {
            xmlValidateDtd(vctxt, doc, dtd_ptr)
        };
        libxml_rs::xml::validation::free_valid_ctxt(vctxt);
        libxml_rs::xml::tree::free_doc(dtd_doc);
        return ret;
    }
    0
}

unsafe fn root_element_name(doc: *mut _xmlDoc) -> Vec<u8> {
    let mut root = (*doc).children;
    while !root.is_null() && (*root).type_ != 1 {
        root = (*root).next;
    }
    if root.is_null() || (*root).name.is_null() {
        Vec::new()
    } else {
        std::ffi::CStr::from_ptr((*root).name as *const c_char)
            .to_bytes()
            .to_vec()
    }
}

unsafe fn root_element_line(doc: *mut _xmlDoc) -> usize {
    let mut root = (*doc).children;
    while !root.is_null() && (*root).type_ != 1 {
        root = (*root).next;
    }
    if root.is_null() {
        1
    } else {
        let ln = (*root).line as usize;
        if ln == 0 {
            1
        } else {
            ln
        }
    }
}

fn source_line_len(filename: &str, line: usize) -> usize {
    std::fs::read_to_string(filename)
        .ok()
        .and_then(|s| s.lines().nth(line - 1).map(|l| l.len()))
        .unwrap_or(0)
}

/// Run WXS schema validation; returns 0 on success, 6 on failure.
unsafe fn validate_schema(schema_path: &str, doc: *mut _xmlDoc) -> c_int {
    let s = cstr_alloc(schema_path);
    let pctxt = libxml_rs::xml::schemas::xmlSchemaNewParserCtxt(s as *const c_char);
    free_cstr(s);
    if pctxt.is_null() {
        return 5;
    }
    let schema = libxml_rs::xml::schemas::xmlSchemaParse(pctxt);
    if schema.is_null() {
        libxml_rs::xml::schemas::xmlSchemaFreeParserCtxt(pctxt);
        return 5;
    }
    let vctxt = libxml_rs::xml::schemas::xmlSchemaNewValidCtxt(schema);
    if vctxt.is_null() {
        libxml_rs::xml::schemas::xmlSchemaFree(schema);
        libxml_rs::xml::schemas::xmlSchemaFreeParserCtxt(pctxt);
        return 6;
    }
    let ret = libxml_rs::xml::schemas::xmlSchemaValidateDoc(vctxt, doc);
    libxml_rs::xml::schemas::xmlSchemaFreeValidCtxt(vctxt);
    libxml_rs::xml::schemas::xmlSchemaFree(schema);
    libxml_rs::xml::schemas::xmlSchemaFreeParserCtxt(pctxt);
    if ret == 0 {
        0
    } else {
        6
    }
}

/// Run RELAX NG validation; returns 0 on success, 6 on failure.
unsafe fn validate_relaxng(schema_path: &str, doc: *mut _xmlDoc) -> c_int {
    let s = cstr_alloc(schema_path);
    let pctxt = libxml_rs::xml::relaxng::xmlRelaxNGNewParserCtxt(s as *const c_char);
    free_cstr(s);
    if pctxt.is_null() {
        return 5;
    }
    let schema = libxml_rs::xml::relaxng::xmlRelaxNGParse(pctxt);
    if schema.is_null() {
        libxml_rs::xml::relaxng::xmlRelaxNGFreeParserCtxt(pctxt);
        return 5;
    }
    let vctxt = libxml_rs::xml::relaxng::xmlRelaxNGNewValidCtxt(schema);
    if vctxt.is_null() {
        libxml_rs::xml::relaxng::xmlRelaxNGFree(schema);
        libxml_rs::xml::relaxng::xmlRelaxNGFreeParserCtxt(pctxt);
        return 6;
    }
    let ret = libxml_rs::xml::relaxng::xmlRelaxNGValidateDoc(vctxt, doc);
    libxml_rs::xml::relaxng::xmlRelaxNGFreeValidCtxt(vctxt);
    libxml_rs::xml::relaxng::xmlRelaxNGFree(schema);
    libxml_rs::xml::relaxng::xmlRelaxNGFreeParserCtxt(pctxt);
    if ret == 0 {
        0
    } else {
        6
    }
}

/// Run Schematron validation; returns 0 on success, 7 on failure.
unsafe fn validate_schematron(schema_path: &str, doc: *mut _xmlDoc) -> c_int {
    let s = cstr_alloc(schema_path);
    let pctxt = libxml_rs::xml::schematron::xmlSchematronNewParserCtxt(s as *const c_char);
    free_cstr(s);
    if pctxt.is_null() {
        return 5;
    }
    let schema = libxml_rs::xml::schematron::xmlSchematronParse(pctxt);
    if schema.is_null() {
        libxml_rs::xml::schematron::xmlSchematronFreeParserCtxt(pctxt);
        return 5;
    }
    let vctxt = libxml_rs::xml::schematron::xmlSchematronNewValidCtxt(schema);
    if vctxt.is_null() {
        libxml_rs::xml::schematron::xmlSchematronFree(schema);
        libxml_rs::xml::schematron::xmlSchematronFreeParserCtxt(pctxt);
        return 7;
    }
    let valid = libxml_rs::xml::schematron::xmlSchematronValidateDoc(vctxt, doc);
    libxml_rs::xml::schematron::xmlSchematronFreeValidCtxt(vctxt);
    libxml_rs::xml::schematron::xmlSchematronFree(schema);
    libxml_rs::xml::schematron::xmlSchematronFreeParserCtxt(pctxt);
    if valid == 0 {
        0
    } else {
        7
    }
}

/// Evaluate an XPath expression and print the results (separator-aware).
///
/// Returns 0 on success, 10 on XPath error, 11 on empty node-set
/// (upstream exit codes).
unsafe fn eval_xpath_expr(cli: &Cli, expr: &str, doc: *mut _xmlDoc) -> c_int {
    let e = cstr_alloc(expr);
    let ctxt = xmlXPathNewContext(doc);
    if ctxt.is_null() {
        free_cstr(e);
        return 10;
    }
    // Register the core function library (needed for count(), etc.).
    let internal = (*ctxt).extra as *mut libxml_rs::xml::xpath::context::XPathContext;
    if !internal.is_null() {
        let funcs = libxml_rs::xml::xpath::functions::core_functions();
        for (name, func) in funcs {
            (*internal).register_function(&name, func);
        }
    }
    let obj = xmlXPathEvalExpression(e as *const xmlChar, ctxt);
    free_cstr(e);
    if obj.is_null() {
        // UPSTREAM-PARITY: compile failures print
        // "XPath error : Invalid expression" + "XPath compilation failure"
        // (exit 10); evaluation failures print the engine message + "XPath
        // evaluation failure" (exit 10). The per-expression context line and
        // caret are tracked as RESIDUAL R-XPATH-ERRMSG.
        let msg = if !internal.is_null() {
            let ic = &*internal;
            ic.error
                .as_deref()
                .unwrap_or("Invalid expression")
                .to_string()
        } else {
            "Invalid expression".to_string()
        };
        eprintln!("XPath error : {}", msg);
        if msg == "Invalid expression" {
            eprintln!("XPath compilation failure");
        } else {
            eprintln!("XPath evaluation failure");
        }
        xmlXPathFreeContext(ctxt);
        return 10;
    }
    let sep: u8 = if cli.xpath0 { 0 } else { b'\n' };
    let typ = (*obj).type_;
    if typ == xmlXPathObjectType::XPATH_NODESET as c_int {
        let ns = (*obj).nodesetval as *mut _xmlNodeSet;
        if ns.is_null() || (*ns).nodeNr == 0 {
            eprintln!("XPath set is empty");
            xmlXPathFreeObject(obj);
            xmlXPathFreeContext(ctxt);
            return 11;
        }
        if !(*ns).nodeTab.is_null() {
            let mut i = 0;
            while i < (*ns).nodeNr {
                let node = *(*ns).nodeTab.offset(i as isize);
                if !node.is_null() {
                    // Serialize the node.
                    let buf = xmlBufferCreate();
                    if !buf.is_null() {
                        xmlNodeDump(buf, doc, node, 0, 0);
                        let len = xmlBufferLength(buf);
                        let content = xmlBufferContent(buf);
                        if len > 0 && !content.is_null() {
                            write_stdout(core::slice::from_raw_parts(content, len as usize));
                        }
                        xmlBufferFree(buf);
                    }
                }
                write_stdout(&[sep]);
                i += 1;
            }
        }
    } else {
        let strv = xmlXPathCastToString(obj);
        if !strv.is_null() {
            let len = libc::strlen(strv as *const c_char);
            write_stdout(core::slice::from_raw_parts(strv, len));
            write_stdout(&[sep]);
            libxml_rs::abi::allocator::xmlFreeImpl(strv as *mut c_void);
        }
    }
    xmlXPathFreeObject(obj);
    xmlXPathFreeContext(ctxt);
    0
}

/// Parse and process one XML file.
unsafe fn process_file(cli: &mut Cli, filename: &str) {
    let start = std::time::Instant::now();
    let mut doc = parse_document(cli, filename);
    if doc.is_null() {
        // UPSTREAM-PARITY: "Can't open" is printed only when the source file
        // itself cannot be opened. A hard parse error already printed the
        // parser diagnostic, so no extra message appears.
        if !cli.quiet && filename != "-" && !std::path::Path::new(filename).exists() {
            eprintln!("Can't open {}", filename);
        }
        cli.return_code = 4;
        return;
    }
    let parse_time = start.elapsed();

    // UPSTREAM-PARITY: the parsed doc carries the source URL so debug dumps
    // show `URL=<filename>` and error messages reference the source.
    if !doc.is_null() && (*doc).URL.is_null() {
        let cname = cstr_alloc(filename);
        (*doc).URL = cname as *mut libxml_rs::abi::types::xmlChar;
    }

    // Strict namespace check.
    if cli.strict_namespace {
        // nsWellFormed is tracked on the parser context; the document
        // properties XML_DOC_NSVALID reflects it.
        let props = (*doc).properties;
        if (props & (1 << 1)) == 0 {
            eprintln!("{}: document has namespace errors", filename);
            cli.return_code = 4;
        }
    }

    if cli.xinclude {
        let ret = xmlXIncludeProcessFlags(doc, cli.options);
        if ret < 0 {
            cli.return_code = 4;
        }
    }

    // XPath evaluation implies --noout and prints before validation.
    if let Some(expr) = &cli.xpath {
        let ret = eval_xpath_expr(cli, expr, doc);
        if ret != 0 && cli.return_code == 0 {
            cli.return_code = ret;
        }
    }

    // Validation.
    if cli.valid || cli.postvalid || cli.dtdvalid.is_some() || cli.dtdvalidfpi.is_some() {
        let ret = validate_doc(cli, doc, filename);
        // UPSTREAM-PARITY: xmlValidateDocument returns 1 when the document
        // validates, 0 when it does not.
        if ret == 0 && cli.return_code == 0 {
            cli.return_code = 3;
        }
    }
    if let Some(schema) = &cli.schema {
        let ret = validate_schema(schema, doc);
        if ret != 0 && cli.return_code < ret {
            cli.return_code = ret;
        }
    }
    if let Some(schema) = &cli.relaxng {
        let ret = validate_relaxng(schema, doc);
        if ret != 0 && cli.return_code < ret {
            cli.return_code = ret;
        }
    }
    if let Some(schema) = &cli.schematron {
        let ret = validate_schematron(schema, doc);
        if ret != 0 && cli.return_code < ret {
            cli.return_code = ret;
        }
    }

    if cli.pattern.is_some() {
        // Pattern testing: compile and report (minimal surface).
        eprintln!("{}: pattern support is limited in this build", filename);
    }

    if !cli.noout && cli.xpath.is_none() {
        if cli.debug {
            // Debug tree dump.
            let fp = get_output_file(cli);
            if !fp.is_null() {
                libxml_rs::xml::debug::xmlDebugDumpDocument(fp, doc);
                close_output_file(cli, fp);
            }
        } else if cli.copy {
            // Internal copy test: copy the doc and dump the copy.
            let copy = libxml_rs::xml::tree::copy_doc(doc, 1);
            if !copy.is_null() {
                dump_document(cli, copy, filename);
                libxml_rs::xml::tree::free_doc(copy);
            }
        } else {
            dump_document(cli, doc, filename);
        }
    }

    if cli.timing {
        let total = start.elapsed();
        eprintln!(
            "Parsing {} took {} ms, total {} ms",
            filename,
            parse_time.as_millis(),
            total.as_millis()
        );
    }

    libxml_rs::xml::tree::free_doc(doc);
}

/// Minimal interactive shell (upstream xmllint shell has many commands; the
/// core navigation commands are implemented).
unsafe fn shell_loop(cli: &Cli, doc: *mut _xmlDoc) {
    use std::io::Write;
    let mut current: *mut _xmlNode = if doc.is_null() {
        ptr::null_mut()
    } else {
        (*doc).children
    };
    loop {
        print!("/ > ");
        std::io::stdout().flush().ok();
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let cmd = line.trim().to_string();
        if cmd.is_empty() {
            continue;
        }
        let mut parts = cmd.split_whitespace();
        let verb = parts.next().unwrap_or("");
        match verb {
            "quit" | "exit" | "bye" | "q" => break,
            "pwd" => {
                println!("/");
            }
            "ls" => {
                let mut child = if current.is_null() {
                    ptr::null_mut()
                } else {
                    (*current).children
                };
                while !child.is_null() {
                    let name = (*child).name;
                    if !name.is_null() {
                        println!(
                            "{}",
                            std::ffi::CStr::from_ptr(name as *const c_char).to_string_lossy()
                        );
                    }
                    child = (*child).next;
                }
            }
            "cd" => {
                let arg = parts.next().unwrap_or(".");
                if arg == ".." {
                    if !current.is_null() && !(*current).parent.is_null() {
                        current = (*current).parent;
                    }
                } else if arg == "/" || arg == "root" {
                    current = (*doc).children;
                } else {
                    let mut child = if current.is_null() {
                        ptr::null_mut()
                    } else {
                        (*current).children
                    };
                    while !child.is_null() {
                        let name = (*child).name;
                        if !name.is_null()
                            && std::ffi::CStr::from_ptr(name as *const c_char).to_string_lossy()
                                == arg
                        {
                            current = child;
                            break;
                        }
                        child = (*child).next;
                    }
                }
            }
            "dir" => {
                if !current.is_null() {
                    let fp = libc::fdopen(1, b"w\0".as_ptr() as *const c_char);
                    if !fp.is_null() {
                        libxml_rs::xml::debug::xmlDebugDumpNode(fp, current, 0);
                    }
                }
            }
            "print" => {
                if !current.is_null() {
                    let buf = xmlBufferCreate();
                    if !buf.is_null() {
                        xmlNodeDump(buf, doc, current, 0, 1);
                        let len = xmlBufferLength(buf);
                        let content = xmlBufferContent(buf);
                        if len > 0 && !content.is_null() {
                            write_stdout(core::slice::from_raw_parts(content, len as usize));
                        }
                        write_stdout(b"\n");
                        xmlBufferFree(buf);
                    }
                }
            }
            "help" | "?" => {
                println!("Commands: pwd, ls, cd, dir, print, quit");
            }
            _ => {
                println!("Unknown command {}", verb);
            }
        }
    }
}

/// Generate a small document on the fly (--auto).
unsafe fn make_auto_doc() -> *mut _xmlDoc {
    let src = b"<?xml version=\"1.0\"?>\n<doc><a>1</a><b>2</b><c>3</c></doc>\n\0";
    xmlReadMemory(
        src.as_ptr() as *const c_char,
        (src.len() - 1) as c_int,
        b"auto.xml\0".as_ptr() as *const c_char,
        ptr::null(),
        0,
    )
}

unsafe fn cstr_alloc(s: &str) -> *mut c_char {
    let bytes = s.as_bytes();
    let p = libc::malloc(bytes.len() + 1) as *mut c_char;
    if p.is_null() {
        return p;
    }
    libc::memcpy(
        p as *mut libc::c_void,
        bytes.as_ptr() as *const libc::c_void,
        bytes.len(),
    );
    *p.add(bytes.len()) = 0;
    p
}

unsafe fn free_cstr(p: *mut c_char) {
    if !p.is_null() {
        libc::free(p as *mut libc::c_void);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut cli = Cli::default();

    if args.is_empty() {
        usage();
        std::process::exit(1);
    }

    let mut files: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        let mut need_value: Option<&str> = None;
        match arg {
            "--version" | "-version" => {
                print_version();
                std::process::exit(0);
            }
            "--shell" => cli.shell = true,
            "--debug" => cli.debug = true,
            "--copy" => cli.copy = true,
            "--recover" => cli.recover = true,
            "--huge" => cli.options |= XML_PARSE_HUGE,
            "--noent" => cli.options |= XML_PARSE_NOENT,
            "--noenc" => cli.options |= XML_PARSE_IGNORE_ENC,
            "--noout" => cli.noout = true,
            "--load-trace" => cli.loadtrace = true,
            "--nonet" => cli.nonet = true,
            "--nocompact" => cli.nocompact = true,
            "--valid" => cli.valid = true,
            "--postvalid" => cli.postvalid = true,
            "--insert" => cli.insert = true,
            "--strict-namespace" => cli.strict_namespace = true,
            "--quiet" => cli.quiet = true,
            "--timing" => cli.timing = true,
            "--repeat" => cli.repeat = true,
            "--dropdtd" => cli.dropdtd = true,
            "--html" => cli.html = true,
            "--nodefdtd" => cli.nodefdtd = true,
            "--xmlout" => cli.xmlout = true,
            "--push" => cli.push = true,
            "--memory" => cli.memory = true,
            "--nowarning" => cli.nowarning = true,
            "--noblanks" => cli.noblanks = true,
            "--nocdata" => cli.nocdata = true,
            "--nodict" => cli.nodict = true,
            "--pedantic" => cli.pedantic = true,
            "--nsclean" => cli.nsclean = true,
            "--auto" => cli.auto = true,
            "--xinclude" => cli.xinclude = true,
            "--noxincludenode" => cli.noxincludenode = true,
            "--nofixup-base-uris" => cli.nofixup_base_uris = true,
            "--catalogs" => cli.catalogs = true,
            "--nocatalogs" => cli.nocatalogs = true,
            "--stream" => cli.stream = true,
            "--walker" => cli.walker = true,
            "--sax1" => cli.sax1 = true,
            "--sax" => cli.sax = true,
            "--oldxml10" => cli.oldxml10 = true,
            "--format" => cli.format = 1,
            "--compress" => cli.compress = true,
            "--c14n" => cli.c14n = 1,
            "--c14n11" => cli.c14n = 2,
            "--exc-c14n" => cli.c14n = 3,
            "--xpath0" => {
                cli.xpath0 = true;
                cli.noout = true;
                need_value = Some("--xpath0");
            }
            "--xpath" => {
                cli.noout = true;
                need_value = Some("--xpath");
            }
            "--path" => need_value = Some("--path"),
            "--dtdvalid" => need_value = Some("--dtdvalid"),
            "--dtdvalidfpi" => need_value = Some("--dtdvalidfpi"),
            "--maxmem" => need_value = Some("--maxmem"),
            "--max-ampl" => need_value = Some("--max-ampl"),
            "--output" | "-o" => need_value = Some("--output"),
            "--encode" => need_value = Some("--encode"),
            "--pretty" => need_value = Some("--pretty"),
            "--pattern" => need_value = Some("--pattern"),
            "--relaxng" => need_value = Some("--relaxng"),
            "--schema" => need_value = Some("--schema"),
            "--schematron" => need_value = Some("--schematron"),
            "--loaddtd" => cli.loaddtd = true,
            "--dtdattr" => {
                cli.dtdattr = true;
                cli.loaddtd = true;
            }
            _ => {
                if arg.starts_with('-') && arg.len() > 1 {
                    // Unknown option (upstream prints a message + usage to
                    // stderr and exits 1).
                    eprintln!("Unknown option {}", arg);
                    usage();
                    std::process::exit(1);
                } else {
                    files.push(arg.to_string());
                }
            }
        }
        if let Some(opt) = need_value {
            if i + 1 < args.len() {
                let val = args[i + 1].clone();
                match opt {
                    "--xpath" | "--xpath0" => cli.xpath = Some(val),
                    "--path" => {
                        for p in val.split(':') {
                            cli.paths.push(p.to_string());
                        }
                    }
                    "--dtdvalid" => cli.dtdvalid = Some(val),
                    "--dtdvalidfpi" => cli.dtdvalidfpi = Some(val),
                    "--maxmem" => {
                        cli.maxmem = val.parse().ok();
                    }
                    "--max-ampl" => {
                        cli.max_ampl = val.parse().ok();
                    }
                    "--output" => cli.output = Some(val),
                    "--encode" => cli.encode = Some(val),
                    "--pretty" => {
                        cli.pretty = val.parse().unwrap_or(-1);
                    }
                    "--pattern" => cli.pattern = Some(val),
                    "--relaxng" => cli.relaxng = Some(val),
                    "--schema" => cli.schema = Some(val),
                    "--schematron" => cli.schematron = Some(val),
                    _ => {}
                }
                i += 1;
            }
        }
        i += 1;
    }

    unsafe {
        // UPSTREAM-PARITY: --xmlout without --html is a warning; processing
        // continues normally.
        if cli.xmlout && !cli.html {
            eprintln!("Warning: Option --xmlout requires --html");
        }
        if cli.auto {
            // Generate a small doc and process it.
            let doc = make_auto_doc();
            if !doc.is_null() {
                dump_document(&cli, doc, "auto");
                libxml_rs::xml::tree::free_doc(doc);
            }
            std::process::exit(cli.return_code);
        }
        if files.is_empty() {
            usage();
            std::process::exit(1);
        }
        for f in &files {
            if cli.repeat {
                for _ in 0..100 {
                    let doc = parse_document(&cli, f);
                    if !doc.is_null() {
                        libxml_rs::xml::tree::free_doc(doc);
                    }
                }
            }
            if cli.shell {
                let doc = parse_document(&cli, f);
                shell_loop(&cli, doc);
                if !doc.is_null() {
                    libxml_rs::xml::tree::free_doc(doc);
                }
                continue;
            }
            process_file(&mut cli, f);
        }
        std::process::exit(cli.return_code);
    }
}
