//! libgbm (`gbm` crate) for dmabuf screencopy targets the GPU samples in place.

use std::fs::File;
use std::os::fd::OwnedFd;
use std::rc::Rc;

use crate::dmabuf::dev_t;

pub struct Gbm {
    dev: gbm::Device<File>,
    /// The opened node's device number.
    rdev: Option<dev_t>,
}

impl Gbm {
    fn on(f: File) -> Option<Gbm> {
        let rdev = node_rdev(&f);
        gbm::Device::new(f).ok().map(|dev| Gbm { dev, rdev })
    }

    fn open_path(p: &std::path::Path) -> Option<Gbm> {
        let f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(p)
            .ok()?;
        Gbm::on(f)
    }

    /// The first DRM render node with a working gbm device.
    pub fn open() -> Option<Gbm> {
        (128..136).find_map(|n| Gbm::open_path(format!("/dev/dri/renderD{n}").as_ref()))
    }

    /// The render node of the GPU behind DRM node `dev` (linux-dmabuf feedback `main_device`).
    pub fn open_device(dev: dev_t) -> Option<Gbm> {
        Gbm::open_path(&crate::dmabuf::render_node(dev)?)
    }

    /// Whether this device is the GPU behind DRM node `dev`.
    pub fn is_device(&self, dev: dev_t) -> bool {
        self.rdev
            .is_some_and(|r| crate::dmabuf::same_device(r, dev))
    }
}

fn node_rdev(f: &File) -> Option<dev_t> {
    use std::os::unix::fs::MetadataExt;
    Some(f.metadata().ok()?.rdev() as dev_t)
}

/// A buffer object: plane 0's layout in `stride`/`offset`, every plane's in `planes`.
pub struct Bo {
    bo: gbm::BufferObject<()>,
    _gbm: Rc<Gbm>,
    pub stride: u32,
    pub offset: u32,
    /// The modifier the buffer is declared with (`dmabuf::MOD_INVALID` for implicit).
    pub modifier: u64,
    /// (offset, stride) per plane.
    pub planes: Vec<(u32, u32)>,
}

impl Bo {
    fn wrap(gbm: &Rc<Gbm>, bo: gbm::BufferObject<()>) -> Bo {
        let planes = (0..bo.plane_count() as i32)
            .map(|p| (bo.offset(p), bo.stride_for_plane(p)))
            .collect();
        Bo {
            stride: bo.stride(),
            offset: bo.offset(0),
            modifier: bo.modifier().into(),
            planes,
            bo,
            _gbm: gbm.clone(),
        }
    }

    /// A single-plane buffer in the driver's own (implicit) layout.
    pub fn new(gbm: &Rc<Gbm>, w: u32, h: u32, fourcc: u32) -> Option<Bo> {
        let format = gbm::Format::try_from(fourcc).ok()?;
        let bo = gbm
            .dev
            .create_buffer_object::<()>(w, h, format, gbm::BufferObjectFlags::RENDERING)
            .ok()?;
        (bo.plane_count() == 1).then(|| Bo::wrap(gbm, bo))
    }

    /// A buffer in one of the explicit `modifiers` (the driver picks the best it supports).
    pub fn with_modifiers(
        gbm: &Rc<Gbm>,
        w: u32,
        h: u32,
        fourcc: u32,
        modifiers: &[u64],
    ) -> Option<Bo> {
        let format = gbm::Format::try_from(fourcc).ok()?;
        let bo = gbm
            .dev
            .create_buffer_object_with_modifiers2::<()>(
                w,
                h,
                format,
                modifiers.iter().map(|&m| gbm::Modifier::from(m)),
                gbm::BufferObjectFlags::RENDERING,
            )
            .ok()?;
        let bo = Bo::wrap(gbm, bo);
        (modifiers.contains(&bo.modifier) && (1..=4).contains(&bo.planes.len())).then_some(bo)
    }

    /// A new dmabuf fd for the buffer.
    pub fn fd(&self) -> Option<OwnedFd> {
        self.bo.fd().ok()
    }

    /// A new dmabuf fd per plane.
    pub fn plane_fds(&self) -> Option<Vec<OwnedFd>> {
        (0..self.planes.len() as i32)
            .map(|p| self.bo.fd_for_plane(p).ok())
            .collect()
    }
}
