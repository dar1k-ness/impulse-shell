//! linux-dmabuf-v1 parameters for capture buffers: `zwp_linux_dmabuf_feedback_v1` (v4+: main
//! device, format table, preference tranches) or the v3 `modifier` list, and the format modifiers
//! to allocate with, in the compositor's order of preference.

use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};

pub use libc::dev_t;

/// `DRM_FORMAT_MOD_INVALID`: implicit modifier (the layout is derived from the dmabuf).
pub const MOD_INVALID: u64 = 0x00ff_ffff_ffff_ffff;
/// `DRM_FORMAT_MOD_LINEAR`.
pub const MOD_LINEAR: u64 = 0;

/// A `dev_t` sent as a protocol `array` (native endianness).
pub fn dev_from_bytes(b: &[u8]) -> Option<dev_t> {
    Some(dev_t::from_ne_bytes(b.try_into().ok()?))
}

/// Why a feedback format table was not read.
#[derive(Debug, PartialEq, Eq)]
pub enum TableError {
    /// fstat on the table's fd failed (errno).
    Stat(i32),
    /// The file has fewer bytes than the compositor announced.
    Short { file: u64, announced: u32 },
    /// mmap failed (errno).
    Map(i32),
}

impl std::fmt::Display for TableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let os = |e: &i32| std::io::Error::from_raw_os_error(*e);
        match self {
            TableError::Stat(e) => write!(f, "fstat: {}", os(e)),
            TableError::Short { file, announced } => {
                write!(f, "the file has {file} bytes, {announced} announced")
            }
            TableError::Map(e) => write!(f, "mmap: {}", os(e)),
        }
    }
}

/// The (format, modifier) pairs of a feedback format table: 16-byte entries of a `u32` format,
/// 4 bytes of padding and a `u64` modifier, mapped read-only private as the protocol requires.
/// A file shorter than the `size` the compositor announced is refused: mmap maps past the end
/// of a file, and reading a page wholly past it is a SIGBUS (mmap(2)).
pub fn read_format_table(fd: &OwnedFd, size: u32) -> Result<Vec<(u32, u64)>, TableError> {
    let len = size as usize;
    if len == 0 {
        return Ok(Vec::new());
    }
    let errno = || std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
    // SAFETY: an all-zero `stat` is a valid value for fstat to fill in.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: `fd` is an open descriptor and `st` a live `stat` to write into.
    if unsafe { libc::fstat(fd.as_raw_fd(), &mut st) } != 0 {
        return Err(TableError::Stat(errno()));
    }
    let file = u64::try_from(st.st_size).unwrap_or(0);
    if file < size as u64 {
        return Err(TableError::Short {
            file,
            announced: size,
        });
    }
    // SAFETY: a fresh private read-only mapping of `len` bytes of `fd`, unmapped below.
    let p = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            len,
            libc::PROT_READ,
            libc::MAP_PRIVATE,
            fd.as_raw_fd(),
            0,
        )
    };
    if p == libc::MAP_FAILED {
        return Err(TableError::Map(errno()));
    }
    // SAFETY: `p` maps `len` bytes until the munmap below, all of them inside the file as
    // fstat measured it above (at least `len` bytes), so readable. A compositor that truncates
    // the file while it is read could still fault the read: the protocol asks for no seal that
    // would rule it out.
    let bytes = unsafe { std::slice::from_raw_parts(p as *const u8, len) };
    let table = bytes
        .as_chunks::<16>()
        .0
        .iter()
        .map(|e| {
            let f = u32::from_ne_bytes(e[0..4].try_into().unwrap());
            let m = u64::from_ne_bytes(e[8..16].try_into().unwrap());
            (f, m)
        })
        .collect();
    // SAFETY: unmaps exactly the mapping made above; `bytes` is not used after this.
    unsafe { libc::munmap(p, len) };
    Ok(table)
}

/// One preference tranche: format + modifier pairs the compositor wants on `device` (`None` for
/// the v3 `modifier` list, which names no device).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tranche {
    pub device: Option<dev_t>,
    pub flags: u32,
    pub formats: Vec<(u32, u64)>,
}

/// A complete set of dmabuf parameters, tranches in descending order of preference.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Feedback {
    pub main_device: Option<dev_t>,
    pub tranches: Vec<Tranche>,
}

impl Feedback {
    /// A v3 `zwp_linux_dmabuf_v1.modifier` event.
    pub fn add_v3_modifier(&mut self, format: u32, modifier: u64) {
        if self.tranches.is_empty() {
            self.tranches.push(Tranche::default());
        }
        let t = &mut self.tranches[0];
        if !t.formats.contains(&(format, modifier)) {
            t.formats.push((format, modifier));
        }
    }

    fn on_alloc_dev(&self, t: &Tranche, on_dev: &dyn Fn(dev_t) -> bool) -> bool {
        t.device.is_none_or(on_dev)
    }

    /// Whether buffers allocated where `on_dev` is true live on the main device (always when the
    /// compositor names none).
    fn alloc_on_main(&self, on_dev: &dyn Fn(dev_t) -> bool) -> bool {
        self.main_device.is_none_or(on_dev)
    }

    /// Modifiers the compositor advertises for `fourcc` on the allocation device.
    fn advertised(&self, fourcc: u32, on_dev: &dyn Fn(dev_t) -> bool) -> Vec<u64> {
        self.tranches
            .iter()
            .filter(|t| self.on_alloc_dev(t, on_dev))
            .flat_map(|t| t.formats.iter())
            .filter(|&&(f, _)| f == fourcc)
            .map(|&(_, m)| m)
            .collect()
    }

    /// Explicit modifier lists to allocate a `fourcc` buffer with, best tranche first: the
    /// tranches aimed at the allocation device (`on_dev`), restricted to the modifiers the
    /// client can import (`usable`), each modifier once. Allocating off the main device the
    /// only safe layout is linear.
    pub fn modifier_tranches(
        &self,
        fourcc: u32,
        on_dev: &dyn Fn(dev_t) -> bool,
        usable: &[u64],
    ) -> Vec<Vec<u64>> {
        if !self.alloc_on_main(on_dev) {
            let main = |d| Some(d) == self.main_device;
            let linear = self.advertised(fourcc, &main).contains(&MOD_LINEAR)
                && usable.contains(&MOD_LINEAR);
            return if linear {
                vec![vec![MOD_LINEAR]]
            } else {
                Vec::new()
            };
        }
        let mut seen = Vec::new();
        let mut out = Vec::new();
        for t in self
            .tranches
            .iter()
            .filter(|t| self.on_alloc_dev(t, on_dev))
        {
            let mods: Vec<u64> = t
                .formats
                .iter()
                .filter(|&&(f, m)| {
                    f == fourcc && m != MOD_INVALID && usable.contains(&m) && !seen.contains(&m)
                })
                .map(|&(_, m)| m)
                .collect();
            seen.extend(&mods);
            if !mods.is_empty() {
                out.push(mods);
            }
        }
        out
    }

    /// The modifier to declare for a buffer gbm allocated with its own (implicit) layout
    /// `bo_modifier`: that modifier if advertised, else `MOD_INVALID` if implicit modifiers are,
    /// else `None` (the compositor would reject it). With nothing advertised for `fourcc` (a v3
    /// compositor that sent no list) the buffer goes as before, with gbm's modifier.
    pub fn implicit_modifier(
        &self,
        fourcc: u32,
        bo_modifier: u64,
        on_dev: &dyn Fn(dev_t) -> bool,
    ) -> Option<u64> {
        if !self.alloc_on_main(on_dev) {
            return None;
        }
        let adv = self.advertised(fourcc, on_dev);
        if adv.is_empty() || adv.contains(&bo_modifier) {
            Some(bo_modifier)
        } else if adv.contains(&MOD_INVALID) {
            Some(MOD_INVALID)
        } else {
            None
        }
    }
}

/// Collects `zwp_linux_dmabuf_feedback_v1` events; `done` yields the parameters they describe.
#[derive(Default)]
pub struct FeedbackBuilder {
    /// The last received format table: tranche indices refer to it until a new one arrives.
    table: Vec<(u32, u64)>,
    next: Feedback,
    tranche: Tranche,
}

impl FeedbackBuilder {
    pub fn format_table(&mut self, fd: &OwnedFd, size: u32) {
        self.table = read_format_table(fd, size).unwrap_or_else(|e| {
            // Tranches index this table: without it the feedback names no format and the
            // capture falls back to wl_shm.
            tracing::warn!("dmabuf: the feedback format table of {size} bytes is unreadable: {e}");
            Vec::new()
        });
    }

    pub fn main_device(&mut self, dev: &[u8]) {
        self.next.main_device = dev_from_bytes(dev);
    }

    pub fn tranche_target_device(&mut self, dev: &[u8]) {
        self.tranche.device = dev_from_bytes(dev);
    }

    pub fn tranche_flags(&mut self, flags: u32) {
        self.tranche.flags = flags;
    }

    /// 16-bit native-endian indices into the format table.
    pub fn tranche_formats(&mut self, indices: &[u8]) {
        for &i in indices.as_chunks::<2>().0 {
            let i = u16::from_ne_bytes(i) as usize;
            if let Some(&p) = self.table.get(i)
                && !self.tranche.formats.contains(&p)
            {
                self.tranche.formats.push(p);
            }
        }
    }

    pub fn tranche_done(&mut self) {
        self.next.tranches.push(std::mem::take(&mut self.tranche));
    }

    pub fn done(&mut self) -> Feedback {
        self.tranche = Tranche::default();
        std::mem::take(&mut self.next)
    }
}

/// The DRM render node of the GPU behind `dev` (any of its nodes): `/dev/dri/renderD*` listed
/// under `/sys/dev/char/<major>:<minor>/device/drm`, as libdrm's `drmGetDeviceFromDevId` does.
pub fn render_node(dev: dev_t) -> Option<PathBuf> {
    render_node_in(Path::new("/sys"), dev)
}

pub fn render_node_in(sys: &Path, dev: dev_t) -> Option<PathBuf> {
    let dir = sys.join(format!(
        "dev/char/{}:{}/device/drm",
        libc::major(dev),
        libc::minor(dev)
    ));
    let mut nodes: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.starts_with("renderD"))
        .collect();
    nodes.sort();
    nodes.first().map(|n| Path::new("/dev/dri").join(n))
}

/// Whether two DRM nodes belong to the same GPU: the protocol forbids comparing dev_t values
/// (primary and render nodes differ), so compare the devices sysfs resolves them to.
pub fn same_device(a: dev_t, b: dev_t) -> bool {
    same_device_in(Path::new("/sys"), a, b)
}

pub fn same_device_in(sys: &Path, a: dev_t, b: dev_t) -> bool {
    if a == b {
        return true;
    }
    let dev = |d: dev_t| {
        std::fs::canonicalize(sys.join(format!(
            "dev/char/{}:{}/device",
            libc::major(d),
            libc::minor(d)
        )))
        .ok()
    };
    matches!((dev(a), dev(b)), (Some(x), Some(y)) if x == y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const XR24: u32 = u32::from_le_bytes(*b"XR24");
    const AR24: u32 = u32::from_le_bytes(*b"AR24");
    const TILED: u64 = 0x0100_0000_0000_0002;
    const CCS: u64 = 0x0100_0000_0000_0004;

    fn table_fd(entries: &[(u32, u64)]) -> (OwnedFd, u32) {
        let mut f = tempfile::tempfile().unwrap();
        for &(fmt, m) in entries {
            f.write_all(&fmt.to_ne_bytes()).unwrap();
            f.write_all(&[0xaa; 4]).unwrap();
            f.write_all(&m.to_ne_bytes()).unwrap();
        }
        (OwnedFd::from(f), (entries.len() * 16) as u32)
    }

    fn idx(i: &[u16]) -> Vec<u8> {
        i.iter().flat_map(|i| i.to_ne_bytes()).collect()
    }

    fn dev(major: u32, minor: u32) -> Vec<u8> {
        libc::makedev(major, minor).to_ne_bytes().to_vec()
    }

    #[test]
    fn format_table_reads_native_pairs_skipping_padding() {
        let e = [(XR24, MOD_LINEAR), (AR24, TILED), (XR24, MOD_INVALID)];
        let (fd, size) = table_fd(&e);
        assert_eq!(read_format_table(&fd, size).unwrap(), e);
        assert_eq!(read_format_table(&fd, 0).unwrap(), []);
    }

    /// A compositor that announces more bytes than the table's file has: mmap would map the
    /// pages past its end and reading them is a SIGBUS, so the table is refused, with the
    /// reason, and the feedback built from it names no format.
    #[test]
    fn format_table_shorter_than_announced_is_refused() {
        let (fd, size) = table_fd(&[(XR24, TILED)]);
        assert_eq!(size, 16);
        let announced = 3 * 4096;
        assert_eq!(
            read_format_table(&fd, announced),
            Err(TableError::Short {
                file: 16,
                announced
            })
        );
        assert_eq!(
            TableError::Short {
                file: 16,
                announced
            }
            .to_string(),
            "the file has 16 bytes, 12288 announced"
        );
        let mut b = FeedbackBuilder::default();
        b.format_table(&fd, announced);
        assert_eq!(b.table, []);
        // One byte short within the last page is refused the same way.
        assert_eq!(
            read_format_table(&fd, 17),
            Err(TableError::Short {
                file: 16,
                announced: 17
            })
        );
    }

    #[test]
    fn feedback_events_build_tranches_in_order() {
        let (fd, size) = table_fd(&[(XR24, MOD_LINEAR), (XR24, TILED), (AR24, TILED)]);
        let mut b = FeedbackBuilder::default();
        b.format_table(&fd, size);
        b.main_device(&dev(226, 128));
        b.tranche_target_device(&dev(226, 0));
        b.tranche_flags(1);
        b.tranche_formats(&idx(&[1]));
        b.tranche_done();
        b.tranche_target_device(&dev(226, 128));
        b.tranche_flags(0);
        b.tranche_formats(&idx(&[1, 0]));
        b.tranche_formats(&idx(&[2, 9]));
        b.tranche_done();
        let fb = b.done();
        assert_eq!(fb.main_device, Some(libc::makedev(226, 128)));
        assert_eq!(
            fb.tranches,
            [
                Tranche {
                    device: Some(libc::makedev(226, 0)),
                    flags: 1,
                    formats: vec![(XR24, TILED)],
                },
                Tranche {
                    device: Some(libc::makedev(226, 128)),
                    flags: 0,
                    formats: vec![(XR24, TILED), (XR24, MOD_LINEAR), (AR24, TILED)],
                },
            ]
        );
        // A re-sent set replaces the old one; the table stays until a new one arrives.
        b.main_device(&dev(226, 129));
        b.tranche_target_device(&dev(226, 129));
        b.tranche_formats(&idx(&[0]));
        b.tranche_done();
        let fb = b.done();
        assert_eq!(fb.tranches.len(), 1);
        assert_eq!(fb.tranches[0].formats, [(XR24, MOD_LINEAR)]);
    }

    #[test]
    fn modifiers_follow_tranche_preference_on_the_allocation_device() {
        let main = libc::makedev(226, 128);
        let scanout = libc::makedev(226, 129);
        let fb = Feedback {
            main_device: Some(main),
            tranches: vec![
                Tranche {
                    device: Some(scanout),
                    flags: 1,
                    formats: vec![(XR24, CCS)],
                },
                Tranche {
                    device: Some(main),
                    flags: 0,
                    formats: vec![(XR24, TILED), (AR24, TILED)],
                },
                Tranche {
                    device: Some(main),
                    flags: 0,
                    formats: vec![(XR24, TILED), (XR24, MOD_LINEAR), (XR24, MOD_INVALID)],
                },
            ],
        };
        let on_main = |d| d == main;
        let all = [MOD_LINEAR, TILED, CCS];
        assert_eq!(
            fb.modifier_tranches(XR24, &on_main, &all),
            [vec![TILED], vec![MOD_LINEAR]]
        );
        assert_eq!(
            fb.modifier_tranches(XR24, &on_main, &[MOD_LINEAR]),
            [vec![MOD_LINEAR]]
        );
        assert!(fb.modifier_tranches(XR24, &on_main, &[]).is_empty());
        assert!(
            fb.modifier_tranches(u32::from_le_bytes(*b"AB24"), &on_main, &all)
                .is_empty()
        );
        assert_eq!(fb.implicit_modifier(XR24, TILED, &on_main), Some(TILED));
        assert_eq!(fb.implicit_modifier(XR24, CCS, &on_main), Some(MOD_INVALID));
        assert_eq!(fb.implicit_modifier(AR24, MOD_LINEAR, &on_main), None);
        // Off the main device: linear only, never implicit.
        let other = |d| d == libc::makedev(226, 130);
        assert_eq!(fb.modifier_tranches(XR24, &other, &all), [vec![MOD_LINEAR]]);
        assert!(fb.modifier_tranches(AR24, &other, &all).is_empty());
        assert_eq!(fb.implicit_modifier(XR24, MOD_LINEAR, &other), None);
    }

    #[test]
    fn v3_modifier_list_has_no_device() {
        let mut fb = Feedback::default();
        assert_eq!(fb.implicit_modifier(XR24, TILED, &|_| false), Some(TILED));
        fb.add_v3_modifier(XR24, MOD_LINEAR);
        fb.add_v3_modifier(XR24, TILED);
        fb.add_v3_modifier(XR24, MOD_LINEAR);
        let any = |_| false;
        assert_eq!(
            fb.modifier_tranches(XR24, &any, &[TILED, MOD_LINEAR]),
            [vec![MOD_LINEAR, TILED]]
        );
        assert_eq!(fb.implicit_modifier(XR24, CCS, &any), None);
        assert_eq!(fb.implicit_modifier(AR24, CCS, &any), Some(CCS));
    }

    #[test]
    fn render_node_and_device_identity_from_sysfs() {
        let sys = tempfile::tempdir().unwrap();
        let s = sys.path();
        let gpu0 = s.join("devices/pci0000:00/0000:00:02.0");
        let gpu1 = s.join("devices/pci0000:00/0000:01:00.0");
        for (g, card, render) in [
            (&gpu0, "card0", "renderD128"),
            (&gpu1, "card1", "renderD129"),
        ] {
            std::fs::create_dir_all(g.join("drm").join(card)).unwrap();
            std::fs::create_dir_all(g.join("drm").join(render)).unwrap();
        }
        std::fs::create_dir_all(s.join("dev/char")).unwrap();
        for (node, g, name) in [
            ("226:0", &gpu0, "card0"),
            ("226:128", &gpu0, "renderD128"),
            ("226:1", &gpu1, "card1"),
            ("226:129", &gpu1, "renderD129"),
        ] {
            let d = g.join("drm").join(name);
            std::os::unix::fs::symlink(g, d.join("device")).unwrap();
            std::os::unix::fs::symlink(&d, s.join("dev/char").join(node)).unwrap();
        }
        let mk = libc::makedev;
        assert_eq!(
            render_node_in(s, mk(226, 1)),
            Some("/dev/dri/renderD129".into())
        );
        assert_eq!(
            render_node_in(s, mk(226, 128)),
            Some("/dev/dri/renderD128".into())
        );
        assert_eq!(render_node_in(s, mk(226, 7)), None);
        assert!(same_device_in(s, mk(226, 0), mk(226, 128)));
        assert!(same_device_in(s, mk(226, 129), mk(226, 129)));
        assert!(!same_device_in(s, mk(226, 0), mk(226, 129)));
        assert!(!same_device_in(s, mk(226, 7), mk(226, 8)));
    }
}
