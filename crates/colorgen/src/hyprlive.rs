//! Matugen outputs that are part of Hyprland's Lua config (`~/.config/hypr/hyprland/colors.lua`).
//!
//! Hyprland watches every file its config `require`s and re-runs the whole config when one
//! changes, which freezes the compositor for ~150 ms on every theme switch. Here the file is
//! written with autoreload paused and its Lua runs once through `hyprctl eval`, which only sets
//! the values it names; the file still carries them into the next start or reload.

use crate::fsutil::write_atomic;
use crate::sys::Sys;
use std::path::Path;

/// A Lua file under `$XDG_CONFIG_HOME/hypr`, which Hyprland's config may `require`.
pub fn is_hypr_lua(config_home: &Path, out: &Path) -> bool {
    out.starts_with(config_home.join("hypr")) && out.extension().is_some_and(|e| e == "lua")
}

fn set_disable_autoreload(on: bool) -> String {
    format!("eval hl.config({{ misc = {{ disable_autoreload = {on} }} }})")
}

/// `Some(true)` when Hyprland reloads its config on file changes, `None` without Hyprland.
fn autoreload(sys: &dyn Sys) -> Option<bool> {
    let v: serde_json::Value =
        serde_json::from_str(&sys.hypr_request("j/getoption misc:disable_autoreload")?).ok()?;
    let disabled = v
        .get("bool")
        .and_then(|b| b.as_bool())
        .or_else(|| v.get("int").and_then(|i| i.as_i64()).map(|i| i != 0))?;
    Some(!disabled)
}

/// `hyprctl eval` of `text`. Unnamed `hl.window_rule`s get a name from the file and their
/// position, so the next switch updates them instead of stacking new rules on every switch.
pub fn eval_request(out: &Path, text: &str) -> String {
    let file: String = out
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .collect();
    format!(
        "eval local window_rule, n = hl.window_rule, 0
hl.window_rule = function(rule)
    if type(rule) == \"table\" and rule.name == nil then
        n = n + 1
        rule.name = \"rusty-impulse {file} \" .. n
    end
    return window_rule(rule)
end
local ok, err = pcall(function()
{text}
end)
hl.window_rule = window_rule
if not ok then error(err, 0) end"
    )
}

/// Writes `text` to `out` (see the module doc). Without a Lua Hyprland it is a plain write,
/// and Hyprland's own reload applies the file as before.
pub fn write(sys: &dyn Sys, out: &Path, text: &str) -> std::io::Result<()> {
    let ok = |cmd: &str| sys.hypr_request(cmd).is_some_and(|r| r.trim() == "ok");
    let Some(reloads) = autoreload(sys) else {
        return write_atomic(out, text.as_bytes());
    };
    if reloads && !ok(&set_disable_autoreload(true)) {
        return write_atomic(out, text.as_bytes());
    }
    let written = write_atomic(out, text.as_bytes());
    let applied = written.is_ok() && ok(&eval_request(out, text));
    if reloads {
        ok(&set_disable_autoreload(false));
        if written.is_ok() && !applied {
            sys.hypr_request("reload");
        }
    }
    written
}
