//! Persistent state in `Dirs::state`: the current wallpaper, shown ids, downloaded files and the taste.

use crate::error::{Error, Result, io};
use crate::fsutil;
use crate::taste::Taste;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const FILE: &str = "state.json";
const HISTORY_CAP: usize = 1000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Current {
    pub id: String,
    pub path: PathBuf,
    pub page: String,
    pub accent: Option<String>,
    pub emb: Vec<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct State {
    pub current: Option<Current>,
    /// Candidate ids already shown, oldest first; never offered again.
    pub history: Vec<String>,
    /// Downloads made by wallpick, oldest first.
    pub files: Vec<PathBuf>,
    pub liked: Vec<PathBuf>,
    pub taste: Taste,
}

static LOCK: Mutex<()> = Mutex::new(());

/// Reads the state; a missing file is a fresh state, an unreadable one is moved aside to
/// `state.json.corrupt` (kept for inspection) and replaced by a fresh state.
pub(crate) fn load(dir: &Path) -> Result<State> {
    let path = dir.join(FILE);
    match std::fs::read(&path) {
        Ok(b) => Ok(serde_json::from_slice(&b).unwrap_or_else(|_| {
            let _ = std::fs::rename(&path, dir.join(format!("{FILE}.corrupt")));
            State::default()
        })),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
        Err(e) => Err(io(&path)(e)),
    }
}

fn save(dir: &Path, s: &State) -> Result<()> {
    let json = serde_json::to_vec(s).map_err(|source| Error::Json {
        what: FILE.into(),
        source,
    })?;
    fsutil::write_atomic(&dir.join(FILE), &json)
}

/// Read-modify-write under a process-wide lock; nothing is written when `f` fails.
pub(crate) fn update<T>(dir: &Path, f: impl FnOnce(&mut State) -> Result<T>) -> Result<T> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = load(dir)?;
    let out = f(&mut s)?;
    save(dir, &s)?;
    Ok(out)
}

impl State {
    /// Makes `cur` the current wallpaper, remembers what was shown and deletes the oldest
    /// downloads beyond `keep`; liked files and the current one are never deleted, and only files
    /// inside `downloads` are touched.
    pub(crate) fn record(
        &mut self,
        cur: Current,
        shown: &[(String, Vec<f32>)],
        downloads: &Path,
        keep: usize,
    ) {
        for (id, emb) in shown {
            self.taste.add_seen(emb);
            if !self.history.contains(id) {
                self.history.push(id.clone());
            }
        }
        if self.history.len() > HISTORY_CAP {
            let extra = self.history.len() - HISTORY_CAP;
            self.history.drain(..extra);
        }
        self.files.retain(|f| *f != cur.path);
        self.files.push(cur.path.clone());
        while self.files.len() > keep.max(1) {
            let old = self.files.remove(0);
            if !self.liked.contains(&old) && old != cur.path && old.starts_with(downloads) {
                let _ = std::fs::remove_file(&old);
            }
        }
        self.current = Some(cur);
    }

    pub(crate) fn vote(&mut self, like: bool) -> Result<()> {
        let cur = self.current.clone().ok_or(Error::NoCurrent)?;
        self.taste.vote(&cur.emb, like);
        self.liked.retain(|p| *p != cur.path);
        if like {
            self.liked.push(cur.path);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cur(dir: &Path, id: &str) -> Current {
        let path = dir.join(format!("{id}.jpg"));
        std::fs::write(&path, b"x").unwrap();
        Current {
            id: id.into(),
            path,
            page: String::new(),
            accent: None,
            emb: vec![1.0, 0.0],
        }
    }

    #[test]
    fn keeps_newest_and_liked_downloads() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        let mut s = State::default();
        s.record(cur(d, "a"), &[("a".into(), vec![1.0, 0.0])], d, 2);
        s.vote(true).unwrap();
        s.record(cur(d, "b"), &[("b".into(), vec![0.0, 1.0])], d, 2);
        s.record(cur(d, "c"), &[], d, 2);
        s.record(cur(d, "d"), &[], d, 2);
        assert!(d.join("a.jpg").exists(), "liked");
        assert!(!d.join("b.jpg").exists());
        assert!(d.join("c.jpg").exists() && d.join("d.jpg").exists());
        assert_eq!(s.files, [d.join("c.jpg"), d.join("d.jpg")]);
        assert_eq!(s.history, ["a", "b"]);
        assert_eq!(s.current.as_ref().unwrap().id, "d");
        // files outside the downloads dir are never deleted
        let other = tempfile::tempdir().unwrap();
        let mut s = State {
            files: vec![other.path().join("x.jpg")],
            ..State::default()
        };
        std::fs::write(other.path().join("x.jpg"), b"x").unwrap();
        s.record(cur(d, "e"), &[], d, 1);
        assert!(other.path().join("x.jpg").exists());
    }

    #[test]
    fn persists_and_recovers_from_corruption() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        assert_eq!(load(d).unwrap(), State::default());
        assert!(matches!(update(d, |s| s.vote(true)), Err(Error::NoCurrent)));
        assert!(!d.join(FILE).exists(), "a failed update writes nothing");
        update(d, |s| {
            s.history.push("x".into());
            Ok(())
        })
        .unwrap();
        assert_eq!(load(d).unwrap().history, ["x"]);
        std::fs::write(d.join(FILE), b"{not json").unwrap();
        assert_eq!(load(d).unwrap(), State::default());
        assert!(d.join("state.json.corrupt").exists());
    }
}
