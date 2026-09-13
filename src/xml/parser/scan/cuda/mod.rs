//! §16.9 — optional CUDA structural accelerator.
//!
//! The GPU backend is compiled only under the `cuda` feature and is **never**
//! linked at build time: the CUDA **driver API** (`libcuda.so.1`) is resolved
//! dynamically with `dlopen`/`dlsym`, so a normal libxml-rs build requires no
//! NVIDIA hardware, no toolkit, no `nvcc`, and no proprietary runtime
//! (§16.9.1). The device code is a small PTX blob (`struct_scan.ptx`, generated
//! from `struct_scan.cu` — see `PROVENANCE.md`) embedded as a string and JIT'd
//! by the installed driver.
//!
//! # What runs on the GPU (§16.9.2)
//!
//! Only Stage-1, massively data-parallel structural classification: one warp
//! per `COARSE` (4 KiB) block, producing the per-block **first terminator per
//! content class** (the exact CPU `StructIndex` table) plus per-block counts of
//! text terminators, non-ASCII bytes, line breaks and quote bytes. The compact
//! structural representation is returned to CPU Stage-2 (`run_len_indexed`),
//! which is unchanged. No DOM node, pointer, callback, or ABI structure is
//! built on the device.
//!
//! # Fail-closed (§16.9.6)
//!
//! Every entry point returns `Option`; any driver/init/allocation/launch error
//! yields `None` and the caller falls back to the CPU scanner. An XML parse
//! must never fail because CUDA failed.

use core::ffi::{c_char, c_void, CStr};
use std::sync::OnceLock;

use parking_lot::Mutex;

/// Coarse block size; must match `parallel::COARSE` and `struct_scan.cu`.
pub(crate) const COARSE: usize = 4096;

/// Embedded device code (PTX for `compute_80`, forward-JIT'd by the driver).
const PTX: &str = include_str!("struct_scan.ptx");

/// Kernel entry name.
const KERNEL: &[u8] = b"struct_scan\0";

// ── CUDA driver API types (declared locally; no cuda.h dependency) ──────────

type CUresult = i32;
type CUdevice = i32;
type CUcontext = *mut c_void;
type CUmodule = *mut c_void;
type CUfunction = *mut c_void;
type CUdeviceptr = u64;
type CUstream = *mut c_void;

const CUDA_SUCCESS: CUresult = 0;
const CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR: i32 = 75;
const CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MINOR: i32 = 76;
const CU_DEVICE_ATTRIBUTE_MULTIPROCESSOR_COUNT: i32 = 16;
/// `CU_MEMHOSTALLOC_PORTABLE | CU_MEMHOSTALLOC_WRITECOMBINED`
pub(crate) const HOST_ALLOC_WRITE_COMBINED: u32 = 0x04;
pub(crate) const HOST_ALLOC_PORTABLE: u32 = 0x01;

type FnInit = unsafe extern "C" fn(u32) -> CUresult;
type FnDeviceGet = unsafe extern "C" fn(*mut CUdevice, i32) -> CUresult;
type FnDeviceGetAttribute = unsafe extern "C" fn(*mut i32, i32, CUdevice) -> CUresult;
type FnPrimaryCtxRetain = unsafe extern "C" fn(*mut CUcontext, CUdevice) -> CUresult;
type FnCtxSetCurrent = unsafe extern "C" fn(CUcontext) -> CUresult;
type FnDeviceGetName = unsafe extern "C" fn(*mut c_char, i32, CUdevice) -> CUresult;
type FnModuleLoadData = unsafe extern "C" fn(*mut CUmodule, *const c_void) -> CUresult;
type FnModuleGetFunction =
    unsafe extern "C" fn(*mut CUfunction, CUmodule, *const c_char) -> CUresult;
type FnMemAlloc = unsafe extern "C" fn(*mut CUdeviceptr, usize) -> CUresult;
type FnMemFree = unsafe extern "C" fn(CUdeviceptr) -> CUresult;
type FnMemcpyHtoD = unsafe extern "C" fn(CUdeviceptr, *const c_void, usize) -> CUresult;
type FnMemcpyDtoH = unsafe extern "C" fn(*mut c_void, CUdeviceptr, usize) -> CUresult;
type FnLaunchKernel = unsafe extern "C" fn(
    CUfunction,
    u32,
    u32,
    u32,
    u32,
    u32,
    u32,
    u32,
    CUstream,
    *mut *mut c_void,
    *mut *mut c_void,
) -> CUresult;
type FnCtxSynchronize = unsafe extern "C" fn() -> CUresult;
type FnGetErrorString = unsafe extern "C" fn(CUresult, *mut *const c_char) -> CUresult;
type FnMemHostAlloc = unsafe extern "C" fn(*mut *mut c_void, usize, u32) -> CUresult;
type FnMemFreeHost = unsafe extern "C" fn(*mut c_void) -> CUresult;

/// Resolved driver-API function table.
struct Api {
    init: FnInit,
    device_get: FnDeviceGet,
    device_get_attribute: FnDeviceGetAttribute,
    device_get_name: FnDeviceGetName,
    primary_ctx_retain: FnPrimaryCtxRetain,
    ctx_set_current: FnCtxSetCurrent,
    module_load_data: FnModuleLoadData,
    module_get_function: FnModuleGetFunction,
    mem_alloc: FnMemAlloc,
    mem_free: FnMemFree,
    memcpy_htod: FnMemcpyHtoD,
    memcpy_dtoh: FnMemcpyDtoH,
    launch_kernel: FnLaunchKernel,
    ctx_synchronize: FnCtxSynchronize,
    get_error_string: FnGetErrorString,
    mem_host_alloc: FnMemHostAlloc,
    mem_free_host: FnMemFreeHost,
    /// Kept open for the process lifetime.
    _lib: *mut c_void,
}

impl Api {
    fn error(&self, code: CUresult) -> String {
        let mut p: *const c_char = core::ptr::null();
        let s = unsafe {
            if (self.get_error_string)(code, &mut p) == CUDA_SUCCESS && !p.is_null() {
                CStr::from_ptr(p).to_string_lossy().into_owned()
            } else {
                format!("cuda error {code}")
            }
        };
        s
    }
}

unsafe fn load_api() -> Option<Api> {
    unsafe {
        let lib = libc::dlopen(c"libcuda.so.1".as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL);
        if lib.is_null() {
            return None;
        }
        macro_rules! sym {
            ($name:expr, $t:ty) => {{
                let p = libc::dlsym(lib, $name.as_ptr());
                if p.is_null() {
                    libc::dlclose(lib);
                    return None;
                }
                core::mem::transmute::<*mut c_void, $t>(p)
            }};
        }
        Some(Api {
            init: sym!(c"cuInit", FnInit),
            device_get: sym!(c"cuDeviceGet", FnDeviceGet),
            device_get_attribute: sym!(c"cuDeviceGetAttribute", FnDeviceGetAttribute),
            device_get_name: sym!(c"cuDeviceGetName", FnDeviceGetName),
            primary_ctx_retain: sym!(c"cuDevicePrimaryCtxRetain", FnPrimaryCtxRetain),
            ctx_set_current: sym!(c"cuCtxSetCurrent", FnCtxSetCurrent),
            module_load_data: sym!(c"cuModuleLoadData", FnModuleLoadData),
            module_get_function: sym!(c"cuModuleGetFunction", FnModuleGetFunction),
            mem_alloc: sym!(c"cuMemAlloc_v2", FnMemAlloc),
            mem_free: sym!(c"cuMemFree_v2", FnMemFree),
            memcpy_htod: sym!(c"cuMemcpyHtoD_v2", FnMemcpyHtoD),
            memcpy_dtoh: sym!(c"cuMemcpyDtoH_v2", FnMemcpyDtoH),
            launch_kernel: sym!(c"cuLaunchKernel", FnLaunchKernel),
            ctx_synchronize: sym!(c"cuCtxSynchronize", FnCtxSynchronize),
            get_error_string: sym!(c"cuGetErrorString", FnGetErrorString),
            mem_host_alloc: sym!(c"cuMemHostAlloc", FnMemHostAlloc),
            mem_free_host: sym!(c"cuMemFreeHost", FnMemFreeHost),
            _lib: lib,
        })
    }
}

/// A per-process device context, module and reusable device buffers.
pub(crate) struct State {
    api: Api,
    func: CUfunction,
    ctx: CUcontext,
    pub(crate) device_name: String,
    pub(crate) compute_capability: (i32, i32),
    pub(crate) sm_count: i32,
    // Device buffers, grown on demand.
    data_buf: CUdeviceptr,
    data_cap: usize,
    ends_buf: CUdeviceptr,
    ends_cap: usize,
    first_buf: CUdeviceptr,
    first_cap: usize,
    stats_buf: CUdeviceptr,
    stats_cap: usize,
}

static STATE: OnceLock<Option<Mutex<State>>> = OnceLock::new();

// SAFETY: `State` owns CUDA handles and device pointers that are only ever
// touched while its `Mutex` is held and the primary context is current on the
// calling thread; the driver objects are process-global and not tied to the
// creating thread.
unsafe impl Send for State {}

/// Grow a device buffer to at least `need` bytes (freeing the old one).
/// Returns `CUDA_SUCCESS` or the failing `cuMemAlloc` code.
unsafe fn ensure_buf(api: &Api, buf: &mut CUdeviceptr, cap: &mut usize, need: usize) -> CUresult {
    if need <= *cap {
        return CUDA_SUCCESS;
    }
    unsafe {
        if *buf != 0 {
            (api.mem_free)(*buf);
        }
        // Round up to avoid reallocating on every small size change.
        let newcap = need.next_power_of_two().max(4096);
        let mut p: CUdeviceptr = 0;
        let code = (api.mem_alloc)(&mut p, newcap);
        if code != CUDA_SUCCESS {
            *buf = 0;
            *cap = 0;
            return code;
        }
        *buf = p;
        *cap = newcap;
    }
    CUDA_SUCCESS
}

fn debug() -> bool {
    std::env::var_os("LIBXML_RS_ACCEL_DEBUG").is_some()
}

fn init() -> Option<Mutex<State>> {
    unsafe {
        let api = load_api()?;
        let code = (api.init)(0);
        if code != CUDA_SUCCESS {
            if debug() {
                eprintln!("[cuda] cuInit: {}", api.error(code));
            }
            return None;
        }
        let mut dev: CUdevice = 0;
        if (api.device_get)(&mut dev, 0) != CUDA_SUCCESS {
            return None;
        }
        let mut ctx: CUcontext = core::ptr::null_mut();
        if (api.primary_ctx_retain)(&mut ctx, dev) != CUDA_SUCCESS {
            return None;
        }
        if (api.ctx_set_current)(ctx) != CUDA_SUCCESS {
            return None;
        }
        let mut major = 0;
        let mut minor = 0;
        let mut sms = 0;
        let _ = (api.device_get_attribute)(
            &mut major,
            CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MAJOR,
            dev,
        );
        let _ = (api.device_get_attribute)(
            &mut minor,
            CU_DEVICE_ATTRIBUTE_COMPUTE_CAPABILITY_MINOR,
            dev,
        );
        let _ = (api.device_get_attribute)(&mut sms, CU_DEVICE_ATTRIBUTE_MULTIPROCESSOR_COUNT, dev);
        let mut namebuf = [0i8; 128];
        let _ = (api.device_get_name)(namebuf.as_mut_ptr(), namebuf.len() as i32, dev);
        let device_name = unsafe { CStr::from_ptr(namebuf.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        // cuModuleLoadData wants a NUL-terminated image.
        let mut image = Vec::with_capacity(PTX.len() + 1);
        image.extend_from_slice(PTX.as_bytes());
        image.push(0);
        let mut module: CUmodule = core::ptr::null_mut();
        let code = (api.module_load_data)(&mut module, image.as_ptr() as *const c_void);
        if code != CUDA_SUCCESS {
            if debug() {
                eprintln!("[cuda] cuModuleLoadData: {}", api.error(code));
            }
            return None;
        }
        let mut func: CUfunction = core::ptr::null_mut();
        let code = (api.module_get_function)(&mut func, module, KERNEL.as_ptr() as *const c_char);
        if code != CUDA_SUCCESS {
            if debug() {
                eprintln!("[cuda] cuModuleGetFunction: {}", api.error(code));
            }
            return None;
        }
        Some(Mutex::new(State {
            api,
            func,
            ctx,
            device_name,
            compute_capability: (major, minor),
            sm_count: sms,
            data_buf: 0,
            data_cap: 0,
            ends_buf: 0,
            ends_cap: 0,
            first_buf: 0,
            first_cap: 0,
            stats_buf: 0,
            stats_cap: 0,
        }))
    }
}

/// `LIBXML_RS_ACCEL` policy (§16.9.5): which Stage-1 classifier to prefer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Accel {
    /// Never use the GPU (the ordinary CPU build).
    Cpu,
    /// Force the GPU for every non-empty eligible input.
    Cuda,
    /// Use the GPU only above the measured crossover (default).
    Auto,
}

fn parse_accel() -> Accel {
    match std::env::var("LIBXML_RS_ACCEL") {
        Ok(v) => match v.to_ascii_lowercase().as_str() {
            "cpu" | "off" | "0" | "false" | "no" => Accel::Cpu,
            "cuda" | "gpu" | "on" | "1" | "true" | "yes" => Accel::Cuda,
            _ => Accel::Auto,
        },
        Err(_) => Accel::Auto,
    }
}

/// Resolved accel policy (env read once per process).
pub(crate) fn accel_mode() -> Accel {
    static MODE: OnceLock<Accel> = OnceLock::new();
    *MODE.get_or_init(parse_accel)
}

/// §16.9.5 measured crossover for `auto`.
///
/// This is set from the §16.9.4/§16.9.5 transfer-inclusive matrix: `auto`
/// selects CUDA only at or above the size where the full cost (host staging,
/// H2D, kernel, D2H, reconciliation) beats the CPU prepass. `usize::MAX` means
/// the device never won and `auto` stays on the CPU — a valid result the spec
/// explicitly allows. `LIBXML_RS_ACCEL=cuda` still forces the GPU for
/// diagnosis/measurement.
pub(crate) const AUTO_THRESHOLD_BYTES: usize = usize::MAX;

/// Whether the GPU should classify a span of `len` bytes under the resolved
/// policy. Deliberately checks the size BEFORE touching the device, so the
/// default (`auto`, CPU-winning) build never initializes CUDA.
pub(crate) fn select(len: usize) -> bool {
    match accel_mode() {
        Accel::Cpu => false,
        Accel::Cuda => len > 0 && available(),
        Accel::Auto => len >= AUTO_THRESHOLD_BYTES && available(),
    }
}

/// `Some` when a usable CUDA device and module are available.
pub(crate) fn state() -> Option<&'static Mutex<State>> {
    STATE
        .get_or_init(|| {
            let s = init();
            if debug() {
                match &s {
                    Some(_) => eprintln!("[cuda] initialized"),
                    None => eprintln!("[cuda] unavailable; CPU fallback"),
                }
            }
            s
        })
        .as_ref()
}

/// Whether the CUDA accelerator can be used at all.
pub(crate) fn available() -> bool {
    state().is_some()
}

/// Device descriptor (name, compute capability, SM count) for receipts and
/// diagnostics; `None` when CUDA is unavailable.
pub(crate) fn device_info() -> Option<(String, (i32, i32), i32)> {
    let st = state()?;
    let g = st.lock();
    Some((g.device_name.clone(), g.compute_capability, g.sm_count))
}

/// The compact per-block structural output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CudaStruct {
    /// `blocks[i][c]` = block-local first terminator of class `c`, or `u16::MAX`.
    pub blocks: Vec<[u16; 4]>,
    /// Per-block counts: text terminators, non-ASCII, line breaks, quote bytes.
    pub stats: Vec<[u32; 4]>,
    /// Device wall time of `struct_index` including transfer, in nanoseconds.
    pub wall_ns: u64,
    /// Kernel-only time in nanoseconds (diagnostic).
    pub kernel_ns: u64,
}

impl State {
    /// Allocate page-locked host memory for the §16.9.4 transfer bench.
    fn host_alloc(&self, bytes: usize, write_combined: bool) -> Option<*mut c_void> {
        let flags = HOST_ALLOC_PORTABLE
            | if write_combined {
                HOST_ALLOC_WRITE_COMBINED
            } else {
                0
            };
        let mut p: *mut c_void = core::ptr::null_mut();
        unsafe {
            if (self.api.mem_host_alloc)(&mut p, bytes, flags) != CUDA_SUCCESS {
                return None;
            }
        }
        Some(p)
    }

    /// Free page-locked host memory.
    fn host_free(&self, p: *mut c_void) {
        if !p.is_null() {
            unsafe {
                (self.api.mem_free_host)(p);
            }
        }
    }

    /// Runs the Stage-1 kernel over one document and returns the compact
    /// structural output, or `None` on any device error (fail closed).
    pub(crate) fn index(&mut self, bytes: &[u8]) -> Option<CudaStruct> {
        self.index_offsets(&[bytes])
    }

    /// Batch form (§16.9.3): uploads every source into one COARSE-aligned device
    /// region, launches once over the union of blocks. Each source stays
    /// semantically independent (no block straddles two documents).
    pub(crate) fn index_offsets(&mut self, inputs: &[&[u8]]) -> Option<CudaStruct> {
        // SAFETY: the caller holds the process-wide `Mutex<State>`, so exactly
        // one thread touches the context, module and device buffers at a time;
        // the primary context is made current before any launch.
        unsafe { self.index_offsets_inner(inputs) }
    }

    unsafe fn index_offsets_inner(&mut self, inputs: &[&[u8]]) -> Option<CudaStruct> {
        macro_rules! fail {
            ($what:expr, $code:expr) => {{
                if debug() {
                    eprintln!("[cuda] {}: {}", $what, self.api.error($code));
                }
                return None;
            }};
            ($what:expr) => {{
                if debug() {
                    eprintln!("[cuda] {} failed", $what);
                }
                return None;
            }};
        }
        let code = (self.api.ctx_set_current)(self.ctx);
        if code != CUDA_SUCCESS {
            fail!("cuCtxSetCurrent", code);
        }
        // Staging: each input at a COARSE-aligned offset. A single input needs
        // no staging at all — it is uploaded directly (the `ends` table already
        // clamps the trailing partial block), avoiding a full-size host copy.
        let single = inputs.len() == 1;
        let mut staging: Vec<u8> = Vec::new();
        let mut nblocks = 0usize;
        if single {
            nblocks = inputs[0].len().div_ceil(COARSE);
        } else {
            for inp in inputs {
                staging.extend_from_slice(inp);
                let pad = (COARSE - (inp.len() % COARSE)) % COARSE;
                staging.resize(staging.len() + pad, 0);
                nblocks += inp.len().div_ceil(COARSE);
            }
        }
        let (upload_ptr, len): (*const u8, usize) = if single {
            (inputs[0].as_ptr(), inputs[0].len())
        } else {
            (staging.as_ptr(), staging.len())
        };
        if nblocks == 0 {
            return Some(CudaStruct {
                blocks: Vec::new(),
                stats: Vec::new(),
                wall_ns: 0,
                kernel_ns: 0,
            });
        }
        // per-block end offsets
        let mut ends = vec![0u32; nblocks];
        let mut b = 0usize;
        for inp in inputs {
            let base = b * COARSE;
            let nb = inp.len().div_ceil(COARSE);
            for k in 0..nb {
                let end = base + (k * COARSE + COARSE).min(inp.len());
                ends[b + k] = end as u32;
            }
            b += nb;
        }

        let t0 = std::time::Instant::now();
        unsafe {
            let mut acode = CUDA_SUCCESS;
            for (buf, cap, need, what) in [
                (&mut self.data_buf, &mut self.data_cap, len.max(1), "data"),
                (&mut self.ends_buf, &mut self.ends_cap, nblocks * 4, "ends"),
                (
                    &mut self.first_buf,
                    &mut self.first_cap,
                    nblocks * 8,
                    "first",
                ),
                (
                    &mut self.stats_buf,
                    &mut self.stats_cap,
                    nblocks * 16,
                    "stats",
                ),
            ] {
                acode = ensure_buf(&self.api, buf, cap, need);
                if acode != CUDA_SUCCESS {
                    if debug() {
                        eprintln!("[cuda] cuMemAlloc({what}): {}", self.api.error(acode));
                    }
                    return None;
                }
            }
            let _ = acode;
            let code = (self.api.memcpy_htod)(self.data_buf, upload_ptr as *const c_void, len);
            if code != CUDA_SUCCESS {
                fail!("cuMemcpyHtoD(data)", code);
            }
            let code =
                (self.api.memcpy_htod)(self.ends_buf, ends.as_ptr() as *const c_void, nblocks * 4);
            if code != CUDA_SUCCESS {
                fail!("cuMemcpyHtoD(ends)", code);
            }

            let threads = 256u32;
            let warps_per_cta = threads / 32;
            let grid = (nblocks as u32).div_ceil(warps_per_cta);
            let mut p_data = self.data_buf as *mut c_void;
            let mut p_ends = self.ends_buf as *mut c_void;
            let mut nblocks_arg = nblocks as u32;
            let mut p_first = self.first_buf as *mut c_void;
            let mut p_stats = self.stats_buf as *mut c_void;
            let mut params: [*mut c_void; 5] = [
                &mut p_data as *mut _ as *mut c_void,
                &mut p_ends as *mut _ as *mut c_void,
                &mut nblocks_arg as *mut _ as *mut c_void,
                &mut p_first as *mut _ as *mut c_void,
                &mut p_stats as *mut _ as *mut c_void,
            ];
            let t_k0 = std::time::Instant::now();
            let code = (self.api.launch_kernel)(
                self.func,
                grid,
                1,
                1,
                threads,
                1,
                1,
                0,
                core::ptr::null_mut(),
                params.as_mut_ptr(),
                core::ptr::null_mut(),
            );
            if code != CUDA_SUCCESS {
                fail!("cuLaunchKernel", code);
            }
            let code = (self.api.ctx_synchronize)();
            if code != CUDA_SUCCESS {
                fail!("cuCtxSynchronize", code);
            }
            let kernel_ns = t_k0.elapsed().as_nanos() as u64;

            let mut first = vec![0u16; nblocks * 4];
            let mut stats = vec![0u32; nblocks * 4];
            let code = (self.api.memcpy_dtoh)(
                first.as_mut_ptr() as *mut c_void,
                self.first_buf,
                nblocks * 8,
            );
            if code != CUDA_SUCCESS {
                fail!("cuMemcpyDtoH(first)", code);
            }
            let code = (self.api.memcpy_dtoh)(
                stats.as_mut_ptr() as *mut c_void,
                self.stats_buf,
                nblocks * 16,
            );
            if code != CUDA_SUCCESS {
                fail!("cuMemcpyDtoH(stats)", code);
            }
            let mut blocks = Vec::with_capacity(nblocks);
            let mut stats_out = Vec::with_capacity(nblocks);
            for i in 0..nblocks {
                blocks.push([
                    first[i * 4],
                    first[i * 4 + 1],
                    first[i * 4 + 2],
                    first[i * 4 + 3],
                ]);
                stats_out.push([
                    stats[i * 4],
                    stats[i * 4 + 1],
                    stats[i * 4 + 2],
                    stats[i * 4 + 3],
                ]);
            }
            Some(CudaStruct {
                blocks,
                stats: stats_out,
                wall_ns: t0.elapsed().as_nanos() as u64,
                kernel_ns,
            })
        }
    }
}

/// Single-document entry point used by the tokenizer. Returns `None` whenever
/// CUDA is unavailable or fails, so the caller falls back to the CPU scanner.
pub(crate) fn struct_index(bytes: &[u8]) -> Option<CudaStruct> {
    let st = state()?;
    let mut g = st.lock();
    g.index(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Independent scalar reference: the exact `TERM_TABLE` semantics, computed
    /// without reusing any crate code (so a shared bug cannot hide).
    fn scalar(data: &[u8]) -> (Vec<[u16; 4]>, Vec<[u32; 4]>) {
        let mut blocks = Vec::new();
        let mut stats = Vec::new();
        for chunk in data.chunks(COARSE) {
            let mut first = [u16::MAX; 4];
            let mut sp = 0u32;
            let mut na = 0u32;
            let mut lb = 0u32;
            let mut q = 0u32;
            for (i, &b) in chunk.iter().enumerate() {
                let printable = (0x20..=0x7E).contains(&b);
                let text = !(printable && b != b'<' && b != b'&' && b != b']');
                let com = !(printable && b != b'-');
                let cdr = !(printable && b != b']');
                let pi = !(printable && b != b'?');
                if text {
                    if first[0] == u16::MAX {
                        first[0] = i as u16;
                    }
                    sp += 1;
                }
                if com && first[1] == u16::MAX {
                    first[1] = i as u16;
                }
                if cdr && first[2] == u16::MAX {
                    first[2] = i as u16;
                }
                if pi && first[3] == u16::MAX {
                    first[3] = i as u16;
                }
                if b >= 0x80 {
                    na += 1;
                }
                if b == b'\n' || b == b'\r' {
                    lb += 1;
                }
                if b == b'"' || b == b'\'' {
                    q += 1;
                }
            }
            blocks.push(first);
            stats.push([sp, na, lb, q]);
        }
        (blocks, stats)
    }

    fn check(data: &[u8], ctx: &str) {
        let cs = struct_index(data).expect("device");
        let (b, s) = scalar(data);
        assert_eq!(cs.blocks, b, "blocks mismatch: {ctx} (len {})", data.len());
        assert_eq!(cs.stats, s, "stats mismatch: {ctx} (len {})", data.len());
    }

    #[test]
    fn reports_availability_without_panicking() {
        let _ = available();
    }

    /// §16.9.6 differential across the required adversarial dimensions:
    /// arbitrary offsets/alignments, block/state boundaries, malformed XML,
    /// long attributes, huge text runs, Unicode, CDATA, comments, DOCTYPE
    /// internal subsets, and entity references.
    #[test]
    fn cuda_index_matches_scalar_on_adversarial_inputs() {
        if !available() {
            eprintln!("[cuda] no device; differential test skipped");
            return;
        }
        // Deterministic xorshift64 (no thread_rng: reproducible).
        struct Xs(u64);
        impl Xs {
            fn next(&mut self) -> u64 {
                let mut x = self.0;
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                self.0 = x;
                x
            }
        }
        let mut rng = Xs(0x16_9_c0da);

        // 1. Block-boundary runs at every offset around COARSE, with every
        //    structural byte planted at the boundaries.
        for off in 0..8usize {
            for extra in [0usize, 1, 2, COARSE - 1, COARSE, COARSE + 1, 2 * COARSE + 3] {
                let mut d = vec![b'a'; off];
                d.extend(std::iter::repeat(b'x').take(extra));
                for tail in [
                    &b"<"[..],
                    &b"&"[..],
                    &b"]"[..],
                    &b"-"[..],
                    &b"?"[..],
                    &b"\n"[..],
                    &b"\r"[..],
                    &b"\x00"[..],
                    &b"\x7f"[..],
                    &b"\xc3\xa9"[..],
                    &b"\xf0\x9f\x98\x80"[..],
                ] {
                    let mut dd = d.clone();
                    dd.extend_from_slice(tail);
                    check(&dd, "boundary");
                }
            }
        }
        // 2. Huge clean runs (single and spanning many blocks).
        for n in [
            COARSE,
            COARSE + 1,
            3 * COARSE - 1,
            3 * COARSE,
            3 * COARSE + 2,
        ] {
            check(&vec![b'x'; n], "huge-run");
            let mut d = vec![b'x'; n];
            d[n - 1] = b'<';
            check(&d, "huge-run-tail");
        }
        // 3. Realistic constructs: comments, CDATA, DOCTYPE with internal
        //    subset, entities, long attributes, Unicode text, malformed.
        let constructs: &[&[u8]] = &[
            b"<r><!-- comment < & ] -- - --><a/></r>",
            b"<r><![CDATA[ x ]] ]> still ]]></r>",
            b"<!DOCTYPE r [<!ELEMENT r (#PCDATA)><!ENTITY e '<!-- c -->'>]><r>&e;</r>",
            b"<r a=\"long attribute value with - ] ? < & inside\">t</r>",
            "<r>\u{00e9}\u{20ac}\u{1F600} mixed Unicode \u{4e2d}\u{6587}</r>".as_bytes(),
            b"<r>ab\xc3\xe2\x82\xac\xf0\x9f\x98</r>",
            b"<r>]]> misplaced</r>",
            b"<r a='x' b=']' c='-' d='?'>text</r>",
            b"<!--",
            b"<r>",
            b"not xml at all < & ] - ?",
        ];
        for c in constructs {
            check(c, "construct");
            // repeat to push it across a block boundary
            let mut d = c.to_vec();
            while d.len() < 2 * COARSE + 7 {
                d.extend_from_slice(c);
            }
            check(&d, "construct-repeat");
        }
        // 4. Random adversarial buffers, all block-boundary lengths and the
        //    full byte range (this is the "one wrong bit" net).
        for iter in 0..300u64 {
            let len = match iter % 8 {
                0 => (rng.next() % 40) as usize,
                1 => COARSE - 1 + (rng.next() % 3) as usize,
                2 => 2 * COARSE + (rng.next() % 3) as usize,
                3 => 5 * COARSE - 1,
                4 => (rng.next() % 9000) as usize,
                5 => 10 * COARSE + 17,
                6 => (rng.next() % (90 * COARSE as u64)) as usize,
                _ => (rng.next() % 64) as usize,
            };
            let dist = iter % 6;
            let mut d = Vec::with_capacity(len);
            for i in 0..len {
                let b = match dist {
                    0 => (rng.next() & 0xFF) as u8,
                    1 => {
                        if (rng.next() & 0x1F) == 0 {
                            [b'<', b'&', b']', b'-', b'?', b'\n', b'\r', b'"', b'\'']
                                [(rng.next() % 9) as usize]
                        } else {
                            0x20 + (rng.next() % 0x5F) as u8
                        }
                    }
                    2 => 0x20 + (rng.next() % 0x5F) as u8,
                    3 => {
                        if i % COARSE == 0 {
                            b'<'
                        } else {
                            b'y'
                        }
                    }
                    4 => [b'<', 0xC3, 0x80, 0xE2, 0xF0, 0xFF, b'\n'][(rng.next() % 7) as usize],
                    _ => b'z',
                };
                d.push(b);
            }
            check(&d, "random");
        }
        // 5. Empty and single-byte.
        check(b"", "empty");
        check(b"<", "single");
    }

    /// §16.9.3 batching: the one-launch batch form must produce, per source,
    /// exactly the same blocks as classifying each source alone.
    #[test]
    fn cuda_batch_matches_per_document() {
        if !available() {
            return;
        }
        let docs: Vec<Vec<u8>> = vec![
            b"<a>x</a>".to_vec(),
            vec![b'c'; COARSE + 100],
            b"<r><!-- hi --><![CDATA[ ]]></r>".to_vec(),
            vec![],
            vec![b'\n'; 7],
            b"<r a='long' >text</r>".to_vec(),
        ];
        let refs: Vec<&[u8]> = docs.iter().map(|v| v.as_slice()).collect();
        // Per-document blocks via the single entry point.
        let single: Vec<Vec<[u16; 4]>> = docs
            .iter()
            .map(|d| struct_index(d).expect("device").blocks)
            .collect();
        // One batched launch.
        let st = state().expect("device");
        let mut g = st.lock();
        let batched = g.index_offsets(&refs).expect("batch");
        let mut off = 0usize;
        for (i, d) in docs.iter().enumerate() {
            let nb = d.len().div_ceil(COARSE);
            assert_eq!(
                &batched.blocks[off..off + nb],
                single[i].as_slice(),
                "doc {i} blocks"
            );
            off += nb;
        }
        assert_eq!(off, batched.blocks.len());
    }

    /// Best of `reps` runs of a device call, in nanoseconds.
    fn best_ns(reps: usize, mut f: impl FnMut() -> CUresult) -> u64 {
        let mut best = u64::MAX;
        for _ in 0..reps {
            let t = std::time::Instant::now();
            let _ = f();
            best = best.min(t.elapsed().as_nanos() as u64);
        }
        best
    }

    /// §16.9.3/§16.9.4/§16.9.5 measurement harness. Run with:
    /// `cargo test --release --features cuda -- ...cuda_transfer_bench --ignored --nocapture`
    ///
    /// Every headline number is transfer-inclusive (§16.9.4); `kernel_ns` is a
    /// diagnostic only and is never presented as the speedup.
    #[test]
    #[ignore = "§16.9 measurement harness"]
    fn cuda_transfer_bench() {
        if !available() {
            eprintln!("[cuda] no device; nothing to measure");
            return;
        }
        let (name, cc, sm) = device_info().expect("device");
        eprintln!("device={name} cc={cc:?} sm_count={sm}");
        let cfg = crate::xml::parser::scan::pool::config();
        eprintln!(
            "shape,mib,cpu_ns,gpu_wall_ns,gpu_kernel_ns,h2d_pageable_ns,h2d_pinned_ns,d2h_ns,speedup_vs_cpu"
        );

        let st = state().expect("device");
        // Warm both backends so private-pool construction and PTX JIT do not
        // pollute the timings.
        let warm = vec![b'x'; 1 << 20];
        let _ = crate::xml::parser::scan::parallel::cpu_blocks(&warm, cfg);
        {
            let mut g = st.lock();
            let _ = g.index(&warm).expect("gpu warm");
        }

        let min5 = |f: &mut dyn FnMut()| -> u64 {
            let mut best = u64::MAX;
            for _ in 0..5 {
                let t = std::time::Instant::now();
                f();
                best = best.min(t.elapsed().as_nanos() as u64);
            }
            best
        };

        for mib in [1usize, 4, 16, 64, 256] {
            let n = mib << 20;
            let mut data = vec![b'x'; n];
            let mut i = 0;
            while i < n {
                data[i] = b'<';
                i += 4096;
            }
            // CPU baseline: the §16.8 parallel prepass (best of 5).
            let cpu = crate::xml::parser::scan::parallel::cpu_blocks(&data, cfg);
            let cpu_ns = min5(&mut || {
                let _ = crate::xml::parser::scan::parallel::cpu_blocks(&data, cfg);
            });

            let mut g = st.lock();
            // GPU end to end, best of 5; verify against the CPU index each pass.
            let mut gpu_wall = u64::MAX;
            let mut kern = 0u64;
            for _ in 0..5 {
                let cs = g.index(&data).expect("gpu");
                assert_eq!(cs.blocks, cpu, "GPU/CPU index divergence at {mib} MiB");
                if cs.wall_ns < gpu_wall {
                    gpu_wall = cs.wall_ns;
                    kern = cs.kernel_ns;
                }
            }

            // Transfer components (§16.9.4), best of 5.
            let dev = g.data_buf;
            let h2d_pageable = best_ns(5, || unsafe {
                (g.api.memcpy_htod)(dev, data.as_ptr() as *const c_void, n)
            });
            let h2d_pinned = if let Some(pin) = g.host_alloc(n, false) {
                unsafe {
                    core::ptr::copy_nonoverlapping(data.as_ptr(), pin as *mut u8, n);
                }
                let v = best_ns(5, || unsafe {
                    (g.api.memcpy_htod)(dev, pin as *const c_void, n)
                });
                g.host_free(pin);
                v
            } else {
                0
            };
            let mut dst = vec![0u8; cpu.len() * 8];
            let d2h = best_ns(5, || unsafe {
                (g.api.memcpy_dtoh)(dst.as_mut_ptr() as *mut c_void, g.first_buf, cpu.len() * 8)
            });
            eprintln!(
                "single,{mib},{cpu_ns},{gpu_wall},{kern},{h2d_pageable},{h2d_pinned},{d2h},{:.3}",
                cpu_ns as f64 / gpu_wall as f64
            );
        }

        // §16.9.3 batch: many ~1 MiB documents classified in one launch.
        for ndocs in [64usize, 256] {
            let docs: Vec<Vec<u8>> = (0..ndocs)
                .map(|i| {
                    let mut d = vec![b'y'; 1 << 20];
                    d[0] = b'<';
                    d[4096] = b'-';
                    d[i % (1 << 20)] = b']';
                    d
                })
                .collect();
            let refs: Vec<&[u8]> = docs.iter().map(|d| d.as_slice()).collect();
            let cpu: Vec<Vec<[u16; 4]>> = docs
                .iter()
                .map(|d| crate::xml::parser::scan::parallel::cpu_blocks(d, cfg))
                .collect();
            let cpu_ns = min5(&mut || {
                for d in &docs {
                    let _ = crate::xml::parser::scan::parallel::cpu_blocks(d, cfg);
                }
            });
            let mut g = st.lock();
            let mut bwall = u64::MAX;
            let mut bkern = 0u64;
            for _ in 0..5 {
                let bs = g.index_offsets(&refs).expect("batch");
                let mut off = 0;
                for (i, c) in cpu.iter().enumerate() {
                    assert_eq!(
                        &bs.blocks[off..off + c.len()],
                        c.as_slice(),
                        "batch doc {i}"
                    );
                    off += c.len();
                }
                if bs.wall_ns < bwall {
                    bwall = bs.wall_ns;
                    bkern = bs.kernel_ns;
                }
            }
            eprintln!(
                "batch{ndocs},1,{cpu_ns},{bwall},{bkern},{:.3}",
                cpu_ns as f64 / bwall as f64
            );
        }
    }
}
