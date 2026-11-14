//! A Wayland proxy that can hold a client's wp_presentation_feedback events back, for the
//! order a test cannot get by timing: the client hears what became of its commits (`presented`,
//! `discarded`) only after the compositor has gone on to repaint, as a client kept off the CPU
//! does. It can also keep them from the client for good, for the surfaces of the moment: a
//! compositor answers nothing for a surface it does not draw (Hyprland for a bar under a
//! fullscreen window), which the test's compositor does not do on request. Two more things
//! Hyprland does and the test's compositor does not: it answers `discarded` for a surface it
//! composes its frames without, and it keeps a closed window's last frame on screen while it
//! animates the window away, having said on its event socket that it closed the layer
//! (`closelayer`) before it answers anything the client asked after.

use std::collections::{HashMap, HashSet};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::held_requests::{recv, send};

const WL_DISPLAY: u32 = 1;

/// One connection's objects, just enough to know which ids are wp_presentation_feedbacks and
/// which surfaces they are about.
struct Conn {
    client: UnixStream,
    registries: HashSet<u32>,
    presentations: HashSet<u32>,
    compositors: HashSet<u32>,
    surfaces: HashSet<u32>,
    /// The surfaces nothing is heard about any more ([`HeldFeedback::silence_surfaces`]).
    silent: HashSet<u32>,
    /// The surfaces that were there at [`HeldFeedback::discard_for_new_surfaces`].
    old: Option<HashSet<u32>>,
    /// Feedbacks of surfaces made after it: their `presented` reaches the client as
    /// `discarded`.
    discards: HashSet<u32>,
    /// [`HeldFeedback::discard_once_for_new_surfaces`].
    once: bool,
    /// The surfaces a buffer was attached to since their last commit.
    attached: HashSet<u32>,
    /// The feedbacks asked for since the last commit of their surface, by surface.
    asked: HashMap<u32, Vec<u32>>,
    layer_shells: HashSet<u32>,
    /// zwlr_layer_surface_v1 id to its wl_surface's.
    layers: HashMap<u32, u32>,
    /// zwlr_layer_surface_v1 id to its namespace.
    namespaces: HashMap<u32, String>,
    /// The surfaces whose destruction the compositor does not hear of
    /// ([`HeldFeedback::keep_surfaces_shown`]).
    kept: HashSet<u32>,
    feedbacks: HashSet<u32>,
    /// Feedbacks of silent surfaces: their events and the delete_id after them are dropped.
    unanswered: HashSet<u32>,
    /// Feedbacks whose last event is held: the delete_id that frees their id is held with it,
    /// so the client gives the id to no new object before it has read that event.
    ended: HashSet<u32>,
    /// The held events, in order.
    held: Vec<u8>,
}

#[derive(Default)]
struct State {
    holding: bool,
    conns: Vec<Conn>,
    /// [`HeldFeedback::on_layer_destroy`].
    layer_destroyed: Option<LayerDestroyed>,
}

/// Told the namespace of a layer surface the client destroys.
type LayerDestroyed = Box<dyn Fn(&str) + Send>;

/// A socket (`path`, a WAYLAND_DISPLAY value) whose clients talk to the compositor at
/// `upstream`: every message and fd passes unchanged and at once, except a
/// wp_presentation_feedback's events while they are held.
pub struct HeldFeedback {
    path: PathBuf,
    state: Arc<Mutex<State>>,
    _dir: tempfile::TempDir,
}

impl HeldFeedback {
    /// The compositor of this process's WAYLAND_DISPLAY behind a fresh socket.
    pub fn start() -> Self {
        let d = PathBuf::from(std::env::var_os("WAYLAND_DISPLAY").expect("WAYLAND_DISPLAY"));
        if d.is_absolute() {
            Self::start_on(&d)
        } else {
            let dir = std::env::var_os("XDG_RUNTIME_DIR").expect("XDG_RUNTIME_DIR");
            Self::start_on(&Path::new(&dir).join(d))
        }
    }

    /// The compositor listening on the socket `upstream` behind a fresh socket.
    pub fn start_on(upstream: &Path) -> Self {
        let upstream = upstream.to_path_buf();
        let dir = tempfile::tempdir().expect("proxy dir");
        let path = dir.path().join("wayland-proxy");
        let listener = UnixListener::bind(&path).expect("bind the proxy socket");
        let state = Arc::new(Mutex::new(State::default()));
        let st = state.clone();
        std::thread::spawn(move || {
            for client in listener.incoming().flatten() {
                let server = UnixStream::connect(&upstream).expect("connect to the compositor");
                let k = {
                    let mut g = st.lock().unwrap();
                    g.conns.push(Conn {
                        client: client.try_clone().unwrap(),
                        registries: HashSet::new(),
                        presentations: HashSet::new(),
                        compositors: HashSet::new(),
                        surfaces: HashSet::new(),
                        silent: HashSet::new(),
                        old: None,
                        discards: HashSet::new(),
                        once: false,
                        attached: HashSet::new(),
                        asked: HashMap::new(),
                        layer_shells: HashSet::new(),
                        layers: HashMap::new(),
                        namespaces: HashMap::new(),
                        kept: HashSet::new(),
                        feedbacks: HashSet::new(),
                        unanswered: HashSet::new(),
                        ended: HashSet::new(),
                        held: Vec::new(),
                    });
                    g.conns.len() - 1
                };
                let st = st.clone();
                std::thread::spawn(move || pump(&st, k, &client, &server));
            }
        });
        HeldFeedback {
            path,
            state,
            _dir: dir,
        }
    }

    /// The socket clients connect to.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Feedback events the compositor sends from now on stay here.
    pub fn hold(&self) {
        self.state.lock().unwrap().holding = true;
    }

    /// The clients hear nothing more about the commits of the surfaces they have now: the
    /// feedback they ask for on one stays unanswered, as for a surface the compositor does not
    /// draw. Surfaces made later are answered for.
    pub fn silence_surfaces(&self) {
        for c in &mut self.state.lock().unwrap().conns {
            c.silent = c.surfaces.clone();
        }
    }

    /// The feedback the clients ask for on a surface they make from now on is answered
    /// `discarded` where the compositor answers `presented`: as by a compositor that composed
    /// the frame without that surface (Hyprland, for a surface it rounds to one pixel).
    pub fn discard_for_new_surfaces(&self) {
        for c in &mut self.state.lock().unwrap().conns {
            c.old = Some(c.surfaces.clone());
        }
    }

    /// [`HeldFeedback::discard_for_new_surfaces`], by a compositor that composes that frame
    /// as soon as such a surface is mapped and none for a commit that changes nothing: the
    /// feedback asked for on a commit of such a surface that attaches no buffer stays
    /// unanswered.
    pub fn discard_once_for_new_surfaces(&self) {
        self.discard_for_new_surfaces();
        for c in &mut self.state.lock().unwrap().conns {
            c.once = true;
        }
    }

    /// The compositor does not hear of the destruction of the surfaces the clients have now
    /// (wl_surface.destroy and zwlr_layer_surface_v1.destroy are dropped): each stays on
    /// screen with its last frame, as a closed window does while a compositor animates it
    /// away (Hyprland's `layersOut`, `fadeLayersOut`), until its client disconnects.
    pub fn keep_surfaces_shown(&self) {
        for c in &mut self.state.lock().unwrap().conns {
            c.kept = c.surfaces.clone();
        }
    }

    /// Calls `f` with the namespace of each layer surface a client destroys from now on,
    /// before the compositor hears of that or of anything the client asks after it: where
    /// Hyprland writes `closelayer` to its event socket.
    pub fn on_layer_destroy(&self, f: impl Fn(&str) + Send + 'static) {
        self.state.lock().unwrap().layer_destroyed = Some(Box::new(f));
    }

    /// Hands the clients the held events, in order, and stops holding.
    pub fn release(&self) {
        let mut g = self.state.lock().unwrap();
        g.holding = false;
        for c in &mut g.conns {
            c.ended.clear();
            send(
                c.client.as_raw_fd(),
                &std::mem::take(&mut c.held),
                &mut Vec::new(),
            );
        }
    }
}

fn header(m: &[u8]) -> (u32, u16) {
    let w = |i: usize| u32::from_ne_bytes(m[i..i + 4].try_into().unwrap());
    (w(0), (w(4) & 0xffff) as u16)
}

fn arg(m: &[u8], i: usize) -> u32 {
    u32::from_ne_bytes(m[8 + 4 * i..12 + 4 * i].try_into().unwrap())
}

/// Takes the whole messages at the front of `buf`.
fn split(buf: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    while buf.len() >= 8 {
        let size = (u32::from_ne_bytes(buf[4..8].try_into().unwrap()) >> 16) as usize;
        if size < 8 || buf.len() < size {
            break;
        }
        out.push(buf.drain(..size).collect());
    }
    out
}

/// Copies both directions of one connection, noting the feedbacks asked for on the way up
/// and holding their events on the way down.
fn pump(state: &Mutex<State>, k: usize, client: &UnixStream, server: &UnixStream) {
    let (cfd, sfd) = (client.as_raw_fd(), server.as_raw_fd());
    let (mut up, mut down) = (Vec::new(), Vec::new());
    let (mut up_fds, mut down_fds): (Vec<RawFd>, Vec<RawFd>) = (Vec::new(), Vec::new());
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let mut pfd = [cfd, sfd].map(|fd| libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        });
        // SAFETY: two valid pollfds.
        if unsafe { libc::poll(pfd.as_mut_ptr(), 2, -1) } < 0 {
            return;
        }
        if pfd[0].revents != 0 {
            let n = recv(cfd, &mut buf, &mut up_fds, 0);
            if n <= 0 {
                return;
            }
            up.extend_from_slice(&buf[..n as usize]);
            let mut out = Vec::new();
            let mut g = state.lock().unwrap();
            let g = &mut *g;
            let c = &mut g.conns[k];
            for m in split(&mut up) {
                let (obj, op) = header(&m);
                if let (7, Some(namespace)) = (op, c.namespaces.get(&obj)) {
                    // zwlr_layer_surface_v1.destroy
                    if let Some(f) = &g.layer_destroyed {
                        f(namespace);
                    }
                }
                if obj == WL_DISPLAY && op == 1 {
                    c.registries.insert(arg(&m, 0));
                } else if c.registries.contains(&obj) && op == 0 {
                    // bind(name, interface string, version, new id)
                    let len = arg(&m, 1) as usize;
                    let id = arg(&m, 2 + len.div_ceil(4) + 1);
                    match &m[16..16 + len.saturating_sub(1)] {
                        b"wp_presentation" => c.presentations.insert(id),
                        b"wl_compositor" => c.compositors.insert(id),
                        b"zwlr_layer_shell_v1" => c.layer_shells.insert(id),
                        _ => false,
                    };
                } else if c.compositors.contains(&obj) && op == 0 {
                    // create_surface(new id)
                    c.surfaces.insert(arg(&m, 0));
                } else if c.layer_shells.contains(&obj) && op == 0 {
                    // get_layer_surface(new id, surface, output, layer, namespace string)
                    c.layers.insert(arg(&m, 0), arg(&m, 1));
                    let len = (arg(&m, 4) as usize).saturating_sub(1);
                    let namespace = String::from_utf8_lossy(&m[28..28 + len]).into_owned();
                    c.namespaces.insert(arg(&m, 0), namespace);
                } else if c.presentations.contains(&obj) && op == 1 {
                    // feedback(surface, new id)
                    let (surface, id) = (arg(&m, 0), arg(&m, 1));
                    if c.silent.contains(&surface) {
                        c.unanswered.insert(id);
                    } else {
                        c.feedbacks.insert(id);
                        if c.old.as_ref().is_some_and(|old| !old.contains(&surface)) {
                            c.discards.insert(id);
                            c.asked.entry(surface).or_default().push(id);
                        }
                    }
                } else if c.surfaces.contains(&obj) && op == 1 {
                    // wl_surface.attach
                    c.attached.insert(obj);
                } else if c.surfaces.contains(&obj) && op == 6 {
                    // wl_surface.commit
                    let asked = c.asked.remove(&obj).unwrap_or_default();
                    if !c.attached.remove(&obj) && c.once {
                        for id in asked {
                            c.feedbacks.remove(&id);
                            c.discards.remove(&id);
                            c.unanswered.insert(id);
                        }
                    }
                } else if (c.kept.contains(&obj) && op == 0)
                    || (c.layers.get(&obj).is_some_and(|s| c.kept.contains(s)) && op == 7)
                {
                    // wl_surface.destroy, zwlr_layer_surface_v1.destroy
                    continue;
                }
                out.extend_from_slice(&m);
            }
            if !out.is_empty() {
                send(sfd, &out, &mut up_fds);
            }
        }
        if pfd[1].revents != 0 {
            let n = recv(sfd, &mut buf, &mut down_fds, 0);
            if n <= 0 {
                return;
            }
            down.extend_from_slice(&buf[..n as usize]);
            let mut out = Vec::new();
            let mut g = state.lock().unwrap();
            let holding = g.holding;
            let c = &mut g.conns[k];
            for m in split(&mut down) {
                let (obj, op) = header(&m);
                if c.unanswered.contains(&obj) {
                    continue;
                }
                let mut m = m;
                if c.discards.contains(&obj) {
                    match op {
                        // sync_output belongs to a `presented`.
                        0 => continue,
                        // presented: `discarded` (opcode 2, no arguments) in its place.
                        1 => {
                            m.truncate(8);
                            m[4..8].copy_from_slice(&((8u32 << 16) | 2).to_ne_bytes());
                        }
                        _ => {}
                    }
                    if op != 0 {
                        c.discards.remove(&obj);
                    }
                }
                let (obj, op) = header(&m);
                if c.feedbacks.contains(&obj) {
                    // presented and discarded end the feedback; sync_output (0) does not.
                    if op != 0 {
                        c.feedbacks.remove(&obj);
                    }
                    if holding {
                        if op != 0 {
                            c.ended.insert(obj);
                        }
                        c.held.extend_from_slice(&m);
                        continue;
                    }
                } else if obj == WL_DISPLAY && op == 1 {
                    let id = arg(&m, 0);
                    if c.ended.remove(&id) {
                        c.held.extend_from_slice(&m);
                        continue;
                    }
                    if c.unanswered.contains(&id) {
                        continue;
                    }
                    c.registries.remove(&id);
                    c.presentations.remove(&id);
                    c.compositors.remove(&id);
                    c.surfaces.remove(&id);
                    c.silent.remove(&id);
                    c.attached.remove(&id);
                    c.asked.remove(&id);
                    c.layer_shells.remove(&id);
                    c.layers.remove(&id);
                    c.namespaces.remove(&id);
                }
                out.extend_from_slice(&m);
            }
            // Written under the lock, so released events never overtake this batch.
            if !out.is_empty() {
                send(cfd, &out, &mut down_fds);
            }
            drop(g);
        }
    }
}
