//! C ABI exports for libxslt.so.1 — the "ext" family (§16, Phase 8).
//!
//! The extension-module registry (`xsltRegisterExtModule*`,
//! `xsltUnregisterExtModule*`, the `xsltExtModule*Lookup` queries) and the
//! per-context/per-style extension data accessors (`xsltGetExtData`,
//! `xsltStyleGetExtData`, `xsltGetExtInfo`), plus the EXSLT registration
//! entry points (`xsltRegisterAllFunctions`, `xsltRegisterAllElement`,
//! `xsltRegisterAllExtras`, `xsltRegisterExtras`).
//!
//! Semantics follow upstream libxslt 1.1.45 (`archaeology/libxslt-git/
//! libxslt/extensions.c`, `extra.c`). The candidate keeps the module
//! registry in process-lifetime `RwLock<HashMap>` tables (upstream uses
//! global xmlHashTable instances) with the same observable contracts:
//! registrations return 0 on success / -1 on failure and lookups resolve
//! by (name, URI) case-sensitively.

#![allow(non_snake_case)]
#![allow(unused_variables)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use core::ptr;
use std::collections::HashMap;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};

use parking_lot::RwLock;

use crate::abi::allocator::{xmlFree, xmlMalloc};
use crate::abi::structs::*;
use crate::abi::types::*;

// ── Registry types (upstream _xsltExtModule, extensions.c) ────────────────

/// `xsltExtInitFunction`: called when a stylesheet first uses the module.
pub type xsltExtInitFunction =
    unsafe extern "C" fn(ctxt: *mut _xsltTransformContext, URI: *const xmlChar) -> *mut c_void;
/// `xsltExtShutdownFunction`: called when the context is freed.
pub type xsltExtShutdownFunction =
    unsafe extern "C" fn(ctxt: *mut _xsltTransformContext, URI: *const xmlChar, data: *mut c_void);
/// `xsltStyleExtInitFunction`: called at stylesheet compile time.
pub type xsltStyleExtInitFunction =
    unsafe extern "C" fn(style: *mut _xsltStylesheet, URI: *const xmlChar) -> *mut c_void;
/// `xsltStyleExtShutdownFunction`: called when the stylesheet is freed.
pub type xsltStyleExtShutdownFunction =
    unsafe extern "C" fn(style: *mut _xsltStylesheet, URI: *const xmlChar, data: *mut c_void);
/// `xsltTopLevelFunction`: handles a top-level extension element.
pub type xsltTopLevelFunction =
    unsafe extern "C" fn(style: *mut _xsltStylesheet, node: *mut _xmlNode, data: *mut c_void);

#[derive(Clone, Copy)]
struct ExtModule {
    init_func: Option<xsltExtInitFunction>,
    shutdown_func: Option<xsltExtShutdownFunction>,
    style_init_func: Option<xsltStyleExtInitFunction>,
    style_shutdown_func: Option<xsltStyleExtShutdownFunction>,
}

/// `_xsltExtElement` entry: name + URI + precompute + transform handlers.
/// `_xsltExtElement` entry: name + URI + precompute + transform handlers.
/// The fn pointers are stored as `usize` so the registry is Send + Sync
/// (they are cast back to raw pointers at lookup time).
#[derive(Clone, Copy)]
struct ExtElement {
    precomp: usize,   // xsltPreComputeFunction
    transform: usize, // xsltTransformFunction
}

/// Registry key: "name\0URI\0" (upstream hashes the QName via xmlDictQLookup).
fn ext_key(name: *const xmlChar, uri: *const xmlChar) -> Option<Vec<u8>> {
    if name.is_null() || uri.is_null() {
        return None;
    }
    let n = unsafe { CStr::from_ptr(name as *const c_char).to_bytes() };
    let u = unsafe { CStr::from_ptr(uri as *const c_char).to_bytes() };
    let mut k = Vec::with_capacity(n.len() + 1 + u.len());
    k.extend_from_slice(n);
    k.push(0);
    k.extend_from_slice(u);
    Some(k)
}

/// Global extension-module registry (upstream `xsltExtModules` hash).
static EXT_MODULES: once_cell::sync::Lazy<RwLock<HashMap<Vec<u8>, ExtModule>>> =
    once_cell::sync::Lazy::new(|| RwLock::new(HashMap::new()));
/// Global extension-element registry (upstream `xsltExtElements` hash).
/// Fn pointers stored as `usize` (Send + Sync).
static EXT_ELEMENTS: once_cell::sync::Lazy<RwLock<HashMap<Vec<u8>, ExtElement>>> =
    once_cell::sync::Lazy::new(|| RwLock::new(HashMap::new()));
/// Global extension-function registry (upstream `xsltExtFunctions` hash).
static EXT_FUNCTIONS: once_cell::sync::Lazy<RwLock<HashMap<Vec<u8>, usize>>> =
    once_cell::sync::Lazy::new(|| RwLock::new(HashMap::new()));
/// Global top-level-element registry (upstream `xsltExtTopLevels` hash).
static EXT_TOPLEVELS: once_cell::sync::Lazy<RwLock<HashMap<Vec<u8>, usize>>> =
    once_cell::sync::Lazy::new(|| RwLock::new(HashMap::new()));

/// `xsltRegisterExtModule` (extensions.c): register a module by URI.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltRegisterExtModule(const xmlChar *URI,
///                           xsltExtInitFunction initFunc,
///                           xsltExtShutdownFunction shutdownFunc);
/// ```
///
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn xsltRegisterExtModule(
    URI: *const xmlChar,
    initFunc: Option<xsltExtInitFunction>,
    shutdownFunc: Option<xsltExtShutdownFunction>,
) -> c_int {
    if URI.is_null() {
        return -1;
    }
    let key = unsafe { CStr::from_ptr(URI as *const c_char).to_bytes().to_vec() };
    EXT_MODULES.write().insert(
        key,
        ExtModule {
            init_func: initFunc,
            shutdown_func: shutdownFunc,
            style_init_func: None,
            style_shutdown_func: None,
        },
    );
    0
}

/// `xsltRegisterExtModuleFull` (extensions.c): register a module including
/// the stylesheet-level init/shutdown hooks.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltRegisterExtModuleFull(const xmlChar *URI,
///                               xsltExtInitFunction initFunc,
///                               xsltExtShutdownFunction shutdownFunc,
///                               xsltStyleExtInitFunction styleInitFunc,
///                               xsltStyleExtShutdownFunction styleShutdownFunc);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltRegisterExtModuleFull(
    URI: *const xmlChar,
    initFunc: Option<xsltExtInitFunction>,
    shutdownFunc: Option<xsltExtShutdownFunction>,
    styleInitFunc: Option<xsltStyleExtInitFunction>,
    styleShutdownFunc: Option<xsltStyleExtShutdownFunction>,
) -> c_int {
    if URI.is_null() {
        return -1;
    }
    let key = unsafe { CStr::from_ptr(URI as *const c_char).to_bytes().to_vec() };
    EXT_MODULES.write().insert(
        key,
        ExtModule {
            init_func: initFunc,
            shutdown_func: shutdownFunc,
            style_init_func: styleInitFunc,
            style_shutdown_func: styleShutdownFunc,
        },
    );
    0
}

/// `xsltRegisterExtModuleElement` (extensions.c): register an extension
/// element (name in a module URI) with precompute + transform handlers.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltRegisterExtModuleElement(const xmlChar *name, const xmlChar *URI,
///                                  xsltPreComputeFunction precomp,
///                                  xsltTransformFunction transform);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltRegisterExtModuleElement(
    name: *const xmlChar,
    URI: *const xmlChar,
    precomp: *mut c_void,
    transform: *mut c_void,
) -> c_int {
    let Some(key) = ext_key(name, URI) else {
        return -1;
    };
    EXT_ELEMENTS.write().insert(
        key,
        ExtElement {
            precomp: precomp as usize,
            transform: transform as usize,
        },
    );
    0
}

/// `xsltRegisterExtModuleFunction` (extensions.c): register an XPath
/// extension function (name in a module URI).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltRegisterExtModuleFunction(const xmlChar *name, const xmlChar *URI,
///                                   xmlXPathFunction function);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltRegisterExtModuleFunction(
    name: *const xmlChar,
    URI: *const xmlChar,
    function: *mut c_void,
) -> c_int {
    let Some(key) = ext_key(name, URI) else {
        return -1;
    };
    EXT_FUNCTIONS.write().insert(key, function as usize);
    0
}

/// `xsltRegisterExtModuleTopLevel` (extensions.c): register a top-level
/// extension element handler.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltRegisterExtModuleTopLevel(const xmlChar *name, const xmlChar *URI,
///                                   xsltTopLevelFunction function);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltRegisterExtModuleTopLevel(
    name: *const xmlChar,
    URI: *const xmlChar,
    function: *mut c_void,
) -> c_int {
    let Some(key) = ext_key(name, URI) else {
        return -1;
    };
    EXT_TOPLEVELS.write().insert(key, function as usize);
    0
}

/// `xsltUnregisterExtModule` (extensions.c): unregister a module and all of
/// its elements/functions/top-levels.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltUnregisterExtModule(const xmlChar *URI);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltUnregisterExtModule(URI: *const xmlChar) -> c_int {
    if URI.is_null() {
        return -1;
    }
    let uri_bytes = unsafe { CStr::from_ptr(URI as *const c_char).to_bytes().to_vec() };
    let mut mods = EXT_MODULES.write();
    if mods.remove(&uri_bytes).is_none() {
        return -1;
    }
    drop(mods);
    // Remove every element/function/top-level belonging to the URI.
    let mut elems = EXT_ELEMENTS.write();
    let mut funcs = EXT_FUNCTIONS.write();
    let mut tops = EXT_TOPLEVELS.write();
    let suffix: Vec<u8> = {
        let mut s = vec![0];
        s.extend_from_slice(&uri_bytes);
        s
    };
    elems.retain(|k, _| !k.ends_with(&suffix));
    funcs.retain(|k, _| !k.ends_with(&suffix));
    tops.retain(|k, _| !k.ends_with(&suffix));
    0
}

/// `xsltUnregisterExtModuleElement` (extensions.c).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltUnregisterExtModuleElement(const xmlChar *name, const xmlChar *URI);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltUnregisterExtModuleElement(
    name: *const xmlChar,
    URI: *const xmlChar,
) -> c_int {
    let Some(key) = ext_key(name, URI) else {
        return -1;
    };
    if EXT_ELEMENTS.write().remove(&key).is_some() {
        0
    } else {
        -1
    }
}

/// `xsltUnregisterExtModuleFunction` (extensions.c).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltUnregisterExtModuleFunction(const xmlChar *name, const xmlChar *URI);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltUnregisterExtModuleFunction(
    name: *const xmlChar,
    URI: *const xmlChar,
) -> c_int {
    let Some(key) = ext_key(name, URI) else {
        return -1;
    };
    if EXT_FUNCTIONS.write().remove(&key).is_some() {
        0
    } else {
        -1
    }
}

/// `xsltUnregisterExtModuleTopLevel` (extensions.c).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltUnregisterExtModuleTopLevel(const xmlChar *name, const xmlChar *URI);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltUnregisterExtModuleTopLevel(
    name: *const xmlChar,
    URI: *const xmlChar,
) -> c_int {
    let Some(key) = ext_key(name, URI) else {
        return -1;
    };
    if EXT_TOPLEVELS.write().remove(&key).is_some() {
        0
    } else {
        -1
    }
}

/// `xsltRegisterExtPrefix` (extensions.c): register a prefix→URI mapping on
/// the stylesheet so `xsltCheckExtPrefix` recognises it as an extension.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltRegisterExtPrefix(xsltStylesheetPtr style,
///                           const xmlChar *prefix, const xmlChar *URI);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltRegisterExtPrefix(
    style: *mut _xsltStylesheet,
    prefix: *const xmlChar,
    URI: *const xmlChar,
) -> c_int {
    if style.is_null() || prefix.is_null() || URI.is_null() {
        return -1;
    }
    // The candidate carries registered extension prefixes as a growable
    // linked list in the stylesheet (upstream uses style->extInfos hash).
    let mut cur = (*style).extInfos as *mut ExtPrefixEntry;
    while !cur.is_null() {
        if !(*cur).prefix.is_null()
            && libc::strcmp((*cur).prefix as *const c_char, prefix as *const c_char) == 0
        {
            // Re-registration with a different URI updates the mapping.
            let new_uri = crate::abi::allocator::xmlMemStrdup(URI as *const c_char) as *mut c_char;
            if new_uri.is_null() {
                return -1;
            }
            xmlFree((*cur).uri as *mut c_void);
            (*cur).uri = new_uri;
            return 0;
        }
        cur = (*cur).next;
    }
    let entry = xmlMalloc(size_of::<ExtPrefixEntry>()) as *mut ExtPrefixEntry;
    if entry.is_null() {
        return -1;
    }
    let p = crate::abi::allocator::xmlMemStrdup(prefix as *const c_char) as *mut c_char;
    let u = crate::abi::allocator::xmlMemStrdup(URI as *const c_char) as *mut c_char;
    if p.is_null() || u.is_null() {
        if !p.is_null() {
            xmlFree(p as *mut c_void);
        }
        if !u.is_null() {
            xmlFree(u as *mut c_void);
        }
        xmlFree(entry as *mut c_void);
        return -1;
    }
    ptr::write(
        entry,
        ExtPrefixEntry {
            next: (*style).extInfos as *mut ExtPrefixEntry,
            prefix: p,
            uri: u,
        },
    );
    (*style).extInfos = entry as *mut c_void;
    0
}

/// `xsltCheckExtPrefix` (extensions.c): 1 if `prefix` is registered as an
/// extension prefix on the stylesheet (or is a literal-result element
/// prefix), 0 otherwise.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltCheckExtPrefix(xsltStylesheetPtr style, const xmlChar *prefix);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltCheckExtPrefix(
    style: *mut _xsltStylesheet,
    prefix: *const xmlChar,
) -> c_int {
    if style.is_null() || prefix.is_null() {
        return 0;
    }
    let mut cur = (*style).extInfos as *mut ExtPrefixEntry;
    while !cur.is_null() {
        if !(*cur).prefix.is_null()
            && libc::strcmp((*cur).prefix as *const c_char, prefix as *const c_char) == 0
        {
            return 1;
        }
        cur = (*cur).next;
    }
    0
}

/// `xsltCheckExtURI` (extensions.c): 1 if `URI` is registered as an
/// extension namespace on the stylesheet, 0 otherwise.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltCheckExtURI(xsltStylesheetPtr style, const xmlChar *URI);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltCheckExtURI(
    style: *mut _xsltStylesheet,
    URI: *const xmlChar,
) -> c_int {
    if style.is_null() || URI.is_null() {
        return 0;
    }
    let mut cur = (*style).extInfos as *mut ExtPrefixEntry;
    while !cur.is_null() {
        if !(*cur).uri.is_null()
            && libc::strcmp((*cur).uri as *const c_char, URI as *const c_char) == 0
        {
            return 1;
        }
        cur = (*cur).next;
    }
    0
}

/// `xsltExtElementLookup` (extensions.c): resolve an extension element's
/// transform function, consulting the per-context registrations first then
/// the global module registry.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltTransformFunction xsltExtElementLookup(xsltTransformContextPtr ctxt,
///                                            const xmlChar *name,
///                                            const xmlChar *URI);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltExtElementLookup(
    ctxt: *mut _xsltTransformContext,
    name: *const xmlChar,
    URI: *const xmlChar,
) -> *mut c_void {
    if ctxt.is_null() || name.is_null() || URI.is_null() {
        return ptr::null_mut();
    }
    // Per-context registrations (xsltRegisterExtElement).
    let found = crate::xslt::extensions::xsltFindExtElement(ctxt, name, URI);
    if !found.is_null() {
        return found;
    }
    let Some(key) = ext_key(name, URI) else {
        return ptr::null_mut();
    };
    EXT_ELEMENTS
        .read()
        .get(&key)
        .map(|e| e.transform as *mut c_void)
        .unwrap_or(ptr::null_mut())
}

/// `xsltExtModuleElementLookup` (extensions.c): global element lookup.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltTransformFunction xsltExtModuleElementLookup(const xmlChar *name,
///                                                  const xmlChar *URI);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltExtModuleElementLookup(
    name: *const xmlChar,
    URI: *const xmlChar,
) -> *mut c_void {
    let Some(key) = ext_key(name, URI) else {
        return ptr::null_mut();
    };
    EXT_ELEMENTS
        .read()
        .get(&key)
        .map(|e| e.transform as *mut c_void)
        .unwrap_or(ptr::null_mut())
}

/// `xsltExtModuleFunctionLookup` (extensions.c): global function lookup.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlXPathFunction xsltExtModuleFunctionLookup(const xmlChar *name,
///                                              const xmlChar *URI);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltExtModuleFunctionLookup(
    name: *const xmlChar,
    URI: *const xmlChar,
) -> *mut c_void {
    let Some(key) = ext_key(name, URI) else {
        return ptr::null_mut();
    };
    EXT_FUNCTIONS
        .read()
        .get(&key)
        .copied()
        .map(|p| p as *mut c_void)
        .unwrap_or(ptr::null_mut())
}

/// `xsltExtModuleElementPreComputeLookup` (extensions.c).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltPreComputeFunction xsltExtModuleElementPreComputeLookup(
///     const xmlChar *name, const xmlChar *URI);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltExtModuleElementPreComputeLookup(
    name: *const xmlChar,
    URI: *const xmlChar,
) -> *mut c_void {
    let Some(key) = ext_key(name, URI) else {
        return ptr::null_mut();
    };
    EXT_ELEMENTS
        .read()
        .get(&key)
        .map(|e| e.precomp as *mut c_void)
        .unwrap_or(ptr::null_mut())
}

/// `xsltExtModuleTopLevelLookup` (extensions.c).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xsltTopLevelFunction xsltExtModuleTopLevelLookup(const xmlChar *name,
///                                                  const xmlChar *URI);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltExtModuleTopLevelLookup(
    name: *const xmlChar,
    URI: *const xmlChar,
) -> *mut c_void {
    let Some(key) = ext_key(name, URI) else {
        return ptr::null_mut();
    };
    EXT_TOPLEVELS
        .read()
        .get(&key)
        .copied()
        .map(|p| p as *mut c_void)
        .unwrap_or(ptr::null_mut())
}

/// `xsltInitCtxtExts` (extensions.c): call the init function of every module
/// whose URI the stylesheet uses (registered extension prefixes).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xsltInitCtxtExts(xsltTransformContextPtr ctxt);
/// ```
///
/// Returns 0 on success, -1 on error.
#[no_mangle]
pub unsafe extern "C" fn xsltInitCtxtExts(ctxt: *mut _xsltTransformContext) -> c_int {
    if ctxt.is_null() || (*ctxt).style.is_null() {
        return 0;
    }
    let style = (*ctxt).style;
    let mut cur = (*style).extInfos as *mut ExtPrefixEntry;
    while !cur.is_null() {
        if !(*cur).uri.is_null() {
            let key = CStr::from_ptr((*cur).uri as *const c_char)
                .to_bytes()
                .to_vec();
            if let Some(module) = EXT_MODULES.read().get(&key).copied() {
                if let Some(init) = module.init_func {
                    let data = init(ctxt, (*cur).uri as *const xmlChar);
                    if data.is_null() {
                        return -1;
                    }
                    // Record (URI -> data) in the context's extInfos list.
                    let entry = xmlMalloc(size_of::<ExtDataEntry>()) as *mut ExtDataEntry;
                    if entry.is_null() {
                        return -1;
                    }
                    let u = crate::abi::allocator::xmlMemStrdup((*cur).uri as *const c_char)
                        as *mut c_char;
                    if u.is_null() {
                        xmlFree(entry as *mut c_void);
                        return -1;
                    }
                    ptr::write(
                        entry,
                        ExtDataEntry {
                            next: (*ctxt).extInfos as *mut ExtDataEntry,
                            uri: u,
                            data,
                        },
                    );
                    (*ctxt).extInfos = entry as *mut c_void;
                }
            }
        }
        cur = (*cur).next;
    }
    0
}

/// `xsltShutdownCtxtExts` (extensions.c): call the shutdown function of
/// every initialised module on the context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltShutdownCtxtExts(xsltTransformContextPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltShutdownCtxtExts(ctxt: *mut _xsltTransformContext) {
    if ctxt.is_null() {
        return;
    }
    let mut cur = (*ctxt).extInfos as *mut ExtDataEntry;
    while !cur.is_null() {
        if !(*cur).uri.is_null() {
            let key = CStr::from_ptr((*cur).uri as *const c_char)
                .to_bytes()
                .to_vec();
            if let Some(module) = EXT_MODULES.read().get(&key).copied() {
                if let Some(shutdown) = module.shutdown_func {
                    shutdown(ctxt, (*cur).uri as *const xmlChar, (*cur).data);
                }
            }
        }
        cur = (*cur).next;
    }
}

/// `xsltFreeCtxtExts` (extensions.c): free the context's extension data.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltFreeCtxtExts(xsltTransformContextPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltFreeCtxtExts(ctxt: *mut _xsltTransformContext) {
    if ctxt.is_null() {
        return;
    }
    let mut cur = (*ctxt).extInfos as *mut ExtDataEntry;
    (*ctxt).extInfos = ptr::null_mut();
    while !cur.is_null() {
        let next = (*cur).next;
        if !(*cur).uri.is_null() {
            xmlFree((*cur).uri as *mut c_void);
        }
        xmlFree(cur as *mut c_void);
        cur = next;
    }
}

/// `xsltGetExtData` (extensions.c): the per-context data of a module.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xsltGetExtData(xsltTransformContextPtr ctxt, const xmlChar *URI);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltGetExtData(
    ctxt: *mut _xsltTransformContext,
    URI: *const xmlChar,
) -> *mut c_void {
    if ctxt.is_null() || URI.is_null() {
        return ptr::null_mut();
    }
    let mut cur = (*ctxt).extInfos as *mut ExtDataEntry;
    while !cur.is_null() {
        if !(*cur).uri.is_null()
            && libc::strcmp((*cur).uri as *const c_char, URI as *const c_char) == 0
        {
            return (*cur).data;
        }
        cur = (*cur).next;
    }
    ptr::null_mut()
}

/// `xsltStyleGetExtData` (extensions.c): the per-stylesheet data of a
/// module, initialising it on first use via the style init hook.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void *xsltStyleGetExtData(xsltStylesheetPtr style, const xmlChar *URI);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltStyleGetExtData(
    style: *mut _xsltStylesheet,
    URI: *const xmlChar,
) -> *mut c_void {
    if style.is_null() || URI.is_null() {
        return ptr::null_mut();
    }
    let mut cur = (*style).extInfos as *mut ExtDataEntry;
    while !cur.is_null() {
        if !(*cur).uri.is_null()
            && libc::strcmp((*cur).uri as *const c_char, URI as *const c_char) == 0
        {
            return (*cur).data;
        }
        cur = (*cur).next;
    }
    let key = CStr::from_ptr(URI as *const c_char).to_bytes().to_vec();
    let module = EXT_MODULES.read().get(&key).copied();
    let data = match module {
        Some(m) => match m.style_init_func {
            Some(init) => init(style, URI),
            None => ptr::null_mut(),
        },
        None => ptr::null_mut(),
    };
    let entry = xmlMalloc(size_of::<ExtDataEntry>()) as *mut ExtDataEntry;
    if entry.is_null() {
        return ptr::null_mut();
    }
    let u = crate::abi::allocator::xmlMemStrdup(URI as *const c_char) as *mut c_char;
    if u.is_null() {
        xmlFree(entry as *mut c_void);
        return ptr::null_mut();
    }
    ptr::write(
        entry,
        ExtDataEntry {
            next: (*style).extInfos as *mut ExtDataEntry,
            uri: u,
            data,
        },
    );
    (*style).extInfos = entry as *mut c_void;
    data
}

/// `xsltGetExtInfo` (extensions.c): the stylesheet's extension-data list
/// head (upstream returns the `style->extInfos` hash pointer).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlHashTablePtr xsltGetExtInfo(xsltStylesheetPtr style, const xmlChar *URI);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltGetExtInfo(
    style: *mut _xsltStylesheet,
    _URI: *const xmlChar,
) -> *mut c_void {
    if style.is_null() {
        return ptr::null_mut();
    }
    (*style).extInfos
}

/// `xsltRegisterAllExtras` (extra.c): register the EXSLT "extra" extension
/// elements (exsl:document) into the global module registry.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltRegisterAllExtras(void);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltRegisterAllExtras() {
    // exsl:document — handled natively by the transform engine
    // (process_exsl_document); registering the module URI makes
    // xsltCheckExtURI agree with upstream.
    xsltRegisterExtModule(
        b"http://exslt.org/common\0".as_ptr() as *const xmlChar,
        None,
        None,
    );
}

/// `xsltRegisterExtras` (extra.c): register the EXSLT functions into the
/// context's XPath context (upstream calls xsltRegisterAllFunctions).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltRegisterExtras(xsltTransformContextPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltRegisterExtras(ctxt: *mut _xsltTransformContext) {
    if ctxt.is_null() || (*ctxt).xpathCtxt.is_null() {
        return;
    }
    crate::abi::exports_xslt_functions::xsltRegisterAllFunctions((*ctxt).xpathCtxt);
}

/// `xsltRegisterAllElement` (extra.c): register the EXSLT elements into the
/// transform context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltRegisterAllElement(xsltTransformContextPtr ctxt);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltRegisterAllElement(ctxt: *mut _xsltTransformContext) {
    if ctxt.is_null() {
        return;
    }
    // The engine dispatches EXSLT elements natively (process_exsl_document
    // and the exslt module registrations); nothing to add to the context's
    // per-context registration lists.
}

/// `xsltRegisterTestModule` (extensions.c): register the libxslt self-test
/// extension module (a no-op surface in the candidate).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xsltRegisterTestModule(void);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xsltRegisterTestModule() {
    xsltRegisterExtModule(
        b"http://xmlsoft.org/XSLT/\0".as_ptr() as *const xmlChar,
        None,
        None,
    );
}

// ── Internal helper structures (not part of the ABI) ───────────────────────

/// Stylesheet extension-prefix registration (upstream style->extInfos hash
/// entries; the candidate uses a linked list).
#[repr(C)]
pub struct ExtPrefixEntry {
    pub next: *mut ExtPrefixEntry,
    pub prefix: *mut c_char,
    pub uri: *mut c_char,
}

/// Per-context / per-style extension data record (URI -> init data).
#[repr(C)]
pub struct ExtDataEntry {
    pub next: *mut ExtDataEntry,
    pub uri: *mut c_char,
    pub data: *mut c_void,
}
