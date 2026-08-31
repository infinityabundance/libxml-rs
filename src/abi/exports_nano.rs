//! exports_nano — xmlNanoFTP*/xmlNanoHTTP*/xmlIOFTP*/xmlIOHTTP* C ABI family (§11.1-I).
//!
//! Implements the legacy network client APIs from upstream `nanoftp.c` /
//! `nanohttp.c` (nanoftp.h / nanohttp.h) plus the protocol I/O callbacks
//! from xmlIO.h — 48 exported entry points in total:
//!
//! - 22 × `xmlNanoFTP*` (nanoftp.h)
//! - 17 × `xmlNanoHTTP*` (nanohttp.h)
//! - 5 × `xmlIOHTTP*` (xmlIO.h)
//! - 4 × `xmlIOFTP*` (xmlIO.h)
//!
//! # Offline design (INTENTIONAL)
//!
//! This crate is an offline forensic reimplementation: there is no network
//! stack, so no real sockets are ever created. The context types
//! (`xmlNanoFTPCtxt` / `xmlNanoHTTPCtxt`) are opaque to callers; state is
//! kept in side registries (`FTP_CTXTS` / `HTTP_CTXTS`) keyed by an
//! allocated handle pointer, mirroring the field layout of the upstream
//! structs (nanoftp.c / nanohttp.c) as closely as the fake transport
//! allows.
//!
//! The FTP control-plane lifecycle (`NewCtxt → Connect → … → Quit → Close`)
//! is internally consistent: `xmlNanoFTPConnect` simulates a successful
//! control connection by allocating a fake fd and `xmlNanoFTPGetConnection`
//! returns a fake data fd. Everything that requires an actual server
//! round-trip (list, get, open a fetch, read data, responses) returns the
//! upstream documented failure value and is *not* faked. The HTTP client
//! never fabricates success: without a network no context can be returned,
//! so `xmlNanoHTTPMethodRedir` (and everything built on it) returns NULL
//! exactly as upstream does when the connect fails.
//!
//! # Upstream contract
//!
//! Parity target is upstream `nanoftp.c` and `nanohttp.c` (libxml2 2.15.3,
//! SRC-LIBXML2-2.15.0-NANOHTTP-C / SRC-LIBXML2-2.15.0-NANOFTP-C) with the
//! `nanoftp.h`/`nanohttp.h`/`xmlIO.h` signatures — 48 exported entry points
//! resolved by the oracle DSO (R-000165 closed the nano gaps).
//!
//! # Conceptual behavior
//!
//! This module implements the legacy network-client ABI: FTP control/data
//! lifecycle, HTTP method wrappers and the xmlIO protocol callbacks. Because
//! the crate is an offline forensic reimplementation there is no network
//! stack: the design is documented above — control-plane simulation only,
//! never fabricated data-plane success.
//!
//! # Ownership & safety invariants
//!
//! Context handles (`xmlNanoFTPCtxtPtr`/`xmlNanoHTTPCtxtPtr`) are owned by the
//! caller and freed with `xmlNanoFTPFreeCtxt`/`xmlNanoHTTPFreeCtxt`; internal
//! state lives in the side registries (`FTP_CTXTS`/`HTTP_CTXTS`) keyed by the
//! allocated handle, so a handle is never dereferenced after its free. Fake
//! fds allocated by the connect simulation are released by the matching close
//! entry points.
//!
//! # Historical quirks & epochs
//!
//! nanohttp/nanoftp date to the 2.4-2.6 era and are deprecated upstream but
//! still exported; the modern 2.10+ epoch kept the ABI for legacy consumers.
//! The offline no-network divergence is a deliberate project-level decision
//! recorded in the header above.
//!
//! # Deliberate oddities
//!
//! The fake-transport simulation (connect returns a fake fd, list/get return
//! upstream failure values) is the deliberate core oddity: the observable
//! contract is kept where it does not require a server, and never faked where
//! it does.
//!
//! # Proving courts
//!
//! The DSO-LOADER court resolves all 48 symbols from the built DSO; the
//! HEADER-COMPILE court compiles the nanoftp/nanohttp headers; the
//! failure-path behaviors are exercised by the data-ABI probe suites.
//!
//! # Tempting simplifications that would break parity
//!
//! A tempting simplification is to make every nano function return the failure
//! value unconditionally (no control-plane simulation) — that would break the
//! documented FTP lifecycle contract (`NewCtxt → Connect → Quit → Close`) that
//! downstream code sequences, and the `xmlNanoFTPConnect`/`GetConnection`
//! entry points would stop matching their upstream return shapes. The
//! simulation must stay as bounded as documented, no more and no less.

#![allow(
    missing_docs,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals
)]
#![allow(unused_variables)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

// SAFETY-SCOPE: EXPORT-NANO-MECHANICAL-001
// (11.1-Z.3 proof scope, classified-generated) — this module is the
// mechanical extern-"C" export surface: every `unsafe` block in it is
// the documented indirection/registry-access pattern whose validity
// rests on the upstream C contract, and the exported signatures are
// machine-measured by the ABI-FUNCTION-SIGNATURE and DSO-LOADER
// courts and the C-API differential probes. The safety contract of
// each export is stated in its own doc comment; this scope covers the
// mechanical wrappers' unsafe blocks.

use core::ffi::c_void;
use core::ptr;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_ulong};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};

/// Upstream `INVALID_SOCKET` (nanoftp.h / nanohttp.h): `(-1)` on POSIX.
const INVALID_SOCKET: c_int = -1;
/// Upstream default FTP port (`21`), used by `xmlNanoFTPNewCtxt`.
const FTP_DEFAULT_PORT: c_int = 21;
/// Upstream default HTTP port (`80`), used by `xmlNanoHTTPNewCtxt`.
const HTTP_DEFAULT_PORT: c_int = 80;

/// C `ftpListCallback` (nanoftp.h) — invoked once per entry by
/// `xmlNanoFTPList`. NULL is legal (upstream only calls it when non-NULL).
type FtpListCallback = Option<
    unsafe extern "C" fn(
        userData: *mut c_void,
        filename: *const c_char,
        attrib: *const c_char,
        owner: *const c_char,
        group: *const c_char,
        size: c_ulong,
        links: c_int,
        year: c_int,
        month: *const c_char,
        day: c_int,
        hour: c_int,
        minute: c_int,
    ),
>;

/// C `ftpDataCallback` (nanoftp.h) — invoked with each data block by
/// `xmlNanoFTPGet`. NULL is legal (upstream requires it non-NULL in Get).
type FtpDataCallback =
    Option<unsafe extern "C" fn(userData: *mut c_void, data: *const c_char, len: c_int)>;

// ═══════════════════════════════════════════════════════════════════════════════
// Side registries and shared helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Opaque handle allocated for each FTP context; its address is the key in
/// `FTP_CTXTS`. Non-zero-sized so every allocation is a distinct address.
#[repr(C)]
struct FtpHandle(u64);

/// Opaque handle allocated for each HTTP context; its address is the key in
/// `HTTP_CTXTS`.
#[repr(C)]
struct HttpHandle(u64);

/// Side registry for live FTP contexts, keyed by handle address. Mirrors
/// `struct xmlNanoFTPCtxt` (nanoftp.c); transport fields are faked.
static FTP_CTXTS: Lazy<Mutex<HashMap<usize, NanoFtpState>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Side registry for live HTTP contexts, keyed by handle address. Mirrors
/// `struct xmlNanoHTTPCtxt` (nanohttp.c); the zlib members are omitted.
static HTTP_CTXTS: Lazy<Mutex<HashMap<usize, NanoHttpState>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Module-level FTP proxy configuration (upstream static `proxy` globals in
/// nanoftp.c). Inert in the offline build: it is stored for API shape but no
/// connection ever dials it.
#[allow(dead_code)]
#[derive(Default)]
struct FtpProxyCfg {
    host: Option<CString>,
    port: i32,
    user: Option<CString>,
    passwd: Option<CString>,
    kind: i32,
}

static FTP_PROXY: Lazy<Mutex<FtpProxyCfg>> = Lazy::new(|| Mutex::new(FtpProxyCfg::default()));

/// Module-level HTTP proxy configuration (upstream `proxy`/`proxyPort`
/// globals in nanohttp.c).
#[allow(dead_code)]
#[derive(Default)]
struct HttpProxyCfg {
    host: Option<CString>,
    port: i32,
}

static HTTP_PROXY: Lazy<Mutex<HttpProxyCfg>> = Lazy::new(|| Mutex::new(HttpProxyCfg::default()));

/// One-time-init flags (upstream `static int initialized`).
static FTP_INITIALIZED: AtomicBool = AtomicBool::new(false);
static HTTP_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Handle allocation counter (any distinct address works).
static NEXT_KEY: AtomicU64 = AtomicU64::new(1);
/// Fake fd allocator: small positive ints starting at 3 (past stdio).
static NEXT_FD: AtomicI32 = AtomicI32::new(3);

/// Mirror of `struct xmlNanoFTPCtxt` (nanoftp.c). Fields follow upstream
/// layout; `control_fd`/`data_fd` hold *fake* fds and the control buffer is
/// never filled (no peer ever answers).
#[allow(dead_code)]
#[derive(Default)]
struct NanoFtpState {
    protocol: Option<CString>,
    hostname: Option<CString>,
    port: i32,
    path: Option<CString>,
    user: Option<CString>,
    passwd: Option<CString>,
    passive: i32,
    control_fd: i32,
    data_fd: i32,
    state: i32,
    return_value: i32,
    control_buf_index: i32,
    control_buf_used: i32,
    control_buf_answer: i32,
}

/// Mirror of `struct xmlNanoHTTPCtxt` (nanohttp.c), minus the zlib members.
/// No HTTP context is ever returned by the offline build, so most fields are
/// inert but kept for layout parity.
#[allow(dead_code)]
#[derive(Default)]
struct NanoHttpState {
    protocol: Option<CString>,
    hostname: Option<CString>,
    port: i32,
    path: Option<CString>,
    query: Option<CString>,
    fd: i32,
    state: i32,
    out: Option<CString>,
    in_: Option<CString>,
    content: Option<CString>,
    inptr: i32,
    inrptr: i32,
    inlen: i32,
    last: i32,
    return_value: i32,
    version: i32,
    content_length: i32,
    content_type: Option<CString>,
    location: Option<CString>,
    auth_header: Option<CString>,
    encoding: Option<CString>,
    mime_type: Option<CString>,
}

fn alloc_ftp_handle() -> *mut c_void {
    let key = NEXT_KEY.fetch_add(1, Ordering::Relaxed);
    Box::into_raw(Box::new(FtpHandle(key))) as *mut c_void
}

fn alloc_http_handle() -> *mut c_void {
    let key = NEXT_KEY.fetch_add(1, Ordering::Relaxed);
    Box::into_raw(Box::new(HttpHandle(key))) as *mut c_void
}

unsafe fn free_ftp_handle(handle: *mut c_void) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle as *mut FtpHandle) });
    }
}

unsafe fn free_http_handle(handle: *mut c_void) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle as *mut HttpHandle) });
    }
}

/// Drop the FTP context: deregister (freeing the state strings) and release
/// the handle. Returns false when `ctx` was NULL or not registered.
fn remove_ftp_ctxt(ctx: *mut c_void) -> bool {
    if ctx.is_null() {
        return false;
    }
    let removed = FTP_CTXTS.lock().remove(&(ctx as usize)).is_some();
    if removed {
        unsafe { free_ftp_handle(ctx) };
    }
    removed
}

/// Drop the HTTP context: deregister (freeing the state strings) and release
/// the handle. Returns false when `ctx` was NULL or not registered.
fn remove_http_ctxt(ctx: *mut c_void) -> bool {
    if ctx.is_null() {
        return false;
    }
    let removed = HTTP_CTXTS.lock().remove(&(ctx as usize)).is_some();
    if removed {
        unsafe { free_http_handle(ctx) };
    }
    removed
}

/// Fake fd for the simulated transport. Small positive ints, never reused by
/// the (fake) socket layer, mirroring what a real `socket(2)` would return.
fn fake_fd() -> c_int {
    NEXT_FD.fetch_add(1, Ordering::Relaxed)
}

/// NULL-aware C string read.
unsafe fn cstr_to_string(p: *const c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let s = unsafe { CStr::from_ptr(p) };
    Some(s.to_string_lossy().into_owned())
}

/// NULL-aware C string read into an owned `CString` (for registry storage).
fn opt_cstring(p: *const c_char) -> Option<CString> {
    unsafe { cstr_to_string(p) }.map(to_cstring)
}

/// Bytes of a NULL-terminated C string (`&[]` for NULL).
const unsafe fn cstr_bytes<'a>(p: *const c_char) -> &'a [u8] {
    if p.is_null() {
        return &[];
    }
    let mut len = 0usize;
    while unsafe { *p.add(len) } != 0 {
        len += 1;
    }
    unsafe { core::slice::from_raw_parts(p as *const u8, len) }
}

fn to_cstring(s: String) -> CString {
    CString::new(s).unwrap_or_default()
}

/// ASCII case-insensitive prefix test (upstream `xmlStrncasecmp` for the
/// `ftp://`/`http://` scheme checks in xmlIO.c).
fn starts_with_ci(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.len() >= needle.len()
        && haystack[..needle.len()]
            .iter()
            .zip(needle.iter())
            .all(|(h, n)| h.eq_ignore_ascii_case(n))
}

/// Result of the minimal URL scan, replicating the fields upstream extracts
/// with `xmlParseURIRaw` in `xmlNanoFTPScanURL` / `xmlNanoHTTPScanURL`.
#[derive(Default)]
struct ParsedUrl {
    scheme: Option<String>,
    host: Option<String>,
    port: Option<i32>,
    path: Option<String>,
    query: Option<String>,
    user: Option<String>,
    passwd: Option<String>,
}

/// Minimal `scheme://[user[:pass]@]host[:port][/path][?query]` splitter.
/// No percent-decoding (upstream calls `xmlURIUnescapeString`); good enough
/// for the API shape of an offline client.
fn parse_url(url: &str) -> ParsedUrl {
    let mut out = ParsedUrl::default();
    let rest = match url.find("://") {
        Some(i) => {
            out.scheme = Some(url[..i].to_string());
            &url[i + 3..]
        }
        None => return out,
    };
    let (authority, tail) = match rest.find(['/', '?']) {
        Some(i) => rest.split_at(i),
        None => (rest, ""),
    };
    if !tail.is_empty() {
        if let Some(qi) = tail.find('?') {
            let (p, q) = tail.split_at(qi);
            if !p.is_empty() {
                out.path = Some(p.to_string());
            }
            if q.len() > 1 {
                out.query = Some(q[1..].to_string());
            }
        } else {
            out.path = Some(tail.to_string());
        }
    }
    if out.path.is_none() {
        out.path = Some("/".to_string());
    }
    let (userinfo, hostport) = match authority.rfind('@') {
        Some(i) => {
            let (u, hp) = authority.split_at(i);
            (u, &hp[1..])
        }
        None => ("", authority),
    };
    if !userinfo.is_empty() {
        match userinfo.find(':') {
            Some(ci) => {
                out.user = Some(userinfo[..ci].to_string());
                out.passwd = Some(userinfo[ci + 1..].to_string());
            }
            None => out.user = Some(userinfo.to_string()),
        }
    }
    match hostport.rfind(':') {
        Some(ci) => {
            out.host = Some(hostport[..ci].to_string());
            out.port = hostport[ci + 1..].parse::<i32>().ok();
        }
        None => out.host = Some(hostport.to_string()),
    }
    out
}

// ═══════════════════════════════════════════════════════════════════════════════
// NanoFTP — legacy FTP client (nanoftp.h / nanoftp.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Initialize the FTP protocol layer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlNanoFTPInit(void);
/// ```
///
/// One-time initialization. Upstream also scans `ftp_proxy`/`FTP_PROXY`
/// environment variables; with no network stack the proxy settings are
/// inert, so env scanning is skipped (no-op).
#[no_mangle]
pub unsafe extern "C" fn xmlNanoFTPInit() {
    if FTP_INITIALIZED.load(Ordering::Relaxed) {
        return;
    }
    FTP_INITIALIZED.store(true, Ordering::Relaxed);
}

/// Cleanup the FTP protocol layer (frees proxy information upstream).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlNanoFTPCleanup(void);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNanoFTPCleanup() {
    FTP_INITIALIZED.store(false, Ordering::Relaxed);
    *FTP_PROXY.lock() = FtpProxyCfg::default();
}

/// Allocate and initialize a new FTP context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void * xmlNanoFTPNewCtxt(const char *URL);
/// ```
///
/// Returns an opaque handle registered in `FTP_CTXTS`, or NULL on
/// allocation failure. Defaults mirror `xmlNanoFTPNewCtxt` (nanoftp.c):
/// port 21, passive mode, `returnValue` 0, `controlFd`/`dataFd` invalid.
#[no_mangle]
pub unsafe extern "C" fn xmlNanoFTPNewCtxt(URL: *const c_char) -> *mut c_void {
    let handle = alloc_ftp_handle();
    let mut st = NanoFtpState {
        port: FTP_DEFAULT_PORT,
        passive: 1,
        control_fd: INVALID_SOCKET,
        data_fd: INVALID_SOCKET,
        control_buf_index: 0,
        control_buf_used: 0,
        control_buf_answer: 0,
        ..NanoFtpState::default()
    };
    if !URL.is_null() {
        let parsed = parse_url(&unsafe { cstr_to_string(URL) }.unwrap_or_default());
        st.protocol = parsed.scheme.map(to_cstring);
        st.hostname = parsed.host.map(to_cstring);
        if let Some(p) = parsed.port {
            st.port = p;
        }
        st.path = parsed.path.map(to_cstring);
        st.user = parsed.user.map(to_cstring);
        st.passwd = parsed.passwd.map(to_cstring);
    }
    FTP_CTXTS.lock().insert(handle as usize, st);
    handle
}

/// Free an FTP context, closing the connection first (upstream).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlNanoFTPFreeCtxt(void * ctx);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNanoFTPFreeCtxt(ctx: *mut c_void) {
    if ctx.is_null() {
        return;
    }
    // Upstream closes the sockets and frees the URL fields; the fake fds
    // need no close and the strings die with the removed registry record.
    remove_ftp_ctxt(ctx);
}

/// Tries to open a control connection to the given server/port.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void * xmlNanoFTPConnectTo(const char *server, int port);
/// ```
///
/// Returns an FTP context or NULL if it failed. The crate cannot do network
/// I/O (offline forensic reimplementation), so this returns the documented
/// failure pointer (NULL). The FTP *state* machine — `xmlNanoFTPConnect`,
/// `xmlNanoFTPQuit`, `xmlNanoFTPClose`, … — remains internally consistent
/// via fake fds (see `xmlNanoFTPConnect`).
#[no_mangle]
pub unsafe extern "C" fn xmlNanoFTPConnectTo(server: *const c_char, port: c_int) -> *mut c_void {
    xmlNanoFTPInit();
    if server.is_null() {
        return ptr::null_mut();
    }
    if port <= 0 {
        return ptr::null_mut();
    }
    // INTENTIONAL (offline): upstream dials server:port and returns the
    // connected context; no TCP connect can happen here, so NULL is the
    // documented failure return.
    ptr::null_mut()
}

/// Start fetching the given `ftp://` resource.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void * xmlNanoFTPOpen(const char *URL);
/// ```
///
/// Returns an FTP context, or NULL. Upstream opens the control connection
/// and then initiates the data-channel fetch (`TYPE I` + `RETR`) via
/// `xmlNanoFTPGetSocket`; the RETR handshake needs server replies, so
/// `GetSocket` fails offline and Open returns NULL as documented.
#[no_mangle]
pub unsafe extern "C" fn xmlNanoFTPOpen(URL: *const c_char) -> *mut c_void {
    xmlNanoFTPInit();
    if URL.is_null() {
        return ptr::null_mut();
    }
    if !unsafe { cstr_bytes(URL) }.starts_with(b"ftp://") {
        return ptr::null_mut();
    }
    let ctxt = xmlNanoFTPNewCtxt(URL);
    if ctxt.is_null() {
        return ptr::null_mut();
    }
    if xmlNanoFTPConnect(ctxt) < 0 {
        xmlNanoFTPFreeCtxt(ctxt);
        return ptr::null_mut();
    }
    // Upstream passes ctxt->path here; our GetSocket can never succeed, so
    // mirror the upstream failure path exactly.
    let path_ptr = ftp_path_ptr(ctxt);
    if xmlNanoFTPGetSocket(ctxt, path_ptr) == INVALID_SOCKET {
        xmlNanoFTPFreeCtxt(ctxt);
        return ptr::null_mut();
    }
    ctxt
}

/// Pointer to the current path of an FTP context (NULL if unset).
fn ftp_path_ptr(ctx: *mut c_void) -> *const c_char {
    let reg = FTP_CTXTS.lock();
    match reg.get(&(ctx as usize)).and_then(|st| st.path.as_ref()) {
        Some(p) => p.as_ptr(),
        None => ptr::null(),
    }
}

/// Tries to open a control connection.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNanoFTPConnect(void *ctx);
/// ```
///
/// Returns -1 in case of error, 0 otherwise. INTENTIONAL (offline): the
/// socket connect is simulated by allocating a fake fd, so the FTP lifecycle
/// (`NewCtxt → Connect → GetConnection → … → Quit → Close`) is internally
/// consistent. No bytes can ever flow on the fake channel.
#[no_mangle]
pub unsafe extern "C" fn xmlNanoFTPConnect(ctx: *mut c_void) -> c_int {
    if ctx.is_null() {
        return -1;
    }
    let mut reg = FTP_CTXTS.lock();
    let st = match reg.get_mut(&(ctx as usize)) {
        Some(st) => st,
        None => return -1,
    };
    if st.hostname.is_none() {
        return -1;
    }
    st.control_fd = fake_fd();
    0
}

/// Close the connection and free both control and data channels.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNanoFTPClose(void *ctx);
/// ```
///
/// Returns -1 in case of error, 0 otherwise.
#[no_mangle]
pub unsafe extern "C" fn xmlNanoFTPClose(ctx: *mut c_void) -> c_int {
    if ctx.is_null() {
        return -1;
    }
    {
        let mut reg = FTP_CTXTS.lock();
        let st = match reg.get_mut(&(ctx as usize)) {
            Some(st) => st,
            None => return -1,
        };
        // Upstream sends QUIT (see xmlNanoFTPQuit) then closes both sockets;
        // the fake channel has no peer, so the QUIT is a no-op.
        st.data_fd = INVALID_SOCKET;
        st.control_fd = INVALID_SOCKET;
    }
    remove_ftp_ctxt(ctx);
    0
}

/// Send a QUIT command to the server.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNanoFTPQuit(void *ctx);
/// ```
///
/// Returns -1 in case of error, 0 otherwise. On the simulated control
/// channel the command is considered delivered (upstream returns 0 on a
/// successful send).
#[no_mangle]
pub unsafe extern "C" fn xmlNanoFTPQuit(ctx: *mut c_void) -> c_int {
    if ctx.is_null() {
        return -1;
    }
    let reg = FTP_CTXTS.lock();
    let st = match reg.get(&(ctx as usize)) {
        Some(st) => st,
        None => return -1,
    };
    if st.control_fd == INVALID_SOCKET {
        return -1;
    }
    0
}

/// (Re)Initialize the FTP proxy context from a proxy URL.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlNanoFTPScanProxy(const char *URL);
/// ```
///
/// `ftp://host/` or `ftp://host:port/`; a NULL URL clears the proxy info.
/// The stored proxy is inert (no network stack ever dials it).
#[no_mangle]
pub unsafe extern "C" fn xmlNanoFTPScanProxy(URL: *const c_char) {
    let mut proxy = FTP_PROXY.lock();
    *proxy = FtpProxyCfg {
        port: 0,
        ..FtpProxyCfg::default()
    };
    if URL.is_null() {
        return;
    }
    let parsed = parse_url(&unsafe { cstr_to_string(URL) }.unwrap_or_default());
    if parsed.scheme.as_deref() != Some("ftp") || parsed.host.is_none() {
        // Upstream raises XML_FTP_URL_SYNTAX here; proxy stays cleared.
        return;
    }
    proxy.host = parsed.host.map(to_cstring);
    if let Some(p) = parsed.port {
        proxy.port = p;
    }
}

/// Setup the FTP proxy information.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlNanoFTPProxy(const char *host, int port,
///                      const char *user, const char *passwd, int type);
/// ```
///
/// `type` is 1 for using SITE, 2 for USER a@b. Stored for API shape; inert
/// in the offline build.
#[no_mangle]
pub unsafe extern "C" fn xmlNanoFTPProxy(
    host: *const c_char,
    port: c_int,
    user: *const c_char,
    passwd: *const c_char,
    kind: c_int,
) {
    let mut proxy = FTP_PROXY.lock();
    proxy.host = opt_cstring(host);
    proxy.user = opt_cstring(user);
    proxy.passwd = opt_cstring(passwd);
    proxy.port = port;
    proxy.kind = kind;
}

/// Update an FTP context by parsing the URL and finding a new path.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNanoFTPUpdateURL(void *ctx, const char *URL);
/// ```
///
/// Returns 0 if Ok, -1 in case of error (other host/scheme/port, or a NULL
/// context/URL, or the context was never initialized with a protocol).
#[no_mangle]
pub unsafe extern "C" fn xmlNanoFTPUpdateURL(ctx: *mut c_void, URL: *const c_char) -> c_int {
    if URL.is_null() || ctx.is_null() {
        return -1;
    }
    let parsed = parse_url(&unsafe { cstr_to_string(URL) }.unwrap_or_default());
    if parsed.scheme.is_none() || parsed.host.is_none() {
        return -1;
    }
    let mut reg = FTP_CTXTS.lock();
    let st = match reg.get_mut(&(ctx as usize)) {
        Some(st) => st,
        None => return -1,
    };
    if st.protocol.is_none() || st.hostname.is_none() {
        return -1;
    }
    let scheme_mismatch = match (st.protocol.as_ref(), parsed.scheme.as_deref()) {
        (Some(a), Some(b)) => a.as_bytes() != b.as_bytes(),
        _ => true,
    };
    let host_mismatch = match (st.hostname.as_ref(), parsed.host.as_deref()) {
        (Some(a), Some(b)) => a.as_bytes() != b.as_bytes(),
        _ => true,
    };
    if scheme_mismatch || host_mismatch {
        return -1;
    }
    if let Some(p) = parsed.port {
        if p != st.port {
            return -1;
        }
        st.port = p;
    }
    st.path = parsed.path.map(to_cstring);
    0
}

/// Get the response from the FTP server after a command.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNanoFTPGetResponse(void *ctx);
/// ```
///
/// Returns the code number, -1 on error. The fake control channel never
/// receives a reply, so the documented error return is produced.
#[no_mangle]
pub unsafe extern "C" fn xmlNanoFTPGetResponse(ctx: *mut c_void) -> c_int {
    if ctx.is_null() {
        return -1;
    }
    let reg = FTP_CTXTS.lock();
    let st = match reg.get(&(ctx as usize)) {
        Some(st) => st,
        None => return -1,
    };
    if st.control_fd == INVALID_SOCKET {
        return -1;
    }
    // Upstream blocks reading a 3-digit reply from the server; there is no
    // peer on the fake control channel, so the read fails as documented.
    -1
}

/// Check if there is a response from the FTP server after a command.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNanoFTPCheckResponse(void *ctx);
/// ```
///
/// Returns the code number, or 0 when nothing is pending. A `select()` on
/// the fake fd never reports readable data → 0.
#[no_mangle]
pub unsafe extern "C" fn xmlNanoFTPCheckResponse(ctx: *mut c_void) -> c_int {
    if ctx.is_null() {
        return -1;
    }
    let reg = FTP_CTXTS.lock();
    let st = match reg.get(&(ctx as usize)) {
        Some(st) => st,
        None => return -1,
    };
    if st.control_fd == INVALID_SOCKET {
        return -1;
    }
    0
}

/// Tries to change the remote directory.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNanoFTPCwd(void *ctx, const char *directory);
/// ```
///
/// Returns -1 in case of error, 1 if CWD worked, 0 if it failed. The fake
/// channel never delivers the required 250 reply, so the command "fails"
/// (0) exactly as upstream does for any non-2xx response.
#[no_mangle]
pub unsafe extern "C" fn xmlNanoFTPCwd(ctx: *mut c_void, directory: *const c_char) -> c_int {
    if ctx.is_null() {
        return -1;
    }
    let reg = FTP_CTXTS.lock();
    let st = match reg.get(&(ctx as usize)) {
        Some(st) => st,
        None => return -1,
    };
    if st.control_fd == INVALID_SOCKET {
        return -1;
    }
    if directory.is_null() {
        return 0;
    }
    0
}

/// Tries to delete an item (file or directory) from the server.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNanoFTPDele(void *ctx, const char *file);
/// ```
///
/// Returns -1 in case of error, 1 if DELE worked, 0 if it failed. The fake
/// channel never delivers the required 250 reply → 0.
#[no_mangle]
pub unsafe extern "C" fn xmlNanoFTPDele(ctx: *mut c_void, file: *const c_char) -> c_int {
    if ctx.is_null() {
        return -1;
    }
    let reg = FTP_CTXTS.lock();
    let st = match reg.get(&(ctx as usize)) {
        Some(st) => st,
        None => return -1,
    };
    if st.control_fd == INVALID_SOCKET || file.is_null() {
        return -1;
    }
    0
}

/// Try to open a data connection to the server (passive mode only).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// SOCKET xmlNanoFTPGetConnection(void *ctx);
/// ```
///
/// Returns -1 in case of error, 0 otherwise (SOCKET == int on POSIX).
/// INTENTIONAL (offline): the PASV/EPSV data channel is simulated by a fake
/// fd, keeping the lifecycle (`GetConnection → Read → CloseConnection`)
/// internally consistent. No bytes can ever flow on it.
#[no_mangle]
pub unsafe extern "C" fn xmlNanoFTPGetConnection(ctx: *mut c_void) -> c_int {
    if ctx.is_null() {
        return INVALID_SOCKET;
    }
    let mut reg = FTP_CTXTS.lock();
    let st = match reg.get_mut(&(ctx as usize)) {
        Some(st) => st,
        None => return INVALID_SOCKET,
    };
    st.data_fd = fake_fd();
    st.data_fd
}

/// Close the data connection from the server.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNanoFTPCloseConnection(void *ctx);
/// ```
///
/// Returns -1 in case of error, 0 otherwise.
#[no_mangle]
pub unsafe extern "C" fn xmlNanoFTPCloseConnection(ctx: *mut c_void) -> c_int {
    if ctx.is_null() {
        return -1;
    }
    let mut reg = FTP_CTXTS.lock();
    let st = match reg.get_mut(&(ctx as usize)) {
        Some(st) => st,
        None => return -1,
    };
    if st.control_fd == INVALID_SOCKET {
        return -1;
    }
    st.data_fd = INVALID_SOCKET;
    0
}

/// Do a listing on the server; entries go to the callback.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNanoFTPList(void *ctx, ftpListCallback callback,
///                    void *userData, const char *filename);
/// ```
///
/// Returns -1 in case of error, 0 otherwise. INTENTIONAL (offline): a real
/// listing requires a data connection and server data, so the documented
/// error return (-1) is produced.
#[no_mangle]
pub unsafe extern "C" fn xmlNanoFTPList(
    ctx: *mut c_void,
    callback: FtpListCallback,
    userData: *mut c_void,
    filename: *const c_char,
) -> c_int {
    if ctx.is_null() {
        return -1;
    }
    let _ = (callback, userData, filename);
    -1
}

/// Initiate fetch of the given file from the server.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// SOCKET xmlNanoFTPGetSocket(void *ctx, const char *filename);
/// ```
///
/// Returns the socket for the data connection, or <0 in case of error.
/// Upstream opens the data channel then drives `TYPE I` + `RETR`
/// handshakes; both need server replies, so the fetch can never be
/// initiated offline → INVALID_SOCKET.
#[no_mangle]
pub unsafe extern "C" fn xmlNanoFTPGetSocket(ctx: *mut c_void, filename: *const c_char) -> c_int {
    if ctx.is_null() {
        return INVALID_SOCKET;
    }
    let reg = FTP_CTXTS.lock();
    let st = match reg.get(&(ctx as usize)) {
        Some(st) => st,
        None => return INVALID_SOCKET,
    };
    if filename.is_null() && st.path.is_none() {
        return INVALID_SOCKET;
    }
    INVALID_SOCKET
}

/// Fetch the given file from the server; data goes to the callback.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNanoFTPGet(void *ctx, ftpDataCallback callback,
///                   void *userData, const char *filename);
/// ```
///
/// Returns -1 in case of error, 0 otherwise. The transfer needs
/// `xmlNanoFTPGetSocket`, which can never succeed offline → -1.
#[no_mangle]
pub unsafe extern "C" fn xmlNanoFTPGet(
    ctx: *mut c_void,
    callback: FtpDataCallback,
    userData: *mut c_void,
    filename: *const c_char,
) -> c_int {
    if ctx.is_null() {
        return -1;
    }
    {
        let reg = FTP_CTXTS.lock();
        let st = match reg.get(&(ctx as usize)) {
            Some(st) => st,
            None => return -1,
        };
        if filename.is_null() && st.path.is_none() {
            return -1;
        }
    }
    if callback.is_none() {
        return -1;
    }
    if xmlNanoFTPGetSocket(ctx, filename) == INVALID_SOCKET {
        return -1;
    }
    let _ = userData;
    -1
}

/// Read @len bytes from the existing FTP data connection.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNanoFTPRead(void *ctx, void *dest, int len);
/// ```
///
/// Returns the number of bytes read. 0 indicates end of connection, -1 a
/// parameter error. The fake data channel reports EOF immediately, so after
/// the parameter checks this returns 0 and closes the data connection,
/// exactly as upstream does at end-of-connection.
#[no_mangle]
pub unsafe extern "C" fn xmlNanoFTPRead(ctx: *mut c_void, dest: *mut c_void, len: c_int) -> c_int {
    if ctx.is_null() {
        return -1;
    }
    let mut reg = FTP_CTXTS.lock();
    let st = match reg.get_mut(&(ctx as usize)) {
        Some(st) => st,
        None => return -1,
    };
    if st.data_fd == INVALID_SOCKET {
        return 0;
    }
    if dest.is_null() {
        return -1;
    }
    if len <= 0 {
        return 0;
    }
    // Simulated recv(): EOF with zero bytes; upstream then closes the data
    // connection and returns 0.
    st.data_fd = INVALID_SOCKET;
    0
}

// ═══════════════════════════════════════════════════════════════════════════════
// NanoHTTP — legacy HTTP client (nanohttp.h / nanohttp.c)
// ═══════════════════════════════════════════════════════════════════════════════

/// Initialize the HTTP protocol layer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlNanoHTTPInit(void);
/// ```
///
/// One-time initialization; upstream also scans `http_proxy`/`HTTP_PROXY`
/// environment variables, skipped here (proxy settings are inert offline).
#[no_mangle]
pub unsafe extern "C" fn xmlNanoHTTPInit() {
    if HTTP_INITIALIZED.load(Ordering::Relaxed) {
        return;
    }
    HTTP_INITIALIZED.store(true, Ordering::Relaxed);
}

/// Cleanup the HTTP protocol layer.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlNanoHTTPCleanup(void);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNanoHTTPCleanup() {
    HTTP_INITIALIZED.store(false, Ordering::Relaxed);
    *HTTP_PROXY.lock() = HttpProxyCfg::default();
}

/// (Re)Initialize the HTTP proxy context from a proxy URL.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlNanoHTTPScanProxy(const char *URL);
/// ```
///
/// `http://myproxy/` or `http://myproxy:3128/`; a NULL URL clears the proxy
/// info. Inert in the offline build.
#[no_mangle]
pub unsafe extern "C" fn xmlNanoHTTPScanProxy(URL: *const c_char) {
    let mut proxy = HTTP_PROXY.lock();
    *proxy = HttpProxyCfg {
        port: 0,
        ..HttpProxyCfg::default()
    };
    if URL.is_null() {
        return;
    }
    let parsed = parse_url(&unsafe { cstr_to_string(URL) }.unwrap_or_default());
    if parsed.scheme.as_deref() != Some("http") || parsed.host.is_none() {
        // Upstream raises XML_HTTP_URL_SYNTAX here; proxy stays cleared.
        return;
    }
    proxy.host = parsed.host.map(to_cstring);
    if let Some(p) = parsed.port {
        proxy.port = p;
    }
}

/// Open a connection to the indicated resource via HTTP GET.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void * xmlNanoHTTPOpen(const char *URL, char **contentType);
/// ```
///
/// Returns NULL in case of failure, otherwise a request handler. The
/// contentType is set to NULL first (upstream). Since no HTTP context can
/// be created offline, this always returns NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlNanoHTTPOpen(
    URL: *const c_char,
    contentType: *mut *mut c_char,
) -> *mut c_void {
    if !contentType.is_null() {
        unsafe { *contentType = ptr::null_mut() };
    }
    xmlNanoHTTPMethod(URL, ptr::null(), ptr::null(), contentType, ptr::null(), 0)
}

/// Open a connection to the indicated resource via HTTP GET, tracking
/// redirects.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void * xmlNanoHTTPOpenRedir(const char *URL, char **contentType, char **redir);
/// ```
///
/// Returns NULL in case of failure; `contentType`/`redir` are cleared first.
#[no_mangle]
pub unsafe extern "C" fn xmlNanoHTTPOpenRedir(
    URL: *const c_char,
    contentType: *mut *mut c_char,
    redir: *mut *mut c_char,
) -> *mut c_void {
    if !contentType.is_null() {
        unsafe { *contentType = ptr::null_mut() };
    }
    if !redir.is_null() {
        unsafe { *redir = ptr::null_mut() };
    }
    xmlNanoHTTPMethodRedir(
        URL,
        ptr::null(),
        ptr::null(),
        contentType,
        redir,
        ptr::null(),
        0,
    )
}

/// Open a connection via HTTP using the given method, headers and input.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void * xmlNanoHTTPMethod(const char *URL, const char *method,
///                          const char *input, char **contentType,
///                          const char *headers, int ilen);
/// ```
///
/// Returns NULL in case of failure (always, offline — see
/// `xmlNanoHTTPMethodRedir`).
#[no_mangle]
pub unsafe extern "C" fn xmlNanoHTTPMethod(
    URL: *const c_char,
    method: *const c_char,
    input: *const c_char,
    contentType: *mut *mut c_char,
    headers: *const c_char,
    ilen: c_int,
) -> *mut c_void {
    xmlNanoHTTPMethodRedir(
        URL,
        method,
        input,
        contentType,
        ptr::null_mut(),
        headers,
        ilen,
    )
}

/// Open a connection via HTTP using the given method, tracking redirects.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void * xmlNanoHTTPMethodRedir(const char *URL, const char *method,
///                               const char *input, char **contentType,
///                               char **redir, const char *headers, int ilen);
/// ```
///
/// Returns NULL in case of failure. Upstream allocates the context, checks
/// the scheme/host, opens a TCP connection (or proxy) and exchanges the
/// request/response headers here. This crate has no network stack (offline
/// forensic reimplementation), so the connect fails and the documented
/// failure return (NULL) is produced — never fake success. The context is
/// registered during validation and freed again before returning.
#[no_mangle]
pub unsafe extern "C" fn xmlNanoHTTPMethodRedir(
    URL: *const c_char,
    method: *const c_char,
    input: *const c_char,
    contentType: *mut *mut c_char,
    redir: *mut *mut c_char,
    headers: *const c_char,
    ilen: c_int,
) -> *mut c_void {
    let _ = (method, input, contentType, redir, headers, ilen);
    if URL.is_null() {
        return ptr::null_mut();
    }
    xmlNanoHTTPInit();

    let handle = alloc_http_handle();
    let mut st = NanoHttpState {
        port: HTTP_DEFAULT_PORT,
        fd: INVALID_SOCKET,
        content_length: -1,
        ..NanoHttpState::default()
    };
    // Upstream xmlNanoHTTPScanURL (nanohttp.c).
    let parsed = parse_url(&unsafe { cstr_to_string(URL) }.unwrap_or_default());
    st.protocol = parsed.scheme.map(to_cstring);
    st.hostname = parsed.host.map(to_cstring);
    if let Some(p) = parsed.port {
        st.port = p;
    }
    st.path = parsed.path.map(to_cstring);
    st.query = parsed.query.map(to_cstring);
    HTTP_CTXTS.lock().insert(handle as usize, st);

    let valid = {
        let reg = HTTP_CTXTS.lock();
        match reg.get(&(handle as usize)) {
            Some(st) => {
                let proto_ok = match st.protocol.as_ref() {
                    Some(p) => p.as_bytes() == "http".as_bytes(),
                    None => false,
                };
                proto_ok && st.hostname.is_some()
            }
            None => false,
        }
    };
    if !valid {
        remove_http_ctxt(handle);
        return ptr::null_mut();
    }
    // INTENTIONAL (offline): the TCP connect (to host or proxy) cannot
    // happen, so upstream's documented failure return is produced.
    remove_http_ctxt(handle);
    ptr::null_mut()
}

/// Read @len bytes from the existing HTTP connection.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNanoHTTPRead(void *ctx, void *dest, int len);
/// ```
///
/// Returns the number of bytes read; 0 is end of connection, -1 a parameter
/// error. No HTTP context can exist offline, so any real call hits the
/// NULL/unknown-context error path → -1.
#[no_mangle]
pub unsafe extern "C" fn xmlNanoHTTPRead(ctx: *mut c_void, dest: *mut c_void, len: c_int) -> c_int {
    if ctx.is_null() {
        return -1;
    }
    if dest.is_null() {
        return -1;
    }
    if len <= 0 {
        return 0;
    }
    if HTTP_CTXTS.lock().get(&(ctx as usize)).is_none() {
        return -1;
    }
    // A registered context would report end-of-connection (upstream recv()
    // at EOF returns 0); unreachable with the current offline flow.
    0
}

/// Close an HTTP context, ending the connection and freeing all data.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlNanoHTTPClose(void *ctx);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNanoHTTPClose(ctx: *mut c_void) {
    if ctx.is_null() {
        return;
    }
    remove_http_ctxt(ctx);
}

/// Get the latest HTTP return code received.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNanoHTTPReturnCode(void *ctx);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNanoHTTPReturnCode(ctx: *mut c_void) -> c_int {
    let reg = HTTP_CTXTS.lock();
    match reg.get(&(ctx as usize)) {
        Some(st) => st.return_value,
        None => -1,
    }
}

/// Get the stashed WWW-Authenticate / Proxy-Authenticate header.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const char * xmlNanoHTTPAuthHeader(void *ctx);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNanoHTTPAuthHeader(ctx: *mut c_void) -> *const c_char {
    let reg = HTTP_CTXTS.lock();
    match reg
        .get(&(ctx as usize))
        .and_then(|st| st.auth_header.as_ref())
    {
        Some(h) => h.as_ptr(),
        None => ptr::null(),
    }
}

/// The specified content length from the HTTP header (-1 if absent).
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNanoHTTPContentLength(void *ctx);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNanoHTTPContentLength(ctx: *mut c_void) -> c_int {
    let reg = HTTP_CTXTS.lock();
    match reg.get(&(ctx as usize)) {
        Some(st) => st.content_length,
        None => -1,
    }
}

/// The redirection URL from the HTTP header, or NULL.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const char * xmlNanoHTTPRedir(void *ctx);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNanoHTTPRedir(ctx: *mut c_void) -> *const c_char {
    let reg = HTTP_CTXTS.lock();
    match reg.get(&(ctx as usize)).and_then(|st| st.location.as_ref()) {
        Some(l) => l.as_ptr(),
        None => ptr::null(),
    }
}

/// The encoding specified in the HTTP headers, or NULL.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const char * xmlNanoHTTPEncoding(void *ctx);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNanoHTTPEncoding(ctx: *mut c_void) -> *const c_char {
    let reg = HTTP_CTXTS.lock();
    match reg.get(&(ctx as usize)).and_then(|st| st.encoding.as_ref()) {
        Some(e) => e.as_ptr(),
        None => ptr::null(),
    }
}

/// The Mime-Type specified in the HTTP headers, or NULL.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// const char * xmlNanoHTTPMimeType(void *ctx);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlNanoHTTPMimeType(ctx: *mut c_void) -> *const c_char {
    let reg = HTTP_CTXTS.lock();
    match reg
        .get(&(ctx as usize))
        .and_then(|st| st.mime_type.as_ref())
    {
        Some(m) => m.as_ptr(),
        None => ptr::null(),
    }
}

/// Fetch the indicated resource via HTTP GET and save it to a file.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNanoHTTPFetch(const char *URL, const char *filename, char **contentType);
/// ```
///
/// Returns -1 in case of failure, 0 in case of success. `Open` can never
/// succeed offline → -1.
#[no_mangle]
pub unsafe extern "C" fn xmlNanoHTTPFetch(
    URL: *const c_char,
    filename: *const c_char,
    contentType: *mut *mut c_char,
) -> c_int {
    if filename.is_null() {
        return -1;
    }
    let ctxt = xmlNanoHTTPOpen(URL, contentType);
    if ctxt.is_null() {
        return -1;
    }
    // Unreachable offline (Open never succeeds); upstream would stream the
    // body into `filename` here.
    xmlNanoHTTPClose(ctxt);
    -1
}

/// Save the output of the HTTP transaction to a file.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlNanoHTTPSave(void *ctxt, const char *filename);
/// ```
///
/// Returns -1 in case of failure, 0 in case of success. No HTTP context can
/// exist offline → -1.
#[no_mangle]
pub unsafe extern "C" fn xmlNanoHTTPSave(ctxt: *mut c_void, filename: *const c_char) -> c_int {
    if ctxt.is_null() || filename.is_null() {
        return -1;
    }
    if HTTP_CTXTS.lock().get(&(ctxt as usize)).is_none() {
        return -1;
    }
    // Unreachable offline: no context and no fetched content exist.
    -1
}

// ═══════════════════════════════════════════════════════════════════════════════
// xmlIO.c protocol I/O callbacks (xmlIO.h)
// ═══════════════════════════════════════════════════════════════════════════════

/// Default `http://` protocol callback: URI matcher.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlIOHTTPMatch(const char *filename);
/// ```
///
/// Returns 1 if the filename starts with `http://` (case-insensitive, like
/// upstream `xmlStrncasecmp`), 0 otherwise.
#[no_mangle]
pub unsafe extern "C" fn xmlIOHTTPMatch(filename: *const c_char) -> c_int {
    if starts_with_ci(unsafe { cstr_bytes(filename) }, b"http://") {
        1
    } else {
        0
    }
}

/// Default `http://` protocol callback: open an HTTP I/O channel.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void * xmlIOHTTPOpen(const char *filename);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlIOHTTPOpen(filename: *const c_char) -> *mut c_void {
    xmlNanoHTTPOpen(filename, ptr::null_mut())
}

/// Default `http://` protocol callback: open an HTTP I/O channel for POST.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void * xmlIOHTTPOpenW(const char *post_uri, int compression);
/// ```
///
/// Upstream 2.13+: "Support for HTTP POST has been removed. Returns NULL."
#[no_mangle]
pub const unsafe extern "C" fn xmlIOHTTPOpenW(
    post_uri: *const c_char,
    compression: c_int,
) -> *mut c_void {
    let _ = (post_uri, compression);
    ptr::null_mut()
}

/// Default `http://` protocol callback: read from the HTTP I/O channel.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlIOHTTPRead(void *context, char *buffer, int len);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlIOHTTPRead(
    context: *mut c_void,
    buffer: *mut c_char,
    len: c_int,
) -> c_int {
    if buffer.is_null() || len < 0 {
        return -1;
    }
    xmlNanoHTTPRead(context, buffer as *mut c_void, len)
}

/// Default `http://` protocol callback: close the HTTP I/O channel.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlIOHTTPClose(void *context);
/// ```
///
/// Returns 0 (upstream).
#[no_mangle]
pub unsafe extern "C" fn xmlIOHTTPClose(context: *mut c_void) -> c_int {
    xmlNanoHTTPClose(context);
    0
}

/// Default `ftp://` protocol callback: URI matcher.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlIOFTPMatch(const char *filename);
/// ```
///
/// Returns 1 if the filename starts with `ftp://` (case-insensitive), 0
/// otherwise.
#[no_mangle]
pub unsafe extern "C" fn xmlIOFTPMatch(filename: *const c_char) -> c_int {
    if starts_with_ci(unsafe { cstr_bytes(filename) }, b"ftp://") {
        1
    } else {
        0
    }
}

/// Default `ftp://` protocol callback: open an FTP I/O channel.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void * xmlIOFTPOpen(const char *filename);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlIOFTPOpen(filename: *const c_char) -> *mut c_void {
    xmlNanoFTPOpen(filename)
}

/// Default `ftp://` protocol callback: read from the FTP I/O channel.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlIOFTPRead(void *context, char *buffer, int len);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlIOFTPRead(
    context: *mut c_void,
    buffer: *mut c_char,
    len: c_int,
) -> c_int {
    if buffer.is_null() || len < 0 {
        return -1;
    }
    xmlNanoFTPRead(context, buffer as *mut c_void, len)
}

/// Default `ftp://` protocol callback: close the FTP I/O channel.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlIOFTPClose(void *context);
/// ```
#[no_mangle]
pub unsafe extern "C" fn xmlIOFTPClose(context: *mut c_void) -> c_int {
    xmlNanoFTPClose(context)
}
