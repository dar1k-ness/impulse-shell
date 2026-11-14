//! When Hyprland has a closed layer of the shell off the screen. It animates a closed layer away
//! on a snapshot of its last frame (`CLayerFadeout`: position and size by `layersOut`, alpha by
//! `fadeLayersOut`) and tells no client when that ends: the finished fadeout is erased after a
//! render (`FadingOutState::cleanupForMonitor`). What it does tell: `closelayer>>NAMESPACE` on
//! the event socket as the animation starts (`CLayerSurface::onUnmap`), and how long the two
//! animations run (`hyprctl animations`). A frame Hyprland renders once that long has passed
//! has the snapshot gone: it steps the animations before every render
//! (`CHyprAnimationManager::frameTick`).

use std::io::Read;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

/// How long Hyprland animates a closed layer, in ms: the longer of `layersOut` and
/// `fadeLayersOut`, each `speed` × 100 ms (hyprutils `CBaseAnimatedVariable::getPercent`) of
/// the nearest overridden node up its tree (`CAnimationTreeController::reset`); 0 for a
/// disabled one and with `animations:enabled` off (the value then warps to its goal).
/// `animations` is the reply to `j/animations`, `enabled` the one to
/// `j/getoption animations:enabled`.
pub fn close_ms(animations: &str, enabled: &str) -> Result<f64, String> {
    let json = |s: &str| serde_json::from_str::<serde_json::Value>(s).map_err(|e| e.to_string());
    let on = json(enabled)?;
    let on = on.get("bool").and_then(|v| v.as_bool());
    let on = on.or_else(|| json(enabled).ok()?.get("int")?.as_i64().map(|i| i != 0));
    let on = on.ok_or("no value of animations:enabled")?;
    let nodes = json(animations)?;
    let nodes = nodes.get(0).and_then(|n| n.as_array());
    let nodes = nodes.ok_or("no animation list")?;
    let ms = |chain: &[&str]| -> Result<f64, String> {
        let node = |name: &&str| nodes.iter().find(|n| n["name"] == **name);
        let own = |n: &&serde_json::Value| n["overridden"] == true;
        let n = chain.iter().filter_map(node).find(own);
        let n = n.ok_or_else(|| format!("no values for {}", chain[0]))?;
        if n["enabled"] != true {
            return Ok(0.0);
        }
        // A spring settles when its value and velocity are small enough: no set length.
        if n["bezier"]
            .as_str()
            .is_some_and(|b| b.starts_with("spring:"))
        {
            return Err(format!("{} follows a spring", chain[0]));
        }
        let speed = n["speed"].as_f64();
        Ok(speed.ok_or_else(|| format!("no speed for {}", chain[0]))? * 100.0)
    };
    let moved = ms(&["layersOut", "layers", "global"])?;
    let faded = ms(&["fadeLayersOut", "fadeLayers", "fade", "global"])?;
    Ok(if on { moved.max(faded) } else { 0.0 })
}

/// What Hyprland answered a read of its animation settings, with the read's number.
pub type Reading = (u64, Result<f64, String>);

/// Hyprland's event socket is gone: nothing more is read from it.
#[derive(Debug)]
pub struct Lost;

/// The shell's ear on Hyprland's event socket, for the layers it closes.
#[derive(Default)]
pub struct Watch {
    conn: Option<Conn>,
}

struct Conn {
    dir: PathBuf,
    sock: UnixStream,
    buf: Vec<u8>,
    /// [`close_ms`]; `None` while a read is out or after one that failed.
    ms: Option<f64>,
    /// The number of the last read started.
    read: u64,
    report: std::sync::Arc<dyn Fn(Reading) + Send + Sync>,
}

/// A closed layer keeps its last frame for Hyprland to animate only while this is not said.
const BLANK: &str = "a closed window gives an empty last frame, not Hyprland's close animation";

impl Watch {
    /// Listens on the event socket of the Hyprland instance at `dir` (`None`: not on
    /// Hyprland) and reads its animation settings on a thread, which hands them to `report`
    /// (for [`Watch::settings`]).
    pub fn start(dir: Option<&Path>, report: impl Fn(Reading) + Send + Sync + 'static) -> Watch {
        let Some(dir) = dir else {
            return Watch { conn: None };
        };
        let sock = UnixStream::connect(dir.join(".socket2.sock"));
        let sock = sock.and_then(|s| s.set_nonblocking(true).map(|()| s));
        let sock = match sock {
            Ok(s) => s,
            Err(e) => {
                elog::eprintln!("off screen: no Hyprland event socket ({e}): {BLANK}");
                return Watch { conn: None };
            }
        };
        let mut conn = Conn {
            dir: dir.to_path_buf(),
            sock,
            buf: Vec::new(),
            ms: None,
            read: 0,
            report: std::sync::Arc::new(report),
        };
        conn.read_settings();
        Watch { conn: Some(conn) }
    }

    /// The event socket, for the loop to wake on.
    pub fn fd(&self) -> Option<OwnedFd> {
        let c = self.conn.as_ref()?;
        c.sock.as_fd().try_clone_to_owned().ok()
    }

    /// Whether the socket is still listened on.
    pub fn alive(&self) -> bool {
        self.conn.is_some()
    }

    /// How long Hyprland animates a layer closed now ([`close_ms`]), when that is known.
    /// Says in the log why not, on Hyprland.
    pub fn close_ms(&self) -> Option<f64> {
        let c = self.conn.as_ref()?;
        if c.ms.is_none() {
            elog::eprintln!("off screen: Hyprland's animation settings are not read: {BLANK}");
        }
        c.ms
    }

    /// Takes a read of the settings from its thread: the last one started counts.
    pub fn settings(&mut self, (read, ms): Reading) {
        let Some(c) = self.conn.as_mut().filter(|c| c.read == read) else {
            return;
        };
        c.ms = match ms {
            Ok(ms) => {
                tracing::debug!(target: "ri::frames", "hyprland animates a closed layer {ms} ms");
                Some(ms)
            }
            Err(e) => {
                elog::eprintln!("off screen: cannot read Hyprland's animations ({e})");
                None
            }
        };
    }

    /// The namespaces of the layers Hyprland closed since the last call, in order; `Err` once
    /// the socket is lost (said in the log). `configreloaded` starts a new read of the settings.
    pub fn closed(&mut self) -> Result<Vec<String>, Lost> {
        let Some(c) = self.conn.as_mut() else {
            return Ok(Vec::new());
        };
        let lost = loop {
            let mut chunk = [0u8; 4096];
            match c.sock.read(&mut chunk) {
                Ok(0) => break Some("Hyprland closed it".to_string()),
                Ok(n) => c.buf.extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break None,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => break Some(e.to_string()),
            }
        };
        let mut closed = Vec::new();
        let mut reloaded = false;
        while let Some(end) = c.buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = c.buf.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line[..end]);
            match line.split_once(">>") {
                Some(("closelayer", ns)) => closed.push(ns.to_string()),
                Some(("configreloaded", _)) => reloaded = true,
                _ => {}
            }
        }
        if reloaded {
            c.read_settings();
        }
        if let Some(e) = lost {
            elog::eprintln!("off screen: lost Hyprland's event socket ({e}): {BLANK}");
            self.conn = None;
            return Err(Lost);
        }
        Ok(closed)
    }
}

impl Conn {
    /// Asks Hyprland for its animation settings on a thread; until it answers they are unknown.
    fn read_settings(&mut self) {
        self.ms = None;
        self.read += 1;
        let (read, sock) = (self.read, self.dir.join(".socket.sock"));
        let report = self.report.clone();
        let ask = move || {
            let get = |cmd: &str| {
                qml::hypr::exchange(&sock, cmd, None).map_err(|e| format!("{cmd}: {e:?}"))
            };
            let ms = get("j/animations")
                .and_then(|a| Ok((a, get("j/getoption animations:enabled")?)))
                .and_then(|(a, on)| close_ms(&a, &on));
            report((read, ms));
        };
        if let Err(e) = std::thread::Builder::new()
            .name("hypr-close".into())
            .spawn(ask)
        {
            elog::eprintln!("off screen: cannot read Hyprland's animations ({e})");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::close_ms;

    fn animations(nodes: &[(&str, bool, bool, f64, &str)]) -> String {
        let nodes: Vec<String> = nodes
            .iter()
            .map(|(name, overridden, enabled, speed, bezier)| {
                format!(
                    r#"{{"name":"{name}","overridden":{overridden},"bezier":"{bezier}","enabled":{enabled},"speed":{speed},"style":""}}"#
                )
            })
            .collect();
        format!("[[{}],[]]", nodes.join(","))
    }

    const ON: &str = r#"{"option": "animations:enabled", "bool": true, "set": true }"#;
    const OFF: &str = r#"{"option": "animations:enabled", "bool": false, "set": true }"#;

    /// The dots' settings as Hyprland 0.57.1 prints them: nodes that are not overridden print
    /// speed 0 and take the values of the nearest overridden node above.
    #[test]
    fn the_longer_of_the_two_animations_from_the_nearest_overridden_node() {
        let dots = animations(&[
            ("fade", false, true, 0.0, ""),
            ("global", true, true, 8.0, "default"),
            ("layersOut", true, true, 2.4, "menu_accel"),
            ("layers", false, true, 0.0, ""),
            ("fadeLayers", false, true, 0.0, ""),
            ("fadeLayersOut", true, true, 2.7, "stall"),
        ]);
        assert_eq!(close_ms(&dots, ON), Ok(270.0));
        let inherited = animations(&[
            ("fade", true, true, 3.0, "default"),
            ("global", true, true, 8.0, "default"),
            ("layersOut", false, true, 0.0, ""),
            ("layers", false, true, 0.0, ""),
            ("fadeLayers", false, true, 0.0, ""),
            ("fadeLayersOut", false, true, 0.0, ""),
        ]);
        assert_eq!(close_ms(&inherited, ON), Ok(800.0));
    }

    #[test]
    fn nothing_to_wait_for_with_animations_off() {
        let a = animations(&[
            ("global", true, true, 8.0, "default"),
            ("layersOut", true, false, 2.4, "menu_accel"),
            ("fadeLayersOut", true, false, 2.7, "stall"),
        ]);
        assert_eq!(close_ms(&a, ON), Ok(0.0));
        let a = animations(&[("global", true, true, 8.0, "default")]);
        assert_eq!(close_ms(&a, OFF), Ok(0.0));
        assert_eq!(close_ms(&a, ON), Ok(800.0));
    }

    #[test]
    fn unknown_for_a_spring_and_for_a_reply_that_is_not_the_settings() {
        let a = animations(&[
            ("global", true, true, 8.0, "default"),
            ("layersOut", true, true, 2.4, "spring:bouncy"),
        ]);
        assert!(close_ms(&a, ON).is_err());
        assert!(close_ms("unknown request", ON).is_err());
        assert!(close_ms(&animations(&[]), ON).is_err());
        let a = animations(&[("global", true, true, 8.0, "default")]);
        assert!(close_ms(&a, "no such option").is_err());
    }
}
