//! MobileCLIP-S0 files, fetched lazily from Hugging Face at a pinned revision and verified.

use crate::Cancel;
use crate::error::{Error, Result, io};
use crate::fsutil;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub(crate) const REPO: &str = "Xenova/mobileclip_s0";
/// Commit of `REPO` the sizes and digests below belong to; bump all of them together.
pub(crate) const REVISION: &str = "757d59c9c6870a76a4b0306f05f5061bca15c39f";

#[derive(Clone, Copy, Debug)]
pub(crate) struct ModelFile {
    /// Path inside the repository.
    pub remote: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

impl ModelFile {
    fn local_name(&self) -> &'static str {
        self.remote.rsplit('/').next().unwrap_or(self.remote)
    }
}

pub(crate) const VISION: ModelFile = ModelFile {
    remote: "onnx/vision_model.onnx",
    size: 45_543_630,
    sha256: "17d3c037b1d488c10c50e09f6009ea5a198caef4e0e8f4ea5617b7cb2d067ac0",
};
pub(crate) const TEXT: ModelFile = ModelFile {
    remote: "onnx/text_model.onnx",
    size: 169_807_789,
    sha256: "f6e9bd5742bfc515889e901634d8a2ff2a57fab8564e4ad3760e800b1a51b77c",
};
pub(crate) const TOKENIZER: ModelFile = ModelFile {
    remote: "tokenizer.json",
    size: 2_224_081,
    sha256: "72ed5c96db5729294468543e4bc75fce14ca63f58e37300290189ba1c1e52b85",
};

/// `<cache>/models/mobileclip_s0-<revision>`: a revision bump never reuses older files.
pub(crate) fn dir(cache: &Path) -> PathBuf {
    cache
        .join("models")
        .join(format!("mobileclip_s0-{}", &REVISION[..12]))
}

/// The Hub endpoint, overridable with `HF_ENDPOINT` like the official `huggingface_hub` client.
pub(crate) fn endpoint() -> String {
    std::env::var("HF_ENDPOINT")
        .ok()
        .map(|e| e.trim_end_matches('/').to_string())
        .filter(|e| !e.is_empty())
        .unwrap_or_else(|| "https://huggingface.co".into())
}

fn url(endpoint: &str, file: &ModelFile) -> String {
    format!("{endpoint}/{REPO}/resolve/{REVISION}/{}", file.remote)
}

/// Returns the local path of `file`, downloading it first when missing or of the wrong size.
/// The download goes to a `.part` file, is checked against the pinned size and SHA-256 and only
/// then renamed into place, so a present file is always a complete, verified one.
pub(crate) fn ensure(
    dir: &Path,
    endpoint: &str,
    file: &ModelFile,
    cancel: &Cancel,
) -> Result<PathBuf> {
    let dst = dir.join(file.local_name());
    if is_complete(&dst, file) {
        return Ok(dst);
    }
    // one download per file at a time within the process; other processes use their own .part
    static DOWNLOAD: Mutex<()> = Mutex::new(());
    let _guard = DOWNLOAD.lock().unwrap_or_else(|e| e.into_inner());
    if is_complete(&dst, file) {
        return Ok(dst);
    }
    std::fs::create_dir_all(dir).map_err(io(dir))?;
    let part = fsutil::part_path(&dst);
    let res = download(&url(endpoint, file), &part, file, cancel)
        .and_then(|()| std::fs::rename(&part, &dst).map_err(io(&dst)));
    if res.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    res?;
    fsutil::sync_dir(dir);
    Ok(dst)
}

fn is_complete(path: &Path, file: &ModelFile) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() == file.size)
}

fn download(url: &str, part: &Path, file: &ModelFile, cancel: &Cancel) -> Result<()> {
    let mut out = File::create(part).map_err(io(part))?;
    let mut hasher = Sha256::new();
    let len = crate::http::stream(url, file.size, cancel, |chunk| {
        hasher.update(chunk);
        out.write_all(chunk)
    })?;
    let actual = hex(&hasher.finalize());
    if len != file.size || actual != file.sha256 {
        return Err(Error::Checksum {
            file: file.remote.to_string(),
            expected: file.sha256.to_string(),
            actual: if len == file.size {
                actual
            } else {
                format!("{actual} ({len} of {} bytes)", file.size)
            },
        });
    }
    out.sync_all().map_err(io(part))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::fake;
    use std::collections::HashMap;

    const BODY: &[u8] = b"pretend this is an onnx graph";

    fn file(sha256: &'static str) -> ModelFile {
        ModelFile {
            remote: "onnx/tiny.onnx",
            size: BODY.len() as u64,
            sha256,
        }
    }

    fn server() -> fake::Server {
        let path = format!("/{REPO}/resolve/{REVISION}/onnx/tiny.onnx");
        fake::serve(HashMap::from([(path, (200, BODY.to_vec()))]))
    }

    fn leftovers(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .map(|r| {
                r.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn verified_download_is_renamed_into_place() {
        let tmp = tempfile::tempdir().unwrap();
        let srv = server();
        let good: &'static str = Box::leak(hex(&Sha256::digest(BODY)).into_boxed_str());
        let p = ensure(tmp.path(), &srv.base, &file(good), &Cancel::new()).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), BODY);
        assert_eq!(leftovers(tmp.path()), ["tiny.onnx"]);
        // present and complete: no second request
        ensure(tmp.path(), &srv.base, &file(good), &Cancel::new()).unwrap();
        assert_eq!(srv.hits.lock().unwrap().len(), 1);
    }

    #[test]
    fn bad_hash_is_rejected_and_leaves_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let srv = server();
        let bad = "0000000000000000000000000000000000000000000000000000000000000000";
        let err = ensure(tmp.path(), &srv.base, &file(bad), &Cancel::new()).unwrap_err();
        assert!(matches!(err, Error::Checksum { .. }), "{err}");
        assert!(
            leftovers(tmp.path()).is_empty(),
            "{:?}",
            leftovers(tmp.path())
        );
    }

    #[test]
    fn oversized_and_missing_files_fail_cleanly() {
        let tmp = tempfile::tempdir().unwrap();
        let srv = server();
        let short = ModelFile {
            size: 4,
            ..file("x")
        };
        let err = ensure(tmp.path(), &srv.base, &short, &Cancel::new()).unwrap_err();
        assert!(matches!(err, Error::TooLarge { .. }), "{err}");
        let missing = ModelFile {
            remote: "onnx/none.onnx",
            ..file("x")
        };
        let err = ensure(tmp.path(), &srv.base, &missing, &Cancel::new()).unwrap_err();
        assert!(matches!(err, Error::Http { .. }), "{err}");
        assert!(leftovers(tmp.path()).is_empty());
    }

    #[test]
    fn cancelled_before_start() {
        let tmp = tempfile::tempdir().unwrap();
        let c = Cancel::new();
        c.cancel();
        let err = ensure(tmp.path(), "http://127.0.0.1:9", &file("x"), &c).unwrap_err();
        assert!(matches!(err, Error::Cancelled));
    }
}
