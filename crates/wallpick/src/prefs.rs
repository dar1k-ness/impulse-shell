use serde::{Deserialize, Serialize};

/// Where candidates come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Wallhaven,
    /// konachan.net, the safe-only mirror; `rating:safe` is always added.
    Konachan,
}

/// The original theme the wallpaper has to sit under: penalises pictures whose brightness fights it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// No brightness preference.
    #[default]
    Auto,
    Dark,
    Light,
}

/// User preferences, the original `config.json` `background.wallpick` (camelCase); every field defaults to
/// the original wallpick default when absent. Text preferences are English (CLIP reads English).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Prefs {
    pub sources: Vec<Source>,
    pub like: Vec<String>,
    pub dislike: Vec<String>,
    /// Candidates looked at per pick (more = better match, slower); clamped to 4..=200.
    pub candidates: usize,
    /// 0 = always the best match, 1 = more variety.
    pub randomness: f32,
    /// One is chosen at random per pick; "" = any wallpaper.
    pub wallhaven_queries: Vec<String>,
    /// wallhaven category bits general/anime/people, e.g. "110".
    pub wallhaven_categories: String,
    pub konachan_tags: Vec<String>,
    pub min_width: u32,
    pub min_height: u32,
    pub landscape: bool,
    /// Prefer wallpapers with a fully drawn background over a subject on a flat backdrop.
    pub full_scene: bool,
    /// Prefer the liked subject at a side of the screen rather than in the centre.
    pub subject_side: bool,
    pub accent_like: Vec<String>,
    pub accent_dislike: Vec<String>,
    /// `false` leaves the accent to the colour generator (`Picked::accent` is `None`).
    pub pick_accent: bool,
    /// Downloaded wallpapers to keep; liked ones are never deleted.
    pub keep: usize,
    pub mode: Mode,
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_string()).collect()
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            sources: vec![Source::Wallhaven],
            like: strings(&[
                "dark minimalist wallpaper",
                "night landscape with mountains",
                "calm nature scenery with soft colors",
            ]),
            dislike: strings(&[
                "people and faces",
                "text, logos and watermarks",
                "bright cluttered busy image",
            ]),
            candidates: 48,
            randomness: 0.35,
            wallhaven_queries: strings(&[
                "landscape",
                "minimalism",
                "night",
                "mountains",
                "space",
                "",
            ]),
            wallhaven_categories: "100".into(),
            konachan_tags: strings(&["scenic", "landscape", "sky", ""]),
            min_width: 1920,
            min_height: 1080,
            landscape: true,
            full_scene: true,
            subject_side: false,
            accent_like: strings(&["cold blue", "teal", "purple"]),
            accent_dislike: strings(&["brown", "muddy olive"]),
            pick_accent: true,
            keep: 30,
            mode: Mode::Auto,
        }
    }
}

impl Prefs {
    pub(crate) fn candidate_count(&self) -> usize {
        self.candidates.clamp(4, 200)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_object_gives_defaults() {
        let p: Prefs = serde_json::from_str("{}").unwrap();
        assert_eq!(p, Prefs::default());
        assert_eq!(p.candidates, 48);
        assert_eq!(p.keep, 30);
        assert_eq!(p.wallhaven_categories, "100");
        assert!(p.pick_accent && p.landscape && p.full_scene && !p.subject_side);
    }

    #[test]
    fn camel_case_fields_override() {
        let p: Prefs = serde_json::from_str(
            r#"{"sources":["konachan","wallhaven"],"wallhavenQueries":["space"],"minWidth":2560,
                "subjectSide":true,"accentLike":["pink"],"pickAccent":false,"keep":5,"mode":"dark",
                "somethingNew":1}"#,
        )
        .unwrap();
        assert_eq!(p.sources, [Source::Konachan, Source::Wallhaven]);
        assert_eq!(p.wallhaven_queries, ["space"]);
        assert_eq!((p.min_width, p.min_height), (2560, 1080));
        assert!(p.subject_side && !p.pick_accent);
        assert_eq!(p.accent_like, ["pink"]);
        assert_eq!(p.keep, 5);
        assert_eq!(p.mode, Mode::Dark);
        assert_eq!(p.like, Prefs::default().like);
    }

    #[test]
    fn round_trips_and_rejects_unknown_source() {
        let p = Prefs::default();
        let v = serde_json::to_value(&p).unwrap();
        assert!(v.get("wallhavenCategories").is_some());
        assert_eq!(serde_json::from_value::<Prefs>(v).unwrap(), p);
        assert!(serde_json::from_str::<Prefs>(r#"{"sources":["flickr"]}"#).is_err());
    }

    #[test]
    fn candidate_count_is_clamped() {
        let mut p = Prefs {
            candidates: 0,
            ..Prefs::default()
        };
        assert_eq!(p.candidate_count(), 4);
        p.candidates = 10_000;
        assert_eq!(p.candidate_count(), 200);
    }
}
