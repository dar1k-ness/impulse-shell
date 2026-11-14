//! Candidate wallpapers from wallhaven (API v1) and konachan.net (Moebooru `post.json`).

use crate::Cancel;
use crate::error::{Error, Result};
use crate::http::{self, encode};
use crate::prefs::{Prefs, Source};
use image::DynamicImage;
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use serde_json::Value;
use std::io::Cursor;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Candidate {
    /// `wallhaven-<id>` / `konachan-<id>`.
    pub id: String,
    pub thumb: String,
    pub full: String,
    pub page: String,
}

#[derive(Clone, Debug)]
pub(crate) struct Endpoints {
    pub wallhaven: String,
    pub konachan: String,
    pub hf: String,
}

impl Endpoints {
    pub(crate) fn official() -> Self {
        Self {
            wallhaven: "https://wallhaven.cc/api/v1".into(),
            konachan: "https://konachan.net".into(),
            hf: crate::model::endpoint(),
        }
    }
}

/// An absolute http(s) URL, also from the protocol-relative `//host/...` form.
fn absolute(u: &str) -> Option<String> {
    if u.starts_with("https://") || u.starts_with("http://") {
        Some(u.to_string())
    } else {
        u.strip_prefix("//").map(|rest| format!("https://{rest}"))
    }
}

/// wallhaven `categories`: three 0/1 bits (general, anime, people), else general only.
fn categories(s: &str) -> &str {
    if s.len() == 3 && s.bytes().all(|b| b == b'0' || b == b'1') && s != "000" {
        s
    } else {
        "100"
    }
}

pub(crate) fn wallhaven_url(
    base: &str,
    p: &Prefs,
    q: &str,
    page: u32,
    seed: Option<&str>,
) -> String {
    let mut url = format!(
        "{base}/search?sorting=random&purity=100&categories={}&atleast={}x{}&page={page}",
        categories(&p.wallhaven_categories),
        p.min_width,
        p.min_height
    );
    if p.landscape {
        url.push_str("&ratios=landscape");
    }
    if !q.is_empty() {
        url.push_str("&q=");
        url.push_str(&encode(q));
    }
    if let Some(s) = seed {
        url.push_str("&seed=");
        url.push_str(&encode(s));
    }
    url
}

/// Candidates, the random-sorting seed and the last page of a wallhaven search response.
pub(crate) fn parse_wallhaven(v: &Value) -> (Vec<Candidate>, Option<String>, u64) {
    let out = v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|w| {
            let id = w["id"]
                .as_str()
                .filter(|id| id.bytes().all(|b| b.is_ascii_alphanumeric()))?;
            Some(Candidate {
                id: format!("wallhaven-{id}"),
                thumb: absolute(w["thumbs"]["large"].as_str()?)?,
                full: absolute(w["path"].as_str()?)?,
                page: w["url"].as_str().and_then(absolute).unwrap_or_default(),
            })
        })
        .collect();
    let seed = v["meta"]["seed"].as_str().map(String::from);
    let last = v["meta"]["last_page"].as_u64().unwrap_or(1);
    (out, seed, last)
}

/// Up to `n` candidates not in `shown`, looked for four pages deep, with those in `shown`
/// that came on the way.
fn wallhaven_query(
    base: &str,
    p: &Prefs,
    q: &str,
    n: usize,
    shown: &[String],
    cancel: &Cancel,
) -> Result<Vec<Candidate>> {
    let mut out: Vec<Candidate> = Vec::new();
    let mut new = 0;
    let mut seed = None;
    for page in 1..=4 {
        let v = http::json(&wallhaven_url(base, p, q, page, seed.as_deref()), cancel)?;
        let (cands, s, last) = parse_wallhaven(&v);
        seed = s.or(seed);
        let empty = cands.is_empty();
        for c in cands {
            let old = shown.contains(&c.id);
            if old || new < n {
                new += usize::from(!old);
                out.push(c);
            }
        }
        if empty || new >= n || u64::from(page) >= last {
            break;
        }
    }
    Ok(out)
}

pub(crate) fn konachan_url(base: &str, p: &Prefs, tag: &str, n: usize) -> String {
    let tags = format!(
        "rating:safe order:random width:>={} height:>={} {}",
        p.min_width,
        p.min_height,
        tag.trim()
    );
    format!(
        "{base}/post.json?limit={}&tags={}",
        (n * 2).clamp(1, 100),
        encode(tags.trim())
    )
}

pub(crate) fn parse_konachan(v: &Value, p: &Prefs, base: &str) -> Vec<Candidate> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|post| {
            let (w, h) = (post["width"].as_u64()?, post["height"].as_u64()?);
            if w < u64::from(p.min_width)
                || h < u64::from(p.min_height)
                || (p.landscape && w * 4 < h * 5)
            {
                return None;
            }
            if post["rating"].as_str().is_some_and(|r| r != "s") {
                return None;
            }
            let id = post["id"].as_u64()?;
            Some(Candidate {
                id: format!("konachan-{id}"),
                thumb: absolute(post["sample_url"].as_str()?)?,
                full: absolute(post["file_url"].as_str()?)?,
                page: format!("{base}/post/show/{id}"),
            })
        })
        .collect()
}

fn konachan_query(
    base: &str,
    p: &Prefs,
    tag: &str,
    n: usize,
    cancel: &Cancel,
) -> Result<Vec<Candidate>> {
    let v = http::json(&konachan_url(base, p, tag, n), cancel)?;
    Ok(parse_konachan(&v, p, base))
}

/// Tries the queries, in random order when `shuffle`, until at least half of `n` distinct
/// candidates not in `shown` arrived; `old` counts those in `shown` that were left out.
fn try_queries(
    list: &[String],
    shuffle: bool,
    n: usize,
    shown: &[String],
    old: &mut usize,
    rng: &mut StdRng,
    mut query: impl FnMut(&str, usize) -> Result<Vec<Candidate>>,
) -> Result<Vec<Candidate>> {
    let mut qs: Vec<&str> = list.iter().map(|s| s.trim()).collect();
    if qs.is_empty() {
        qs.push("");
    }
    if shuffle {
        qs.shuffle(rng);
    }
    let mut out: Vec<Candidate> = Vec::new();
    let mut err = None;
    for q in qs {
        match query(q, n - out.len()) {
            Ok(v) => {
                for c in v {
                    if shown.contains(&c.id) {
                        *old += 1;
                    } else if out.len() < n && !out.iter().any(|o| o.id == c.id) {
                        out.push(c);
                    }
                }
            }
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => err = Some(e),
        }
        if out.len() >= n.div_ceil(2) {
            break;
        }
    }
    match err {
        Some(e) if out.is_empty() => Err(e),
        _ => Ok(out),
    }
}

/// About `prefs.candidates` candidates not in `shown`, split over the configured sources; a
/// failing source is skipped while another one still delivers. `searches` are wallhaven
/// searches to try in their order instead of one of `prefs.wallhaven_queries` at random.
pub(crate) fn candidates(
    ep: &Endpoints,
    p: &Prefs,
    searches: Option<&[String]>,
    shown: &[String],
    rng: &mut StdRng,
    cancel: &Cancel,
) -> Result<Vec<Candidate>> {
    let mut sources = p.sources.clone();
    sources.dedup();
    if sources.is_empty() {
        return Err(Error::NoCandidates("no sources configured".into()));
    }
    let per = p.candidate_count().div_ceil(sources.len());
    let mut out = Vec::new();
    let mut errors = Vec::new();
    let mut old = 0;
    for s in sources {
        let r = match s {
            Source::Wallhaven => try_queries(
                searches.unwrap_or(&p.wallhaven_queries),
                searches.is_none(),
                per,
                shown,
                &mut old,
                rng,
                |q, n| wallhaven_query(&ep.wallhaven, p, q, n, shown, cancel),
            ),
            Source::Konachan => {
                try_queries(&p.konachan_tags, true, per, shown, &mut old, rng, |t, n| {
                    konachan_query(&ep.konachan, p, t, n, cancel)
                })
            }
        };
        match r {
            Ok(v) => out.extend(v),
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => errors.push(format!("{s:?}: {e}")),
        }
    }
    if out.is_empty() {
        let why = if !errors.is_empty() {
            errors.join("; ")
        } else if old > 0 {
            "everything the sources returned was shown before".into()
        } else {
            "sources returned nothing".into()
        };
        return Err(Error::NoCandidates(why));
    }
    Ok(out)
}

/// Decodes with `image`'s default limits (allocation cap), so a hostile file cannot exhaust memory.
pub(crate) fn decode(bytes: &[u8], what: &str) -> Result<DynamicImage> {
    let err = |source| Error::Image {
        what: what.to_string(),
        source,
    };
    image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| err(image::ImageError::IoError(e)))?
        .decode()
        .map_err(err)
}

/// Downloads and decodes the thumbnails on 8 threads, in input order; failures are dropped.
pub(crate) fn thumbnails(
    cands: Vec<Candidate>,
    cancel: &Cancel,
) -> Result<Vec<(Candidate, DynamicImage)>> {
    let next = AtomicUsize::new(0);
    let (tx, rx) = mpsc::channel();
    std::thread::scope(|s| {
        for _ in 0..8.min(cands.len()) {
            let (tx, next, cands) = (tx.clone(), &next, &cands);
            s.spawn(move || {
                while !cancel.is_cancelled() {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(c) = cands.get(i) else { break };
                    let img = http::bytes(&c.thumb, http::THUMB_LIMIT, cancel)
                        .and_then(|b| decode(&b, &c.thumb))
                        .map(|img| {
                            if img.width() > 640 || img.height() > 640 {
                                img.thumbnail(640, 640)
                            } else {
                                img
                            }
                        });
                    if tx.send((i, img.ok())).is_err() {
                        break;
                    }
                }
            });
        }
    });
    drop(tx);
    cancel.check()?;
    let mut got: Vec<(usize, DynamicImage)> = rx
        .into_iter()
        .filter_map(|(i, img)| Some((i, img?)))
        .collect();
    got.sort_by_key(|g| g.0);
    let mut cands: Vec<Option<Candidate>> = cands.into_iter().map(Some).collect();
    Ok(got
        .into_iter()
        .filter_map(|(i, img)| Some((cands[i].take()?, img)))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::fake;
    use rand::SeedableRng;
    use std::collections::HashMap;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        image::RgbImage::from_pixel(w, h, image::Rgb([10, 120, 200]))
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    fn wallhaven_page(base: &str) -> Vec<u8> {
        serde_json::json!({
            "data": [
                {"id": "abc123", "url": "https://wallhaven.cc/w/abc123", "path": format!("{base}/full/abc123.jpg"),
                 "thumbs": {"large": format!("{base}/th/abc123.png")}},
                {"id": "zz9", "url": "https://wallhaven.cc/w/zz9", "path": "//w.wallhaven.cc/full/zz9.png",
                 "thumbs": {"large": format!("{base}/th/broken.png")}},
                {"id": "../evil", "path": "https://x/y.jpg", "thumbs": {"large": "https://x/t.jpg"}},
                {"id": "nothumb", "path": "https://x/y.jpg"},
            ],
            "meta": {"seed": "Xy12", "last_page": 1, "current_page": 1}
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn wallhaven_url_has_the_documented_parameters() {
        let p = Prefs {
            wallhaven_categories: "11x".into(),
            ..Prefs::default()
        };
        let u = wallhaven_url(
            "https://wallhaven.cc/api/v1",
            &p,
            "night sky & stars",
            2,
            Some("ab"),
        );
        assert_eq!(
            u,
            "https://wallhaven.cc/api/v1/search?sorting=random&purity=100&categories=100&atleast=1920x1080\
             &page=2&ratios=landscape&q=night%20sky%20%26%20stars&seed=ab"
        );
    }

    #[test]
    fn wallhaven_parses_and_skips_bad_entries() {
        let v: Value = serde_json::from_slice(&wallhaven_page("http://h")).unwrap();
        let (c, seed, last) = parse_wallhaven(&v);
        assert_eq!(seed.as_deref(), Some("Xy12"));
        assert_eq!(last, 1);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].id, "wallhaven-abc123");
        assert_eq!(c[0].full, "http://h/full/abc123.jpg");
        assert_eq!(c[1].full, "https://w.wallhaven.cc/full/zz9.png");
    }

    #[test]
    fn konachan_parses_filters_and_builds_safe_tags() {
        let p = Prefs::default();
        let u = konachan_url("https://konachan.net", &p, " sky ", 30);
        assert_eq!(
            u,
            "https://konachan.net/post.json?limit=60&tags=rating%3Asafe%20order%3Arandom%20width%3A%3E%3D1920%20height%3A%3E%3D1080%20sky"
        );
        let v = serde_json::json!([
            {"id": 1, "width": 2560, "height": 1440, "rating": "s", "file_url": "https://k/f1.png", "sample_url": "https://k/s1.jpg"},
            {"id": 2, "width": 1080, "height": 1920, "rating": "s", "file_url": "https://k/f2.png", "sample_url": "https://k/s2.jpg"},
            {"id": 3, "width": 2560, "height": 1440, "rating": "q", "file_url": "https://k/f3.png", "sample_url": "https://k/s3.jpg"},
            {"id": 4, "width": 1280, "height": 720, "rating": "s", "file_url": "https://k/f4.png", "sample_url": "https://k/s4.jpg"},
            {"id": 5, "width": 3840, "height": 2160, "file_url": "//k/f5.png", "sample_url": "//k/s5.jpg"},
        ]);
        let c = parse_konachan(&v, &p, "https://konachan.net");
        let ids: Vec<&str> = c.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["konachan-1", "konachan-5"]);
        assert_eq!(c[1].thumb, "https://k/s5.jpg");
        assert_eq!(c[0].page, "https://konachan.net/post/show/1");
    }

    #[test]
    fn fetches_candidates_and_thumbnails_from_a_fake_server() {
        // the JSON points back at the server, so routes are filled once the port is known
        let routes = std::sync::Arc::new(std::sync::Mutex::new(HashMap::new()));
        let srv = fake::serve_dynamic(routes.clone());
        {
            let mut r = routes.lock().unwrap();
            r.insert(
                "/api/v1/search".to_string(),
                (200, wallhaven_page(&srv.base)),
            );
            r.insert("/th/abc123.png".to_string(), (200, png(300, 200)));
            r.insert(
                "/th/broken.png".to_string(),
                (200, b"not an image".to_vec()),
            );
            r.insert(
                "/post.json".to_string(),
                (200, br#"[{"id":7,"width":1920,"height":1080,"rating":"s","file_url":"https://k/f.png","sample_url":"https://k/s.jpg"}]"#.to_vec()),
            );
        }
        let ep = Endpoints {
            wallhaven: format!("{}/api/v1", srv.base),
            konachan: srv.base.clone(),
            hf: srv.base.clone(),
        };
        let p = Prefs {
            sources: vec![Source::Wallhaven, Source::Konachan],
            wallhaven_queries: vec!["space".into()],
            konachan_tags: vec![String::new()],
            candidates: 8,
            ..Prefs::default()
        };
        let mut rng = StdRng::seed_from_u64(3);
        let c = candidates(&ep, &p, None, &[], &mut rng, &Cancel::new()).unwrap();
        let ids: Vec<&str> = c.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["wallhaven-abc123", "wallhaven-zz9", "konachan-7"]);
        let hits = srv.hits.lock().unwrap().clone();
        assert!(
            hits.iter()
                .any(|h| h.starts_with("/api/v1/search?") && h.contains("&q=space")),
            "{hits:?}"
        );
        assert!(
            hits.iter()
                .any(|h| h.starts_with("/post.json?limit=8&tags=rating%3Asafe")),
            "{hits:?}"
        );

        let t = thumbnails(c[..2].to_vec(), &Cancel::new()).unwrap();
        assert_eq!(t.len(), 1, "the broken thumbnail is dropped");
        assert_eq!(t[0].0.id, "wallhaven-abc123");
        assert_eq!((t[0].1.width(), t[0].1.height()), (300, 200));
    }

    #[test]
    fn given_searches_are_tried_in_their_order() {
        let list: Vec<String> = ["a b", "a", "b"].iter().map(|s| s.to_string()).collect();
        let mut asked = Vec::new();
        let mut rng = StdRng::seed_from_u64(7);
        let got = try_queries(&list, false, 4, &[], &mut 0, &mut rng, |q, _| {
            asked.push(q.to_string());
            Ok(if q == "a b" {
                Vec::new()
            } else {
                parse_wallhaven(&serde_json::from_slice(&wallhaven_page("http://h")).unwrap()).0
            })
        })
        .unwrap();
        assert_eq!(asked, ["a b", "a"]);
        assert_eq!(got.len(), 2);
    }

    /// "Dislike" asks for the same again: a search whose answers were all shown before is
    /// not the end, the next one is tried.
    #[test]
    fn a_search_that_was_all_shown_before_gives_way_to_the_next() {
        let page = |ids: &[&str]| -> Vec<Candidate> {
            ids.iter()
                .map(|id| Candidate {
                    id: id.to_string(),
                    thumb: String::new(),
                    full: String::new(),
                    page: String::new(),
                })
                .collect()
        };
        let list: Vec<String> = ["a b", "a", "b"].iter().map(|s| s.to_string()).collect();
        let shown: Vec<String> = ["1", "2", "3"].iter().map(|s| s.to_string()).collect();
        let mut asked = Vec::new();
        let mut old = 0;
        let mut rng = StdRng::seed_from_u64(7);
        let got = try_queries(&list, false, 4, &shown, &mut old, &mut rng, |q, _| {
            asked.push(q.to_string());
            Ok(match q {
                "a b" => page(&["1", "2"]),
                "a" => page(&["3", "4", "5"]),
                _ => page(&["6"]),
            })
        })
        .unwrap();
        assert_eq!(asked, ["a b", "a"]);
        let ids: Vec<&str> = got.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["4", "5"]);
        assert_eq!(old, 3);
    }

    #[test]
    fn nothing_but_what_was_shown_before_is_said_so() {
        let routes = std::sync::Arc::new(std::sync::Mutex::new(HashMap::new()));
        let srv = fake::serve_dynamic(routes.clone());
        routes.lock().unwrap().insert(
            "/api/v1/search".to_string(),
            (200, wallhaven_page(&srv.base)),
        );
        let ep = Endpoints {
            wallhaven: format!("{}/api/v1", srv.base),
            konachan: srv.base.clone(),
            hf: srv.base.clone(),
        };
        let p = Prefs {
            sources: vec![Source::Wallhaven],
            candidates: 8,
            ..Prefs::default()
        };
        let searches = ["night city".to_string(), "night".to_string()];
        let mut rng = StdRng::seed_from_u64(3);
        let some = ["wallhaven-abc123".to_string()];
        let c = candidates(&ep, &p, Some(&searches), &some, &mut rng, &Cancel::new()).unwrap();
        let ids: Vec<&str> = c.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["wallhaven-zz9"]);
        let all = ["wallhaven-abc123".to_string(), "wallhaven-zz9".to_string()];
        let err = candidates(&ep, &p, Some(&searches), &all, &mut rng, &Cancel::new()).unwrap_err();
        assert_eq!(
            err.to_string(),
            Error::NoCandidates("everything the sources returned was shown before".into())
                .to_string()
        );
        let hits = srv.hits.lock().unwrap().clone();
        assert!(hits.iter().any(|h| h.ends_with("&q=night")), "{hits:?}");
    }

    #[test]
    fn all_sources_failing_is_an_error_not_a_panic() {
        let srv = fake::serve(HashMap::from([(
            "/api/v1/search".to_string(),
            (500, b"down".to_vec()),
        )]));
        let ep = Endpoints {
            wallhaven: format!("{}/api/v1", srv.base),
            konachan: srv.base.clone(),
            hf: srv.base.clone(),
        };
        let mut rng = StdRng::seed_from_u64(0);
        let err =
            candidates(&ep, &Prefs::default(), None, &[], &mut rng, &Cancel::new()).unwrap_err();
        assert!(matches!(err, Error::NoCandidates(_)), "{err}");
        let p = Prefs {
            sources: vec![],
            ..Prefs::default()
        };
        assert!(matches!(
            candidates(&ep, &p, None, &[], &mut rng, &Cancel::new()),
            Err(Error::NoCandidates(_))
        ));
    }
}
