//! The top bar arrangement from a free-text wish (Russian or English).
//!
//! A clause naming a group and a side ("часы слева", "music on the right") is a hard placement;
//! style words ("like a mac", "как в винде", "compact") choose a preset; anything else gets the
//! convenient default. CLIP is only consulted for English wishes that match no keyword.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BarGroup {
    /// Media player and system resources.
    Media,
    Workspaces,
    /// Clock, date and the quick buttons next to it.
    Clock,
}

impl BarGroup {
    pub const ALL: [BarGroup; 3] = [BarGroup::Media, BarGroup::Workspaces, BarGroup::Clock];

    /// The name the original's `bar.layout` calls the group by.
    pub fn name(self) -> &'static str {
        match self {
            BarGroup::Media => "media",
            BarGroup::Workspaces => "workspaces",
            BarGroup::Clock => "clock",
        }
    }
}

/// Groups in each part of the bar, left to right; every group appears exactly once.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BarLayout {
    pub left: Vec<BarGroup>,
    pub center: Vec<BarGroup>,
    pub right: Vec<BarGroup>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Preset {
    /// Workspaces left, clock centre, media right.
    Useful,
    Mac,
    Windows,
    ClockLeft,
    Music,
    Compact,
}

use BarGroup::{Clock, Media, Workspaces};

impl Preset {
    pub(crate) const ALL: [Preset; 6] = [
        Preset::Useful,
        Preset::Mac,
        Preset::Windows,
        Preset::ClockLeft,
        Preset::Music,
        Preset::Compact,
    ];

    pub(crate) fn layout(self) -> BarLayout {
        let (left, center, right): (&[BarGroup], &[BarGroup], &[BarGroup]) = match self {
            Preset::Useful => (&[Workspaces], &[Clock], &[Media]),
            Preset::Mac => (&[Workspaces], &[], &[Media, Clock]),
            Preset::Windows => (&[Media], &[Workspaces], &[Clock]),
            Preset::ClockLeft => (&[Clock], &[Workspaces], &[Media]),
            Preset::Music => (&[Workspaces], &[Media], &[Clock]),
            Preset::Compact => (&[], &[Media, Workspaces, Clock], &[]),
        };
        BarLayout {
            left: left.to_vec(),
            center: center.to_vec(),
            right: right.to_vec(),
        }
    }

    /// What CLIP reads for the fallback.
    pub(crate) fn description(self) -> &'static str {
        match self {
            Preset::Useful => {
                "a useful, convenient and practical bar for everyday use: workspaces and apps on the left, clock in the middle, music and system stats on the right"
            }
            Preset::Mac => {
                "like macOS: open apps on the left, music, status and the clock at the right edge"
            }
            Preset::Windows => {
                "like the Windows taskbar: apps in the center, clock and status on the right"
            }
            Preset::ClockLeft => {
                "clock and date on the left, workspaces in the center, music on the right"
            }
            Preset::Music => "for listening to music: the music player front and center",
            Preset::Compact => "compact: all widgets squeezed tightly together in the middle",
        }
    }
}

/// Wishes that name no layout; in the CLIP fallback they vote for the convenient default.
pub(crate) const VAGUE_WISHES: [&str; 8] = [
    "make it better",
    "make the interface nicer",
    "customize my desktop",
    "improve the panel, I don't know how",
    "something useful",
    "surprise me",
    "make it look good",
    "change the interface to fit the wallpaper",
];

/// English entries match whole words (plural "s" allowed), Russian ones are stems.
fn hit(word: &str, keys: &[&str]) -> bool {
    keys.iter().any(|k| {
        if k.is_ascii() {
            word == *k || word.strip_suffix('s') == Some(*k)
        } else {
            word.starts_with(k)
        }
    })
}

fn slot_of(w: &str) -> Option<Slot> {
    if hit(w, &["left", "лев", "слев", "налев"]) {
        Some(Slot::Left)
    } else if hit(
        w,
        &[
            "right",
            "справ",
            "направ",
            "правы",
            "права",
            "правой",
            "правую",
            "правом",
            "право",
        ],
    ) {
        Some(Slot::Right)
    } else if hit(
        w,
        &[
            "center",
            "centre",
            "middle",
            "centered",
            "centred",
            "центр",
            "середин",
            "посередин",
            "посреди",
        ],
    ) {
        Some(Slot::Center)
    } else {
        None
    }
}

fn group_of(w: &str) -> Option<BarGroup> {
    const MEDIA: &[&str] = &[
        "music",
        "media",
        "player",
        "song",
        "track",
        "spotify",
        "resource",
        "cpu",
        "ram",
        "memory",
        "stats",
        "музык",
        "медиа",
        "плеер",
        "песн",
        "трек",
        "ресурс",
        "процессор",
        "памят",
    ];
    const SPACES: &[&str] = &[
        "workspace",
        "desktop",
        "workspaces",
        "virtual",
        "рабоч",
        "столы",
        "стол",
        "воркспейс",
        "пространств",
    ];
    const CLOCK: &[&str] = &[
        "clock",
        "time",
        "date",
        "calendar",
        "часы",
        "часов",
        "часам",
        "часики",
        "время",
        "времени",
        "дата",
        "дату",
        "календар",
    ];
    if hit(w, MEDIA) {
        Some(Media)
    } else if hit(w, SPACES) {
        Some(Workspaces)
    } else if hit(w, CLOCK) {
        Some(Clock)
    } else {
        None
    }
}

fn style_of(w: &str) -> Option<Preset> {
    if hit(
        w,
        &[
            "mac",
            "macos",
            "osx",
            "apple",
            "macbook",
            "мак",
            "макос",
            "макбук",
            "эпл",
            "эппл",
            "эпп",
        ],
    ) {
        Some(Preset::Mac)
    } else if hit(
        w,
        &[
            "windows",
            "win11",
            "win10",
            "taskbar",
            "винд",
            "виндовс",
            "таскбар",
            "винда",
        ],
    ) {
        Some(Preset::Windows)
    } else if hit(
        w,
        &[
            "compact",
            "minimal",
            "tight",
            "small",
            "компакт",
            "минимал",
            "плотн",
            "маленьк",
        ],
    ) {
        Some(Preset::Compact)
    } else if hit(
        w,
        &[
            "music",
            "song",
            "listen",
            "spotify",
            "музык",
            "песн",
            "слушаю",
            "слушать",
        ],
    ) {
        Some(Preset::Music)
    } else if hit(
        w,
        &[
            "useful",
            "convenient",
            "practical",
            "handy",
            "better",
            "nicer",
            "удобн",
            "полезн",
            "лучше",
            "практичн",
        ],
    ) {
        Some(Preset::Useful)
    } else {
        None
    }
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct Wish {
    pub style: Option<Preset>,
    pub places: Vec<(BarGroup, Slot)>,
}

fn clauses(text: &str) -> Vec<Vec<String>> {
    let lower = text.to_lowercase();
    let mut out = Vec::new();
    for part in lower.split([',', ';', '.', '!', '?', '\n']) {
        let mut cur: Vec<String> = Vec::new();
        for w in part
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
        {
            if matches!(w, "and" | "и" | "а" | "but" | "но" | "then" | "потом") {
                out.push(std::mem::take(&mut cur));
            } else {
                cur.push(w.to_string());
            }
        }
        out.push(cur);
    }
    out.retain(|c| !c.is_empty());
    out
}

pub(crate) fn parse(text: &str) -> Wish {
    let mut wish = Wish::default();
    for c in clauses(text) {
        let slot = c.iter().find_map(|w| slot_of(w));
        let group = c.iter().find_map(|w| group_of(w));
        match (group, slot) {
            (Some(g), Some(s)) => {
                wish.places.retain(|p| p.0 != g);
                wish.places.push((g, s));
            }
            _ => {
                if wish.style.is_none() {
                    wish.style = c.iter().find_map(|w| style_of(w));
                }
            }
        }
    }
    wish
}

pub(crate) fn has_latin(text: &str) -> bool {
    text.chars().any(|c| c.is_ascii_alphabetic())
}

/// `preset` with the requested groups moved: to the left edge, the end of the centre, or the
/// right edge.
pub(crate) fn arrange(preset: Preset, places: &[(BarGroup, Slot)]) -> BarLayout {
    let mut l = preset.layout();
    for &(g, s) in places {
        for part in [&mut l.left, &mut l.center, &mut l.right] {
            part.retain(|x| *x != g);
        }
        match s {
            Slot::Left => l.left.insert(0, g),
            Slot::Center => l.center.push(g),
            Slot::Right => l.right.push(g),
        }
    }
    l
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(text: &str) -> BarLayout {
        let w = parse(text);
        arrange(w.style.unwrap_or(Preset::Useful), &w.places)
    }

    fn complete(l: &BarLayout) -> bool {
        let all: Vec<BarGroup> = l
            .left
            .iter()
            .chain(&l.center)
            .chain(&l.right)
            .copied()
            .collect();
        all.len() == 3 && BarGroup::ALL.iter().all(|g| all.contains(g))
    }

    #[test]
    fn styles_in_both_languages() {
        for (text, preset) in [
            ("like on a mac", Preset::Mac),
            ("сделай как на макбуке", Preset::Mac),
            ("windows style taskbar", Preset::Windows),
            ("хочу как в винде", Preset::Windows),
            ("keep it compact", Preset::Compact),
            ("покомпактнее", Preset::Useful),
            ("компактно пожалуйста", Preset::Compact),
            ("I listen to music a lot", Preset::Music),
            ("часто слушаю музыку", Preset::Music),
            ("make my top bar more useful", Preset::Useful),
            ("сделай удобнее", Preset::Useful),
        ] {
            let got = parse(text).style.unwrap_or(Preset::Useful);
            assert_eq!(got, preset, "{text:?}");
        }
    }

    #[test]
    fn placements_in_both_languages() {
        assert_eq!(parse("clock on the left").places, [(Clock, Slot::Left)]);
        assert_eq!(parse("часы слева").places, [(Clock, Slot::Left)]);
        assert_eq!(
            parse("музыку по центру, а рабочие столы справа").places,
            [(Media, Slot::Center), (Workspaces, Slot::Right)]
        );
        assert_eq!(
            parse("workspaces in the middle and the time on the right").places,
            [(Workspaces, Slot::Center), (Clock, Slot::Right)]
        );
        assert_eq!(parse("время в правом углу").places, [(Clock, Slot::Right)]);
        assert!(
            parse("часто меняю обои").places.is_empty(),
            "«часто» is not the clock"
        );
        assert!(
            parse("поправь панель").places.is_empty(),
            "«поправь» is not a side"
        );
        // a later placement of the same group wins
        assert_eq!(
            parse("clock left, clock right").places,
            [(Clock, Slot::Right)]
        );
    }

    #[test]
    fn arrangements_are_complete_and_follow_the_wish() {
        let l = layout("like a mac, clock on the left");
        assert_eq!(
            l,
            BarLayout {
                left: vec![Clock, Workspaces],
                center: vec![],
                right: vec![Media]
            }
        );
        let l = layout("часы слева");
        assert_eq!(
            l,
            BarLayout {
                left: vec![Clock, Workspaces],
                center: vec![],
                right: vec![Media]
            }
        );
        assert_eq!(layout("whatever"), Preset::Useful.layout());
        for p in Preset::ALL {
            assert!(complete(&p.layout()), "{p:?}");
            for g in BarGroup::ALL {
                for s in [Slot::Left, Slot::Center, Slot::Right] {
                    assert!(complete(&arrange(p, &[(g, s)])));
                }
            }
        }
    }

    #[test]
    fn serde_names() {
        let l = Preset::Windows.layout();
        let v = serde_json::to_value(&l).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"left": ["media"], "center": ["workspaces"], "right": ["clock"]})
        );
        assert_eq!(serde_json::from_value::<BarLayout>(v).unwrap(), l);
        assert!(has_latin("make it nicer") && !has_latin("сделай красиво"));
    }
}
