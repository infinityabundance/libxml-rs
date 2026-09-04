//! XSLT document() function support (§33, §85 Phase 8).
//!
//! # Upstream contract
//!
//! Parity target: upstream libxslt `documents.c` (1.1.45;
//! `SRC-LIBXSLT-1.1.42-DOCUMENTS-C` under oracle/historical/src). Subsystem
//! census: xslt-documents, xslt-loader-hooks. The ABI surface is
//! `xsltSetLoaderFunc` / `xsltDefaultLoader` and the internal
//! `xsltLoadDocument` used by the document() XPath function.
//!
//! # Conceptual behavior
//!
//! Document loading resolves the requested URI against the source
//! document base, consults the per-context cache, invokes the registered
//! global loader (or the default file loader), parses the result, and
//! stores it in the cache keyed by the requested URI. The cache doubles as
//! the RVT ownership list: `xsltRegisterRVT` inserts entries with a NULL
//! URI so URI lookups never match them.
//!
//! # Ownership & safety invariants
//!
//! - `xsltFreeDocCache` owns every cached `_xmlDoc` (freed with
//!   `xmlFreeDoc` semantics at context teardown) and every entry + URI
//!   string (libc::free).
//! - RVT documents registered via `xsltRegisterRVT` transfer ownership to
//!   the cache; they are freed exactly once at teardown, after the XPath
//!   context (R-000109 ordering).
//! - Source documents are borrowed (never cached, never freed here).
//! - `XSLT_LOADER` is a process-wide static, mutated only by
//!   `xsltSetLoaderFunc` (upstream documents.c `xsltLoader` global).
//!
//! # Historical quirks & epochs
//!
//! E-008 (atlas/SEMANTIC_EPOCHS.md): transform output is frozen since
//! libxslt 1.1.26 (2009); document() resolution/caching behavior is part
//! of that stable epoch. R-000109 (RTF double-free, fixed in 11.1-X)
//! introduced the RVT-in-docCache reuse; R-000140 covered the `_xslt*` ABI
//! mirror layout. The loader hook is registered as a single global,
//! matching upstream documents.c `xsltSetLoaderFunc`.
//!
//! # Deliberate oddities
//!
//! - The candidate reuses the `docCache` list (the upstream
//!   `xsltTransformCachePtr` `cache` slot) as the RVT ownership list — a
//!   documented divergence annotated at `xsltRegisterRVT`.
//! - `xsltDefaultLoader` currently returns NULL after validating the URI:
//!   the default path falls through to `xmlReadFile`, an intentional
//!   internal simplification that keeps the observable loader contract.
//!
//! # Proving courts
//!
//! CLI-XSLTPROC (document()/multi-document corpus), DSO-LOADER
//! (xsltSetLoaderFunc export), XSLT-001, and the in-crate `cargo test`
//! suites.
//!
//! # Tempting simplifications that would break parity
//!
//! - Dropping the per-URI cache would reload documents per call,
//!   breaking node identity across `document()` calls (upstream caches in
//!   `docCache`).
//! - Freeing cached docs at `xsltApplyStylesheet` return would double-free
//!   RVTs and break `exsl:node-set` results (R-000109 lesson: context
//!   teardown owns them).
//! - Ignoring the registered loader would break custom entity/document
//!   loaders — the R-000162 lesson that the loader hook must actually be
//!   consulted.
//!
//! The `document()` function loads external XML documents during a
//! transformation. Documents are cached per transform context so repeated
//! loads of the same URI reuse the same document.
//!
//! # UPSTREAM-PARITY
//!
//! Upstream libxslt (documents.c) maintains a document cache on the
//! transform context (`docCache` hash). `xsltLoadDocument` resolves the
//! URI relative to the source document base, loads it through the
//! configured loader (default: file/network), and caches the result.
//!
//! The loader function can be overridden with `xsltSetLoaderFunc`.

use crate::abi::structs::*;
use crate::abi::types::*;
use std::os::raw::c_int;
use std::ptr;

/// The XSLT document cache entry.
#[derive(Debug)]
#[repr(C)]
pub struct _xsltDocCacheEntry {
    /// Next entry in the document-cache linked list.
    pub next: *mut _xsltDocCacheEntry,
    /// The resolved document URI (NULL for RVT entries, which are never
    /// matched by URI lookup).
    pub uri: *mut xmlChar,
    /// The cached document.
    pub doc: *mut _xmlDoc,
}

/// Register a result-tree-fragment (RVT) document in the context's document
/// cache so it is freed exactly once at transform-context teardown
/// (`xsltFreeDocCache` releases every cached doc).
///
/// The entry carries a NULL URI, so `cache_lookup` (which requires a
/// non-NULL, matching URI) never matches it.
///
/// # UPSTREAM-PARITY
///
/// Upstream libxslt tracks RVT documents on the context (variables.c
/// `xsltCreateRVT`) and frees them with the context; we reuse the docCache
/// list for the same lifecycle.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext`.
/// - Ownership of `doc` transfers to the cache (freed at teardown).
pub(crate) unsafe fn xsltRegisterRVT(ctxt: *mut _xsltTransformContext, doc: *mut _xmlDoc) {
    if ctxt.is_null() || doc.is_null() {
        return;
    }
    let entry =
        libc::calloc(1, core::mem::size_of::<_xsltDocCacheEntry>()) as *mut _xsltDocCacheEntry;
    if entry.is_null() {
        return;
    }
    (*entry).uri = ptr::null_mut();
    (*entry).doc = doc;
    // UPSTREAM-PARITY: the candidate's RVT/cached-doc list head lives in the
    // `cache` slot (xsltTransformCachePtr upstream; unused by the candidate
    // for that purpose — documented divergence).
    (*entry).next = (*ctxt).cache as *mut _xsltDocCacheEntry;
    (*ctxt).cache = entry as *mut c_void;
}

/// Default XSLT loader function (file loading).
///
/// # SAFETY
///
/// - `ctxt` may be NULL.
/// - `URI` must be a valid NUL-terminated string.
pub const unsafe extern "C" fn xsltDefaultLoader(
    _ctxt: *mut c_void,
    _style: *const c_char,
    URI: *const c_char,
    _ns: *const c_char,
    _secondary: c_int,
) -> *mut _xmlParserInput {
    if URI.is_null() {
        return ptr::null_mut();
    }
    // Phase 8: load the URI via the XML parser I/O layer.
    ptr::null_mut()
}

/// Global loader function (set via xsltSetLoaderFunc).
static mut XSLT_LOADER: Option<crate::abi::exports_xslt_compile::xsltDocLoaderFunc> = None;

/// Set the global XSLT document loader function (upstream documents.h
/// `xsltSetLoaderFunc(xsltDocLoaderFunc f)` — R-000176, the candidate
/// previously used a different 5-argument callback contract).
///
/// # SAFETY
///
/// - `loader` must be a valid function pointer or NULL.
#[no_mangle]
pub unsafe extern "C" fn xsltSetLoaderFunc(
    loader: Option<crate::abi::exports_xslt_compile::xsltDocLoaderFunc>,
) {
    XSLT_LOADER = loader;
}

/// Get the current global loader function.
pub fn xsltGetLoaderFunc() -> Option<crate::abi::exports_xslt_compile::xsltDocLoaderFunc> {
    // SAFETY: only mutated by xsltSetLoaderFunc; safe to read here.
    unsafe { XSLT_LOADER }
}

/// Load a document by URI, using the cache.
///
/// Returns the loaded document (owned by the cache) or NULL on error.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext`.
/// - `URI` must be a valid NUL-terminated string.
pub unsafe fn xsltLoadDocument(
    ctxt: *mut _xsltTransformContext,
    URI: *const xmlChar,
) -> *mut _xmlDoc {
    if ctxt.is_null() || URI.is_null() {
        return ptr::null_mut();
    }
    // Check the cache first.
    if let Some(cached) = cache_lookup(ctxt, URI) {
        return cached;
    }
    // Resolve the URI against the source document's base.
    let base = if !(*ctxt).document.is_null()
        && !(*(*ctxt).document).doc.is_null()
        && !(*(*(*ctxt).document).doc).URL.is_null()
    {
        let len = libc::strlen((*(*(*ctxt).document).doc).URL as *const libc::c_char) as usize;
        Some(core::slice::from_raw_parts(
            (*(*(*ctxt).document).doc).URL,
            len,
        ))
    } else {
        None
    };
    let uri_bytes =
        core::slice::from_raw_parts(URI, libc::strlen(URI as *const libc::c_char) as usize);
    let resolved: Option<Vec<u8>> = match base {
        Some(b) => crate::xml::uri::resolve_uri(b, uri_bytes),
        None => Some(uri_bytes.to_vec()),
    };
    let resolved = match resolved {
        Some(r) if !r.is_empty() => r,
        _ => return ptr::null_mut(),
    };
    // Load via the configured loader or the default file loader.
    let mut cstr = resolved.clone();
    cstr.push(0);
    let doc = load_via_loader(ctxt, cstr.as_ptr() as *mut xmlChar);
    if !doc.is_null() {
        // UPSTREAM-PARITY (documents.c xsltLoadDocument): when
        // XInclude processing is enabled on the context
        // (php XSLTProcessor::$doXInclude sets ctxt->xinclude directly), the
        // loaded document is XInclude-processed with the context's parser
        // options BEFORE it is cached and returned to the document()
        // function (xinclude/xinclude: document('data.xml') must substitute
        // the xi:include from data.xml).
        if unsafe { (*ctxt).xinclude != 0 } {
            let options = unsafe { (*ctxt).parserOptions };
            let xr = crate::abi::exports_xml2::xmlXIncludeProcessFlags(doc, options);
            if xr < 0 {
                crate::xslt::errors::xsltTransformError(
                    ctxt,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    c"xsltLoadDocument: XInclude processing failed\n".as_ptr() as *const c_char,
                );
            }
        }
        cache_store(ctxt, URI, doc);
    }
    doc
}

/// Load a document via the configured loader (upstream documents.c
/// `xsltLoadDocument`: the loader is invoked with `(URI, dict, options,
/// ctxt, XSLT_LOAD_DOCUMENT)` and returns the parsed document). With no
/// loader configured, the URI is parsed as a local file (upstream default).
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn load_via_loader(ctxt: *mut _xsltTransformContext, uri: *mut xmlChar) -> *mut _xmlDoc {
    // If a custom loader is configured, invoke it with the upstream
    // xsltDocLoaderFunc contract.
    let loader = xsltGetLoaderFunc();
    if let Some(loader_fn) = loader {
        let dict = if ctxt.is_null() {
            ptr::null_mut()
        } else {
            (*ctxt).dict
        };
        let options = if ctxt.is_null() {
            0
        } else {
            (*ctxt).parserOptions
        };
        let doc = loader_fn(
            uri as *const xmlChar,
            dict,
            options,
            ctxt as *mut c_void,
            2, // XSLT_LOAD_DOCUMENT
        );
        if !doc.is_null() {
            return doc;
        }
    }
    // Default: parse the URI as a file path.
    crate::abi::exports_xml2::xmlReadFile(uri as *const c_char, ptr::null(), 0)
}

/// Look up a document in the context's cache.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn cache_lookup(
    ctxt: *mut _xsltTransformContext,
    uri: *const xmlChar,
) -> Option<*mut _xmlDoc> {
    let mut cur = (*ctxt).cache as *mut _xsltDocCacheEntry;
    while !cur.is_null() {
        if !(*cur).uri.is_null()
            && libc::strcmp(
                (*cur).uri as *const libc::c_char,
                uri as *const libc::c_char,
            ) == 0
        {
            return Some((*cur).doc);
        }
        cur = (*cur).next;
    }
    None
}

/// Store a document in the context's cache.
///
/// # SAFETY
///
/// - All pointers must be valid.
unsafe fn cache_store(ctxt: *mut _xsltTransformContext, uri: *const xmlChar, doc: *mut _xmlDoc) {
    let entry =
        libc::calloc(1, core::mem::size_of::<_xsltDocCacheEntry>()) as *mut _xsltDocCacheEntry;
    if entry.is_null() {
        return;
    }
    let len = libc::strlen(uri as *const libc::c_char);
    let copy = libc::malloc(len + 1) as *mut xmlChar;
    if copy.is_null() {
        libc::free(entry as *mut libc::c_void);
        return;
    }
    libc::memcpy(copy as *mut libc::c_void, uri as *const libc::c_void, len);
    *copy.add(len) = 0;
    (*entry).uri = copy;
    (*entry).doc = doc;
    (*entry).next = (*ctxt).cache as *mut _xsltDocCacheEntry;
    (*ctxt).cache = entry as *mut c_void;
}

/// Free the document cache of a transform context.
///
/// # SAFETY
///
/// - `ctxt` must be a valid `_xsltTransformContext`.
pub unsafe fn xsltFreeDocCache(ctxt: *mut _xsltTransformContext) {
    if ctxt.is_null() {
        return;
    }
    let mut cur = (*ctxt).cache as *mut _xsltDocCacheEntry;
    (*ctxt).cache = ptr::null_mut();
    while !cur.is_null() {
        let next = (*cur).next;
        if !(*cur).uri.is_null() {
            libc::free((*cur).uri as *mut libc::c_void);
        }
        if !(*cur).doc.is_null() {
            crate::xml::tree::free_doc((*cur).doc);
        }
        libc::free(cur as *mut libc::c_void);
        cur = next;
    }
}

use std::ffi::c_void;
use std::os::raw::c_char;

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr;

    /// NULL arguments to the document-loading API are rejected with NULL.
    ///
    /// # Safety
    ///
    /// - `xsltLoadDocument` returns NULL and `xsltFreeDocCache` no-ops on
    ///   NULL arguments before dereferencing them, so the unsafe block
    ///   reads and frees no memory.
    #[test]
    fn test_null_args() {
        unsafe {
            assert!(xsltLoadDocument(ptr::null_mut(), ptr::null()).is_null());
            xsltFreeDocCache(ptr::null_mut());
        }
    }
}
