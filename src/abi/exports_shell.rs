//! exports_shell — XML shell family C ABI exports (11.1-I shell closure).
//!
//! Faithful port of the libxml2 shell section that lived in `debugXML.c`
//! (the file was named `xmlshell.c` before libxml2 2.0 and `shell.c` after
//! the 2.13 refactor). The fourteen `xmlShell*` symbols were removed from
//! `debugXML.h` in 2.13+ but are still exported from the DSO; the reference
//! ABI is libxml2 2.12.6 (the last release declaring them in the public
//! header).
//!
//! # UPSTREAM-PARITY
//!
//! Signatures follow `debugXML.h` (2.0.0 → 2.12.6 are identical):
//!
//! ```c
//! typedef char * (* xmlShellReadlineFunc)(char *prompt);
//! struct _xmlShellCtxt {
//!     char *filename;
//!     xmlDocPtr doc;
//!     xmlNodePtr node;
//!     xmlXPathContextPtr pctxt;
//!     int loaded;
//!     FILE *output;
//!     xmlShellReadlineFunc input;
//! };
//! typedef int (* xmlShellCmd)(xmlShellCtxtPtr ctxt, char *arg,
//!                             xmlNodePtr node, xmlNodePtr node2);
//! ```
//!
//! The `FILE*` is opaque at the ABI boundary (`*mut c_void`); string output
//! goes through the `fwrite`/`fputs` libc symbols declared below. Command
//! functions write their results to `ctxt->output` (or stdout when no
//! context is given), matching upstream's `xmlShellPrintNodeCtxt`/`xmlShellCat`
//! convention. Error messages use the generic error channel, which defaults
//! to stderr (upstream `xmlGenericError` / `xmlGenericErrorContext`).
//!
//! # DIVERGENCES (recorded residuals)
//!
//! - `xmlShellPrintXPathResult`: upstream 2.12.x exported a no-op for a NULL
//!   shell context; per the shell family closure directive it prints the
//!   object with `xmlXPathDebugDumpObject(stdout, list, 0)` — the same call
//!   the 2.12.x shell loop uses for its `xpath` command.
//! - `xmlShellValidate` with a DTD argument parses the DTD file with a
//!   minimal `<!DOCTYPE>` header scan (the crate has no `xmlParseDTD`
//!   external-subset parser); a file that cannot be scanned yields NULL and
//!   `-1`, exactly like upstream's failed `xmlParseDTD`.
//! - The `set` (fragment parse) and `relaxng` shell commands are not
//!   dispatched (`xmlParseInNodeContext` / RELAX NG integration not yet
//!   available in the crate); their help lines are omitted.
//!
//! # Upstream contract
//!
//! Parity target is the libxml2 shell section of `debugXML.c` (the file named
//! `xmlshell.c` before 2.0 and `shell.c` after the 2.13 refactor); the
//! fourteen `xmlShell*` symbols were removed from `debugXML.h` in 2.13+ but
//! are still exported from the DSO, so the reference ABI is 2.12.6 (last
//! release declaring them) while the runtime target is 2.15.3. R-000168
//! (11.1-U) recorded the c_char(u8-on-aarch64) buffer typing fix in the shell
//! debugger.
//!
//! # Conceptual behavior
//!
//! This module implements the interactive XML shell commands: node printing,
//! XPath evaluation and printing, directory/base traversal, element/attr/
//! namespace listing, document validation and the load/save commands, writing
//! through `ctxt->output` (or stdout) and the generic error channel exactly
//! like upstream.
//!
//! # Ownership & safety invariants
//!
//! The `_xmlShellCtxt` is caller-owned; the shell borrows its doc/node/pctxt
//! (never frees them — the caller owns the document). Strings returned by
//! `xmlShellReadlineFunc` are caller-allocated. The `FILE*` output is opaque
//! at the ABI boundary.
//!
//! # Historical quirks & epochs
//!
//! The shell API is the 2.0-era debugging surface kept exported for xmllint
//! compatibility; E-005/E-006 (exit-code reworks in 2.13.0 and 2.15.0)
//! changed the CLI validation behavior that the shells validate command
//! feeds. The 2.13 header removal is itself a historical oddity the candidate
//! mirrors by keeping the exports.
//!
//! # Deliberate oddities
//!
//! The DIVERGENCES listed above are deliberate: `xmlShellPrintXPathResult`
//! prints via `xmlXPathDebugDumpObject` per the shell closure directive,
//! `xmlShellValidate` uses a minimal DOCTYPE header scan, and the `set`/
//! `relaxng` commands are not dispatched.
//!
//! # Proving courts
//!
//! The C14N, CLI-XMLLINT, DTD, HTML, PARSER, RELAXNG, SCHEMATRON, XINCLUDE
//! and XSD court families plus DSO-LOADER cover this module.
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is to drop the shell exports because the
//! interactive shell is rarely used — they are still oracle-DSO symbols and
//! the DSO-LOADER court resolves them; removing them would break downstream
//! embedding of xmllints shell. Another shortcut, routing shell output
//! straight to stdout instead of `ctxt->output`, would break the
//! output-redirection contract the CLI-XMLLINT courts exercise.

#![allow(
    missing_docs,
    missing_debug_implementations,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals
)]

use core::ffi::c_void;
use core::ptr;
use std::os::raw::{c_char, c_int};

use crate::abi::allocator::{xmlFreeImpl, xmlMallocImpl, xmlMallocZero};
use crate::abi::exports_string::xmlStrstr;
use crate::abi::exports_tree::{xmlNodeGetBase, xmlNodeSetBase};
use crate::abi::exports_uri::xmlCanonicPath;
use crate::abi::exports_xml2::{
    xmlReadFile, xmlSaveFile, xmlStrEqual, xmlStrchr, xmlStrdup, xmlValidateDocument,
    xmlValidateDtd, xmlXPathEval, xmlXPathFreeContext, xmlXPathFreeObject, xmlXPathNewContext,
    xmlXPathRegisterNs,
};
use crate::abi::structs::{
    _xmlAttr, _xmlDoc, _xmlDtd, _xmlNode, _xmlNodeSet, _xmlValidCtxt, _xmlXPathContext,
    _xmlXPathObject,
};
use crate::abi::types::{xmlChar, xmlElementType, xmlXPathObjectType};
use crate::xml::xpath::exports::xmlXPathDebugDumpObject;
use crate::xml::{debug, io, tree};

// ═══════════════════════════════════════════════════════════════════════════════
// Shell context / callbacks (debugXML.h layout)
// ═══════════════════════════════════════════════════════════════════════════════

/// `char *(*xmlShellReadlineFunc)(char *prompt)` — returns a NUL-terminated
/// line allocated by the provider; freed by the shell with `free()`.
pub type xmlShellReadlineFunc = Option<unsafe extern "C" fn(prompt: *mut c_char) -> *mut c_char>;

/// The shell context (`struct _xmlShellCtxt`, upstream debugXML.h layout).
#[repr(C)]
pub struct _xmlShellCtxt {
    /// The file name the shell was started on (xmlStrdup'ed / xmlCanonicPath'ed).
    pub filename: *mut c_char,
    /// The current document.
    pub doc: *mut _xmlDoc,
    /// The current node.
    pub node: *mut _xmlNode,
    /// The XPath evaluation context.
    pub pctxt: *mut _xmlXPathContext,
    /// Whether `doc` was loaded by the shell itself (and must be freed).
    pub loaded: c_int,
    /// The output `FILE*` the shell writes results to.
    pub output: *mut c_void,
    /// The line reading callback.
    pub input: xmlShellReadlineFunc,
}

/// `typedef int (*xmlShellCmd)(xmlShellCtxtPtr, char*, xmlNodePtr, xmlNodePtr)`
/// — generic signature of the shell command functions (debugXML.h).
pub type xmlShellCmd = Option<
    unsafe extern "C" fn(*mut _xmlShellCtxt, *mut c_char, *mut _xmlNode, *mut _xmlNode) -> c_int,
>;

extern "C" {
    /// `size_t fwrite(const void *ptr, size_t size, size_t nmemb, FILE *stream)`.
    fn fwrite(ptr: *const c_void, size: usize, nmemb: usize, stream: *mut c_void) -> usize;
    /// `int fputs(const char *s, FILE *stream)`.
    fn fputs(s: *const c_char, stream: *mut c_void) -> c_int;
    /// The libc `FILE *stdout` variable.
    static mut stdout: *mut c_void;
    /// The libc `FILE *stderr` variable.
    static mut stderr: *mut c_void;
}

// ═══════════════════════════════════════════════════════════════════════════════
// Small output helpers (the shell has no variadic fprintf in Rust)
// ═══════════════════════════════════════════════════════════════════════════════

/// Append the bytes of a NUL-terminated C string to `v`.
unsafe fn push_cstr(v: &mut Vec<u8>, s: *const c_char) {
    if s.is_null() {
        return;
    }
    let len = libc::strlen(s);
    v.extend_from_slice(core::slice::from_raw_parts(s as *const u8, len));
}

/// Write raw bytes to a `FILE*`.
unsafe fn out_bytes(fp: *mut c_void, bytes: &[u8]) {
    if fp.is_null() || bytes.is_empty() {
        return;
    }
    unsafe {
        fwrite(bytes.as_ptr() as *const c_void, 1, bytes.len(), fp);
    }
}

/// Write a NUL-terminated C string to a `FILE*`.
unsafe fn out_cstr(fp: *mut c_void, s: *const c_char) {
    if fp.is_null() || s.is_null() {
        return;
    }
    unsafe {
        fputs(s, fp);
    }
}

/// Generic error channel (upstream `xmlGenericError(xmlGenericErrorContext,
/// fmt, ...)`); the default context is stderr.
///
/// Renders `arg` + `mid` + `end`, i.e. the "%s..."-style messages the
/// shell emits.
unsafe fn shell_generic_error(arg: *const c_char, mid: &[u8], end: &[u8]) {
    let mut v = Vec::new();
    unsafe {
        push_cstr(&mut v, arg);
    }
    v.extend_from_slice(mid);
    v.extend_from_slice(end);
    unsafe {
        out_bytes(stderr, &v);
    }
}

/// Dump a node subtree (upstream `xmlElemDump(output, doc, cur)`, i.e.
/// `xmlNodeDumpOutput(output, doc, cur, 0, 0, NULL)`) to a `FILE*` using the
/// crate serializer.
unsafe fn shell_elem_dump(fp: *mut c_void, doc: *mut _xmlDoc, node: *mut _xmlNode) -> c_int {
    if fp.is_null() || node.is_null() {
        return -1;
    }
    let buf = io::buf_create(-1);
    if buf.is_null() {
        return -1;
    }
    let ret = tree::node_dump(buf, doc, node, 0, 0);
    if ret < 0 {
        io::buf_free(buf);
        return -1;
    }
    let content = io::buf_content(buf);
    let len = io::buf_length(buf);
    if !content.is_null() && len > 0 {
        unsafe {
            fwrite(content as *const c_void, 1, len as usize, fp);
        }
    }
    io::buf_free(buf);
    0
}

/// Dump an HTML document to a `FILE*` (upstream `htmlDocDump`).
unsafe fn shell_html_doc_dump(fp: *mut c_void, doc: *mut _xmlDoc) -> c_int {
    if fp.is_null() || doc.is_null() {
        return -1;
    }
    let buf = io::buf_create(-1);
    if buf.is_null() {
        return -1;
    }
    let ret = crate::xml::html::doc_dump(buf, doc);
    if ret < 0 {
        io::buf_free(buf);
        return -1;
    }
    let content = io::buf_content(buf);
    let len = io::buf_length(buf);
    if !content.is_null() && len > 0 {
        unsafe {
            fwrite(content as *const c_void, 1, len as usize, fp);
        }
    }
    io::buf_free(buf);
    ret
}

/// Dump an HTML node subtree to a `FILE*` (upstream `htmlNodeDumpFile`).
unsafe fn shell_html_node_dump_file(fp: *mut c_void, node: *mut _xmlNode) -> c_int {
    if fp.is_null() || node.is_null() {
        return -1;
    }
    let buf = io::buf_create(-1);
    if buf.is_null() {
        return -1;
    }
    let before = io::buf_length(buf);
    crate::xml::html::serialize_node(node, buf, 0, 0);
    let after = io::buf_length(buf);
    if after < 0 || before < 0 {
        io::buf_free(buf);
        return -1;
    }
    let content = io::buf_content(buf);
    let len = io::buf_length(buf);
    if !content.is_null() && len > 0 {
        unsafe {
            fwrite(content as *const c_void, 1, len as usize, fp);
        }
    }
    io::buf_free(buf);
    after - before
}

/// `xmlChar *xmlGetNodePath(const xmlNode *node)` — build the XPath-like path
/// of a node (tree.c 2.12.6). Returns a xmlMalloc'd string or NULL.
unsafe fn shell_get_node_path(node: *const _xmlNode) -> *mut xmlChar {
    if node.is_null() || (*node).type_ == xmlElementType::XML_NAMESPACE_DECL as c_int {
        return ptr::null_mut();
    }

    // Segments are collected from the node up to the root, then reversed;
    // each segment is "sep" + name (+ "[occur]").
    let mut segments: Vec<Vec<u8>> = Vec::new();
    let mut cur: *const _xmlNode = node;

    loop {
        if cur.is_null() {
            break;
        }
        let typ = (*cur).type_;
        let mut seg: Vec<u8> = Vec::new();
        let mut occur: c_int = 0;
        let mut generic: bool;
        let next: *const _xmlNode;

        if typ == xmlElementType::XML_DOCUMENT_NODE as c_int
            || typ == xmlElementType::XML_HTML_DOCUMENT_NODE as c_int
        {
            // Upstream: `if (buffer[0] == '/') break;` — any built segment
            // starts with '/', so a non-empty segment list means we stop here.
            if !segments.is_empty() {
                break;
            }
            seg.extend_from_slice(b"/");
            next = ptr::null();
        } else if typ == xmlElementType::XML_ELEMENT_NODE as c_int {
            generic = false;
            seg.extend_from_slice(b"/");
            let name = (*cur).name;
            let ns = (*cur).ns;
            if !name.is_null() {
                if !ns.is_null() && !(*ns).prefix.is_null() {
                    unsafe {
                        push_cstr(&mut seg, (*ns).prefix as *const c_char);
                    }
                    seg.push(b':');
                    unsafe {
                        push_cstr(&mut seg, name as *const c_char);
                    }
                } else if !ns.is_null() {
                    // Elements in the default namespace are expressed as "*".
                    generic = true;
                    seg.extend_from_slice(b"*");
                } else {
                    unsafe {
                        push_cstr(&mut seg, name as *const c_char);
                    }
                }
            }
            next = (*cur).parent;

            // Thumbler index computation (occurrence among same-name siblings).
            let mut tmp = (*cur).prev;
            while !tmp.is_null() {
                if (*tmp).type_ == xmlElementType::XML_ELEMENT_NODE as c_int
                    && (generic || unsafe { shell_same_element_name(cur, tmp) })
                {
                    occur += 1;
                }
                tmp = (*tmp).prev;
            }
            if occur == 0 {
                let mut tmp = (*cur).next;
                while !tmp.is_null() && occur == 0 {
                    if (*tmp).type_ == xmlElementType::XML_ELEMENT_NODE as c_int
                        && (generic || unsafe { shell_same_element_name(cur, tmp) })
                    {
                        occur += 1;
                    }
                    tmp = (*tmp).next;
                }
                if occur != 0 {
                    occur = 1;
                }
            } else {
                occur += 1;
            }
        } else if typ == xmlElementType::XML_COMMENT_NODE as c_int {
            seg.extend_from_slice(b"/comment()");
            next = (*cur).parent;

            let mut tmp = (*cur).prev;
            while !tmp.is_null() {
                if (*tmp).type_ == xmlElementType::XML_COMMENT_NODE as c_int {
                    occur += 1;
                }
                tmp = (*tmp).prev;
            }
            if occur == 0 {
                let mut tmp = (*cur).next;
                while !tmp.is_null() && occur == 0 {
                    if (*tmp).type_ == xmlElementType::XML_COMMENT_NODE as c_int {
                        occur += 1;
                    }
                    tmp = (*tmp).next;
                }
                if occur != 0 {
                    occur = 1;
                }
            } else {
                occur += 1;
            }
        } else if typ == xmlElementType::XML_TEXT_NODE as c_int
            || typ == xmlElementType::XML_CDATA_SECTION_NODE as c_int
        {
            seg.extend_from_slice(b"/text()");
            next = (*cur).parent;

            let mut tmp = (*cur).prev;
            while !tmp.is_null() {
                if (*tmp).type_ == xmlElementType::XML_TEXT_NODE as c_int
                    || (*tmp).type_ == xmlElementType::XML_CDATA_SECTION_NODE as c_int
                {
                    occur += 1;
                }
                tmp = (*tmp).prev;
            }
            if occur == 0 {
                let mut tmp = (*cur).next;
                while !tmp.is_null() {
                    if (*tmp).type_ == xmlElementType::XML_TEXT_NODE as c_int
                        || (*tmp).type_ == xmlElementType::XML_CDATA_SECTION_NODE as c_int
                    {
                        occur = 1;
                        break;
                    }
                    tmp = (*tmp).next;
                }
            } else {
                occur += 1;
            }
        } else if typ == xmlElementType::XML_PI_NODE as c_int {
            let mut nm = Vec::new();
            nm.extend_from_slice(b"processing-instruction('");
            unsafe {
                push_cstr(&mut nm, (*cur).name as *const c_char);
            }
            nm.extend_from_slice(b"')");
            seg.extend_from_slice(b"/");
            seg.extend_from_slice(&nm);
            next = (*cur).parent;

            let mut tmp = (*cur).prev;
            while !tmp.is_null() {
                if (*tmp).type_ == xmlElementType::XML_PI_NODE as c_int
                    && unsafe { xmlStrEqual((*cur).name, (*tmp).name) != 0 }
                {
                    occur += 1;
                }
                tmp = (*tmp).prev;
            }
            if occur == 0 {
                let mut tmp = (*cur).next;
                while !tmp.is_null() && occur == 0 {
                    if (*tmp).type_ == xmlElementType::XML_PI_NODE as c_int
                        && unsafe { xmlStrEqual((*cur).name, (*tmp).name) != 0 }
                    {
                        occur += 1;
                    }
                    tmp = (*tmp).next;
                }
                if occur != 0 {
                    occur = 1;
                }
            } else {
                occur += 1;
            }
        } else if typ == xmlElementType::XML_ATTRIBUTE_NODE as c_int {
            seg.extend_from_slice(b"/@");
            let attr = cur as *const _xmlAttr;
            let name = (*attr).name;
            if !name.is_null() {
                if !(*attr).ns.is_null() && !(*(*attr).ns).prefix.is_null() {
                    unsafe {
                        push_cstr(&mut seg, (*(*attr).ns).prefix as *const c_char);
                    }
                    seg.push(b':');
                }
                unsafe {
                    push_cstr(&mut seg, name as *const c_char);
                }
            }
            next = (*attr).parent;
        } else {
            return ptr::null_mut();
        }

        if occur != 0 {
            seg.extend_from_slice(format!("[{}]", occur).as_bytes());
        }
        segments.push(seg);
        if next.is_null() {
            break;
        }
        cur = next;
    }

    // Assemble the final path: root-most segment first.
    let mut path: Vec<u8> = Vec::new();
    for seg in segments.iter().rev() {
        path.extend_from_slice(seg);
    }
    path.push(0);

    let ret = xmlMallocImpl(path.len()) as *mut xmlChar;
    if ret.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::copy_nonoverlapping(path.as_ptr(), ret, path.len());
    }
    ret
}

/// Upstream element-name equality for the path thumbler: same local name and
/// same namespace (pointer equality, or equal prefixes when both are set).
unsafe fn shell_same_element_name(a: *const _xmlNode, b: *const _xmlNode) -> bool {
    unsafe {
        if xmlStrEqual((*a).name, (*b).name) == 0 {
            return false;
        }
        let ans = (*a).ns;
        let bns = (*b).ns;
        if ans == bns {
            return true;
        }
        if !ans.is_null() && !bns.is_null() {
            return xmlStrEqual((*ans).prefix, (*bns).prefix) != 0;
        }
        false
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlShellPrintXPathError / xmlShellPrintNode / xmlShellPrintXPathResult
// ═══════════════════════════════════════════════════════════════════════════════

/// `void xmlShellPrintXPathError(int errorType, const char *arg)`.
///
/// Print the XPath error to the default error channel (stderr).
///
/// # SAFETY
///
///
/// - `arg` must point to valid NUL-terminated
///   strings (or NULL where the C contract allows) for the lifetime
///   of the call.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlShellPrintXPathError(errorType: c_int, arg: *const c_char) {
    let default_arg = b"Result\0";
    let arg = if arg.is_null() {
        default_arg.as_ptr() as *const c_char
    } else {
        arg
    };

    if errorType == xmlXPathObjectType::XPATH_UNDEFINED as c_int {
        unsafe { shell_generic_error(arg, b": ", b"no such node\n") };
    } else if errorType == xmlXPathObjectType::XPATH_BOOLEAN as c_int {
        unsafe { shell_generic_error(arg, b" is a Boolean", b"\n") };
    } else if errorType == xmlXPathObjectType::XPATH_NUMBER as c_int {
        unsafe { shell_generic_error(arg, b" is a number", b"\n") };
    } else if errorType == xmlXPathObjectType::XPATH_STRING as c_int {
        unsafe { shell_generic_error(arg, b" is a string", b"\n") };
    } else if errorType == xmlXPathObjectType::XPATH_POINT as c_int {
        unsafe { shell_generic_error(arg, b" is a point", b"\n") };
    } else if errorType == xmlXPathObjectType::XPATH_RANGE as c_int
        || errorType == xmlXPathObjectType::XPATH_LOCATIONSET as c_int
    {
        unsafe { shell_generic_error(arg, b" is a range", b"\n") };
    } else if errorType == xmlXPathObjectType::XPATH_USERS as c_int {
        unsafe { shell_generic_error(arg, b" is user-defined", b"\n") };
    } else if errorType == xmlXPathObjectType::XPATH_XSLT_TREE as c_int {
        unsafe { shell_generic_error(arg, b" is an XSLT value tree", b"\n") };
    }
}

/// Print a node to the output `FILE*` of the context (stdout when `ctxt` is
/// NULL) — upstream static `xmlShellPrintNodeCtxt`.
unsafe fn xmlShellPrintNodeCtxt(ctxt: *mut _xmlShellCtxt, node: *mut _xmlNode) {
    if node.is_null() {
        return;
    }
    let fp = if ctxt.is_null() {
        unsafe { stdout }
    } else {
        (*ctxt).output
    };

    let typ = unsafe { (*node).type_ };
    if typ == xmlElementType::XML_DOCUMENT_NODE as c_int {
        tree::xmlDocDump(fp, node as *mut _xmlDoc);
    } else if typ == xmlElementType::XML_ATTRIBUTE_NODE as c_int {
        unsafe {
            debug::xmlDebugDumpAttrList(fp as *mut debug::_IO_FILE, node as *mut _xmlAttr, 0);
        }
    } else {
        unsafe {
            shell_elem_dump(fp, (*node).doc, node);
        }
    }
    unsafe {
        out_bytes(fp, b"\n");
    }
}

/// `void xmlShellPrintNode(xmlNodePtr node)` — print a node to stdout.
///
/// # SAFETY
///
/// - `node` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlShellPrintNode(node: *mut _xmlNode) {
    unsafe { xmlShellPrintNodeCtxt(ptr::null_mut(), node) };
}

/// `void xmlShellPrintXPathResult(xmlXPathObjectPtr list)`.
///
/// Prints an XPath result object to stdout via `xmlXPathDebugDumpObject`
/// (the same printer the 2.12.x shell loop uses for its `xpath` command).
///
/// # SAFETY
///
/// - `list` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlShellPrintXPathResult(list: *mut _xmlXPathObject) {
    unsafe {
        xmlXPathDebugDumpObject(stdout, list, 0);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlShellList / xmlShellBase / xmlShellDir
// ═══════════════════════════════════════════════════════════════════════════════

/// `int xmlShellList(xmlShellCtxtPtr ctxt, char *arg, xmlNodePtr node,
/// xmlNodePtr node2)` — the shell "ls" command.
///
/// # SAFETY
///
/// - `ctxt`, `_arg`, `node`, `_node2` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlShellList(
    ctxt: *mut _xmlShellCtxt,
    _arg: *mut c_char,
    node: *mut _xmlNode,
    _node2: *mut _xmlNode,
) -> c_int {
    if ctxt.is_null() {
        return 0;
    }
    if node.is_null() {
        unsafe {
            out_bytes((*ctxt).output, b"NULL\n");
        }
        return 0;
    }
    let typ = unsafe { (*node).type_ };
    let mut cur: *mut _xmlNode;
    if typ == xmlElementType::XML_DOCUMENT_NODE as c_int
        || typ == xmlElementType::XML_HTML_DOCUMENT_NODE as c_int
    {
        cur = unsafe { (*(node as *mut _xmlDoc)).children };
    } else if typ == xmlElementType::XML_NAMESPACE_DECL as c_int {
        unsafe {
            debug::xmlLsOneNode((*ctxt).output as *mut debug::_IO_FILE, node);
        }
        return 0;
    } else if !unsafe { (*node).children }.is_null() {
        cur = unsafe { (*node).children };
    } else {
        unsafe {
            debug::xmlLsOneNode((*ctxt).output as *mut debug::_IO_FILE, node);
        }
        return 0;
    }
    while !cur.is_null() {
        unsafe {
            debug::xmlLsOneNode((*ctxt).output as *mut debug::_IO_FILE, cur);
            cur = (*cur).next;
        }
    }
    0
}

/// `int xmlShellBase(xmlShellCtxtPtr ctxt, char *arg, xmlNodePtr node,
/// xmlNodePtr node2)` — the shell "base" command.
///
/// # SAFETY
///
/// - `ctxt`, `_arg`, `node`, `_node2` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlShellBase(
    ctxt: *mut _xmlShellCtxt,
    _arg: *mut c_char,
    node: *mut _xmlNode,
    _node2: *mut _xmlNode,
) -> c_int {
    if ctxt.is_null() {
        return 0;
    }
    if node.is_null() {
        unsafe {
            out_bytes((*ctxt).output, b"NULL\n");
        }
        return 0;
    }

    let base = unsafe { xmlNodeGetBase((*node).doc, node) };

    if base.is_null() {
        unsafe {
            out_bytes((*ctxt).output, b" No base found !!!\n");
        }
    } else {
        unsafe {
            out_cstr((*ctxt).output, base as *const c_char);
            out_bytes((*ctxt).output, b"\n");
            xmlFreeImpl(base as *mut c_void);
        }
    }
    0
}

/// `int xmlShellDir(xmlShellCtxtPtr ctxt, char *arg, xmlNodePtr node,
/// xmlNodePtr node2)` — the shell "dir" command.
///
/// # SAFETY
///
/// - `ctxt`, `_arg`, `node`, `_node2` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlShellDir(
    ctxt: *mut _xmlShellCtxt,
    _arg: *mut c_char,
    node: *mut _xmlNode,
    _node2: *mut _xmlNode,
) -> c_int {
    if ctxt.is_null() {
        return 0;
    }
    if node.is_null() {
        unsafe {
            out_bytes((*ctxt).output, b"NULL\n");
        }
        return 0;
    }
    let typ = unsafe { (*node).type_ };
    unsafe {
        if typ == xmlElementType::XML_DOCUMENT_NODE as c_int
            || typ == xmlElementType::XML_HTML_DOCUMENT_NODE as c_int
        {
            debug::xmlDebugDumpDocumentHead(
                (*ctxt).output as *mut debug::_IO_FILE,
                node as *mut _xmlDoc,
            );
        } else if typ == xmlElementType::XML_ATTRIBUTE_NODE as c_int {
            debug::xmlDebugDumpAttr(
                (*ctxt).output as *mut debug::_IO_FILE,
                node as *mut _xmlAttr,
                0,
            );
        } else {
            debug::xmlDebugDumpOneNode((*ctxt).output as *mut debug::_IO_FILE, node, 0);
        }
    }
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlShellCat / xmlShellLoad / xmlShellWrite / xmlShellSave
// ═══════════════════════════════════════════════════════════════════════════════

/// `int xmlShellCat(xmlShellCtxtPtr ctxt, char *arg, xmlNodePtr node,
/// xmlNodePtr node2)` — the shell "cat" command: dump the serialization of
/// the node (XML or HTML, matching the document type).
///
/// # SAFETY
///
/// - `ctxt`, `_arg`, `node`, `_node2` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlShellCat(
    ctxt: *mut _xmlShellCtxt,
    _arg: *mut c_char,
    node: *mut _xmlNode,
    _node2: *mut _xmlNode,
) -> c_int {
    if ctxt.is_null() {
        return 0;
    }
    if node.is_null() {
        unsafe {
            out_bytes((*ctxt).output, b"NULL\n");
        }
        return 0;
    }
    let out = unsafe { (*ctxt).output };
    let is_html =
        unsafe { (*(*ctxt).doc).type_ == xmlElementType::XML_HTML_DOCUMENT_NODE as c_int };
    let typ = unsafe { (*node).type_ };
    unsafe {
        if is_html {
            if typ == xmlElementType::XML_HTML_DOCUMENT_NODE as c_int {
                shell_html_doc_dump(out, node as *mut _xmlDoc);
            } else {
                shell_html_node_dump_file(out, node);
            }
        } else if typ == xmlElementType::XML_DOCUMENT_NODE as c_int {
            tree::xmlDocDump(out, node as *mut _xmlDoc);
        } else {
            shell_elem_dump(out, (*ctxt).doc, node);
        }
        out_bytes(out, b"\n");
    }
    0
}

/// `int xmlShellLoad(xmlShellCtxtPtr ctxt, char *filename, xmlNodePtr node,
/// xmlNodePtr node2)` — the shell "load" command.
///
/// # SAFETY
///
/// - `ctxt`, `filename`, `_node`, `_node2` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlShellLoad(
    ctxt: *mut _xmlShellCtxt,
    filename: *mut c_char,
    _node: *mut _xmlNode,
    _node2: *mut _xmlNode,
) -> c_int {
    if ctxt.is_null() || filename.is_null() {
        return -1;
    }
    let mut html = 0;
    if !unsafe { (*ctxt).doc }.is_null() {
        html = unsafe {
            ((*(*ctxt).doc).type_ == xmlElementType::XML_HTML_DOCUMENT_NODE as c_int) as c_int
        };
    }

    let doc: *mut _xmlDoc = if html != 0 {
        // Upstream htmlParseFile (the exports_xml2 htmlParseFile is a Phase-1
        // stub; the HTML module provides the real parser).
        unsafe { crate::xml::html::parse_file(filename, ptr::null()) }
    } else {
        unsafe { xmlReadFile(filename, ptr::null(), 0) }
    };

    if !doc.is_null() {
        unsafe {
            if (*ctxt).loaded == 1 {
                tree::free_doc((*ctxt).doc);
            }
            (*ctxt).loaded = 1;
            xmlXPathFreeContext((*ctxt).pctxt);
            if !(*ctxt).filename.is_null() {
                xmlFreeImpl((*ctxt).filename as *mut c_void);
            }
            (*ctxt).doc = doc;
            (*ctxt).node = doc as *mut _xmlNode;
            (*ctxt).pctxt = xmlXPathNewContext(doc);
            (*ctxt).filename = xmlCanonicPath(filename) as *mut c_char;
        }
        0
    } else {
        -1
    }
}

/// `int xmlShellWrite(xmlShellCtxtPtr ctxt, char *filename, xmlNodePtr node,
/// xmlNodePtr node2)` — the shell "write" command: write the subtree under
/// `node` to `filename`.
///
/// # SAFETY
///
/// - `ctxt`, `filename`, `node`, `_node2` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlShellWrite(
    ctxt: *mut _xmlShellCtxt,
    filename: *mut c_char,
    node: *mut _xmlNode,
    _node2: *mut _xmlNode,
) -> c_int {
    if node.is_null() {
        return -1;
    }
    if filename.is_null() || *filename == 0 {
        unsafe {
            shell_generic_error(
                c"Write command requires a filename argument\n".as_ptr() as *const c_char,
                b"",
                b"",
            );
        }
        return -1;
    }
    // Upstream: `if (access(filename, W_OK))` under `#ifdef W_OK`.
    if libc::access(filename, libc::W_OK) != 0 {
        unsafe {
            shell_generic_error(c"Cannot write to ".as_ptr() as *const c_char, b"", b"");
            shell_generic_error(filename, b"", b"\n");
        }
        return -1;
    }
    let typ = unsafe { (*node).type_ };
    unsafe {
        match typ {
            t if t == xmlElementType::XML_DOCUMENT_NODE as c_int => {
                if xmlSaveFile(filename, (*ctxt).doc) < -1 {
                    shell_generic_error(c"Failed to write to ".as_ptr() as *const c_char, b"", b"");
                    shell_generic_error(filename, b"", b"\n");
                    return -1;
                }
            }
            t if t == xmlElementType::XML_HTML_DOCUMENT_NODE as c_int => {
                // Upstream htmlSaveFile: serialize the HTML doc to the file.
                if shell_save_html_doc(filename, (*ctxt).doc) < 0 {
                    shell_generic_error(c"Failed to write to ".as_ptr() as *const c_char, b"", b"");
                    shell_generic_error(filename, b"", b"\n");
                    return -1;
                }
            }
            _ => {
                let f = libc::fopen(filename, c"w".as_ptr() as *const c_char);
                if f.is_null() {
                    shell_generic_error(c"Failed to write to ".as_ptr() as *const c_char, b"", b"");
                    shell_generic_error(filename, b"", b"\n");
                    return -1;
                }
                shell_elem_dump(f as *mut c_void, (*ctxt).doc, node);
                libc::fclose(f);
            }
        }
    }
    0
}

/// Serialize an HTML document to a file (upstream `htmlSaveFile`).
unsafe fn shell_save_html_doc(filename: *const c_char, doc: *mut _xmlDoc) -> c_int {
    if filename.is_null() || doc.is_null() {
        return -1;
    }
    let buf = io::buf_create(-1);
    if buf.is_null() {
        return -1;
    }
    let ret = crate::xml::html::doc_dump(buf, doc);
    if ret < 0 {
        io::buf_free(buf);
        return -1;
    }
    let content = io::buf_content(buf);
    let len = io::buf_length(buf);
    let written = if !content.is_null() && len > 0 {
        let f = libc::fopen(filename, c"w".as_ptr() as *const c_char);
        if f.is_null() {
            io::buf_free(buf);
            return -1;
        }
        let n = libc::fwrite(content as *const c_void, 1, len as usize, f);
        libc::fclose(f);
        n as c_int
    } else {
        0
    };
    io::buf_free(buf);
    written
}

/// `int xmlShellSave(xmlShellCtxtPtr ctxt, char *filename, xmlNodePtr node,
/// xmlNodePtr node2)` — the shell "save" command: write the current document
/// to `filename`, or to its original name when no filename is given.
///
/// # SAFETY
///
/// - `ctxt`, `filename`, `_node`, `_node2` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlShellSave(
    ctxt: *mut _xmlShellCtxt,
    filename: *mut c_char,
    _node: *mut _xmlNode,
    _node2: *mut _xmlNode,
) -> c_int {
    if ctxt.is_null() || unsafe { (*ctxt).doc }.is_null() {
        return -1;
    }
    let mut filename = filename;
    if filename.is_null() || *filename == 0 {
        filename = unsafe { (*ctxt).filename };
    }
    if filename.is_null() {
        return -1;
    }
    // Upstream: `if (access(filename, W_OK))` under `#ifdef W_OK`.
    if libc::access(filename, libc::W_OK) != 0 {
        unsafe {
            shell_generic_error(c"Cannot save to ".as_ptr() as *const c_char, b"", b"");
            shell_generic_error(filename, b"", b"\n");
        }
        return -1;
    }
    let typ = unsafe { (*(*ctxt).doc).type_ };
    unsafe {
        match typ {
            t if t == xmlElementType::XML_DOCUMENT_NODE as c_int => {
                if xmlSaveFile(filename, (*ctxt).doc) < 0 {
                    shell_generic_error(c"Failed to save to ".as_ptr() as *const c_char, b"", b"");
                    shell_generic_error(filename, b"", b"\n");
                }
            }
            t if t == xmlElementType::XML_HTML_DOCUMENT_NODE as c_int => {
                if shell_save_html_doc(filename, (*ctxt).doc) < 0 {
                    shell_generic_error(c"Failed to save to ".as_ptr() as *const c_char, b"", b"");
                    shell_generic_error(filename, b"", b"\n");
                }
            }
            _ => {
                shell_generic_error(
                    c"To save to subparts of a document use the 'write' command\n".as_ptr()
                        as *const c_char,
                    b"",
                    b"",
                );
                return -1;
            }
        }
    }
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlShellValidate
// ═══════════════════════════════════════════════════════════════════════════════

/// Validity callback matching upstream's use of `xmlGenericError` as the
/// valid-ctxt error/warning handler (default generic channel: stderr).
unsafe extern "C" fn shell_valid_error(_ctx: *mut c_void, msg: *const c_char) {
    unsafe {
        if !msg.is_null() {
            out_cstr(stderr, msg);
        }
    }
}

/// Minimal external-DTD loader (upstream `xmlParseDTD(NULL, dtd)`).
///
/// Scans the file for a `<!DOCTYPE name (PUBLIC|SYSTEM) "..." ...>` header
/// and builds a `_xmlDtd` node from it. Declarations are not parsed, so
/// validation against an external DTD is permissive (empty declaration
/// tables). Returns NULL when the file cannot be read/scanned, in which
/// case the caller behaves exactly like upstream's failed `xmlParseDTD`.
unsafe fn shell_parse_dtd(dtd: *const c_char) -> *mut _xmlDtd {
    if dtd.is_null() {
        return ptr::null_mut();
    }
    let path = match unsafe { core::ffi::CStr::from_ptr(dtd) }.to_str() {
        Ok(p) => p,
        Err(_) => return ptr::null_mut(),
    };
    let content = match std::fs::read(path) {
        Ok(c) => c,
        Err(_) => return ptr::null_mut(),
    };
    // Locate the `<!DOCTYPE` keyword (case-insensitive per XML).
    let lower: Vec<u8> = content.iter().map(|b| b.to_ascii_lowercase()).collect();
    let pos = match find_subslice(&lower, b"<!doctype") {
        Some(p) => p,
        // No DOCTYPE header (plain external-subset .dtd files): use the
        // file name as the DTD name so validation can still proceed.
        None => {
            let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
            let name_c = bytes_to_xmlstr(base.as_bytes());
            let dtd_node =
                crate::xml::dtd::new_dtd(ptr::null_mut(), name_c, ptr::null_mut(), ptr::null_mut());
            if !name_c.is_null() {
                xmlFreeImpl(name_c as *mut c_void);
            }
            return dtd_node;
        }
    };
    let mut i = pos + b"<!doctype".len();
    // Skip whitespace.
    while i < content.len() && (content[i] as char).is_ascii_whitespace() {
        i += 1;
    }
    // DTD name (up to the next whitespace).
    let name_start = i;
    while i < content.len()
        && !(content[i] as char).is_ascii_whitespace()
        && content[i] != b'>'
        && content[i] != b'['
    {
        i += 1;
    }
    if i == name_start {
        return ptr::null_mut();
    }
    let name = &content[name_start..i];

    // Optional PUBLIC/SYSTEM identifiers.
    while i < content.len() && (content[i] as char).is_ascii_whitespace() {
        i += 1;
    }
    let mut public_id: Option<&[u8]> = None;
    let mut system_id: Option<&[u8]> = None;
    if i < content.len() && content[i..].to_ascii_lowercase().starts_with(b"public") {
        i += b"public".len();
        while i < content.len() && (content[i] as char).is_ascii_whitespace() {
            i += 1;
        }
        if i < content.len() && content[i] == b'"' {
            let s = i + 1;
            let e = content[s..].iter().position(|&c| c == b'"').map(|p| s + p);
            if let Some(e) = e {
                public_id = Some(&content[s..e]);
            }
        }
        while i < content.len() && (content[i] as char).is_ascii_whitespace() {
            i += 1;
        }
        if i < content.len() && content[i] == b'"' {
            let s = i + 1;
            let e = content[s..].iter().position(|&c| c == b'"').map(|p| s + p);
            if let Some(e) = e {
                system_id = Some(&content[s..e]);
            }
        }
    } else if i < content.len() && content[i..].to_ascii_lowercase().starts_with(b"system") {
        i += b"system".len();
        while i < content.len() && (content[i] as char).is_ascii_whitespace() {
            i += 1;
        }
        if i < content.len() && content[i] == b'"' {
            let s = i + 1;
            let e = content[s..].iter().position(|&c| c == b'"').map(|p| s + p);
            if let Some(e) = e {
                system_id = Some(&content[s..e]);
            }
        }
    }

    let name_c = bytes_to_xmlstr(name);
    let ext_c = match public_id {
        Some(v) => bytes_to_xmlstr(v),
        None => ptr::null_mut(),
    };
    let sys_c = match system_id {
        Some(v) => bytes_to_xmlstr(v),
        None => ptr::null_mut(),
    };
    let dtd_node = crate::xml::dtd::new_dtd(ptr::null_mut(), name_c, ext_c, sys_c);
    if !name_c.is_null() {
        xmlFreeImpl(name_c as *mut c_void);
    }
    if !ext_c.is_null() {
        xmlFreeImpl(ext_c as *mut c_void);
    }
    if !sys_c.is_null() {
        xmlFreeImpl(sys_c as *mut c_void);
    }
    dtd_node
}

/// Find `needle` in `haystack` (byte-wise).
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Copy a byte slice into a NUL-terminated xmlMalloc'd xmlChar string.
unsafe fn bytes_to_xmlstr(bytes: &[u8]) -> *mut xmlChar {
    let buf = xmlMallocImpl(bytes.len() + 1) as *mut xmlChar;
    if buf.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
        *buf.add(bytes.len()) = 0;
    }
    buf
}

/// `int xmlShellValidate(xmlShellCtxtPtr ctxt, char *dtd, xmlNodePtr node,
/// xmlNodePtr node2)` — the shell "validate" command.
///
/// # SAFETY
///
/// - `ctxt`, `dtd`, `_node`, `_node2` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlShellValidate(
    ctxt: *mut _xmlShellCtxt,
    dtd: *mut c_char,
    _node: *mut _xmlNode,
    _node2: *mut _xmlNode,
) -> c_int {
    if ctxt.is_null() || unsafe { (*ctxt).doc }.is_null() {
        return -1;
    }
    // Upstream: `xmlValidCtxt vctxt; memset(&vctxt, 0, sizeof(vctxt));`
    let vctxt = xmlMallocZero(size_of::<_xmlValidCtxt>()) as *mut _xmlValidCtxt;
    if vctxt.is_null() {
        return -1;
    }
    unsafe {
        (*vctxt).error = Some(shell_valid_error);
        (*vctxt).warning = Some(shell_valid_error);
    }
    let mut res = -1;
    unsafe {
        if dtd.is_null() || *dtd == 0 {
            res = xmlValidateDocument(vctxt, (*ctxt).doc);
        } else {
            let subset = shell_parse_dtd(dtd as *const c_char);
            if !subset.is_null() {
                res = xmlValidateDtd(vctxt, (*ctxt).doc, subset);
                crate::xml::dtd::free_dtd(subset);
            }
        }
    }
    unsafe {
        xmlFreeImpl(vctxt as *mut c_void);
    }
    res
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlShellDu / xmlShellPwd
// ═══════════════════════════════════════════════════════════════════════════════

/// `int xmlShellDu(xmlShellCtxtPtr ctxt, char *arg, xmlNodePtr tree,
/// xmlNodePtr node2)` — the shell "du" command: show the structure of the
/// subtree under `tree`, deep-first.
///
/// # SAFETY
///
/// - `ctxt`, `_arg`, `tree`, `_node2` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlShellDu(
    ctxt: *mut _xmlShellCtxt,
    _arg: *mut c_char,
    tree: *mut _xmlNode,
    _node2: *mut _xmlNode,
) -> c_int {
    if ctxt.is_null() {
        return -1;
    }
    if tree.is_null() {
        return -1;
    }
    let out = unsafe { (*ctxt).output };
    let mut indent: c_int = 0;
    let mut node: *mut _xmlNode = tree;
    unsafe {
        while !node.is_null() {
            let typ = (*node).type_;
            if typ == xmlElementType::XML_DOCUMENT_NODE as c_int
                || typ == xmlElementType::XML_HTML_DOCUMENT_NODE as c_int
            {
                out_bytes(out, b"/\n");
            } else if typ == xmlElementType::XML_ELEMENT_NODE as c_int {
                let mut line = Vec::new();
                for _ in 0..indent {
                    line.extend_from_slice(b"  ");
                }
                if !(*node).ns.is_null() && !(*(*node).ns).prefix.is_null() {
                    push_cstr(&mut line, (*(*node).ns).prefix as *const c_char);
                    line.push(b':');
                }
                push_cstr(&mut line, (*node).name as *const c_char);
                line.push(b'\n');
                out_bytes(out, &line);
            }

            /*
             * Browse the full subtree, deep first
             */
            if typ == xmlElementType::XML_DOCUMENT_NODE as c_int
                || typ == xmlElementType::XML_HTML_DOCUMENT_NODE as c_int
            {
                node = (*(node as *mut _xmlDoc)).children;
            } else if !(*node).children.is_null()
                && typ != xmlElementType::XML_ENTITY_REF_NODE as c_int
            {
                node = (*node).children;
                indent += 1;
            } else if node != tree && !(*node).next.is_null() {
                node = (*node).next;
            } else if node != tree {
                while node != tree {
                    if !(*node).parent.is_null() {
                        node = (*node).parent;
                        indent -= 1;
                    }
                    if node != tree && !(*node).next.is_null() {
                        node = (*node).next;
                        break;
                    }
                    if (*node).parent.is_null() {
                        node = ptr::null_mut();
                        break;
                    }
                    if node == tree {
                        node = ptr::null_mut();
                        break;
                    }
                }
                if node == tree {
                    node = ptr::null_mut();
                }
            } else {
                node = ptr::null_mut();
            }
        }
    }
    0
}

/// `int xmlShellPwd(xmlShellCtxtPtr ctxt, char *buffer, xmlNodePtr node,
/// xmlNodePtr node2)` — the shell "pwd" command: full path of `node` into
/// `buffer` (which must hold at least 500 chars).
///
/// # SAFETY
///
/// - `_ctxt`, `buffer`, `node`, `_node2` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlShellPwd(
    _ctxt: *mut _xmlShellCtxt,
    buffer: *mut c_char,
    node: *mut _xmlNode,
    _node2: *mut _xmlNode,
) -> c_int {
    if node.is_null() || buffer.is_null() {
        return -1;
    }

    let path = unsafe { shell_get_node_path(node) };
    if path.is_null() {
        return -1;
    }

    // Upstream: `snprintf(buffer, 499, "%s", path); buffer[499] = '0';`
    let plen = unsafe { tree::xml_strlen(path) } as usize;
    let n = plen.min(498);
    unsafe {
        ptr::copy_nonoverlapping(path as *const u8, buffer as *mut u8, n);
        *buffer.add(n) = 0;
        *buffer.add(499) = b'0' as c_char;
    }
    unsafe {
        xmlFreeImpl(path as *mut c_void);
    }
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// Static command helpers used by the shell loop
// ═══════════════════════════════════════════════════════════════════════════════

/// Upstream static `xmlShellSetBase` — the shell "setbase" command.
unsafe fn xmlShellSetBase(ctxt: *mut _xmlShellCtxt, arg: *mut c_char, node: *mut _xmlNode) {
    let _ = ctxt;
    if !node.is_null() {
        unsafe {
            xmlNodeSetBase(node, arg as *const xmlChar);
        }
    }
}

/// Upstream static `xmlShellRegisterNamespace` — the shell "setns" command:
/// register/unregister `prefix=nsuri` pairs on the XPath context.
unsafe fn xmlShellRegisterNamespace(ctxt: *mut _xmlShellCtxt, arg: *mut c_char) -> c_int {
    let ns_list_dup = unsafe { xmlStrdup(arg as *const xmlChar) };
    if ns_list_dup.is_null() {
        return -1;
    }
    let mut next: *mut xmlChar = ns_list_dup;
    loop {
        if unsafe { *next == 0 } {
            break;
        }
        // find prefix
        let prefix = next;
        let eq = unsafe { xmlStrchr(next, b'=' as xmlChar) };
        if eq.is_null() {
            unsafe {
                out_cstr(
                    (*ctxt).output,
                    c"setns: prefix=[nsuri] required\n".as_ptr() as *const c_char,
                );
            }
            unsafe {
                xmlFreeImpl(ns_list_dup as *mut c_void);
            }
            return -1;
        }
        // split at '='
        unsafe {
            *(eq as *mut xmlChar) = 0;
        }
        let href = unsafe { eq.add(1) };
        // find href
        let space = unsafe { xmlStrchr(href, b' ' as xmlChar) };
        if !space.is_null() {
            unsafe {
                *(space as *mut xmlChar) = 0;
            }
            next = unsafe { space.add(1) as *mut xmlChar };
        } else {
            next = unsafe { href.add(tree::xml_strlen(href) as usize) as *mut xmlChar };
        }

        // do register namespace
        if unsafe { xmlXPathRegisterNs((*ctxt).pctxt, prefix, href) } != 0 {
            unsafe {
                let mut msg = Vec::new();
                msg.extend_from_slice(b"Error: unable to register NS with prefix=\"");
                push_cstr(&mut msg, prefix as *const c_char);
                msg.extend_from_slice(b"\" and href=\"");
                push_cstr(&mut msg, href as *const c_char);
                msg.extend_from_slice(b"\"\n");
                out_bytes((*ctxt).output, &msg);
            }
            unsafe {
                xmlFreeImpl(ns_list_dup as *mut c_void);
            }
            return -1;
        }
    }
    unsafe {
        xmlFreeImpl(ns_list_dup as *mut c_void);
    }
    0
}

/// Upstream static `xmlShellRegisterRootNamespaces` — the shell "setrootns"
/// command: register all namespace declarations found on the root element.
unsafe fn xmlShellRegisterRootNamespaces(ctxt: *mut _xmlShellCtxt, root: *mut _xmlNode) -> c_int {
    if root.is_null()
        || unsafe { (*root).type_ != xmlElementType::XML_ELEMENT_NODE as c_int }
        || unsafe { (*root).nsDef.is_null() }
        || ctxt.is_null()
        || unsafe { (*ctxt).pctxt.is_null() }
    {
        return -1;
    }
    let mut ns = unsafe { (*root).nsDef };
    while !ns.is_null() {
        if unsafe { (*ns).prefix.is_null() } {
            unsafe {
                xmlXPathRegisterNs(
                    (*ctxt).pctxt,
                    c"defaultns".as_ptr() as *const xmlChar,
                    (*ns).href,
                );
            }
        } else {
            unsafe {
                xmlXPathRegisterNs((*ctxt).pctxt, (*ns).prefix, (*ns).href);
            }
        }
        ns = unsafe { (*ns).next };
    }
    0
}

/// Upstream static `xmlShellGrep` — the shell "grep" command: search a
/// string in the subtree under `node`, deep first.
unsafe fn xmlShellGrep(ctxt: *mut _xmlShellCtxt, arg: *mut c_char, node: *mut _xmlNode) {
    if ctxt.is_null() || node.is_null() || arg.is_null() {
        return;
    }
    let mut node = node;
    while !node.is_null() {
        unsafe {
            let typ = (*node).type_;
            if typ == xmlElementType::XML_COMMENT_NODE as c_int {
                if !xmlStrstr((*node).content, arg as *const xmlChar).is_null() {
                    let path = shell_get_node_path(node);
                    if !path.is_null() {
                        let mut line = Vec::new();
                        push_cstr(&mut line, path as *const c_char);
                        line.extend_from_slice(b" : ");
                        out_bytes((*ctxt).output, &line);
                        xmlFreeImpl(path as *mut c_void);
                    }
                    xmlShellList(ctxt, ptr::null_mut(), node, ptr::null_mut());
                }
            } else if typ == xmlElementType::XML_TEXT_NODE as c_int
                && !xmlStrstr((*node).content, arg as *const xmlChar).is_null()
            {
                let path = shell_get_node_path((*node).parent);
                if !path.is_null() {
                    let mut line = Vec::new();
                    push_cstr(&mut line, path as *const c_char);
                    line.extend_from_slice(b" : ");
                    out_bytes((*ctxt).output, &line);
                    xmlFreeImpl(path as *mut c_void);
                }
                xmlShellList(ctxt, ptr::null_mut(), (*node).parent, ptr::null_mut());
            }

            /*
             * Browse the full subtree, deep first
             */
            if typ == xmlElementType::XML_DOCUMENT_NODE as c_int
                || typ == xmlElementType::XML_HTML_DOCUMENT_NODE as c_int
            {
                node = (*(node as *mut _xmlDoc)).children;
            } else if !(*node).children.is_null()
                && typ != xmlElementType::XML_ENTITY_REF_NODE as c_int
            {
                node = (*node).children;
            } else if !(*node).next.is_null() {
                node = (*node).next;
            } else {
                while !node.is_null() {
                    if !(*node).parent.is_null() {
                        node = (*node).parent;
                    }
                    if !(*node).next.is_null() {
                        node = (*node).next;
                        break;
                    }
                    if (*node).parent.is_null() {
                        node = ptr::null_mut();
                        break;
                    }
                }
            }
        }
    }
}

/// Emit the per-type error message for a non-node-set XPath result
/// (upstream's repeated `switch (list->type)` blocks in the shell loop).
unsafe fn shell_result_type_error(arg: *const c_char, typ: c_int) {
    if typ == xmlXPathObjectType::XPATH_UNDEFINED as c_int {
        unsafe { shell_generic_error(arg, b": ", b"no such node\n") };
    } else if typ == xmlXPathObjectType::XPATH_BOOLEAN as c_int {
        unsafe { shell_generic_error(arg, b" is a Boolean", b"\n") };
    } else if typ == xmlXPathObjectType::XPATH_NUMBER as c_int {
        unsafe { shell_generic_error(arg, b" is a number", b"\n") };
    } else if typ == xmlXPathObjectType::XPATH_STRING as c_int {
        unsafe { shell_generic_error(arg, b" is a string", b"\n") };
    } else if typ == xmlXPathObjectType::XPATH_POINT as c_int {
        unsafe { shell_generic_error(arg, b" is a point", b"\n") };
    } else if typ == xmlXPathObjectType::XPATH_RANGE as c_int
        || typ == xmlXPathObjectType::XPATH_LOCATIONSET as c_int
    {
        unsafe { shell_generic_error(arg, b" is a range", b"\n") };
    } else if typ == xmlXPathObjectType::XPATH_USERS as c_int {
        unsafe { shell_generic_error(arg, b" is user-defined", b"\n") };
    } else if typ == xmlXPathObjectType::XPATH_XSLT_TREE as c_int {
        unsafe { shell_generic_error(arg, b" is an XSLT value tree", b"\n") };
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlShell — the interactive loop
// ═══════════════════════════════════════════════════════════════════════════════

/// Build the prompt string for the current node ("/ > ", "name > ",
/// "prefix:name > " or "? > "), NUL-terminated.
unsafe fn shell_build_prompt(ctxt: *mut _xmlShellCtxt) -> Vec<u8> {
    let mut p = Vec::new();
    let node = unsafe { (*ctxt).node };
    let doc = unsafe { (*ctxt).doc };
    if node == doc as *mut _xmlNode {
        p.extend_from_slice(b"/ > ");
    } else if !node.is_null() && !unsafe { (*node).name }.is_null() {
        unsafe {
            if !(*node).ns.is_null() && !(*(*node).ns).prefix.is_null() {
                push_cstr(&mut p, (*(*node).ns).prefix as *const c_char);
                p.push(b':');
            }
            push_cstr(&mut p, (*node).name as *const c_char);
        }
        p.extend_from_slice(b" > ");
    } else {
        p.extend_from_slice(b"? > ");
    }
    p.push(0);
    p
}

/// The shell's "help" output (upstream 2.12.6), written to `ctxt->output`.
unsafe fn shell_print_help(ctxt: *mut _xmlShellCtxt) {
    let out = unsafe { (*ctxt).output };
    const HELP: &[&[u8]] = &[
        b"\tbase         display XML base of the node\n",
        b"\tsetbase URI  change the XML base of the node\n",
        b"\tbye          leave shell\n",
        b"\tcat [node]   display node or current node\n",
        b"\tcd [path]    change directory to path or to root\n",
        b"\tdir [path]   dumps information about the node (namespace, attributes, content)\n",
        b"\tdu [path]    show the structure of the subtree under path or the current node\n",
        b"\texit         leave shell\n",
        b"\thelp         display this help\n",
        b"\tfree         display memory usage\n",
        b"\tload [name]  load a new document with name\n",
        b"\tls [path]    list contents of path or the current directory\n",
        b"\txpath expr   evaluate the XPath expression in that context and print the result\n",
        b"\tsetns nsreg  register a namespace to a prefix in the XPath evaluation context\n",
        b"\t             format for nsreg is: prefix=[nsuri] (i.e. prefix= unsets a prefix)\n",
        b"\tsetrootns    register all namespace found on the root element\n",
        b"\t             the default namespace if any uses 'defaultns' prefix\n",
        b"\tpwd          display current working directory\n",
        b"\twhereis      display absolute path of [path] or current working directory\n",
        b"\tquit         leave shell\n",
        b"\tsave [name]  save this document to name or the original name\n",
        b"\twrite [name] write the current node to the filename\n",
        b"\tvalidate     check the document for errors\n",
        b"\tgrep string  search for a string in the subtree\n",
    ];
    for line in HELP {
        unsafe {
            out_bytes(out, line);
        }
    }
}

/// `void xmlShell(xmlDocPtr doc, char *filename, xmlShellReadlineFunc input,
/// FILE *output)` — the XML shell: an interactive loop allowing to load,
/// validate, view, modify and save a document.
///
/// # SAFETY
///
/// - `doc`, `filename`, `output` must be valid pointers (or NULL
///   where the upstream C contract allows), obtained from the
///   matching constructor/owner and not yet freed; the callee may
///   take or keep ownership exactly as the C API specifies.
///
/// The caller must not race this call with concurrent mutation of the
/// same objects from other threads (per-object state is not internally
/// synchronized). Violating any of the above is undefined behavior.
///
/// Exercised by the C-API differential courts
/// (courts/suites/data-abi/*-family-probe.c) and the CLI differential
/// courts; those pass byte-for-byte against the upstream oracle.
#[no_mangle]
pub unsafe extern "C" fn xmlShell(
    doc: *mut _xmlDoc,
    filename: *mut c_char,
    input: xmlShellReadlineFunc,
    output: *mut c_void,
) {
    if doc.is_null() || filename.is_null() || input.is_none() {
        return;
    }
    let output = if output.is_null() {
        unsafe { stdout }
    } else {
        output
    };

    let ctxt = xmlMallocZero(size_of::<_xmlShellCtxt>()) as *mut _xmlShellCtxt;
    if ctxt.is_null() {
        return;
    }
    unsafe {
        (*ctxt).loaded = 0;
        (*ctxt).doc = doc;
        (*ctxt).input = input;
        (*ctxt).output = output;
        (*ctxt).filename = xmlStrdup(filename as *const xmlChar) as *mut c_char;
        (*ctxt).node = doc as *mut _xmlNode;
        (*ctxt).pctxt = xmlXPathNewContext(doc);
    }
    if unsafe { (*ctxt).pctxt }.is_null() {
        unsafe {
            xmlFreeImpl(ctxt as *mut c_void);
        }
        return;
    }

    let mut cmdline: *mut c_char = ptr::null_mut();
    loop {
        // Prompt.
        let prompt = unsafe { shell_build_prompt(ctxt) };
        let readline = unsafe { (*ctxt).input };
        cmdline = match readline {
            Some(f) => f(prompt.as_ptr() as *mut c_char),
            None => break,
        };
        if cmdline.is_null() {
            break;
        }

        // Parse the command itself (skip leading spaces/tabs).
        let clen = unsafe { libc::strlen(cmdline) } as usize;
        let cbytes = unsafe { core::slice::from_raw_parts(cmdline as *const u8, clen) };
        let mut i = 0usize;
        while i < clen && (cbytes[i] == b' ' || cbytes[i] == b'\t') {
            i += 1;
        }
        let mut command: Vec<u8> = Vec::new();
        while i < clen
            && cbytes[i] != b' '
            && cbytes[i] != b'\t'
            && cbytes[i] != b'\n'
            && cbytes[i] != b'\r'
        {
            command.push(cbytes[i]);
            i += 1;
        }
        if command.is_empty() {
            unsafe {
                libc::free(cmdline as *mut c_void);
            }
            cmdline = ptr::null_mut();
            continue;
        }

        // Parse the argument (rest of the line).
        while i < clen && (cbytes[i] == b' ' || cbytes[i] == b'\t') {
            i += 1;
        }
        let mut arg: Vec<u8> = Vec::new();
        while i < clen && cbytes[i] != b'\n' && cbytes[i] != b'\r' {
            arg.push(cbytes[i]);
            i += 1;
        }

        // NUL-terminated views for the C API.
        command.push(0);
        let cmd: &[u8] = &command;
        let mut argn = arg.clone();
        argn.push(0);
        let arg_cstr: *mut c_char = argn.as_mut_ptr() as *mut c_char;
        let arg_xml: *const xmlChar = argn.as_ptr() as *const xmlChar;

        // start interpreting the command
        if cmd == b"exit\0" || cmd == b"quit\0" || cmd == b"bye\0" {
            break;
        }
        if cmd == b"help\0" {
            unsafe { shell_print_help(ctxt) };
        } else if cmd == b"validate\0" {
            unsafe {
                xmlShellValidate(ctxt, arg_cstr, ptr::null_mut(), ptr::null_mut());
            }
        } else if cmd == b"load\0" {
            unsafe {
                xmlShellLoad(ctxt, arg_cstr, ptr::null_mut(), ptr::null_mut());
            }
        } else if cmd == b"save\0" {
            unsafe {
                xmlShellSave(ctxt, arg_cstr, ptr::null_mut(), ptr::null_mut());
            }
        } else if cmd == b"write\0" {
            if arg.is_empty() {
                unsafe {
                    shell_generic_error(
                        c"Write command requires a filename argument\n".as_ptr() as *const c_char,
                        b"",
                        b"",
                    );
                }
            } else {
                unsafe {
                    xmlShellWrite(ctxt, arg_cstr, (*ctxt).node, ptr::null_mut());
                }
            }
        } else if cmd == b"grep\0" {
            unsafe {
                xmlShellGrep(ctxt, arg_cstr, (*ctxt).node);
            }
        } else if cmd == b"free\0" {
            unsafe {
                if arg.is_empty() {
                    crate::abi::allocator::xmlMemShow((*ctxt).output, 0);
                } else {
                    let mut len: c_int = 0;
                    let arg_s = core::str::from_utf8(&argn[..argn.len() - 1]).unwrap_or("");
                    if let Ok(v) = arg_s.trim().parse::<c_int>() {
                        len = v;
                    }
                    crate::abi::allocator::xmlMemShow((*ctxt).output, len);
                }
            }
        } else if cmd == b"pwd\0" {
            let mut dir = [0 as c_char; 500];
            unsafe {
                if xmlShellPwd(ctxt, dir.as_mut_ptr(), (*ctxt).node, ptr::null_mut()) == 0 {
                    let mut line = Vec::new();
                    push_cstr(&mut line, dir.as_mut_ptr());
                    line.extend_from_slice(b"\n");
                    out_bytes((*ctxt).output, &line);
                }
            }
        } else if cmd == b"du\0" {
            unsafe {
                if arg.is_empty() {
                    xmlShellDu(ctxt, ptr::null_mut(), (*ctxt).node, ptr::null_mut());
                } else {
                    (*(*ctxt).pctxt).node = (*ctxt).node;
                    let list = xmlXPathEval(arg_xml, (*ctxt).pctxt);
                    if !list.is_null() {
                        let typ = (*list).type_;
                        if typ == xmlXPathObjectType::XPATH_NODESET as c_int {
                            let ns = (*list).nodesetval as *mut _xmlNodeSet;
                            if !ns.is_null() {
                                for indx in 0..(*ns).nodeNr {
                                    let n = *(*ns).nodeTab.add(indx as usize);
                                    xmlShellDu(ctxt, ptr::null_mut(), n, ptr::null_mut());
                                }
                            }
                        } else {
                            shell_result_type_error(arg_cstr, typ);
                        }
                        xmlXPathFreeObject(list);
                    } else {
                        shell_generic_error(arg_cstr, b": ", b"no such node\n");
                    }
                    (*(*ctxt).pctxt).node = ptr::null_mut();
                }
            }
        } else if cmd == b"base\0" {
            unsafe {
                xmlShellBase(ctxt, ptr::null_mut(), (*ctxt).node, ptr::null_mut());
            }
        } else if cmd == b"setns\0" {
            unsafe {
                if arg.is_empty() {
                    shell_generic_error(
                        c"setns: prefix=[nsuri] required\n".as_ptr() as *const c_char,
                        b"",
                        b"",
                    );
                } else {
                    xmlShellRegisterNamespace(ctxt, arg_cstr);
                }
            }
        } else if cmd == b"setrootns\0" {
            unsafe {
                let root = tree::doc_get_root_element((*ctxt).doc);
                xmlShellRegisterRootNamespaces(ctxt, root);
            }
        } else if cmd == b"xpath\0" {
            unsafe {
                if arg.is_empty() {
                    shell_generic_error(
                        c"xpath: expression required\n".as_ptr() as *const c_char,
                        b"",
                        b"",
                    );
                } else {
                    (*(*ctxt).pctxt).node = (*ctxt).node;
                    let list = xmlXPathEval(arg_xml, (*ctxt).pctxt);
                    xmlXPathDebugDumpObject((*ctxt).output, list, 0);
                    xmlXPathFreeObject(list);
                }
            }
        } else if cmd == b"setbase\0" {
            unsafe {
                xmlShellSetBase(ctxt, arg_cstr, (*ctxt).node);
            }
        } else if cmd == b"ls\0" || cmd == b"dir\0" {
            let is_dir = cmd == b"dir\0";
            unsafe {
                if arg.is_empty() {
                    if is_dir {
                        xmlShellDir(ctxt, ptr::null_mut(), (*ctxt).node, ptr::null_mut());
                    } else {
                        xmlShellList(ctxt, ptr::null_mut(), (*ctxt).node, ptr::null_mut());
                    }
                } else {
                    (*(*ctxt).pctxt).node = (*ctxt).node;
                    let list = xmlXPathEval(arg_xml, (*ctxt).pctxt);
                    if !list.is_null() {
                        let typ = (*list).type_;
                        if typ == xmlXPathObjectType::XPATH_NODESET as c_int {
                            let ns = (*list).nodesetval as *mut _xmlNodeSet;
                            if !ns.is_null() {
                                for indx in 0..(*ns).nodeNr {
                                    let n = *(*ns).nodeTab.add(indx as usize);
                                    if is_dir {
                                        xmlShellDir(ctxt, ptr::null_mut(), n, ptr::null_mut());
                                    } else {
                                        xmlShellList(ctxt, ptr::null_mut(), n, ptr::null_mut());
                                    }
                                }
                            }
                        } else {
                            shell_result_type_error(arg_cstr, typ);
                        }
                        xmlXPathFreeObject(list);
                    } else {
                        shell_generic_error(arg_cstr, b": ", b"no such node\n");
                    }
                    (*(*ctxt).pctxt).node = ptr::null_mut();
                }
            }
        } else if cmd == b"whereis\0" {
            let mut dir = [0 as c_char; 500];
            unsafe {
                if arg.is_empty() {
                    if xmlShellPwd(ctxt, dir.as_mut_ptr(), (*ctxt).node, ptr::null_mut()) == 0 {
                        let mut line = Vec::new();
                        push_cstr(&mut line, dir.as_mut_ptr());
                        line.extend_from_slice(b"\n");
                        out_bytes((*ctxt).output, &line);
                    }
                } else {
                    (*(*ctxt).pctxt).node = (*ctxt).node;
                    let list = xmlXPathEval(arg_xml, (*ctxt).pctxt);
                    if !list.is_null() {
                        let typ = (*list).type_;
                        if typ == xmlXPathObjectType::XPATH_NODESET as c_int {
                            let ns = (*list).nodesetval as *mut _xmlNodeSet;
                            if !ns.is_null() {
                                for indx in 0..(*ns).nodeNr {
                                    let n = *(*ns).nodeTab.add(indx as usize);
                                    if xmlShellPwd(ctxt, dir.as_mut_ptr(), n, ptr::null_mut()) == 0
                                    {
                                        let mut line = Vec::new();
                                        push_cstr(&mut line, dir.as_mut_ptr());
                                        line.extend_from_slice(b"\n");
                                        out_bytes((*ctxt).output, &line);
                                    }
                                }
                            }
                        } else {
                            shell_result_type_error(arg_cstr, typ);
                        }
                        xmlXPathFreeObject(list);
                    } else {
                        shell_generic_error(arg_cstr, b": ", b"no such node\n");
                    }
                    (*(*ctxt).pctxt).node = ptr::null_mut();
                }
            }
        } else if cmd == b"cd\0" {
            unsafe {
                if arg.is_empty() {
                    (*ctxt).node = (*ctxt).doc as *mut _xmlNode;
                } else {
                    // Upstream strips a trailing '/' from the argument.
                    let mut argn = argn;
                    let l = argn.len();
                    if l >= 3 && argn[l - 2] == b'/' {
                        argn[l - 2] = 0;
                    }
                    (*(*ctxt).pctxt).node = (*ctxt).node;
                    let list = xmlXPathEval(argn.as_ptr() as *const xmlChar, (*ctxt).pctxt);
                    if !list.is_null() {
                        let typ = (*list).type_;
                        if typ == xmlXPathObjectType::XPATH_NODESET as c_int {
                            let ns = (*list).nodesetval as *mut _xmlNodeSet;
                            if !ns.is_null() {
                                if (*ns).nodeNr == 1 {
                                    (*ctxt).node = *(*ns).nodeTab;
                                    if !(*ctxt).node.is_null()
                                        && (*(*ctxt).node).type_
                                            == xmlElementType::XML_NAMESPACE_DECL as c_int
                                    {
                                        shell_generic_error(
                                            c"cannot cd to namespace\n".as_ptr() as *const c_char,
                                            b"",
                                            b"",
                                        );
                                        (*ctxt).node = ptr::null_mut();
                                    }
                                } else {
                                    let mut msg = Vec::new();
                                    push_cstr(&mut msg, arg_cstr);
                                    msg.extend_from_slice(b" is a ");
                                    msg.extend_from_slice((*ns).nodeNr.to_string().as_bytes());
                                    msg.extend_from_slice(b" Node Set\n");
                                    out_bytes(stderr, &msg);
                                }
                            } else {
                                let mut msg = Vec::new();
                                push_cstr(&mut msg, arg_cstr);
                                msg.extend_from_slice(b" is an empty Node Set\n");
                                out_bytes(stderr, &msg);
                            }
                        } else {
                            shell_result_type_error(arg_cstr, typ);
                        }
                        xmlXPathFreeObject(list);
                    } else {
                        shell_generic_error(arg_cstr, b": ", b"no such node\n");
                    }
                    (*(*ctxt).pctxt).node = ptr::null_mut();
                }
            }
        } else if cmd == b"cat\0" {
            unsafe {
                if arg.is_empty() {
                    xmlShellCat(ctxt, ptr::null_mut(), (*ctxt).node, ptr::null_mut());
                } else {
                    // UPSTREAM-PARITY: the 2.12.x loop reuses the outer
                    // `i` (the argument length) in `if (i > 0)`, which is
                    // always true here, so the separator is emitted before
                    // every node of the node-set.
                    (*(*ctxt).pctxt).node = (*ctxt).node;
                    let list = xmlXPathEval(arg_xml, (*ctxt).pctxt);
                    if !list.is_null() {
                        let typ = (*list).type_;
                        if typ == xmlXPathObjectType::XPATH_NODESET as c_int {
                            let ns = (*list).nodesetval as *mut _xmlNodeSet;
                            if !ns.is_null() {
                                for indx in 0..(*ns).nodeNr {
                                    if i > 0 {
                                        out_bytes((*ctxt).output, b" -------\n");
                                    }
                                    let n = *(*ns).nodeTab.add(indx as usize);
                                    xmlShellCat(ctxt, ptr::null_mut(), n, ptr::null_mut());
                                }
                            }
                        } else {
                            shell_result_type_error(arg_cstr, typ);
                        }
                        xmlXPathFreeObject(list);
                    } else {
                        shell_generic_error(arg_cstr, b": ", b"no such node\n");
                    }
                    (*(*ctxt).pctxt).node = ptr::null_mut();
                }
            }
        } else {
            let mut msg = Vec::new();
            msg.extend_from_slice(b"Unknown command ");
            msg.extend_from_slice(&command[..command.len() - 1]);
            msg.extend_from_slice(b"\n");
            unsafe {
                out_bytes(stderr, &msg);
            }
        }

        unsafe {
            libc::free(cmdline as *mut c_void);
        }
        cmdline = ptr::null_mut();
    }

    // Cleanup (upstream xmlShell epilogue).
    unsafe {
        xmlXPathFreeContext((*ctxt).pctxt);
        if (*ctxt).loaded != 0 {
            tree::free_doc((*ctxt).doc);
        }
        if !(*ctxt).filename.is_null() {
            xmlFreeImpl((*ctxt).filename as *mut c_void);
        }
        xmlFreeImpl(ctxt as *mut c_void);
        if !cmdline.is_null() {
            libc::free(cmdline as *mut c_void);
        }
    }
}
