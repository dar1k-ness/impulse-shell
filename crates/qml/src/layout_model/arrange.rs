//! The original's bar groups (`bar.layout`: which of media, workspaces and the clock each part of the bar
//! holds) and the desktop clock's spot, applied to a layout: the panel that the original's settings place
//! (`edge = "rusty"`) takes its group widgets to the zones `bar.layout` names, so the panel keeps
//! one widget per module and its zones keep giving way widget by widget.

use super::{ContainerKind, Layout, PanelPosition, Place, Store, Width, Zone};
use std::io;
use std::path::Path;

/// The original's bar groups in their own order, each with the widget types that make it up.
const GROUPS: [(&str, &[&str]); 3] = [
    ("media", &["resources", "media"]),
    ("workspaces", &["workspaces"]),
    ("clock", &["clock", "util-buttons", "battery"]),
];

/// The widget types that stretch in the original's center: as long as the center-side module.
const CENTER_STRETCH: [&str; 2] = ["media", "clock"];

/// `bar.layout`'s parts (left, center, right) as the original's BarGroupLayout reads them: unknown names
/// and repeats dropped, a group no part names back at its own place in the center.
pub fn normalize(parts: [&[String]; 3]) -> [Vec<&'static str>; 3] {
    let mut out: [Vec<&'static str>; 3] = Default::default();
    let mut placed: Vec<&str> = vec![];
    for (k, part) in parts.iter().enumerate() {
        for name in part.iter() {
            if let Some((g, _)) = GROUPS.iter().find(|(g, _)| g == name)
                && !placed.contains(g)
            {
                placed.push(g);
                out[k].push(g);
            }
        }
    }
    for (i, (g, _)) in GROUPS.iter().enumerate() {
        if placed.contains(g) {
            continue;
        }
        let rank = |n: &str| GROUPS.iter().position(|(h, _)| *h == n).unwrap_or(0);
        let at = out[1]
            .iter()
            .position(|n| rank(n) > i)
            .unwrap_or(out[1].len());
        out[1].insert(at, g);
    }
    out
}

fn group_of(r#type: &str) -> Option<usize> {
    GROUPS.iter().position(|(_, types)| types.contains(&r#type))
}

/// Puts the group widgets of every panel at the original's bar edge where `parts` says, in order,
/// with the panel's center separators between the groups that share a zone. The start zone's
/// groups follow its sidebar button, the end zone's come before the tray and indicators.
/// Returns whether anything moved.
pub fn bar_groups(layout: &mut Layout, parts: [&[String]; 3]) -> bool {
    let parts = normalize(parts);
    let zones = [Zone::Start, Zone::Center, Zone::End];
    let mut changed = false;
    for (m, c) in layout
        .monitors
        .iter()
        .enumerate()
        .flat_map(|(m, mon)| (0..mon.containers.len()).map(move |c| (m, c)))
        .collect::<Vec<_>>()
    {
        // Every id of the monitor: a separator made below takes one that is free.
        let mut ids: Vec<String> = layout.monitors[m]
            .containers
            .iter()
            .flat_map(|c| std::iter::once(&c.id).chain(c.widgets.iter().map(|wd| &wd.id)))
            .cloned()
            .collect();
        let c = &mut layout.monitors[m].containers[c];
        if !matches!(&c.kind, ContainerKind::Panel(p) if p.position == PanelPosition::RustyBar) {
            continue;
        }
        let zone_of = |wd: &super::Widget| match wd.place {
            Place::Panel { zone, .. } => Some(zone),
            _ => None,
        };
        let mut groups: [Vec<super::Widget>; 3] = Default::default();
        let mut separators = vec![];
        let mut rest: [Vec<super::Widget>; 3] = Default::default();
        let mut center_at = None;
        for wd in &c.widgets {
            let Some(z) = zone_of(wd) else { continue };
            let k = zones.iter().position(|y| *y == z).unwrap_or(1);
            if let Some(g) = group_of(&wd.r#type) {
                if k == 1 {
                    center_at.get_or_insert(rest[1].len());
                }
                groups[g].push(wd.clone());
            } else if wd.r#type == "separator" && k == 1 {
                center_at.get_or_insert(rest[1].len());
                separators.push(wd.clone());
            } else {
                rest[k].push(wd.clone());
            }
        }
        let mut separators = separators.into_iter();
        let mut placed: [Vec<super::Widget>; 3] = Default::default();
        for (k, names) in parts.iter().enumerate() {
            let mut first = true;
            for name in names {
                let Some(g) = GROUPS.iter().position(|(h, _)| h == name) else {
                    continue;
                };
                if groups[g].is_empty() {
                    continue;
                }
                if !first {
                    // A zone with one group needs no separator, and the layout keeps none
                    // for it: the groups coming back together get new ones.
                    let mut s = separators.next().unwrap_or_else(|| {
                        let id = (1u32..)
                            .map(|n| format!("separator-{n}"))
                            .find(|id| !ids.contains(id))
                            .unwrap_or_default();
                        super::Widget {
                            id,
                            r#type: "separator".into(),
                            place: Place::Panel {
                                zone: zones[k],
                                width: Width::Auto,
                            },
                            settings: toml::Table::new(),
                        }
                    });
                    if !ids.contains(&s.id) {
                        ids.push(s.id.clone());
                    }
                    s.place = Place::Panel {
                        zone: zones[k],
                        width: Width::Auto,
                    };
                    placed[k].push(s);
                }
                first = false;
                for mut wd in groups[g].iter().cloned() {
                    let width = if k == 1 && CENTER_STRETCH.contains(&wd.r#type.as_str()) {
                        Width::Stretch
                    } else {
                        Width::Auto
                    };
                    wd.place = Place::Panel {
                        zone: zones[k],
                        width,
                    };
                    placed[k].push(wd);
                }
            }
        }
        let mut widgets = vec![];
        for (k, mut own) in rest.into_iter().enumerate() {
            let at = match k {
                0 => usize::from(
                    own.first()
                        .is_some_and(|w| w.r#type == "left-sidebar-button"),
                ),
                1 => center_at.unwrap_or(0).min(own.len()),
                _ => own
                    .iter()
                    .position(|w| w.r#type == "systray" || w.r#type == "indicators")
                    .unwrap_or(own.len()),
            };
            let tail = own.split_off(at);
            widgets.extend(own);
            widgets.append(&mut placed[k]);
            widgets.extend(tail);
        }
        widgets.extend(c.widgets.iter().filter(|wd| zone_of(wd).is_none()).cloned());
        if widgets != c.widgets {
            c.widgets = widgets;
            changed = true;
        }
    }
    changed
}

/// Moves the desktop clock (`type = "clock"` in a desktop container) of the primary monitor to
/// `(x, y)`, where it stays (`placementStrategy = "free"`). Returns whether anything changed.
pub fn desktop_clock(layout: &mut Layout, x: i32, y: i32) -> bool {
    let Some(m) = layout.primary() else {
        return false;
    };
    let mut changed = false;
    for c in layout.monitors[m].containers.iter_mut() {
        if !matches!(c.kind, ContainerKind::Desktop) {
            continue;
        }
        for wd in c.widgets.iter_mut().filter(|wd| wd.r#type == "clock") {
            let size = match wd.place {
                Place::Desktop { size, .. } => size,
                _ => continue,
            };
            let place = Place::Desktop { x, y, size };
            let free = toml::Value::String("free".into());
            if wd.place != place || wd.settings.get("placementStrategy") != Some(&free) {
                wd.place = place;
                wd.settings.insert("placementStrategy".into(), free);
                changed = true;
            }
        }
    }
    changed
}

/// Applies `f` to the layout file at `path` and saves it the way edit mode does (atomically,
/// keeping the previous file); a shell watching it reloads. A missing or unreadable file is
/// left to the shell, which builds its default: nothing to arrange yet.
pub fn in_file(path: &Path, f: impl FnOnce(&mut Layout) -> bool) -> io::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let (mut store, mut loaded) = Store::open(path);
    if loaded.source == super::LoadSource::Empty {
        return Ok(false);
    }
    if !f(&mut loaded.layout) {
        return Ok(false);
    }
    store.save(&loaded.layout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout_model::Container;
    use crate::layout_model::rusty::{Monitor, from_options};
    use serde_json::json;

    fn monitor() -> Monitor {
        Monitor {
            connector: "DP-1".into(),
        }
    }

    fn parts(l: &[&str], c: &[&str], r: &[&str]) -> [Vec<String>; 3] {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        [v(l), v(c), v(r)]
    }

    fn panel(l: &Layout) -> &Container {
        l.monitors[0]
            .containers
            .iter()
            .find(|c| c.id == "panel")
            .unwrap()
    }

    fn zones(l: &Layout) -> Vec<(String, Zone, Width)> {
        panel(l)
            .widgets
            .iter()
            .map(|w| match w.place {
                Place::Panel { zone, width } => (w.id.clone(), zone, width),
                _ => unreachable!(),
            })
            .collect()
    }

    fn arranged(p: &[Vec<String>; 3]) -> (bool, Layout) {
        let mut l = from_options(&json!({}), &monitor());
        let moved = bar_groups(&mut l, [&p[0], &p[1], &p[2]]);
        (moved, l)
    }

    #[test]
    fn the_original_s_own_layout_leaves_the_seed_as_it_is() {
        let (moved, _) = arranged(&parts(&[], &["media", "workspaces", "clock"], &[]));
        assert!(!moved);
        let (moved, _) = arranged(&parts(&["nonsense"], &[], &[]));
        assert!(
            !moved,
            "unknown names and missing groups are the original's own places"
        );
    }

    #[test]
    fn a_layout_seeded_from_the_original_keeps_its_bar_layout() {
        let o = json!({"bar": {"layout": {"left": ["clock"], "center": ["workspaces"], "right": ["media"]}}});
        let seeded = from_options(&o, &monitor());
        let (_, l) = arranged(&parts(&["clock"], &["workspaces"], &["media"]));
        assert_eq!(seeded, l);
        let stock = from_options(&json!({}), &monitor());
        let o = json!({"bar": {"layout": {"center": ["media", "workspaces", "clock"]}}});
        assert_eq!(from_options(&o, &monitor()), stock);
    }

    #[test]
    fn spread_puts_each_group_on_its_side_with_one_widget_per_module() {
        let (moved, l) = arranged(&parts(&["media"], &["workspaces"], &["clock"]));
        assert!(moved);
        let (s, c, e) = (Zone::Start, Zone::Center, Zone::End);
        let (a, st) = (Width::Auto, Width::Stretch);
        let got = zones(&l);
        let got: Vec<(&str, Zone, Width)> =
            got.iter().map(|(i, z, w)| (i.as_str(), *z, *w)).collect();
        assert_eq!(
            got,
            [
                ("left-sidebar-button", s, a),
                ("resources", s, a),
                ("media", s, a),
                ("active-window", s, st),
                ("workspaces", c, a),
                ("weather", e, a),
                ("spacer", e, st),
                ("clock", e, a),
                ("util-buttons", e, a),
                ("battery", e, a),
                ("systray", e, a),
                ("indicators", e, a),
            ]
        );
    }

    /// A zone with one group has no separator; the groups put back in the center get theirs
    /// again, so each is a group of its own as before (without them the center is one run and
    /// its stretching media and clock shrink to their least length).
    #[test]
    fn the_groups_back_in_the_center_are_the_seed_again() {
        let stock = from_options(&json!({}), &monitor());
        for there in [
            parts(&["media"], &["workspaces"], &["clock"]),
            parts(&["workspaces"], &["media", "clock"], &[]),
        ] {
            let (_, mut l) = arranged(&there);
            assert_ne!(l, stock);
            let back = parts(&[], &["media", "workspaces", "clock"], &[]);
            assert!(bar_groups(&mut l, [&back[0], &back[1], &back[2]]));
            assert_eq!(l, stock);
        }
    }

    #[test]
    fn groups_sharing_a_zone_keep_a_separator_between_them() {
        let (_, l) = arranged(&parts(&["workspaces"], &["clock", "media"], &[]));
        let ids: Vec<String> = zones(&l)
            .into_iter()
            .filter(|(_, z, _)| *z == Zone::Center)
            .map(|(i, _, _)| i)
            .collect();
        assert_eq!(
            ids,
            [
                "clock",
                "util-buttons",
                "battery",
                "separator-1",
                "resources",
                "media"
            ]
        );
        let l2 = {
            let mut l2 = l.clone();
            let p = parts(&["workspaces"], &["clock", "media"], &[]);
            assert!(
                !bar_groups(&mut l2, [&p[0], &p[1], &p[2]]),
                "arranging twice is one arrangement"
            );
            l2
        };
        assert_eq!(l2, l);
    }

    #[test]
    fn a_panel_on_its_own_edge_is_the_layout_s_to_arrange() {
        let mut l = from_options(&json!({}), &monitor());
        for c in &mut l.monitors[0].containers {
            if let ContainerKind::Panel(p) = &mut c.kind {
                p.position = PanelPosition::Edge {
                    edge: crate::layout_model::Edge::Bottom,
                    thickness: 40,
                };
            }
        }
        let p = parts(&["media"], &["workspaces"], &["clock"]);
        assert!(!bar_groups(&mut l, [&p[0], &p[1], &p[2]]));
    }

    #[test]
    fn the_desktop_clock_stays_where_it_is_put() {
        let mut l = from_options(&json!({}), &monitor());
        assert!(desktop_clock(&mut l, 640, 300));
        assert!(!desktop_clock(&mut l, 640, 300));
        let d = l.monitors[0]
            .containers
            .iter()
            .find(|c| c.id == "desktop")
            .unwrap();
        let clock = d.widgets.iter().find(|w| w.r#type == "clock").unwrap();
        assert!(matches!(clock.place, Place::Desktop { x: 640, y: 300, .. }));
        assert_eq!(clock.settings["placementStrategy"].as_str(), Some("free"));
    }

    #[test]
    fn a_layout_file_is_arranged_in_place_and_a_missing_one_left_to_the_shell() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("layout.toml");
        let p = parts(&["media"], &["workspaces"], &["clock"]);
        let arrange = |l: &mut Layout| bar_groups(l, [&p[0], &p[1], &p[2]]);
        assert!(!in_file(&path, arrange).unwrap());
        assert!(!path.exists());
        let seed = from_options(&json!({}), &monitor());
        std::fs::write(&path, seed.to_toml()).unwrap();
        assert!(in_file(&path, arrange).unwrap());
        let (_, loaded) = Store::open(&path);
        assert_eq!(zones(&loaded.layout)[1].0, "resources");
    }
}
