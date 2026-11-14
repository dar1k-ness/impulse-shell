//! Side effects of the colors scripts behind one trait, so equivalence tests can record them.

use std::path::{Path, PathBuf};

/// Gets a notification's chosen action key, or `None` once it is dismissed (at once when it has
/// no actions or cannot be shown).
pub type OnAction = Box<dyn FnOnce(Option<String>) + Send>;

pub trait Sys: Send + Sync {
    /// Native kde-material-you-colors run (see [`crate::kde::recolor`]).
    fn kde_recolor(
        &self,
        dirs: &crate::kde::Dirs,
        seed: &str,
        dark: bool,
        sv: usize,
    ) -> crate::kde::Outcome {
        crate::kde::recolor(dirs, seed, dark, sv, &crate::kde::LiveBus::session())
    }

    /// `hyprctl -j <cmd>` over the Hyprland request socket.
    fn hypr_json(&self, cmd: &str) -> Option<serde_json::Value>;
    /// A raw request on the Hyprland request socket (`hyprctl` without flags is `cmd`, `-j` is
    /// `j/cmd`): the reply, `None` without Hyprland.
    fn hypr_request(&self, _cmd: &str) -> Option<String> {
        None
    }
    /// `gsettings get org.gnome.desktop.interface <key>` (string keys only).
    fn gsettings_get(&self, key: &str) -> Option<String>;
    /// `gsettings set org.gnome.desktop.interface <key> <value>`.
    fn gsettings_set(&self, key: &str, value: &str);
    /// Run to completion: stdout of a successful exit, `None` on failure or a non-zero exit.
    fn run(&self, argv: &[String], cwd: Option<&Path>) -> Option<String>;
    /// Start and forget.
    fn spawn(&self, argv: &[String]);
    /// `command -v <name>`.
    fn has_command(&self, name: &str) -> bool;
    /// Send `sig` to every process whose executable is exactly `name` (its `comm` or the basename
    /// of `argv[0]`, never an argument) and return once they have all exited, or after a bounded
    /// deadline.
    fn kill_exact_wait(&self, name: &str, sig: i32);
    /// `pgrep -f <name> && kill -<sig> $(pidof <name>)`: the signal goes only to a process that
    /// is still the one found, not to another one that got its pid since.
    fn signal_named(&self, name: &str, sig: i32);
    /// `/dev/pts/[0-9]+`.
    fn ttys(&self) -> Vec<PathBuf>;
    /// Writes all of `data` to the terminal `tty`, waiting while it takes no more; gives up on
    /// one that takes nothing for a bounded time.
    fn write_tty(&self, tty: &Path, data: &[u8]);
    /// `notify-send -A ...` without waiting for the user: `then` gets the outcome later.
    fn notify(
        &self,
        app: &str,
        category: Option<&str>,
        summary: &str,
        body: &str,
        actions: &[(&str, &str)],
        then: OnAction,
    );
    /// `jq --arg v "$v" '.a.b = $v' config.json > tmp && mv tmp config.json` on the shell config.
    /// The shell overrides it to route the edit through its Config singleton, which otherwise
    /// saves its pending adapter state back over the edited file.
    fn set_config(&self, file: &Path, path: &[&str], value: &str) {
        crate::switchwall::jq_set(file, path, value);
    }
}
