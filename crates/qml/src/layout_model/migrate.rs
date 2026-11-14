//! Schema versions. A file older than [`VERSION`](super::VERSION) is brought up to date step
//! by step on the raw TOML before it is read, so each step only knows its own two versions;
//! a newer file is refused rather than read with keys this build would drop.

use super::{Layout, LoadError, VERSION, format, rusty};
use std::path::PathBuf;

/// Rewrites a version-`from` layout table into version `from + 1`.
pub struct Migration {
    pub from: u32,
    pub run: fn(&mut toml::Table, &Inputs) -> Result<(), String>,
}

/// What a step may read besides the file: the original's options file, for a step that adds the original's
/// widgets where the user's settings put them. `None` reads every option at its default.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inputs {
    pub rusty_impulse_config: Option<PathBuf>,
}

impl Inputs {
    /// The user's options ([`rusty::config_path`]).
    pub fn ri() -> Inputs {
        Inputs {
            rusty_impulse_config: Some(rusty::config_path()),
        }
    }

    /// The original's options, read only when a step asks for them. A file the original itself could not read
    /// reads as the original's defaults, as a missing one does: the layout is not refused over it.
    pub fn ri_options(&self) -> serde_json::Value {
        self.rusty_impulse_config
            .as_deref()
            .and_then(|p| rusty::read_options(p).ok())
            .unwrap_or_else(|| serde_json::Value::Object(Default::default()))
    }
}

/// One entry per schema change, in order. Version 1 is the first schema.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        from: 1,
        run: add_the_original_desktop_widgets,
    },
    Migration {
        from: 2,
        run: drop_the_original_placement_setting,
    },
    Migration {
        from: 3,
        run: add_the_original_sidebar_sliders,
    },
];

/// Version 1 to 2: every desktop container gets each of the original's background widgets
/// ([`rusty::desktop_widgets`]) it has none of, where the user's options put it (as the first
/// layout made from them would, [`Inputs::ri_options`]), so turning one on in
/// the original's settings (its widgets.toml `enabledBy` flag) shows it. The first layout made from the original
/// left out the ones the original's settings had off then, and nothing could add them since: edit mode,
/// the only way to remove one, is not in stable builds. Entries already there stay as they
/// are; a new one goes where the first layout puts it among them (weather under the clock).
/// Anything not shaped like a layout is left to the reader, which names the error.
fn add_the_original_desktop_widgets(t: &mut toml::Table, inputs: &Inputs) -> Result<(), String> {
    let ri_defaults = rusty::desktop_widgets(&inputs.ri_options());
    for monitor in tables(t.get_mut("monitor")) {
        // Ids are unique among the containers and widgets of a monitor.
        let mut taken: Vec<String> = vec![];
        for c in tables(monitor.get_mut("container")) {
            taken.extend(c.get("id").and_then(toml::Value::as_str).map(str::to_owned));
            for w in tables(c.get_mut("widget")) {
                taken.extend(w.get("id").and_then(toml::Value::as_str).map(str::to_owned));
            }
        }
        for c in tables(monitor.get_mut("container")) {
            if c.get("kind").and_then(toml::Value::as_str) != Some("desktop") {
                continue;
            }
            let Some(widgets) = c
                .entry("widget")
                .or_insert(toml::Value::Array(vec![]))
                .as_array_mut()
            else {
                continue;
            };
            for (i, w) in ri_defaults.iter().enumerate() {
                if widgets
                    .iter()
                    .any(|e| type_of(e) == Some(w.r#type.as_str()))
                {
                    continue;
                }
                let above = &ri_defaults[i + 1..];
                let at = widgets
                    .iter()
                    .position(|e| above.iter().any(|a| type_of(e) == Some(a.r#type.as_str())))
                    .unwrap_or(widgets.len());
                let id = (1..)
                    .map(|n| match n {
                        1 => w.id.clone(),
                        n => format!("{}-{n}", w.id),
                    })
                    .find(|id| !taken.contains(id))
                    .expect("some id is free");
                taken.push(id.clone());
                let mut entry = w.clone();
                entry.id = id;
                widgets.insert(at, format::widget_value(&entry));
            }
        }
    }
    Ok(())
}

/// Version 2 to 3: the original's own background widgets are placed as the original's settings say
/// (`background.widgets.<name>.placementStrategy`), live, as the original's settings already decide
/// whether they show. Version 2 kept a copy of that setting on each `desktop-clock` and
/// `desktop-weather` entry, made once from the original's options, which the original's settings could then no
/// longer change; the copy goes.
fn drop_the_original_placement_setting(t: &mut toml::Table, _: &Inputs) -> Result<(), String> {
    for monitor in tables(t.get_mut("monitor")) {
        for c in tables(monitor.get_mut("container")) {
            for w in tables(c.get_mut("widget")) {
                let ri_own = matches!(
                    w.get("type").and_then(toml::Value::as_str),
                    Some("desktop-clock" | "desktop-weather")
                );
                let Some(settings) = w.get_mut("settings").and_then(toml::Value::as_table_mut)
                else {
                    continue;
                };
                if ri_own {
                    settings.remove("placementStrategy");
                }
                if settings.is_empty() {
                    w.remove("settings");
                }
            }
        }
    }
    Ok(())
}

/// Version 3 to 4: the original's quick sliders show as the original's settings say (`sidebar.quickSliders`: whether
/// they show, and which of brightness, volume and microphone), live. Version 3 had a `sliders`
/// entry only when the original's settings had the sliders on as the first layout was made, with a copy
/// of the three choices, so the original's settings could no longer change either. Every sidebar gets a
/// `sliders` entry when it has none, at the top, where the first layout puts it
/// ([`rusty::sliders_widget`]), and the copies go.
fn add_the_original_sidebar_sliders(t: &mut toml::Table, _: &Inputs) -> Result<(), String> {
    for monitor in tables(t.get_mut("monitor")) {
        // Ids are unique among the containers and widgets of a monitor.
        let mut taken: Vec<String> = vec![];
        for c in tables(monitor.get_mut("container")) {
            taken.extend(c.get("id").and_then(toml::Value::as_str).map(str::to_owned));
            for w in tables(c.get_mut("widget")) {
                taken.extend(w.get("id").and_then(toml::Value::as_str).map(str::to_owned));
            }
        }
        for c in tables(monitor.get_mut("container")) {
            if c.get("kind").and_then(toml::Value::as_str) != Some("sidebar") {
                continue;
            }
            for w in tables(c.get_mut("widget")) {
                if w.get("type").and_then(toml::Value::as_str) != Some("sliders") {
                    continue;
                }
                let Some(settings) = w.get_mut("settings").and_then(toml::Value::as_table_mut)
                else {
                    continue;
                };
                for k in ["showBrightness", "showVolume", "showMic"] {
                    settings.remove(k);
                }
                if settings.is_empty() {
                    w.remove("settings");
                }
            }
            let columns = c
                .get("columns")
                .and_then(toml::Value::as_integer)
                .and_then(|n| u32::try_from(n).ok());
            let (Some(columns), Some(widgets)) = (
                columns,
                c.entry("widget")
                    .or_insert(toml::Value::Array(vec![]))
                    .as_array_mut(),
            ) else {
                continue;
            };
            if widgets.iter().any(|e| type_of(e) == Some("sliders")) {
                continue;
            }
            let mut entry = rusty::sliders_widget(columns);
            entry.id = (1..)
                .map(|n| match n {
                    1 => entry.id.clone(),
                    n => format!("{}-{n}", entry.id),
                })
                .find(|id| !taken.contains(id))
                .expect("some id is free");
            taken.push(entry.id.clone());
            widgets.insert(0, format::widget_value(&entry));
        }
    }
    Ok(())
}

/// Migrates `table` to the newest version `migrations` reach (with [`MIGRATIONS`]: [`VERSION`])
/// and stores that version in it. Returns the version the file had.
pub fn run(
    table: &mut toml::Table,
    migrations: &[Migration],
    inputs: &Inputs,
) -> Result<u32, LoadError> {
    let target = migrations
        .last()
        .map_or(VERSION, |m| m.from + 1)
        .max(VERSION);
    let found = match table.get("version") {
        None => return Err(LoadError::MissingVersion),
        Some(toml::Value::Integer(n)) if *n >= 1 => {
            u32::try_from(*n).map_err(|_| LoadError::BadVersion(n.to_string()))?
        }
        Some(v) => return Err(LoadError::BadVersion(v.to_string())),
    };
    if found > target {
        return Err(LoadError::FutureVersion {
            found,
            supported: target,
        });
    }
    let mut v = found;
    while v < target {
        let m = migrations
            .iter()
            .find(|m| m.from == v)
            .ok_or_else(|| LoadError::Migration {
                from: v,
                msg: "no migration from this version".into(),
            })?;
        (m.run)(table, inputs).map_err(|msg| LoadError::Migration { from: v, msg })?;
        v += 1;
        table.insert("version".into(), toml::Value::Integer(v.into()));
    }
    Ok(found)
}

/// [`Layout::parse`] with another migration list.
pub fn parse_with(text: &str, migrations: &[Migration]) -> Result<Layout, LoadError> {
    parse_with_inputs(text, migrations, &Inputs::default())
}

/// [`parse_with`] with what the steps may read.
pub fn parse_with_inputs(
    text: &str,
    migrations: &[Migration],
    inputs: &Inputs,
) -> Result<Layout, LoadError> {
    super::format::parse(text, migrations, inputs).map(|(l, _)| l)
}

/// The tables of array `v`, when it is one.
fn tables(v: Option<&mut toml::Value>) -> impl Iterator<Item = &mut toml::Table> {
    v.and_then(toml::Value::as_array_mut)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_table_mut)
}

fn type_of(w: &toml::Value) -> Option<&str> {
    w.get("type").and_then(toml::Value::as_str)
}
