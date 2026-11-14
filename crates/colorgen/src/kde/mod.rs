//! Native recolouring of Qt/KDE apps: what the kde-material-you-colors Python tool did on every
//! theme switch (1.8 s CPU, numpy and a Qt process per Plasma helper), from colorgen's own
//! Material engine. Same colours; see [`apply`] for what it does differently.

pub mod accent;
pub mod apply;
pub mod color;
pub mod config;
pub mod deco;
pub mod kconfig;
pub mod pywal;
pub mod theme;

pub use apply::{Body, Bus, Dirs, Report};
use std::time::Duration;

#[derive(Debug)]
pub enum Outcome {
    Applied(Report),
    Failed(String),
}

/// One recolour for seed `seed` (`#rrggbb`), scheme variant index `sv` (`-sv`).
pub fn recolor(dirs: &Dirs, seed: &str, dark: bool, sv: usize, bus: &dyn Bus) -> Outcome {
    let o = config::Options::read(&dirs.config.join("kde-material-you-colors/config.conf"));
    let wallpaper = seed;
    let Some(seed) = color::parse(seed) else {
        return Outcome::Failed(format!("bad colour {seed:?}"));
    };
    let Some(&variant) = theme::VARIANTS.get(sv) else {
        return Outcome::Failed(format!("bad scheme variant {sv}"));
    };
    let params = theme::Params {
        variant,
        chroma_mult: o.chroma_mult,
        tone_mult: o.tone_mult,
        custom_colors: o.custom_colors.clone(),
        toolbar_opacity: o.toolbar_opacity,
        toolbar_opacity_dark: o.toolbar_opacity_dark,
    };
    let theme = match theme::Theme::new(seed, &params) {
        Ok(t) => t,
        Err(_) => return Outcome::Failed("no temperature range for this seed".into()),
    };
    let r = apply::apply(dirs, &theme, &o, dark, wallpaper, bus);
    if let Some(hook) = &o.on_change_hook {
        run_hook(hook);
    }
    Outcome::Applied(r)
}

/// The user's `on_change_hook`, detached and reaped on exit; its pid.
pub(crate) fn run_hook(hook: &str) -> Option<u32> {
    run_hook_then(hook, |_| {})
}

/// [`run_hook`], then `f` with its exit status on the reaper thread once it is reaped.
fn run_hook_then(
    hook: &str,
    f: impl FnOnce(Option<std::process::ExitStatus>) + Send + 'static,
) -> Option<u32> {
    let ch = workpool::userenv::command("sh")
        .args(["-c", hook])
        .stdin(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let pid = ch.id();
    workpool::reap_then(ch, f);
    Some(pid)
}

/// [`Bus`] on the shell's shared session connection.
pub struct LiveBus(Option<zbus::blocking::Connection>);

const TIMEOUT: Duration = Duration::from_secs(2);

impl LiveBus {
    pub fn session() -> LiveBus {
        LiveBus(bus::session())
    }

    fn call<B>(
        &self,
        dest: &str,
        path: &str,
        iface: &str,
        member: &str,
        body: &B,
    ) -> Option<zbus::Message>
    where
        B: serde::ser::Serialize + zbus::zvariant::DynamicType,
    {
        bus::call_timeout(
            self.0.as_ref()?,
            TIMEOUT,
            Some(dest),
            path,
            Some(iface),
            member,
            body,
        )
        .ok()
    }
}

impl Bus for LiveBus {
    fn signal(&self, path: &str, iface: &str, member: &str, body: Body) {
        let Some(c) = &self.0 else { return };
        let none = None::<&str>;
        let _ = match body {
            Body::Empty => c.emit_signal(none, path, iface, member, &()),
            Body::Ints(v) if v.len() == 2 => {
                c.emit_signal(none, path, iface, member, &(v[0], v[1]))
            }
            Body::Ints(v) => c.emit_signal(none, path, iface, member, &(v[0],)),
            Body::Changed(g) => {
                let m: std::collections::HashMap<String, Vec<Vec<u8>>> = g
                    .into_iter()
                    .map(|(g, k)| (g, k.into_iter().map(String::into_bytes).collect()))
                    .collect();
                c.emit_signal(none, path, iface, member, &(m,))
            }
        };
    }

    fn has_owner(&self, name: &str) -> bool {
        self.call(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "NameHasOwner",
            &(name,),
        )
        .and_then(|m| m.body().deserialize::<bool>().ok())
        .unwrap_or(false)
    }

    fn call(&self, dest: &str, path: &str, iface: &str, member: &str) {
        let _ = LiveBus::call(self, dest, path, iface, member, &());
    }

    fn konsole_sessions(&self) -> Vec<(String, String)> {
        let names = self
            .call(
                "org.freedesktop.DBus",
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "ListNames",
                &(),
            )
            .and_then(|m| m.body().deserialize::<Vec<String>>().ok())
            .unwrap_or_default();
        let mut out = Vec::new();
        for svc in names.into_iter().filter(|n| n.contains("org.kde.konsole")) {
            let xml = self
                .call(
                    &svc,
                    "/Sessions",
                    "org.freedesktop.DBus.Introspectable",
                    "Introspect",
                    &(),
                )
                .and_then(|m| m.body().deserialize::<String>().ok())
                .unwrap_or_default();
            for part in xml.split("<node name=\"").skip(1) {
                if let Some(n) = part.split('"').next() {
                    out.push((svc.clone(), format!("/Sessions/{n}")));
                }
            }
        }
        out
    }

    fn konsole_profile(&self, service: &str, session: &str) -> Option<String> {
        self.call(service, session, "org.kde.konsole.Session", "profile", &())
            .and_then(|m| m.body().deserialize::<String>().ok())
    }

    fn set_konsole_profile(&self, service: &str, session: &str, profile: &str) {
        let _ = self.call(
            service,
            session,
            "org.kde.konsole.Session",
            "setProfile",
            &(profile,),
        );
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn on_change_hook_leaves_no_zombie() {
        let (tx, rx) = std::sync::mpsc::channel();
        let pid = super::run_hook_then("exit 0", move |st| tx.send(st).unwrap()).unwrap();
        assert!(rx.recv().unwrap().is_some_and(|st| st.success()));
        assert!(
            !std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "hook {pid} left a zombie"
        );
    }
}
