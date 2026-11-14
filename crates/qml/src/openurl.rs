//! `Qt.openUrlExternally` as QGenericUnixServices does it: the xdg-desktop-portal first
//! (OpenURI.OpenURI, OpenURI.OpenFile with an fd for local files, Email.ComposeEmail for
//! mailto:), and a detached `xdg-open URL` whenever the portal call fails: no portal, or one
//! without that interface (xdg-desktop-portal with no OpenURI backend answers UnknownMethod).
//! Unlike a blocking port of it, nothing here runs on the GUI thread: the portal reply can take
//! the whole D-Bus timeout and opening a FIFO or a stalled network file blocks indefinitely.
use std::collections::BTreeMap;
use std::os::fd::AsFd;
use std::os::unix::fs::OpenOptionsExt;
use zbus::zvariant::{Fd, Value};

#[derive(Debug, PartialEq)]
pub enum Target {
    /// `shown` is `QUrl::toString()`, what the portal is given.
    Uri {
        shown: String,
        url: String,
    },
    File {
        path: String,
        url: String,
    },
    Email {
        address: String,
        subject: String,
        body: String,
        url: String,
    },
}

fn decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}

/// Where QGenericUnixServices takes `url`: a local file by its descriptor, `mailto:` to the
/// mail portal, the rest as a url. `url` in each is `QUrl::toEncoded()`, what `xdg-open`
/// is started with.
pub fn classify(url: &crate::qurl::QUrl) -> Target {
    let (shown, encoded) = (url.to_string(), url.to_encoded());
    if url.is_local_file() {
        return Target::File {
            path: url.to_local_file(),
            url: encoded,
        };
    }
    if let Some(rest) = shown.strip_prefix("mailto:") {
        let rest = rest.split('#').next().unwrap_or("");
        let (addr, query) = rest.split_once('?').unwrap_or((rest, ""));
        let item = |k: &str| {
            query
                .split('&')
                .find_map(|kv| {
                    kv.split_once('=')
                        .filter(|(n, _)| *n == k)
                        .map(|(_, v)| decode(v))
                })
                .unwrap_or_default()
        };
        return Target::Email {
            address: decode(addr),
            subject: item("subject"),
            body: item("body"),
            url: encoded,
        };
    }
    Target::Uri {
        shown,
        url: encoded,
    }
}

const DESKTOP: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";

/// `None`: the portal did not take it (no bus, no portal, an error reply): fall back to xdg-open.
fn portal(t: &Target) -> Option<()> {
    let conn = crate::dbus::session()?;
    let none: BTreeMap<&str, Value> = BTreeMap::new();
    let r = match t {
        Target::Uri { shown, .. } => conn.call_method(
            Some(DESKTOP),
            PATH,
            Some("org.freedesktop.portal.OpenURI"),
            "OpenURI",
            &("", shown.as_str(), &none),
        ),
        Target::File { path, .. } => {
            // O_PATH (accepted by OpenFile): a FIFO or device is never opened for reading.
            let f = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_PATH)
                .open(path)
                .ok()?;
            conn.call_method(
                Some(DESKTOP),
                PATH,
                Some("org.freedesktop.portal.OpenURI"),
                "OpenFile",
                &("", Fd::from(f.as_fd()), &none),
            )
        }
        Target::Email {
            address,
            subject,
            body,
            ..
        } => {
            let mut o: BTreeMap<&str, Value> = BTreeMap::new();
            o.insert("address", Value::from(address.as_str()));
            o.insert("subject", Value::from(subject.as_str()));
            o.insert("body", Value::from(body.as_str()));
            o.insert("attachment_fds", Value::from(Vec::<Fd>::new()));
            conn.call_method(
                Some(DESKTOP),
                PATH,
                Some("org.freedesktop.portal.Email"),
                "ComposeEmail",
                &("", &o),
            )
        }
    };
    r.ok().map(drop)
}

/// Qt's fallback: `QProcess::startDetached("xdg-open", {url})`.
pub(crate) fn xdg_open(url: &str) -> bool {
    crate::detached::Launch::own_env(&["xdg-open".into(), url.into()])
        .start("Qt.openUrlExternally")
        .map(workpool::adopt)
        .is_some()
}

/// Returns at once, like Qt's asynchronous portal call; the portal (or `xdg-open`) runs on a
/// pool thread.
pub fn open(url: &crate::qurl::QUrl) -> bool {
    let t = classify(url);
    workpool::long(move || {
        if portal(&t).is_none() {
            xdg_open(match &t {
                Target::Uri { url, .. } | Target::File { url, .. } | Target::Email { url, .. } => {
                    url
                }
            });
        }
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Qt's fallback is `QProcess::startDetached("xdg-open", {url})`: the program leads a
    /// session of its own. It used to stay in the shell's session and group.
    #[test]
    fn xdg_open_starts_in_its_own_session() {
        use crate::native::tests::{REPORT, assert_detached, report};
        const TEST: &str = concat!(module_path!(), "::xdg_open_starts_in_its_own_session");
        if !crate::testenv::in_own_process(TEST) {
            // An `xdg-open`, found first in PATH, that reports about itself.
            let dir = tempfile::tempdir().unwrap();
            let bin = dir.path().join("bin");
            std::fs::create_dir(&bin).unwrap();
            crate::testenv::install_script(
                &bin.join("xdg-open"),
                &format!(
                    "#!/bin/sh\n{}\n",
                    REPORT.replace("\"$0\"", "\"$FAKE_FIFO\"")
                ),
            );
            let fifo = dir.path().join("report");
            crate::testenv::fifo(&fifo);
            let mut path = bin.into_os_string();
            path.push(":");
            path.push(std::env::var_os("PATH").unwrap());
            crate::testenv::run_in_own_process(
                TEST,
                [("PATH", path.as_os_str()), ("FAKE_FIFO", fifo.as_os_str())],
            );
            return;
        }
        assert!(xdg_open("https://example.org/"));
        let r = report(std::path::Path::new(
            &std::env::var_os("FAKE_FIFO").unwrap(),
        ));
        assert_detached(&r);
        workpool::wait_idle();
    }

    #[test]
    fn classify_like_qurl() {
        let classify = |s: &str| classify(&crate::qurl::QUrl::parse(s));
        assert_eq!(
            classify("file:///tmp/a%20b"),
            Target::File {
                path: "/tmp/a b".into(),
                url: "file:///tmp/a%20b".into()
            }
        );
        assert_eq!(
            classify("https://example.org/a b?q=ü"),
            Target::Uri {
                shown: "https://example.org/a b?q=ü".into(),
                url: "https://example.org/a%20b?q=%C3%BC".into()
            }
        );
        assert_eq!(
            classify("mailto:a@b.c?subject=Hi%21&body=x y"),
            Target::Email {
                address: "a@b.c".into(),
                subject: "Hi!".into(),
                body: "x y".into(),
                url: "mailto:a@b.c?subject=Hi%21&body=x%20y".into()
            }
        );
    }
}
