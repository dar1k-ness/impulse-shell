use super::migrate::{self, Migration};
use super::*;

const SAMPLE: &str = r##"
version = 5

[[monitor]]
connector = "DP-1"
primary = true
edid = { make = "Dell", model = "U2720Q", serial = "ABC1" }

[[monitor.container]]
id = "bar"
kind = "panel"
edge = "top"
thickness = 40
length = "60%"
align = "center"
floating = true

[[monitor.container.widget]]
id = "workspaces"
type = "workspaces"
zone = "start"
width = "auto"

[[monitor.container.widget]]
id = "clock"
type = "clock"
zone = "center"
width = 120
settings = { format = "hh:mm", seconds = false }

[[monitor.container]]
id = "desk"
kind = "desktop"

[[monitor.container.widget]]
id = "weather"
type = "weather"
x = 40
y = -20
w = 300
h = 160

[[monitor.container]]
id = "right"
kind = "sidebar"
side = "right"
columns = 4

[[monitor.container.widget]]
id = "media"
type = "media"
size = 4

[[monitor]]
connector = "HDMI-A-1"
"##;

fn sample() -> Layout {
    Layout::parse(SAMPLE).unwrap()
}

fn out(connector: &str, serial: Option<&str>) -> MonitorId {
    MonitorId {
        connector: connector.into(),
        edid: serial.map(|s| Edid {
            make: "Dell".into(),
            model: "U2720Q".into(),
            serial: s.into(),
        }),
    }
}

fn panel_widget(id: &str, zone: Zone) -> Widget {
    Widget {
        id: id.into(),
        r#type: id.into(),
        place: Place::Panel {
            zone,
            width: Width::Auto,
        },
        settings: toml::Table::new(),
    }
}

fn zone_ids(l: &Layout, zone: Zone) -> Vec<String> {
    l.monitors[0].containers[0]
        .widgets
        .iter()
        .filter(|w| matches!(w.place, Place::Panel { zone: z, .. } if z == zone))
        .map(|w| w.id.clone())
        .collect()
}

fn schema_err(text: &str) -> String {
    match Layout::parse(text) {
        Err(LoadError::Schema(e)) => e.to_string(),
        Err(LoadError::Format(e)) => e,
        other => panic!("expected a schema error, got {other:?}"),
    }
}

#[test]
fn parses_every_container_and_place() {
    let l = sample();
    assert_eq!(l.monitors.len(), 2);
    let m = &l.monitors[0];
    assert!(m.primary);
    assert_eq!(m.edid.as_ref().unwrap().serial, "ABC1");
    assert_eq!(
        m.containers[0].kind,
        ContainerKind::Panel(PanelProps {
            position: PanelPosition::Edge {
                edge: Edge::Top,
                thickness: 40,
            },
            length: Length::Percent(60),
            align: Align::Center,
            floating: true,
        })
    );
    let (_, clock) = m.widget("clock").unwrap();
    assert_eq!(
        clock.place,
        Place::Panel {
            zone: Zone::Center,
            width: Width::Px(120)
        }
    );
    assert_eq!(clock.settings["format"].as_str(), Some("hh:mm"));
    assert_eq!(
        m.widget("weather").unwrap().1.place,
        Place::Desktop {
            x: 40,
            y: -20,
            size: Some((300, 160))
        }
    );
    assert_eq!(
        m.containers[2].kind,
        ContainerKind::Sidebar(SidebarProps {
            side: Side::Right,
            columns: 4
        })
    );
    assert_eq!(
        m.widget("media").unwrap().1.place,
        Place::Sidebar { size: 4 }
    );
    assert!(l.monitors[1].containers.is_empty());
}

#[test]
fn round_trips_through_toml() {
    let l = sample();
    let text = l.to_toml();
    assert!(
        text.starts_with(&format!("version = {VERSION}\n")),
        "{text}"
    );
    // A default panel width is left out, so the user's file only holds what they chose.
    assert!(!text.contains("width = \"auto\""), "{text}");
    assert_eq!(Layout::parse(&text).unwrap(), l);
    assert_eq!(Layout::parse(&text).unwrap().to_toml(), text);
}

#[test]
fn widget_settings_are_kept_verbatim() {
    let text = SAMPLE.replace(
        "settings = { format = \"hh:mm\", seconds = false }",
        "settings = { format = \"hh:mm\", nested = { a = [1, 2] }, future_key = 3.5 }",
    );
    let l = Layout::parse(&text).unwrap();
    let s = &l.monitors[0].widget("clock").unwrap().1.settings;
    assert_eq!(s["future_key"].as_float(), Some(3.5));
    assert_eq!(s["nested"]["a"].as_array().unwrap().len(), 2);
    assert_eq!(Layout::parse(&l.to_toml()).unwrap(), l);
}

#[test]
fn lengths_and_widths() {
    for (src, want) in [
        ("\"fill\"", Length::Fill),
        ("\"100%\"", Length::Percent(100)),
        ("800", Length::Px(800)),
    ] {
        let l =
            Layout::parse(&SAMPLE.replace("length = \"60%\"", &format!("length = {src}"))).unwrap();
        let ContainerKind::Panel(p) = &l.monitors[0].containers[0].kind else {
            panic!()
        };
        assert_eq!(p.length, want, "{src}");
        assert_eq!(Layout::parse(&l.to_toml()).unwrap(), l);
    }
    for bad in ["\"0%\"", "\"101%\"", "\"half\"", "0", "-5"] {
        assert!(
            Layout::parse(&SAMPLE.replace("length = \"60%\"", &format!("length = {bad}"))).is_err(),
            "{bad}"
        );
    }
    let l = Layout::parse(&SAMPLE.replace("width = \"auto\"", "width = \"stretch\"")).unwrap();
    assert_eq!(
        l.monitors[0].widget("workspaces").unwrap().1.place,
        Place::Panel {
            zone: Zone::Start,
            width: Width::Stretch
        }
    );
    assert!(Layout::parse(&SAMPLE.replace("width = \"auto\"", "width = \"wide\"")).is_err());
}

#[test]
fn panel_defaults_when_optional_keys_are_missing() {
    let l = Layout::parse(
        "version = 3\n[[monitor]]\nconnector = \"DP-1\"\n[[monitor.container]]\nid = \"bar\"\nkind = \"panel\"\nedge = \"bottom\"\nthickness = 30\n",
    )
    .unwrap();
    let ContainerKind::Panel(p) = &l.monitors[0].containers[0].kind else {
        panic!()
    };
    assert_eq!(
        (p.length, p.align, p.floating),
        (Length::Fill, Align::Center, false)
    );
}

#[test]
fn rejects_what_does_not_belong() {
    assert!(
        schema_err(&SAMPLE.replace("floating = true", "floating = true\nside = \"left\""))
            .contains("`side`")
    );
    assert!(schema_err(&SAMPLE.replace("size = 4", "size = 4\nzone = \"start\"")).contains("zone"));
    assert!(
        schema_err(&SAMPLE.replace("primary = true\n", "primary = true\ncolour = 1\n"))
            .contains("colour")
    );
    assert!(schema_err(&SAMPLE.replace("kind = \"desktop\"", "kind = \"dock\"")).contains("dock"));
    assert!(schema_err(&SAMPLE.replace("edge = \"top\"\n", "")).contains("edge"));
}

#[test]
fn validation_rejects_inconsistent_layouts() {
    let cases = [
        (
            SAMPLE.replace("id = \"clock\"", "id = \"workspaces\""),
            "workspaces",
        ),
        (SAMPLE.replace("id = \"desk\"", "id = \"bar\""), "bar"),
        (SAMPLE.replace("id = \"clock\"", "id = \"\""), "id"),
        (SAMPLE.replace("type = \"clock\"", "type = \"\""), "type"),
        (
            SAMPLE.replace("thickness = 40", "thickness = 0"),
            "thickness",
        ),
        (SAMPLE.replace("size = 4", "size = 5"), "size"),
        (SAMPLE.replace("w = 300", "w = 0"), "w"),
        (
            SAMPLE.replace(
                "connector = \"HDMI-A-1\"",
                "connector = \"HDMI-A-1\"\nprimary = true",
            ),
            "primary",
        ),
        (
            SAMPLE.replace("connector = \"HDMI-A-1\"", "connector = \"\""),
            "connector",
        ),
    ];
    for (text, needle) in cases {
        let e = schema_err(&text);
        assert!(e.contains(needle), "{needle}: {e}");
    }
    let dup = format!(
        "{SAMPLE}\n[[monitor]]\nconnector = \"DP-1\"\nedid = {{ make = \"Dell\", model = \"U2720Q\", serial = \"ABC1\" }}\n"
    );
    assert!(schema_err(&dup).contains("DP-1"));
    // The same id may be used on another monitor.
    let other = SAMPLE.replace(
        "connector = \"HDMI-A-1\"",
        "connector = \"HDMI-A-1\"\n[[monitor.container]]\nid = \"bar\"\nkind = \"desktop\"",
    );
    Layout::parse(&other).unwrap();
}

#[test]
fn version_errors() {
    assert_eq!(
        Layout::parse("[[monitor]]\nconnector = \"DP-1\"\n"),
        Err(LoadError::MissingVersion)
    );
    assert!(matches!(
        Layout::parse("version = 0"),
        Err(LoadError::BadVersion(_))
    ));
    assert!(matches!(
        Layout::parse("version = \"1\""),
        Err(LoadError::BadVersion(_))
    ));
    assert_eq!(
        Layout::parse(&format!(
            "version = {}\n[[monitor]]\nconnector = \"DP-1\"\nhologram = true\n",
            VERSION + 1
        )),
        Err(LoadError::FutureVersion {
            found: VERSION + 1,
            supported: VERSION
        })
    );
    assert!(matches!(
        Layout::parse("version = [1"),
        Err(LoadError::Syntax(_))
    ));
}

fn rename_kind(t: &mut toml::Table, from: &str, to: &str) {
    for m in t
        .get_mut("monitor")
        .and_then(|v| v.as_array_mut())
        .into_iter()
        .flatten()
    {
        for c in m
            .get_mut("container")
            .and_then(|v| v.as_array_mut())
            .into_iter()
            .flatten()
        {
            let t = c.as_table_mut().unwrap();
            if let Some(v) = t.remove(from) {
                t.insert(to.into(), v);
            }
        }
    }
}

#[test]
fn migrations_run_in_order_from_any_old_version() {
    // A schema history where v1 used `type` for containers and v2 used `role`; v3 to v5
    // changed nothing a desktop container has.
    const STEPS: &[Migration] = &[
        Migration {
            from: 1,
            run: |t, _| {
                rename_kind(t, "type", "role");
                Ok(())
            },
        },
        Migration {
            from: 2,
            run: |t, _| {
                rename_kind(t, "role", "kind");
                Ok(())
            },
        },
        Migration {
            from: 3,
            run: |_, _| Ok(()),
        },
        Migration {
            from: 4,
            run: |_, _| Ok(()),
        },
    ];
    let body = "[[monitor]]\nconnector = \"DP-1\"\n[[monitor.container]]\nid = \"desk\"\n";
    let want =
        migrate::parse_with(&format!("version = 5\n{body}kind = \"desktop\"\n"), STEPS).unwrap();
    assert_eq!(want.monitors[0].containers[0].kind, ContainerKind::Desktop);
    for (v, key) in [(1, "type"), (2, "role"), (3, "kind")] {
        let got = migrate::parse_with(
            &format!("version = {v}\n{body}{key} = \"desktop\"\n"),
            STEPS,
        );
        assert_eq!(got, Ok(want.clone()), "from v{v}");
    }
    assert!(matches!(
        migrate::parse_with("version = 6", STEPS),
        Err(LoadError::FutureVersion {
            found: 6,
            supported: 5
        })
    ));
}

#[test]
fn migration_failures_and_gaps_are_errors() {
    const FAILS: &[Migration] = &[Migration {
        from: 1,
        run: |_, _| Err("cannot convert".into()),
    }];
    assert_eq!(
        migrate::parse_with("version = 1", FAILS),
        Err(LoadError::Migration {
            from: 1,
            msg: "cannot convert".into()
        })
    );
    const GAP: &[Migration] = &[Migration {
        from: 2,
        run: |_, _| Ok(()),
    }];
    assert!(matches!(
        migrate::parse_with("version = 1", GAP),
        Err(LoadError::Migration { from: 1, .. })
    ));
}

#[test]
fn shipped_migrations_reach_the_current_version() {
    for (i, m) in migrate::MIGRATIONS.iter().enumerate() {
        assert_eq!(m.from as usize, i + 1);
    }
    assert_eq!(migrate::MIGRATIONS.len() as u32 + 1, VERSION);
}

#[test]
fn monitors_match_by_edid_then_connector() {
    let l = sample();
    // Same monitor, same port.
    assert_eq!(
        l.monitor_for(&out("DP-1", Some("ABC1"))),
        MonitorMatch::Edid(0)
    );
    // The layout follows the monitor to another port.
    assert_eq!(
        l.monitor_for(&out("DP-3", Some("ABC1"))),
        MonitorMatch::Edid(0)
    );
    // A monitor without EDID uses what is stored for its port.
    assert_eq!(
        l.monitor_for(&out("HDMI-A-1", None)),
        MonitorMatch::Connector(1)
    );
    assert_eq!(
        l.monitor_for(&out("DP-1", None)),
        MonitorMatch::Connector(0)
    );
    // Another monitor on the primary's port is a new monitor, not the primary.
    assert!(matches!(
        l.monitor_for(&out("DP-1", Some("ZZZ9"))),
        MonitorMatch::New(_)
    ));
}

#[test]
fn unknown_monitor_gets_a_copy_of_the_primary_layout() {
    let mut l = sample();
    l.monitors.swap(0, 1);
    let MonitorMatch::New(m) = l.monitor_for(&out("DP-2", Some("NEW"))) else {
        panic!()
    };
    assert_eq!(m.connector, "DP-2");
    assert_eq!(m.edid.as_ref().unwrap().serial, "NEW");
    assert!(!m.primary);
    assert_eq!(m.containers, l.monitors[1].containers);
    let MonitorMatch::New(m) = Layout::default().monitor_for(&out("DP-2", None)) else {
        panic!()
    };
    assert!(m.containers.is_empty());
}

#[test]
fn identical_monitors_are_told_apart_by_connector() {
    let one = |c: &str| MonitorLayout {
        connector: c.into(),
        edid: out(c, Some("")).edid,
        primary: false,
        containers: vec![],
    };
    let mut l = Layout {
        monitors: vec![one("DP-1"), one("DP-2")],
    };
    for m in &mut l.monitors {
        m.edid.as_mut().unwrap().serial = "SAME".into();
    }
    let got = l.resolve(&[out("DP-2", Some("SAME")), out("DP-1", Some("SAME"))]);
    assert_eq!(got, [MonitorMatch::Edid(1), MonitorMatch::Edid(0)]);
    // Moved together to new ports: each stored layout is still used once.
    let got = l.resolve(&[out("DP-5", Some("SAME")), out("DP-6", Some("SAME"))]);
    assert_eq!(got, [MonitorMatch::Edid(0), MonitorMatch::Edid(1)]);
    let got = l.resolve(&[
        out("DP-5", Some("SAME")),
        out("DP-6", Some("SAME")),
        out("DP-7", Some("SAME")),
    ]);
    assert!(matches!(got[2], MonitorMatch::New(_)));
}

#[test]
fn live_follows_monitors_by_edid_across_ports() {
    // Two stored monitors with EDIDs; the cables swap ports. Each output shows the layout of
    // the monitor plugged into it, not of the port it was on.
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("layout.toml");
    let mut l = sample();
    l.monitors[0].connector = "DP-1".into();
    l.monitors[0].edid = out("DP-1", Some("AAA")).edid;
    l.monitors[1].connector = "DP-2".into();
    l.monitors[1].edid = out("DP-2", Some("BBB")).edid;
    Store::open(&p).0.save(&l).unwrap();
    let mut live = Live::open(&p);
    assert!(live.set_outputs(&[out("DP-2", Some("AAA")), out("DP-1", Some("BBB"))]));
    let serial = |c: &str| live.monitor(c).unwrap().edid.clone().unwrap().serial;
    assert_eq!(
        (serial("DP-2"), serial("DP-1")),
        ("AAA".into(), "BBB".into())
    );
}

#[test]
fn a_blank_edid_duplicates_a_missing_one() {
    let mut l = sample();
    let mut copy = l.monitors[1].clone();
    copy.primary = false;
    copy.edid = match copy.edid {
        None => Some(Edid::default()),
        Some(_) => None,
    };
    l.monitors.push(copy);
    assert!(l.validate().is_err());
}

#[test]
fn sizes_have_upper_bounds() {
    let base = sample().to_toml();
    for (from, to) in [
        ("columns = ", "columns = 100000 #"),
        ("thickness = ", "thickness = 4000000 #"),
    ] {
        if base.contains(from) {
            let bad = base.replacen(from, to, 1);
            assert!(Layout::parse(&bad).is_err(), "{to}");
        }
    }
    let valid = |place: Place| {
        let mut l = sample();
        let w = l
            .monitors
            .iter_mut()
            .flat_map(|m| m.containers.iter_mut())
            .flat_map(|c| c.widgets.iter_mut())
            .find(|w| matches!(w.place, Place::Desktop { .. }))
            .unwrap();
        w.place = place;
        l.validate().is_ok()
    };
    assert!(!valid(Place::Desktop {
        x: i32::MAX,
        y: 0,
        size: None,
    }));
    assert!(!valid(Place::Desktop {
        x: 0,
        y: 0,
        size: Some((MAX_PX + 1, 10)),
    }));
    assert!(valid(Place::Desktop {
        x: -(MAX_PX as i32),
        y: MAX_PX as i32,
        size: Some((MAX_PX, 1)),
    }));
}

#[test]
fn empty_edid_counts_as_none() {
    let l = sample();
    let blank = MonitorId {
        connector: "HDMI-A-1".into(),
        edid: Some(Edid::default()),
    };
    assert_eq!(l.monitor_for(&blank), MonitorMatch::Connector(1));
    let MonitorMatch::New(m) = l.monitor_for(&MonitorId {
        connector: "X-1".into(),
        edid: Some(Edid::default()),
    }) else {
        panic!()
    };
    assert_eq!(m.edid, None);
}

#[test]
fn editor_attach_adds_new_monitors_and_follows_moved_ones() {
    let mut e = Editor::new(sample());
    e.apply(Edit::RemoveWidget {
        monitor: 0,
        id: "media".into(),
    })
    .unwrap();
    let rev = e.revision();
    let idx = e.attach(&[out("DP-3", Some("ABC1")), out("DP-2", Some("NEW"))]);
    assert_eq!(idx, [0, 2]);
    assert!(e.revision() > rev);
    assert_eq!(e.layout().monitors[0].connector, "DP-3");
    assert_eq!(e.layout().monitors[2].connector, "DP-2");
    // Undo keeps the attached monitor and the new connector name.
    assert!(e.undo());
    assert_eq!(e.layout().monitors.len(), 3);
    assert_eq!(e.layout().monitors[0].connector, "DP-3");
    assert!(e.layout().monitors[0].widget("media").is_some());
    // Attaching what is already attached changes nothing.
    let rev = e.revision();
    assert_eq!(e.attach(&[out("DP-3", Some("ABC1"))]), [0]);
    assert_eq!(e.revision(), rev);
}

#[test]
fn editor_undo_redo() {
    let mut e = Editor::new(sample());
    let original = e.layout().clone();
    assert!(!e.can_undo() && !e.undo());
    assert!(
        e.apply(Edit::RemoveWidget {
            monitor: 0,
            id: "clock".into()
        })
        .unwrap()
    );
    let mut s = toml::Table::new();
    s.insert("unit".into(), "C".into());
    e.apply(Edit::SetSettings {
        monitor: 0,
        id: "weather".into(),
        settings: s.clone(),
    })
    .unwrap();
    let edited = e.layout().clone();
    assert_eq!(edited.monitors[0].widget("weather").unwrap().1.settings, s);
    assert!(e.undo() && e.undo());
    assert_eq!(e.layout(), &original);
    assert!(!e.can_undo() && e.can_redo());
    assert!(e.redo() && e.redo());
    assert_eq!(e.layout(), &edited);
    assert!(!e.redo());
    // A new edit after undo drops the redo branch.
    e.undo();
    e.apply(Edit::SetPrimary { monitor: 1 }).unwrap();
    assert!(!e.can_redo());
    assert!(e.layout().monitors[1].primary && !e.layout().monitors[0].primary);
}

#[test]
fn rejected_and_no_op_edits_leave_no_trace() {
    let mut e = Editor::new(sample());
    let before = e.layout().clone();
    let rev = e.revision();
    let bad = [
        (
            Edit::RemoveWidget {
                monitor: 9,
                id: "clock".into(),
            },
            EditError::NoMonitor(9),
        ),
        (
            Edit::RemoveWidget {
                monitor: 0,
                id: "nope".into(),
            },
            EditError::NoWidget("nope".into()),
        ),
        (
            Edit::RemoveContainer {
                monitor: 0,
                id: "nope".into(),
            },
            EditError::NoContainer("nope".into()),
        ),
        (
            Edit::SetContainerKind {
                monitor: 0,
                id: "bar".into(),
                kind: ContainerKind::Desktop,
            },
            EditError::KindChange {
                from: "panel",
                to: "desktop",
            },
        ),
    ];
    for (edit, want) in bad {
        assert_eq!(e.apply(edit), Err(want));
    }
    // A place that doesn't fit the target container fails validation.
    assert!(matches!(
        e.apply(Edit::MoveWidget {
            monitor: 0,
            id: "clock".into(),
            container: "desk".into(),
            place: Place::Sidebar { size: 1 },
            index: None,
        }),
        Err(EditError::Invalid(_))
    ));
    // A duplicate id too.
    assert!(matches!(
        e.apply(Edit::AddWidget {
            monitor: 0,
            container: "bar".into(),
            widget: panel_widget("clock", Zone::End),
            index: None,
        }),
        Err(EditError::Invalid(_))
    ));
    // A batch is all or nothing.
    assert!(
        e.apply_all(&[
            Edit::RemoveWidget {
                monitor: 0,
                id: "clock".into()
            },
            Edit::RemoveWidget {
                monitor: 0,
                id: "clock".into()
            },
        ])
        .is_err()
    );
    // Setting what is already there is not an undo step.
    assert_eq!(e.apply(Edit::SetPrimary { monitor: 0 }), Ok(false));
    assert_eq!(e.layout(), &before);
    assert_eq!(e.revision(), rev);
    assert!(!e.can_undo());
}

#[test]
fn batch_is_one_undo_step() {
    let mut e = Editor::new(sample());
    let before = e.layout().clone();
    let id = e.layout().monitors[0].fresh_id("clock");
    assert_eq!(id, "clock-2");
    e.apply_all(&[
        Edit::AddWidget {
            monitor: 0,
            container: "bar".into(),
            widget: panel_widget(&id, Zone::End),
            index: None,
        },
        Edit::RemoveContainer {
            monitor: 0,
            id: "right".into(),
        },
    ])
    .unwrap();
    assert!(e.layout().monitors[0].container("right").is_none());
    assert!(e.undo());
    assert_eq!(e.layout(), &before);
    assert!(!e.can_undo());
}

#[test]
fn widgets_are_ordered_within_their_zone() {
    let mut e = Editor::new(sample());
    for (id, index) in [("a", None), ("b", Some(0)), ("c", Some(1)), ("d", Some(99))] {
        e.apply(Edit::AddWidget {
            monitor: 0,
            container: "bar".into(),
            widget: panel_widget(id, Zone::Start),
            index,
        })
        .unwrap();
    }
    assert_eq!(
        zone_ids(e.layout(), Zone::Start),
        ["b", "c", "workspaces", "a", "d"]
    );
    // Drag "a" to the front of the center zone, then within it to the end.
    let to_center = |index| Edit::MoveWidget {
        monitor: 0,
        id: "a".into(),
        container: "bar".into(),
        place: Place::Panel {
            zone: Zone::Center,
            width: Width::Stretch,
        },
        index,
    };
    e.apply(to_center(Some(0))).unwrap();
    assert_eq!(zone_ids(e.layout(), Zone::Center), ["a", "clock"]);
    assert_eq!(
        zone_ids(e.layout(), Zone::Start),
        ["b", "c", "workspaces", "d"]
    );
    e.apply(to_center(Some(1))).unwrap();
    assert_eq!(zone_ids(e.layout(), Zone::Center), ["clock", "a"]);
    // A resize (index None) keeps the position.
    e.apply(Edit::MoveWidget {
        monitor: 0,
        id: "c".into(),
        container: "bar".into(),
        place: Place::Panel {
            zone: Zone::Start,
            width: Width::Px(64),
        },
        index: None,
    })
    .unwrap();
    assert_eq!(
        zone_ids(e.layout(), Zone::Start),
        ["b", "c", "workspaces", "d"]
    );
}

#[test]
fn widget_moves_between_containers() {
    let mut e = Editor::new(sample());
    e.apply(Edit::MoveWidget {
        monitor: 0,
        id: "clock".into(),
        container: "desk".into(),
        place: Place::Desktop {
            x: 10,
            y: 10,
            size: Some((200, 80)),
        },
        index: None,
    })
    .unwrap();
    let m = &e.layout().monitors[0];
    let (ci, w) = m.widget("clock").unwrap();
    assert_eq!(m.containers[ci].id, "desk");
    assert_eq!(w.settings["format"].as_str(), Some("hh:mm"));
}

#[test]
fn containers_are_added_changed_and_removed() {
    let mut e = Editor::new(sample());
    let dock = Container {
        id: "dock".into(),
        kind: ContainerKind::Panel(PanelProps {
            position: PanelPosition::Edge {
                edge: Edge::Bottom,
                thickness: 56,
            },
            length: Length::Px(600),
            align: Align::Center,
            floating: true,
        }),
        widgets: vec![],
    };
    e.apply(Edit::AddContainer {
        monitor: 1,
        container: dock.clone(),
        index: None,
    })
    .unwrap();
    let left = ContainerKind::Panel(PanelProps {
        position: PanelPosition::Edge {
            edge: Edge::Left,
            thickness: 48,
        },
        length: Length::Fill,
        align: Align::Start,
        floating: false,
    });
    e.apply(Edit::SetContainerKind {
        monitor: 1,
        id: "dock".into(),
        kind: left.clone(),
    })
    .unwrap();
    assert_eq!(e.layout().monitors[1].containers[0].kind, left);
    let text = e.layout().to_toml();
    assert_eq!(&Layout::parse(&text).unwrap(), e.layout());
    e.apply(Edit::RemoveContainer {
        monitor: 0,
        id: "bar".into(),
    })
    .unwrap();
    assert!(e.layout().monitors[0].widget("clock").is_none());
}

#[test]
fn undo_history_is_bounded() {
    let mut e = Editor::with_limit(sample(), 2);
    for i in 0..3 {
        let mut s = toml::Table::new();
        s.insert("n".into(), toml::Value::Integer(i));
        e.apply(Edit::SetSettings {
            monitor: 0,
            id: "clock".into(),
            settings: s,
        })
        .unwrap();
    }
    assert!(e.undo() && e.undo() && !e.undo());
    let s = &e.layout().monitors[0].widget("clock").unwrap().1.settings;
    assert_eq!(s["n"].as_integer(), Some(0));
}

#[test]
fn default_path_is_under_xdg_config() {
    assert_eq!(
        default_path(),
        crate::xdg::config_home().join("rusty-impulse/layout.toml")
    );
}

mod store {
    use super::super::store::Polled;
    use super::*;
    use std::fs;

    fn dir() -> (tempfile::TempDir, std::path::PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("rusty-impulse/layout.toml");
        (d, p)
    }

    #[test]
    fn missing_file_loads_empty_and_save_creates_it() {
        let (_d, p) = dir();
        let (mut s, l) = Store::open(&p);
        assert_eq!(l.source, LoadSource::Empty);
        assert!(l.problems.is_empty() && l.layout.monitors.is_empty());
        assert!(s.save(&sample()).unwrap());
        assert!(!s.previous_path().exists());
        let (_, l) = Store::open(&p);
        assert_eq!((l.source, l.layout), (LoadSource::Main, sample()));
    }

    #[test]
    fn save_keeps_previous_and_skips_identical_writes() {
        let (_d, p) = dir();
        let (mut s, _) = Store::open(&p);
        let first = sample();
        s.save(&first).unwrap();
        assert!(!s.save(&first).unwrap());
        assert!(!s.previous_path().exists());
        let mut e = Editor::new(first.clone());
        e.apply(Edit::SetPrimary { monitor: 1 }).unwrap();
        assert!(s.save(e.layout()).unwrap());
        assert_eq!(
            Layout::parse(&fs::read_to_string(s.previous_path()).unwrap()).unwrap(),
            first
        );
        assert_eq!(&Store::open(&p).1.layout, e.layout());
        // No temp files are left beside it.
        let names: Vec<_> = fs::read_dir(p.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        let mut names = names;
        names.sort();
        assert_eq!(names, ["layout.toml", "layout.toml.prev"]);
    }

    #[test]
    fn live_attach_moves_the_generation_only_when_it_adds_a_monitor() {
        let (_d, p) = dir();
        Store::open(&p).0.save(&sample()).unwrap();
        let mut live = Live::open(&p);
        let known = sample().monitors[0].connector.clone();
        let g = live.generation();
        assert_eq!(live.attach(&known), 0);
        assert_eq!(live.generation(), g);
        let n = live.editor().layout().monitors.len();
        assert_eq!(live.attach("HDMI-A-9"), n);
        assert!(live.generation() > g);
        let g = live.generation();
        assert_eq!(live.attach("HDMI-A-9"), n);
        assert_eq!(live.generation(), g);
    }

    #[cfg(unix)]
    #[test]
    fn save_keeps_the_file_mode() {
        use std::os::unix::fs::PermissionsExt;
        let (_d, p) = dir();
        let (mut s, _) = Store::open(&p);
        s.save(&sample()).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        let mut l = sample();
        l.monitors.pop();
        s.save(&l).unwrap();
        assert_eq!(
            fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn broken_file_falls_back_to_previous_and_is_kept_aside() {
        let (_d, p) = dir();
        let (mut s, _) = Store::open(&p);
        s.save(&sample()).unwrap();
        let mut second = sample();
        second.monitors.pop();
        s.save(&second).unwrap();
        fs::write(&p, "version = 1\n[[monitor]\n").unwrap();
        let (mut s, l) = Store::open(&p);
        assert_eq!(l.source, LoadSource::Previous);
        assert_eq!(l.layout, sample());
        assert!(matches!(l.problems[..], [LoadError::Syntax(_)]));
        s.save(&l.layout).unwrap();
        assert_eq!(
            fs::read_to_string(s.kept_aside().unwrap()).unwrap(),
            "version = 1\n[[monitor]\n"
        );
        assert_eq!(Store::open(&p).1.source, LoadSource::Main);
    }

    #[test]
    fn both_files_broken_loads_empty() {
        let (_d, p) = dir();
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, "nonsense").unwrap();
        let (s, l) = Store::open(&p);
        fs::write(s.previous_path(), "version = 1\nmonitor = 3\n").unwrap();
        let (_, l2) = Store::open(&p);
        assert_eq!(l.source, LoadSource::Empty);
        assert_eq!(l2.source, LoadSource::Empty);
        assert_eq!(l2.problems.len(), 2);
    }

    #[test]
    fn newer_file_is_never_overwritten() {
        let (_d, p) = dir();
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        let newer = "version = 99\nfancy = true\n";
        fs::write(&p, newer).unwrap();
        let (mut s, l) = Store::open(&p);
        assert_eq!(l.source, LoadSource::Empty);
        assert!(s.read_only().is_some());
        let err = s.save(&sample()).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(fs::read_to_string(&p).unwrap(), newer);
        assert!(!s.previous_path().exists() && s.kept_aside().is_none());
    }

    #[test]
    fn invalid_utf8_is_broken_not_read_only() {
        let (_d, p) = dir();
        let (mut s, _) = Store::open(&p);
        s.save(&sample()).unwrap();
        let mut second = sample();
        second.monitors.pop();
        s.save(&second).unwrap();
        fs::write(&p, b"version = 1\n\xff\xfe\n").unwrap();
        let (mut s, l) = Store::open(&p);
        assert_eq!((l.source, &l.layout), (LoadSource::Previous, &sample()));
        assert!(matches!(&l.problems[..], [LoadError::Syntax(m)] if m.contains("UTF-8")));
        assert!(s.read_only().is_none());
        assert!(s.save(&l.layout).unwrap());
        assert_eq!(
            fs::read(s.kept_aside().unwrap()).unwrap(),
            b"version = 1\n\xff\xfe\n"
        );
    }

    #[test]
    fn every_broken_file_is_kept() {
        let (_d, p) = dir();
        let mut kept = Vec::new();
        for junk in ["nonsense 1", "nonsense 2"] {
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, junk).unwrap();
            let (mut s, _) = Store::open(&p);
            s.save(&sample()).unwrap();
            kept.push(s.kept_aside().unwrap().to_path_buf());
        }
        assert_ne!(kept[0], kept[1]);
        let texts: Vec<_> = kept
            .iter()
            .map(|k| fs::read_to_string(k).unwrap())
            .collect();
        assert_eq!(texts, ["nonsense 1", "nonsense 2"]);
    }

    #[test]
    fn save_refuses_a_file_changed_since_it_was_read() {
        let (_d, p) = dir();
        let (mut s, _) = Store::open(&p);
        s.save(&sample()).unwrap();
        let mut theirs = sample();
        theirs.monitors.pop();
        fs::write(&p, theirs.to_toml()).unwrap();
        let mut ours = sample();
        ours.monitors[0].primary = !ours.monitors[0].primary;
        let err = s.save(&ours).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&p).unwrap(), theirs.to_toml());
        assert!(!s.previous_path().exists());
        // Once the change is read, saving works again.
        assert!(matches!(s.poll(), Polled::Changed(l) if l == theirs));
        assert!(s.save(&ours).unwrap());
    }

    #[test]
    fn poll_skips_the_store_s_own_writes() {
        let (_d, p) = dir();
        let (mut s, _) = Store::open(&p);
        assert!(matches!(s.poll(), Polled::Unchanged));
        s.save(&sample()).unwrap();
        assert!(matches!(s.poll(), Polled::Unchanged));
    }

    #[test]
    fn reload_keeps_the_layout_while_the_file_is_unusable() {
        let (_d, p) = dir();
        Store::open(&p).0.save(&sample()).unwrap();
        let mut live = Live::open(&p);
        let g = live.generation();
        for junk in ["version = 1\n[[monitor]\n", "", "version = 99\n"] {
            fs::write(&p, junk).unwrap();
            assert!(live.reload().is_some(), "{junk:?}");
            assert_eq!(live.editor().layout(), &sample(), "{junk:?}");
            assert_eq!(live.generation(), g);
            assert_eq!(live.problems().len(), 1);
        }
        let mut next = sample();
        next.monitors.pop();
        fs::write(&p, next.to_toml()).unwrap();
        assert_eq!(live.reload(), None);
        assert_eq!(live.editor().layout(), &next);
        assert!(live.generation() > g && live.problems().is_empty());
        assert_eq!(live.source(), LoadSource::Main);
    }

    #[test]
    fn reload_writes_a_removed_layout_back() {
        let (_d, p) = dir();
        Store::open(&p).0.save(&sample()).unwrap();
        let mut live = Live::open(&p);
        fs::remove_file(&p).unwrap();
        let g = live.generation();
        assert!(live.reload().is_some());
        assert_eq!(live.editor().layout(), &sample());
        assert_eq!(live.generation(), g);
        assert_eq!(Store::open(&p).1.layout, sample());
        assert_eq!(live.reload(), None);
    }

    #[test]
    fn reports_migrated_files() {
        let (_d, p) = dir();
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, SAMPLE).unwrap();
        assert_eq!(Store::open(&p).1.migrated_from, None);
    }
}

/// A version-1 layout made from the original while its weather was off: a hand-placed clock
/// and nothing else on the desktop.
const V1_WITHOUT_WEATHER: &str = r#"version = 1

[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "panel"
kind = "panel"
edge = "rusty"

[[monitor.container.widget]]
id = "weather"
type = "weather"
zone = "end"

[[monitor.container]]
id = "desktop"
kind = "desktop"

[[monitor.container.widget]]
id = "desktop-clock"
type = "desktop-clock"
x = 235
y = 545
settings = { placementStrategy = "free", style = "digital" }

[[monitor]]
connector = "HDMI-A-1"

[[monitor.container]]
id = "desktop"
kind = "desktop"
"#;

fn ri_default(r#type: &str) -> Widget {
    rusty::desktop_widgets(&serde_json::json!({}))
        .into_iter()
        .find(|w| w.r#type == r#type)
        .unwrap()
}

#[test]
fn version_1_desktops_get_the_original_widgets_they_lack() {
    let l = Layout::parse(V1_WITHOUT_WEATHER).unwrap();
    let v2 = Layout::parse(&V1_WITHOUT_WEATHER.replace("version = 1", "version = 2")).unwrap();
    let clock = v2.monitors[0].containers[1].widgets[0].clone();
    // The weather goes under the clock, as the original stacks them; the clock stays exactly as it was.
    assert_eq!(
        l.monitors[0].containers[1].widgets,
        vec![ri_default("weather"), clock]
    );
    assert_eq!(
        l.monitors[0].containers[1].widgets[0].place,
        Place::Desktop {
            x: 400,
            y: 100,
            size: None
        }
    );
    // A desktop with nothing on it gets both, the panel nothing.
    assert_eq!(
        l.monitors[1].containers[0].widgets,
        vec![ri_default("weather"), ri_default("clock")]
    );
    assert_eq!(l.monitors[0].containers[0], v2.monitors[0].containers[0]);
    // Migrated once, it stays as it is.
    assert_eq!(Layout::parse(&l.to_toml()).unwrap(), l);
}

#[test]
fn the_migration_keeps_entries_and_their_order_and_finds_a_free_id() {
    // Both there, in the user's order and with the user's ids: nothing changes.
    let both = V1_WITHOUT_WEATHER.replace(
        "settings = { placementStrategy = \"free\", style = \"digital\" }\n",
        "settings = { placementStrategy = \"free\", style = \"digital\" }\n\n\
         [[monitor.container.widget]]\nid = \"rain\"\ntype = \"desktop-weather\"\nx = 7\ny = 8\n",
    );
    let l = Layout::parse(&both).unwrap();
    let v2 = Layout::parse(&both.replace("version = 1", "version = 2")).unwrap();
    assert_eq!(l.monitors[0].containers, v2.monitors[0].containers);
    // Only the weather: the clock goes over it, under an id nothing else on the monitor has.
    let weather_only = V1_WITHOUT_WEATHER
        .replace(
            "id = \"desktop-clock\"\ntype = \"desktop-clock\"",
            "id = \"desktop-clock\"\ntype = \"desktop-weather\"",
        )
        .replace("id = \"panel\"", "id = \"desktop-clock-2\"");
    let l = Layout::parse(&weather_only).unwrap();
    let w = &l.monitors[0].containers[1].widgets;
    assert_eq!(
        w.iter()
            .map(|w| (w.id.as_str(), w.r#type.as_str()))
            .collect::<Vec<_>>(),
        [("desktop-clock", "weather"), ("desktop-clock-3", "clock")]
    );
    assert_eq!(w[1].place, ri_default("clock").place);
    assert_eq!(w[1].settings, ri_default("clock").settings);
}

/// The migration adds only widgets whose type shows while its widgets.toml `enabledBy` flag
/// is on, so an added one shows nothing until the original's settings turn it on.
#[test]
fn the_migration_adds_only_types_the_original_settings_turn_on() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml/widgets.toml");
    let mut reg = crate::widgets::Registry::default();
    assert!(reg.load_manifest_file(&manifest).is_empty());
    for w in rusty::desktop_widgets(&serde_json::json!({})) {
        let t = reg.get(&w.r#type).unwrap();
        let desktop = t.variant(crate::widgets::Placement::Desktop).unwrap();
        assert!(desktop.enabled_by.is_some(), "{}", w.r#type);
        assert!(
            t.resolve_settings(&w.settings).errors.is_empty(),
            "{}",
            w.r#type
        );
    }
}

/// The migration adds the original's widgets where the user's options put them, as the first layout
/// made from those options does, not at the original's defaults.
#[test]
fn the_migration_places_added_widgets_where_the_original_s_options_put_them() {
    let d = tempfile::tempdir().unwrap();
    let config = d.path().join("config.json");
    std::fs::write(
        &config,
        r#"{"background": {"widgets": {
            "clock": {"x": 1500, "y": 30, "placementStrategy": "free"},
            "weather": {"x": 60.4, "y": 700, "placementStrategy": "mostBusy"}
        }}}"#,
    )
    .unwrap();
    let p = d.path().join("layout.toml");
    std::fs::write(&p, V1_WITHOUT_WEATHER).unwrap();
    let inputs = migrate::Inputs {
        rusty_impulse_config: Some(config.clone()),
    };
    let (_, loaded) = Store::open_with(&p, inputs);
    assert_eq!(loaded.migrated_from, Some(1));
    let options = rusty::read_options(&config).unwrap();
    let from_options = rusty::desktop_widgets(&options);
    let at = |x, y| Place::Desktop { x, y, size: None };
    assert_eq!(
        from_options.iter().map(|w| &w.place).collect::<Vec<_>>(),
        [&at(60, 700), &at(1500, 30)]
    );
    // The first monitor gets the weather the user placed; the clock there is the user's own.
    assert_eq!(
        loaded.layout.monitors[0].containers[1].widgets[0],
        from_options[0]
    );
    assert_eq!(
        loaded.layout.monitors[1].containers[0].widgets,
        from_options
    );
}

/// Version 3 drops the copy of the original's placement setting version 2 kept on the original's own background
/// widgets (the original's settings place them, live), and touches nothing else.
#[test]
fn version_2_the_original_widgets_lose_their_copy_of_the_original_s_placement() {
    let v2 = V1_WITHOUT_WEATHER.replace("version = 1", "version = 2").replace(
        "kind = \"desktop\"\n\n[[monitor.container.widget]]",
        "kind = \"desktop\"\n\n[[monitor.container.widget]]\nid = \"w\"\ntype = \"desktop-weather\"\n\
         x = 1\ny = 2\nsettings = { placementStrategy = \"mostBusy\" }\n\n\
         [[monitor.container.widget]]\nid = \"other\"\ntype = \"desktop-other\"\nx = 3\ny = 4\n\
         settings = { placementStrategy = \"free\" }\n\n[[monitor.container.widget]]",
    );
    let l = Layout::parse(&v2).unwrap();
    let w = &l.monitors[0].containers[1].widgets;
    let settings = |id: &str| {
        w.iter()
            .find(|w| w.id == id)
            .map(|w| w.settings.clone())
            .unwrap()
    };
    assert!(settings("w").is_empty());
    let mut clock = toml::Table::new();
    clock.insert("style".into(), "digital".into());
    assert_eq!(settings("desktop-clock"), clock);
    let mut other = toml::Table::new();
    other.insert("placementStrategy".into(), "free".into());
    assert_eq!(settings("other"), other);
    assert!(!l.to_toml().contains("placementStrategy = \"mostBusy\""));
}

/// Reading an old file never writes it: it stays byte for byte until the layout is edited,
/// and is then kept as `layout.toml.v1`.
#[test]
fn a_migrated_file_is_not_written_by_reading_it() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("layout.toml");
    std::fs::write(&p, V1_WITHOUT_WEATHER).unwrap();
    let (_, loaded) = Store::open(&p);
    assert_eq!(loaded.migrated_from, Some(1));
    assert_eq!(loaded.layout.monitors[0].containers[1].widgets.len(), 2);
    let mut live = Live::open(&p);
    assert_eq!(live.reload(), None);
    assert_eq!(std::fs::read_to_string(&p).unwrap(), V1_WITHOUT_WEATHER);
    assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1);
}

/// A hand-written version 2 file is migrated in memory only: reading and polling it leave it
/// byte for byte on disk, and the migrated layout keeps every id, position, size, setting
/// other than the original's own placement copy, and the order the file gives.
#[test]
fn a_manual_version_2_file_stays_on_disk_and_keeps_all_but_the_original_s_placement_copy() {
    const MANUAL_V2: &str = r#"# my layout, edited by hand
version = 2

[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "desk"
kind = "desktop"

[[monitor.container.widget]]
id = "my-weather"
type = "desktop-weather"
x = 11
y = 22
w = 300
h = 120
settings = { placementStrategy = "mostBusy", units = "metric" }

[[monitor.container.widget]]
id = "note"
type = "desktop-other"
x = 5
y = 6
settings = { placementStrategy = "free", text = "hi" }

[[monitor.container.widget]]
id = "my-clock"
type = "desktop-clock"
x = 1500
y = 30
settings = { placementStrategy = "leastBusy" }
"#;
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("layout.toml");
    std::fs::write(&p, MANUAL_V2).unwrap();
    let inputs = migrate::Inputs {
        rusty_impulse_config: None,
    };
    let (_, loaded) = Store::open_with(&p, inputs.clone());
    assert_eq!(loaded.migrated_from, Some(2));
    let mut live = Live::open_with(&p, inputs);
    assert_eq!(live.reload(), None);
    assert_eq!(std::fs::read(&p).unwrap(), MANUAL_V2.as_bytes());
    assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1);

    let table = |pairs: &[(&str, &str)]| {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), toml::Value::from(*v)))
            .collect::<toml::Table>()
    };
    let c = &loaded.layout.monitors[0].containers[0];
    assert_eq!(c.id, "desk");
    let got = c
        .widgets
        .iter()
        .map(|w| {
            (
                w.id.as_str(),
                w.r#type.as_str(),
                &w.place,
                w.settings.clone(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        got,
        [
            (
                "my-weather",
                "weather",
                &Place::Desktop {
                    x: 11,
                    y: 22,
                    size: Some((300, 120))
                },
                table(&[("units", "metric")])
            ),
            (
                "note",
                "desktop-other",
                &Place::Desktop {
                    x: 5,
                    y: 6,
                    size: None
                },
                table(&[("placementStrategy", "free"), ("text", "hi")])
            ),
            (
                "my-clock",
                "clock",
                &Place::Desktop {
                    x: 1500,
                    y: 30,
                    size: None
                },
                toml::Table::new()
            ),
        ]
    );
}

const V3_SIDEBARS: &str = r#"version = 3

[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "sliders"
kind = "panel"
edge = "top"
thickness = 40
length = "fill"
align = "center"
floating = false

[[monitor.container]]
id = "sidebar"
kind = "sidebar"
side = "right"
columns = 4

[[monitor.container.widget]]
id = "quickToggles"
type = "quickToggles"
size = 4

[[monitor.container.widget]]
id = "notificationCenter"
type = "notificationCenter"
size = 4

[[monitor]]
connector = "DP-2"

[[monitor.container]]
id = "sidebar"
kind = "sidebar"
side = "right"
columns = 5

[[monitor.container.widget]]
id = "quickToggles"
type = "quickToggles"
size = 5

[[monitor.container.widget]]
id = "mine"
type = "sliders"
size = 3
settings = { showBrightness = true, showVolume = false, showMic = true }
"#;

/// Version 4: a sidebar the first layout gave no sliders (the original's settings had them off then)
/// gets them, at the top and full width, so turning them on in the original's settings shows them; a
/// `sliders` entry already there stays where it is and loses its copy of the original's three choices.
#[test]
fn version_3_sidebars_get_the_original_s_sliders_and_lose_their_copy_of_its_choices() {
    let l = Layout::parse(V3_SIDEBARS).unwrap();
    let cells = |m: usize, c: usize| {
        l.monitors[m].containers[c]
            .widgets
            .iter()
            .map(|w| (w.id.as_str(), w.r#type.as_str(), w.place.clone()))
            .collect::<Vec<_>>()
    };
    let at = |size| Place::Sidebar { size };
    // The id "sliders" is the panel's on this monitor.
    assert_eq!(
        cells(0, 1),
        [
            ("sliders-2", "sliders", at(4)),
            ("quickToggles", "quickToggles", at(4)),
            ("notificationCenter", "notificationCenter", at(4)),
        ]
    );
    assert_eq!(
        cells(1, 0),
        [
            ("quickToggles", "quickToggles", at(5)),
            ("mine", "sliders", at(3)),
        ]
    );
    for m in &l.monitors {
        for c in &m.containers {
            assert!(c.widgets.iter().all(|w| w.settings.is_empty()), "{c:?}");
        }
    }
    // What the migration writes is a layout the shipped widget types take as it is.
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml/widgets.toml");
    let mut reg = crate::widgets::Registry::default();
    assert!(reg.load_manifest_file(&manifest).is_empty());
    let sliders = reg.get("sliders").unwrap();
    assert!(
        sliders
            .resolve_settings(&toml::Table::new())
            .errors
            .is_empty()
    );
    assert_eq!(Layout::parse(&l.to_toml()).unwrap(), l);
}

#[test]
fn version_4_desktop_types_become_the_clock_and_weather_types() {
    let v1 = r#"
version = 4

[[monitor]]
connector = "DP-1"

[[monitor.container]]
id = "desktop"
kind = "desktop"

[[monitor.container.widget]]
id = "clock"
type = "desktop-clock"
x = 10
y = 20
settings = { style = "digital" }

[[monitor.container.widget]]
id = "weather"
type = "desktop-weather"
x = 400
y = 20
"#;
    let l = Layout::parse(v1).unwrap();
    let w = &l.monitors[0].containers[0].widgets;
    assert_eq!(
        w.iter()
            .map(|w| (w.id.as_str(), w.r#type.as_str()))
            .collect::<Vec<_>>(),
        [("clock", "clock"), ("weather", "weather")]
    );
    assert_eq!(w[0].settings["style"].as_str(), Some("digital"));
    assert!(matches!(w[0].place, Place::Desktop { x: 10, y: 20, .. }));
}
