//! Equivalence of `shell::gbm` (gbm crate) with the original hand-written dlopen bindings:
//! same render node, same buffer layout (stride, offset, modifier, planes) and a usable dmabuf
//! fd for the formats and sizes screencopy offers. Ignored in the gate, which has no GPU: on
//! vgem's render node Mesa's software gbm (kms_swrast) is refused its dumb buffers
//! (`DRM_IOCTL_MODE_CREATE_DUMB`: Permission denied), so no format allocates there.

#![allow(dead_code)]

use std::ffi::{CStr, c_void};
use std::os::fd::{FromRawFd, OwnedFd};

type BoCreate = unsafe extern "C" fn(*mut c_void, u32, u32, u32, u32) -> *mut c_void;
type BoPtr = unsafe extern "C" fn(*mut c_void);
type BoI32 = unsafe extern "C" fn(*mut c_void) -> i32;
type BoU32 = unsafe extern "C" fn(*mut c_void) -> u32;
type BoU64 = unsafe extern "C" fn(*mut c_void) -> u64;
type BoOffset = unsafe extern "C" fn(*mut c_void, i32) -> u32;

bits::bit_flags! {
const USE_RENDERING: u32 = 1 << 2;
}

struct OrigGbm {
    _lib: libloading::Library,
    _fd: std::fs::File,
    dev: *mut c_void,
    destroy_dev: BoPtr,
    bo_create: BoCreate,
    bo_destroy: BoPtr,
    bo_fd: BoI32,
    bo_planes: BoI32,
    bo_stride: BoU32,
    bo_offset: BoOffset,
    bo_modifier: BoU64,
}

/// `name` from `lib` as `T`.
///
/// # Safety
/// `T` must be the C signature gbm.h gives `name`.
unsafe fn sym<T: Copy>(lib: &libloading::Library, name: &CStr) -> Option<T> {
    // SAFETY: the caller guarantees `T` is `name`'s type.
    unsafe { lib.get::<T>(name) }.ok().map(|s| *s)
}

impl OrigGbm {
    /// The first DRM render node with a working gbm device.
    pub fn open() -> Option<OrigGbm> {
        // SAFETY: libgbm's initializers have no preconditions.
        let lib = unsafe { libloading::Library::new("libgbm.so.1") }.ok()?;
        type Create = unsafe extern "C" fn(i32) -> *mut c_void;
        // SAFETY: gbm.h: `struct gbm_device *gbm_create_device(int fd)`.
        let create = unsafe { sym::<Create>(&lib, c"gbm_create_device") }?;
        // SAFETY: gbm.h: `void gbm_device_destroy(struct gbm_device *gbm)`.
        let destroy_dev = unsafe { sym::<BoPtr>(&lib, c"gbm_device_destroy") }?;
        // SAFETY: gbm.h: `struct gbm_bo *gbm_bo_create(struct gbm_device *, uint32_t width,
        // uint32_t height, uint32_t format, uint32_t flags)`.
        let bo_create = unsafe { sym::<BoCreate>(&lib, c"gbm_bo_create") }?;
        // SAFETY: gbm.h: `void gbm_bo_destroy(struct gbm_bo *bo)`.
        let bo_destroy = unsafe { sym::<BoPtr>(&lib, c"gbm_bo_destroy") }?;
        // SAFETY: gbm.h: `int gbm_bo_get_fd(struct gbm_bo *bo)`.
        let bo_fd = unsafe { sym::<BoI32>(&lib, c"gbm_bo_get_fd") }?;
        // SAFETY: gbm.h: `int gbm_bo_get_plane_count(struct gbm_bo *bo)`.
        let bo_planes = unsafe { sym::<BoI32>(&lib, c"gbm_bo_get_plane_count") }?;
        // SAFETY: gbm.h: `uint32_t gbm_bo_get_stride(struct gbm_bo *bo)`.
        let bo_stride = unsafe { sym::<BoU32>(&lib, c"gbm_bo_get_stride") }?;
        // SAFETY: gbm.h: `uint32_t gbm_bo_get_offset(struct gbm_bo *bo, int plane)`.
        let bo_offset = unsafe { sym::<BoOffset>(&lib, c"gbm_bo_get_offset") }?;
        // SAFETY: gbm.h: `uint64_t gbm_bo_get_modifier(struct gbm_bo *bo)`.
        let bo_modifier = unsafe { sym::<BoU64>(&lib, c"gbm_bo_get_modifier") }?;
        for n in 128..136 {
            let Ok(f) = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(format!("/dev/dri/renderD{n}"))
            else {
                continue;
            };
            // SAFETY: an open DRM node fd, kept open by `_fd` for the device's lifetime.
            let dev = unsafe { create(std::os::fd::AsRawFd::as_raw_fd(&f)) };
            if dev.is_null() {
                continue;
            }
            return Some(OrigGbm {
                _lib: lib,
                _fd: f,
                dev,
                destroy_dev,
                bo_create,
                bo_destroy,
                bo_fd,
                bo_planes,
                bo_stride,
                bo_offset,
                bo_modifier,
            });
        }
        None
    }
}

impl Drop for OrigGbm {
    fn drop(&mut self) {
        // SAFETY: `dev` is the live device `open` created; no bo of it outlives `orig_bo`.
        unsafe { (self.destroy_dev)(self.dev) }
    }
}

/// The original `Bo::new` + `Bo::fd`: (stride, offset, modifier, fd ok) for a single-plane bo.
fn orig_bo(g: &OrigGbm, w: u32, h: u32, fourcc: u32) -> Option<(u32, u32, u64, bool)> {
    // SAFETY: `g.dev` is a live gbm device.
    let bo = unsafe { (g.bo_create)(g.dev, w, h, fourcc, USE_RENDERING) };
    if bo.is_null() {
        return None;
    }
    // SAFETY: `bo` is a live bo of `g.dev` until the destroy below (as for every call here).
    let r = (unsafe { (g.bo_planes)(bo) } == 1).then(|| {
        // SAFETY: as above.
        let fd = unsafe { (g.bo_fd)(bo) };
        if fd >= 0 {
            // SAFETY: gbm_bo_get_fd returns a new dmabuf fd the caller owns.
            drop(unsafe { OwnedFd::from_raw_fd(fd) });
        }
        // SAFETY: as above.
        let stride = unsafe { (g.bo_stride)(bo) };
        // SAFETY: as above; plane 0 exists.
        let offset = unsafe { (g.bo_offset)(bo, 0) };
        // SAFETY: as above.
        let modifier = unsafe { (g.bo_modifier)(bo) };
        (stride, offset, modifier, fd >= 0)
    });
    // SAFETY: as above; `bo` is not used after this.
    unsafe { (g.bo_destroy)(bo) };
    r
}

fn fourcc(s: &[u8; 4]) -> u32 {
    u32::from_le_bytes(*s)
}

#[test]
#[ignore = "needs a GPU's DRM render node (/dev/dri/renderD*): a container has none, and on \
            a software one (vgem) gbm allocates no buffer; run with --run-ignored on a machine \
            with a GPU"]
fn matches_original_bindings() {
    let Some(orig) = OrigGbm::open() else {
        testenv::unavailable("a DRM render node with gbm (/dev/dri/renderD128..135)");
        return;
    };
    let new =
        std::rc::Rc::new(shell::gbm::Gbm::open().expect("crate opens a node the original opened"));
    let formats = [
        b"XR24", b"AR24", b"XB24", b"AB24", b"AR30", b"XR30", b"RG16",
    ];
    let sizes = [
        (1, 1),
        (64, 64),
        (300, 200),
        (1366, 768),
        (1920, 1080),
        (2560, 1440),
        (3840, 2160),
        (7, 13),
    ];
    let mut compared = 0;
    for f in formats {
        for (w, h) in sizes {
            let a = orig_bo(&orig, w, h, fourcc(f));
            let b = shell::gbm::Bo::new(&new, w, h, fourcc(f)).map(|bo| {
                let fd = bo.fd();
                (bo.stride, bo.offset, bo.modifier, fd.is_some())
            });
            eprintln!(
                "{} {w}x{h}: original {a:?} crate {b:?}",
                std::str::from_utf8(f).unwrap()
            );
            assert_eq!(a, b, "{} {w}x{h}", std::str::from_utf8(f).unwrap());
            compared += a.is_some() as usize;
        }
    }
    assert!(compared > 0, "no format allocated on this device");
}
