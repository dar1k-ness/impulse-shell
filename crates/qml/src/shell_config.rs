//! switchwall's edits of the shell config (`jq --arg v … '.a.b = $v' config.json`) go through
//! the Config singleton when the shell runs one. Written straight to the file they raced
//! Config's own delayed `writeAdapter()`: an adapter change pending in its write timer saved
//! the old `wallpaperPath` back over switchwall's, and a user edit made between the file write
//! and the FileView reload was lost. Here the value lands in `Config.options` on the GUI thread
//! and Config's own save persists it together with every other pending edit, and with what
//! another process (the settings window) saved since Config last read the file
//! (`native::merge_foreign_save`). Without a Config for that file the edit is made on the file
//! under switchwall's directory lock.
use colorgen::sys::Sys;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub struct Edit {
    pub file: PathBuf,
    pub path: Vec<String>,
    pub value: serde_json::Value,
}

static SINK: Mutex<Option<crate::svc::Sender<Edit>>> = Mutex::new(None);

/// Makes the calling World the receiver of config edits (the latest World with a Config wins).
pub(crate) fn install(h: &crate::svc::Handle) -> std::sync::mpsc::Receiver<Edit> {
    let (tx, rx) = h.channel();
    *SINK.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    rx
}

/// Sets the string at `path` in the shell config `file`, through the shell's Config if one
/// is running.
pub fn set(file: &Path, path: &[&str], value: &str) {
    set_json(file, path, serde_json::Value::String(value.to_string()));
}

/// [`set`] with any JSON value: a number, a bool or a list stays one in the config.
pub fn set_json(file: &Path, path: &[&str], value: serde_json::Value) {
    let e = Edit {
        file: file.to_path_buf(),
        path: path.iter().map(|s| s.to_string()).collect(),
        value,
    };
    let sink = SINK.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let e = match sink {
        Some(tx) => match tx.send(e) {
            Ok(()) => return,
            Err(std::sync::mpsc::SendError(e)) => e,
        },
        None => e,
    };
    write_file(&e);
}

fn write_file(e: &Edit) {
    let path: Vec<&str> = e.path.iter().map(String::as_str).collect();
    colorgen::switchwall::jq_set_value(&e.file, &path, e.value.clone());
}

/// The live session for the colors scripts, with config edits routed through [`set`].
pub struct ShellSys;

pub fn sys() -> Arc<dyn Sys> {
    Arc::new(ShellSys)
}

impl Sys for ShellSys {
    fn hypr_json(&self, cmd: &str) -> Option<serde_json::Value> {
        colorgen::live::Live.hypr_json(cmd)
    }
    fn hypr_request(&self, cmd: &str) -> Option<String> {
        colorgen::live::Live.hypr_request(cmd)
    }
    fn gsettings_get(&self, key: &str) -> Option<String> {
        colorgen::live::Live.gsettings_get(key)
    }
    fn gsettings_set(&self, key: &str, value: &str) {
        colorgen::live::Live.gsettings_set(key, value)
    }
    fn run(&self, argv: &[String], cwd: Option<&Path>) -> Option<String> {
        colorgen::live::Live.run(argv, cwd)
    }
    fn spawn(&self, argv: &[String]) {
        colorgen::live::Live.spawn(argv)
    }
    fn has_command(&self, name: &str) -> bool {
        colorgen::live::Live.has_command(name)
    }
    fn kill_exact_wait(&self, name: &str, sig: i32) {
        colorgen::live::Live.kill_exact_wait(name, sig)
    }
    fn signal_named(&self, name: &str, sig: i32) {
        colorgen::live::Live.signal_named(name, sig)
    }
    fn ttys(&self) -> Vec<PathBuf> {
        colorgen::live::Live.ttys()
    }
    fn write_tty(&self, tty: &Path, data: &[u8]) {
        colorgen::live::Live.write_tty(tty, data)
    }
    fn notify(
        &self,
        app: &str,
        category: Option<&str>,
        summary: &str,
        body: &str,
        actions: &[(&str, &str)],
        then: colorgen::sys::OnAction,
    ) {
        colorgen::live::Live.notify(app, category, summary, body, actions, then)
    }
    fn set_config(&self, file: &Path, path: &[&str], value: &str) {
        set(file, path, value)
    }
}

impl crate::runtime::Runtime {
    /// Applies queued config edits to `Config.options` (see the module doc).
    #[doc(hidden)]
    pub fn poll_config_edits(&mut self) -> bool {
        let edits: Vec<Edit> = {
            let mut w = self.world.borrow_mut();
            let Some(&cfg) = w.singletons.get("Config") else {
                return false;
            };
            // Until Config has loaded its file, that load would replace the edited options with
            // the file's old values: edits wait until it is `ready` (a missing file never loads).
            let file = w
                .read(cfg, "filePath")
                .map(|v| v.to_js_string().to_string());
            if !w.read(cfg, "ready").is_some_and(|v| v.truthy())
                && file.is_some_and(|f| Path::new(&f).exists())
            {
                return false;
            }
            if w.config_edits.is_none() {
                let h = w.waker.handle(crate::svc::Svc::Host);
                w.config_edits = Some(install(&h));
            }
            w.config_edits.as_ref().unwrap().try_iter().collect()
        };
        if edits.is_empty() {
            return false;
        }
        let cfg = self.world.borrow().singletons["Config"];
        let mut changed = false;
        for e in edits {
            let ours = self
                .world
                .borrow_mut()
                .read(cfg, "filePath")
                .is_some_and(|v| Path::new(&*v.to_js_string()) == e.file);
            if !ours || e.path.is_empty() || !self.set_option(cfg, &e.path, &e.value) {
                workpool::run(move || write_file(&e));
                continue;
            }
            changed = true;
        }
        changed
    }

    /// `Config.options.a.b = value`, creating missing objects like `jq` does.
    fn set_option(&mut self, cfg: usize, path: &[String], value: &serde_json::Value) -> bool {
        use crate::value::Value;
        let mut e = self.eng();
        let Ok(mut cur) = e.it.get(&Value::Item(cfg), "options") else {
            return false;
        };
        for k in &path[..path.len() - 1] {
            let next = match e.it.get(&cur, k) {
                Ok(v @ (Value::Obj(_) | Value::Item(_))) => v,
                _ => {
                    let o = Value::Obj(crate::eval::new_obj());
                    if e.it.put(&cur, k, o.clone()).is_err() {
                        return false;
                    }
                    o
                }
            };
            cur = next;
        }
        let value = match value {
            serde_json::Value::String(s) => Value::str(s),
            v => match crate::json::parse(&v.to_string()) {
                Ok(v) => v,
                Err(_) => return false,
            },
        };
        let ok = e.it.put(&cur, path.last().unwrap(), value).is_ok();
        e.settle();
        ok
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn set_json_without_a_config_keeps_the_value_type_in_the_file() {
        let t = tempfile::tempdir().unwrap();
        let file = t.path().join("config.json");
        std::fs::write(&file, r#"{"bar": {"vertical": false}}"#).unwrap();
        super::set_json(&file, &["bar", "layout", "left"], json!(["clock", "media"]));
        super::set_json(
            &file,
            &["background", "widgets", "clock", "x"],
            json!(120.0),
        );
        super::set(&file, &["background", "wallpaperPath"], "/w.png");
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(
            v,
            json!({
                "bar": {"vertical": false, "layout": {"left": ["clock", "media"]}},
                "background": {"widgets": {"clock": {"x": 120.0}}, "wallpaperPath": "/w.png"}
            })
        );
    }
}
