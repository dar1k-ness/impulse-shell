//! MobileCLIP-S0 image and text embeddings (ONNX, run with tract: pure Rust, CPU, no AVX2 needed).

use crate::Cancel;
use crate::error::{Error, Result, io, model};
use crate::fsutil;
use crate::model::{self as files, ModelFile};
use crate::tokenizer::{CONTEXT, Tokenizer};
use image::{DynamicImage, imageops::FilterType};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use tract_onnx::prelude::*;

/// Input side of the vision model (`preprocessor_config.json`: shortest edge 256, centre crop
/// 256, bilinear, rescale 1/255, no mean/std normalisation).
const SIZE: usize = 256;
const TEXT_BATCH: usize = 16;

type Plan = Arc<TypedSimplePlan>;

pub(crate) fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

pub(crate) fn normalize(mut v: Vec<f32>) -> Vec<f32> {
    let n = dot(&v, &v).sqrt().max(1e-12);
    v.iter_mut().for_each(|x| *x /= n);
    v
}

fn threads() -> usize {
    std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .clamp(1, 8)
}

pub(crate) struct Clip<'a> {
    dir: PathBuf,
    cache: PathBuf,
    endpoint: String,
    cancel: &'a Cancel,
}

impl<'a> Clip<'a> {
    pub(crate) fn new(cache: &Path, endpoint: &str, cancel: &'a Cancel) -> Self {
        Self {
            dir: files::dir(cache),
            cache: cache.to_path_buf(),
            endpoint: endpoint.to_string(),
            cancel,
        }
    }

    fn file(&self, f: &ModelFile) -> Result<PathBuf> {
        files::ensure(&self.dir, &self.endpoint, f, self.cancel)
    }

    /// Downloads (if needed) and loads the vision model; it stays loaded for the process.
    pub(crate) fn vision(&self) -> Result<Plan> {
        static VISION: Mutex<Option<(PathBuf, Plan)>> = Mutex::new(None);
        let path = self.file(&files::VISION)?;
        let mut slot = VISION.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((p, plan)) = slot.as_ref()
            && *p == path
        {
            return Ok(plan.clone());
        }
        let plan = tract_onnx::onnx()
            .model_for_path(&path)
            .and_then(|m| m.with_input_fact(0, f32::fact([1, 3, SIZE, SIZE]).into()))
            .and_then(|m| m.into_optimized())
            .and_then(|m| m.into_runnable())
            .map_err(model)?;
        *slot = Some((path, plan.clone()));
        Ok(plan)
    }

    /// Unit text embeddings, cached on disk per model revision, so the 170 MB text model is
    /// only loaded when a text was never embedded before.
    pub(crate) fn texts(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        static TEXT: Mutex<()> = Mutex::new(());
        let _guard = TEXT.lock().unwrap_or_else(|e| e.into_inner());
        let cache_path = self
            .cache
            .join(format!("text-embeddings-{}.json", &files::REVISION[..12]));
        let mut cache: HashMap<String, Vec<f32>> = std::fs::read(&cache_path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let mut missing: Vec<&String> = texts.iter().filter(|t| !cache.contains_key(*t)).collect();
        missing.sort();
        missing.dedup();
        if !missing.is_empty() {
            let tok = Tokenizer::from_json(
                &std::fs::read(self.file(&files::TOKENIZER)?).map_err(io(&self.dir))?,
            )?;
            let text_model = self.file(&files::TEXT)?;
            let graph = tract_onnx::onnx()
                .model_for_path(&text_model)
                .map_err(model)?;
            for chunk in missing.chunks(TEXT_BATCH) {
                self.cancel.check()?;
                let n = chunk.len();
                let ids: Vec<i64> = chunk.iter().flat_map(|t| tok.encode_padded(t)).collect();
                let plan = graph
                    .clone()
                    .with_input_fact(0, i64::fact([n, CONTEXT]).into())
                    .and_then(|m| m.into_optimized())
                    .and_then(|m| m.into_runnable())
                    .map_err(model)?;
                let input = Tensor::from_shape(&[n, CONTEXT], &ids).map_err(model)?;
                let out = plan.run(tvec!(input.into())).map_err(model)?;
                let rows = out[0].to_plain_array_view::<f32>().map_err(model)?;
                let flat: Vec<f32> = rows.iter().copied().collect();
                let dim = flat.len() / n;
                if dim == 0 {
                    return Err(Error::Model("text model returned no embedding".into()));
                }
                for (t, row) in chunk.iter().zip(flat.chunks(dim)) {
                    cache.insert((*t).clone(), normalize(row.to_vec()));
                }
            }
            let json = serde_json::to_vec(&cache).map_err(|source| Error::Json {
                what: "text embedding cache".into(),
                source,
            })?;
            fsutil::write_atomic(&cache_path, &json)?;
        }
        Ok(texts.iter().map(|t| cache[t].clone()).collect())
    }

    /// Unit image embeddings, computed on up to 8 threads; `progress(done)` runs on the calling
    /// thread after each image.
    pub(crate) fn images(
        &self,
        imgs: &[DynamicImage],
        progress: &mut dyn FnMut(usize),
    ) -> Result<Vec<Vec<f32>>> {
        if imgs.is_empty() {
            return Ok(Vec::new());
        }
        let plan = self.vision()?;
        let next = AtomicUsize::new(0);
        let stop = AtomicBool::new(false);
        let (tx, rx) = mpsc::channel();
        let mut out: Vec<Option<Vec<f32>>> = vec![None; imgs.len()];
        let res = std::thread::scope(|s| {
            for _ in 0..threads().min(imgs.len()) {
                let (tx, plan, next, stop, cancel) = (tx.clone(), &plan, &next, &stop, self.cancel);
                s.spawn(move || {
                    while !stop.load(Ordering::Relaxed) && !cancel.is_cancelled() {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= imgs.len() || tx.send((i, embed_image(plan, &imgs[i]))).is_err() {
                            break;
                        }
                    }
                });
            }
            drop(tx);
            let mut done = 0;
            for (i, r) in rx {
                match r {
                    Ok(e) => {
                        out[i] = Some(e);
                        done += 1;
                        progress(done);
                    }
                    Err(e) => {
                        stop.store(true, Ordering::Relaxed);
                        return Err(e);
                    }
                }
            }
            Ok(())
        });
        res?;
        self.cancel.check()?;
        out.into_iter().map(|e| e.ok_or(Error::Cancelled)).collect()
    }
}

fn embed_image(plan: &Plan, img: &DynamicImage) -> Result<Vec<f32>> {
    let s = SIZE as u32;
    let rgb = img.resize_to_fill(s, s, FilterType::Triangle).to_rgb8();
    let plane = SIZE * SIZE;
    let mut chw = vec![0f32; 3 * plane];
    for (i, p) in rgb.pixels().enumerate() {
        for c in 0..3 {
            chw[c * plane + i] = f32::from(p.0[c]) / 255.0;
        }
    }
    let x = Tensor::from_shape(&[1, 3, SIZE, SIZE], &chw).map_err(model)?;
    let out = plan.run(tvec!(x.into())).map_err(model)?;
    let v = out[0].to_plain_array_view::<f32>().map_err(model)?;
    Ok(normalize(v.iter().copied().collect()))
}
