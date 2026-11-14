//! wallpick: wallpapers and an accent colour picked to the user's taste.
//!
//! Candidates come from wallhaven and konachan.net; MobileCLIP-S0 (Apple, ONNX export by
//! Xenova, run on the CPU with tract) scores them against English text preferences and a small
//! logistic taste model trained by likes and dislikes. The model files are downloaded on first
//! use into [`Dirs::cache`] from Hugging Face at a pinned revision and verified by SHA-256.
//!
//! Every function blocks and is safe to call from any thread; state updates are serialised
//! within the process. Nothing here starts processes or talks to the compositor.

mod accent;
mod clip;
mod clock;
mod color;
mod error;
mod fsutil;
mod http;
mod layout;
mod model;
mod prefs;
mod score;
mod sources;
mod state;
mod taste;
mod tokenizer;

#[cfg(test)]
mod e2e;

pub use error::{Error, Result};
pub use layout::{BarGroup, BarLayout};
pub use prefs::{Mode, Prefs, Source};

use clip::{Clip, dot};
use color::Lab;
use image::DynamicImage;
use rand::{SeedableRng, rngs::StdRng};
use sources::Endpoints;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Where wallpick keeps its files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dirs {
    /// Taste model, shown history and the current wallpaper (`state.json`).
    pub state: PathBuf,
    /// Model files (~220 MB) and the text embedding cache.
    pub cache: PathBuf,
    /// Saved wallpapers.
    pub downloads: PathBuf,
}

/// Progress of [`pick`], reported on the calling thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Downloading (first run) and loading the model, embedding the text preferences.
    Model,
    /// Querying the sources and fetching thumbnails.
    Candidates,
    /// Embedding thumbnails: `done` of `total` images.
    Scoring { done: usize, total: usize },
    /// Downloading the chosen wallpaper.
    Download,
    /// Choosing the accent colour.
    Accent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picked {
    pub path: PathBuf,
    /// `#rrggbb`; `None` when `pickAccent` is off or nothing on the wallpaper is colourful.
    pub accent: Option<String>,
    /// The wallpaper's page on its site.
    pub page: String,
    /// `wallhaven-<id>` / `konachan-<id>`.
    pub id: String,
}

/// Cooperative cancellation: clones share the flag; long operations check it between steps
/// and network chunks and return [`Error::Cancelled`].
#[derive(Clone, Debug, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    pub(crate) fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vote {
    Like,
    Dislike,
}

/// What a pick goes by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum By<'a> {
    /// The text preferences and searches of [`Prefs`].
    Prefs,
    /// The look of this image, the wallpaper on the desktop.
    Desktop(&'a Path),
    /// These words (English) instead of [`Prefs::like`] and the configured searches.
    Words(&'a str),
    /// What the pick before went by.
    Same,
}

/// Picks, downloads and records a new wallpaper (it becomes the current one for [`vote`]);
/// keeps the `keep` newest downloads plus every liked one.
pub fn pick(
    prefs: &Prefs,
    dirs: &Dirs,
    progress: &dyn Fn(Stage),
    cancel: &Cancel,
) -> Result<Picked> {
    pick_by(By::Prefs, prefs, dirs, progress, cancel)
}

/// [`pick`] going by `by`, which is remembered for a later [`By::Same`].
pub fn pick_by(
    by: By,
    prefs: &Prefs,
    dirs: &Dirs,
    progress: &dyn Fn(Stage),
    cancel: &Cancel,
) -> Result<Picked> {
    pick_with(
        by,
        prefs,
        dirs,
        &Endpoints::official(),
        &mut StdRng::from_os_rng(),
        progress,
        cancel,
    )
}

/// Searches wallhaven knows, one per kind of picture: the ones nearest to the desktop's
/// wallpaper are where its likes are looked for.
const SEARCHES: [&str; 32] = [
    "landscape",
    "mountains",
    "forest",
    "ocean",
    "beach",
    "lake",
    "desert",
    "snow",
    "sky",
    "clouds",
    "sunset",
    "night",
    "stars",
    "space",
    "city",
    "street",
    "architecture",
    "minimalism",
    "abstract",
    "digital art",
    "fantasy art",
    "science fiction",
    "cyberpunk",
    "pixel art",
    "anime",
    "anime girls",
    "flowers",
    "animals",
    "cars",
    "video games",
    "dark",
    "pastel",
];
const NEAREST_SEARCHES: usize = 3;

/// The basis for the wallpaper at `path`: wallpapers tagged like it when it is wallhaven's own,
/// then the [`SEARCHES`] it looks most like.
fn desktop_basis(clip: &Clip, path: &Path) -> Result<state::Basis> {
    let emb = clip
        .images(&[open_image(path)?], &mut |_| {})?
        .pop()
        .unwrap_or_default();
    let prompts: Vec<String> = SEARCHES
        .iter()
        .map(|s| score::WALLPAPER.replace("{}", s))
        .collect();
    let mut near: Vec<(f32, &str)> = clip
        .texts(&prompts)?
        .iter()
        .map(|t| dot(&emb, t))
        .zip(SEARCHES)
        .collect();
    near.sort_by(|a, b| b.0.total_cmp(&a.0));
    let own = path
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.strip_prefix("wallhaven-"))
        .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric()))
        .map(|id| format!("like:{id}"));
    let queries = own
        .into_iter()
        .chain(
            near.iter()
                .take(NEAREST_SEARCHES)
                .map(|(_, s)| (*s).to_string()),
        )
        .collect();
    Ok(state::Basis::Desktop { emb, queries })
}

/// The searches for the user's words: all of them, then each word that says something.
fn word_searches(text: &str) -> Vec<String> {
    let words: Vec<&str> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() > 2 && !["the", "and", "with", "for"].contains(w))
        .collect();
    let mut out = vec![text.trim().to_string()];
    if words.len() > 1 {
        out.extend(words.iter().map(|w| (*w).to_string()));
    }
    out
}

/// wallhaven's general and anime categories on top of the configured ones: what the user
/// asks for by words or by the desktop is looked for in both.
fn wide_categories(c: &str) -> String {
    let bit = |i: usize| c.as_bytes().get(i) == Some(&b'1');
    format!("11{}", if bit(2) { '1' } else { '0' })
}

/// Downloads that fail are replaced by the next sampled candidate this many times.
const DOWNLOAD_ATTEMPTS: usize = 3;

fn pick_with(
    by: By,
    prefs: &Prefs,
    dirs: &Dirs,
    ep: &Endpoints,
    rng: &mut StdRng,
    progress: &dyn Fn(Stage),
    cancel: &Cancel,
) -> Result<Picked> {
    progress(Stage::Model);
    let clip = Clip::new(&dirs.cache, &ep.hf, cancel);
    clip.vision()?;
    let basis = match by {
        By::Prefs => None,
        By::Desktop(path) => Some(desktop_basis(&clip, path)?),
        By::Words(text) => Some(state::Basis::Words {
            text: text.trim().to_string(),
        }),
        By::Same => state::load(&dirs.state)?.basis,
    };
    let mut prefs = prefs.clone();
    let searches = match &basis {
        None => None,
        Some(state::Basis::Desktop { queries, .. }) => Some(queries.clone()),
        Some(state::Basis::Words { text }) => {
            prefs.like = vec![text.clone()];
            Some(word_searches(text))
        }
    };
    if basis.is_some() {
        prefs.wallhaven_categories = wide_categories(&prefs.wallhaven_categories);
    }
    let prefs = &prefs;
    let mut scorer = score::Scorer::new(&clip, prefs)?;
    if let Some(state::Basis::Desktop { emb, .. }) = &basis {
        scorer.like = vec![emb.clone()];
    }

    progress(Stage::Candidates);
    let shown = state::load(&dirs.state)?.history;
    let cands = sources::candidates(ep, prefs, searches.as_deref(), &shown, rng, cancel)?;
    let (cands, imgs): (Vec<_>, Vec<_>) = sources::thumbnails(cands, cancel)?.into_iter().unzip();
    if imgs.is_empty() {
        return Err(Error::NoCandidates(
            "no thumbnail could be downloaded".into(),
        ));
    }

    let n = imgs.len();
    let total = if prefs.subject_side { 3 * n } else { n };
    progress(Stage::Scoring { done: 0, total });
    let embs = clip.images(&imgs, &mut |done| progress(Stage::Scoring { done, total }))?;
    let taste = state::load(&dirs.state)?.taste;
    let mut scores: Vec<f32> = embs
        .iter()
        .zip(&imgs)
        .map(|(e, img)| {
            let flat = if prefs.full_scene {
                score::flat_penalty(img)
            } else {
                0.0
            };
            scorer.score(e, &taste) - score::tone_penalty(img, prefs.mode) - flat
        })
        .collect();
    if prefs.subject_side {
        let halves: Vec<DynamicImage> = imgs
            .iter()
            .flat_map(|img| {
                let (w, h) = (img.width(), img.height());
                [
                    img.crop_imm(0, 0, w / 2, h),
                    img.crop_imm(w - w / 2, 0, w / 2, h),
                ]
            })
            .collect();
        let he = clip.images(&halves, &mut |d| {
            progress(Stage::Scoring { done: n + d, total })
        })?;
        for (i, s) in scores.iter_mut().enumerate() {
            *s = score::side_score(&scorer, &taste, &embs[i], &he[2 * i], &he[2 * i + 1], *s);
        }
    }

    let mut allowed: Vec<usize> = (0..n).collect();
    let mut last_err = None;
    let mut chosen = None;
    for _ in 0..DOWNLOAD_ATTEMPTS {
        let Some(i) = score::sample(&scores, &allowed, prefs.randomness, rng) else {
            break;
        };
        progress(Stage::Download);
        match download(&cands[i], &dirs.downloads, cancel) {
            Ok(got) => {
                chosen = Some((i, got));
                break;
            }
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => {
                allowed.retain(|&a| a != i);
                last_err = Some(e);
            }
        }
    }
    let Some((i, (path, full))) = chosen else {
        return Err(last_err.unwrap_or_else(|| Error::NoCandidates("nothing to download".into())));
    };

    let accent = if prefs.pick_accent {
        progress(Stage::Accent);
        accent::pick(&full, prefs, &clip)?.map(Lab::hex)
    } else {
        None
    };
    cancel.check()?;
    let cand = &cands[i];
    let cur = state::Current {
        id: cand.id.clone(),
        path: path.clone(),
        page: cand.page.clone(),
        accent: accent.clone(),
        emb: embs[i].clone(),
    };
    let seen: Vec<(String, Vec<f32>)> = cands.iter().map(|c| c.id.clone()).zip(embs).collect();
    state::update(&dirs.state, |st| {
        st.record(cur, &seen, basis.is_some(), &dirs.downloads, prefs.keep);
        st.basis = basis;
        Ok(())
    })?;
    Ok(Picked {
        path,
        accent,
        page: cand.page.clone(),
        id: cand.id.clone(),
    })
}

/// Downloads, verifies (decodes) and saves the full image as `<downloads>/<id>.<ext>`.
fn download(
    c: &sources::Candidate,
    downloads: &Path,
    cancel: &Cancel,
) -> Result<(PathBuf, DynamicImage)> {
    let bytes = http::bytes(&c.full, http::IMAGE_LIMIT, cancel)?;
    let img = sources::decode(&bytes, &c.full)?;
    let ext = image::guess_format(&bytes)
        .ok()
        .and_then(|f| f.extensions_str().first().copied())
        .unwrap_or("img");
    let name: String =
        c.id.chars()
            .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
            .collect();
    let path = downloads.join(format!("{name}.{ext}"));
    fsutil::write_atomic(&path, &bytes)?;
    Ok((path, img))
}

/// Likes or dislikes the current wallpaper and retrains the taste model; liked files are kept.
pub fn vote(dirs: &Dirs, v: Vote) -> Result<()> {
    state::update(&dirs.state, |st| st.vote(v == Vote::Like))
}

/// Forgets everything the taste model learnt.
pub fn reset_taste(dirs: &Dirs) -> Result<()> {
    state::update(&dirs.state, |st| {
        st.taste = Default::default();
        Ok(())
    })
}

fn open_image(path: &Path) -> Result<DynamicImage> {
    let bytes = std::fs::read(path).map_err(error::io(path))?;
    sources::decode(&bytes, &path.to_string_lossy())
}

/// The accent wallpick would choose for `image` (`#rrggbb`); for a wallpaper without any
/// colourful area, its most colourful palette colour.
pub fn accent_for(image: &Path, prefs: &Prefs, dirs: &Dirs) -> Result<String> {
    let img = open_image(image)?;
    let cancel = Cancel::new();
    let clip = Clip::new(&dirs.cache, &model::endpoint(), &cancel);
    Ok(accent::pick(&img, prefs, &clip)?
        .unwrap_or_else(|| accent::fallback(&img))
        .hex())
}

/// The bar layout a free-text wish asks for (Russian or English); `None` for an empty wish.
pub fn bar_layout(wish: &str, dirs: &Dirs) -> Result<Option<BarLayout>> {
    if wish.trim().is_empty() {
        return Ok(None);
    }
    let w = layout::parse(wish);
    let preset = match w.style {
        Some(p) => p,
        None if w.places.is_empty() && layout::has_latin(wish) => {
            let cancel = Cancel::new();
            clip_preset(&Clip::new(&dirs.cache, &model::endpoint(), &cancel), wish)?
        }
        None => layout::Preset::Useful,
    };
    Ok(Some(layout::arrange(preset, &w.places)))
}

/// CLIP reading of an English wish that matched no keyword; vague wishes vote for the default.
fn clip_preset(clip: &Clip, wish: &str) -> Result<layout::Preset> {
    use layout::Preset;
    let presets: Vec<Preset> = Preset::ALL
        .into_iter()
        .filter(|p| *p != Preset::Compact)
        .collect();
    let about: Vec<String> = presets
        .iter()
        .map(|p| p.description().to_string())
        .collect();
    let vague: Vec<String> = layout::VAGUE_WISHES
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let wish = vec![wish.to_string()];
    let t = "a desktop top bar: {}";
    let g = score::embed_groups(clip, &[(&about, t), (&vague, t), (&wish, t)])?;
    let text = &g[2][0];
    let vague_sim = score::best(text, &g[1]).unwrap_or(f32::MIN);
    let mut best = (f32::MIN, Preset::Useful);
    for (p, e) in presets.iter().zip(&g[0]) {
        let mut s = dot(e, text);
        if *p == Preset::Useful {
            // a named style must clearly beat the vague reading
            s = s.max(vague_sim - 0.07) + 0.03;
        }
        if s > best.0 {
            best = (s, *p);
        }
    }
    Ok(best.1)
}

/// Top-left corner (logical px) for a `clock`-sized desktop clock on `image` shown cover-cropped
/// on a `screen`-sized output: the calmest spot clear of the bar, the edges and the subject.
pub fn clock_spot(
    image: &Path,
    screen: (u32, u32),
    clock: (u32, u32),
    dirs: &Dirs,
) -> Result<(f64, f64)> {
    let img = open_image(image)?;
    let cancel = Cancel::new();
    let clip = Clip::new(&dirs.cache, &model::endpoint(), &cancel);
    let subject: Vec<String> = clock::SUBJECT.iter().map(|s| (*s).to_string()).collect();
    let background: Vec<String> = clock::BACKGROUND.iter().map(|s| (*s).to_string()).collect();
    let g = score::embed_groups(&clip, &[(&subject, "{}"), (&background, "{}")])?;
    let embs = clip.images(&clock::crops(&img, screen), &mut |_| {})?;
    let sal: Vec<f32> = embs
        .iter()
        .map(|e| score::best(e, &g[0]).unwrap_or(0.0) - score::best(e, &g[1]).unwrap_or(0.0))
        .collect();
    let cells = clock::GRID.0 * clock::GRID.1;
    let side = sal[cells + 1] - sal[cells];
    let heat = clock::heat_map(&sal[..cells], side);
    Ok(clock::calm_spot(
        &clock::Busy::new(&img, screen),
        screen,
        clock,
        &heat,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(root: &Path) -> Dirs {
        Dirs {
            state: root.join("state"),
            cache: root.join("cache"),
            downloads: root.join("dl"),
        }
    }

    #[test]
    fn public_types_are_thread_safe() {
        fn sync<T: Send + Sync>() {}
        sync::<Prefs>();
        sync::<Dirs>();
        sync::<Cancel>();
        sync::<Picked>();
        sync::<Error>();
        sync::<BarLayout>();
    }

    #[test]
    fn cancel_is_shared_by_clones() {
        let c = Cancel::new();
        let d = c.clone();
        assert!(c.check().is_ok());
        d.cancel();
        assert!(c.is_cancelled() && matches!(c.check(), Err(Error::Cancelled)));
    }

    #[test]
    fn vote_needs_a_current_wallpaper_and_reset_clears_taste() {
        let tmp = tempfile::tempdir().unwrap();
        let d = dirs(tmp.path());
        assert!(matches!(vote(&d, Vote::Like), Err(Error::NoCurrent)));
        let path = tmp.path().join("w.jpg");
        state::update(&d.state, |s| {
            s.current = Some(state::Current {
                id: "x".into(),
                path: path.clone(),
                page: String::new(),
                accent: None,
                emb: vec![1.0, 0.0],
            });
            Ok(())
        })
        .unwrap();
        vote(&d, Vote::Like).unwrap();
        let s = state::load(&d.state).unwrap();
        assert_eq!(
            (s.taste.pos.len(), s.liked.clone()),
            (1, vec![path.clone()])
        );
        vote(&d, Vote::Dislike).unwrap();
        let s = state::load(&d.state).unwrap();
        assert_eq!(
            (s.taste.pos.len(), s.taste.neg.len(), s.liked.len()),
            (0, 1, 0)
        );
        reset_taste(&d).unwrap();
        assert_eq!(
            state::load(&d.state).unwrap().taste,
            taste::Taste::default()
        );
    }

    #[test]
    fn bar_layout_without_the_model() {
        let tmp = tempfile::tempdir().unwrap();
        let d = dirs(tmp.path());
        assert_eq!(bar_layout("  ", &d).unwrap(), None);
        let l = bar_layout("часы слева, остальное как на маке", &d)
            .unwrap()
            .unwrap();
        assert_eq!(l.left, [BarGroup::Clock, BarGroup::Workspaces]);
        assert_eq!(
            bar_layout("сделай что-нибудь", &d).unwrap(),
            Some(layout::Preset::Useful.layout())
        );
    }

    #[test]
    fn accent_for_named_preferences_is_offline() {
        let tmp = tempfile::tempdir().unwrap();
        let d = dirs(tmp.path());
        let p = tmp.path().join("a.png");
        image::RgbImage::from_fn(64, 64, |x, _| {
            if x < 40 {
                image::Rgb([20, 20, 20])
            } else {
                image::Rgb([30, 140, 210])
            }
        })
        .save(&p)
        .unwrap();
        let hex = accent_for(&p, &Prefs::default(), &d).unwrap();
        let lab = Lab::from_hex(&hex).unwrap();
        assert!(lab.b < -0.05, "{hex}");
        assert!(matches!(
            accent_for(&tmp.path().join("none.png"), &Prefs::default(), &d),
            Err(Error::Io { .. })
        ));
        let grey = tmp.path().join("g.png");
        image::RgbImage::from_pixel(8, 8, image::Rgb([90, 90, 90]))
            .save(&grey)
            .unwrap();
        assert_eq!(accent_for(&grey, &Prefs::default(), &d).unwrap(), "#5a5a5a");
    }
}

#[cfg(test)]
mod basis_tests {
    use super::*;

    #[test]
    fn words_are_searched_whole_first_then_one_by_one() {
        assert_eq!(
            word_searches(" night city with the rain "),
            ["night city with the rain", "night", "city", "rain"]
        );
        assert_eq!(word_searches("forest"), ["forest"]);
    }

    #[test]
    fn an_asked_pick_looks_in_general_and_anime_and_keeps_people() {
        assert_eq!(wide_categories("100"), "110");
        assert_eq!(wide_categories("011"), "111");
        assert_eq!(wide_categories("junk"), "110");
    }

    #[test]
    fn the_basis_of_a_pick_survives_in_the_state() {
        let t = tempfile::tempdir().unwrap();
        let basis = state::Basis::Words {
            text: "night city".into(),
        };
        state::update(t.path(), |st| {
            st.basis = Some(basis.clone());
            Ok(())
        })
        .unwrap();
        assert_eq!(state::load(t.path()).unwrap().basis, Some(basis));
        let text = std::fs::read_to_string(t.path().join("state.json")).unwrap();
        assert!(
            text.contains(r#""basis":{"words":{"text":"night city"}}"#),
            "{text}"
        );
    }
}
