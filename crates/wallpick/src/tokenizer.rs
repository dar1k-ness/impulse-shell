//! CLIP's byte-level BPE, read from a Hugging Face `tokenizer.json`.
//!
//! Matches the `CLIPTokenizerFast` pipeline that file describes: NFC, whitespace runs collapsed to
//! one space, lowercase; split on `'s|'t|'re|'ve|'m|'ll|'d|\p{L}+|\p{N}|[^\s\p{L}\p{N}]+`; GPT-2
//! byte-to-unicode mapping; BPE with the `</w>` end-of-word suffix; `<|startoftext|>` … `<|endoftext|>`.
//! (The ByteLevel step's own GPT-2 split is a no-op on these whitespace-free pieces.)

use crate::error::{Error, Result};
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use unicode_normalization::UnicodeNormalization;

pub(crate) const CONTEXT: usize = 77;

pub(crate) struct Tokenizer {
    vocab: HashMap<String, u32>,
    ranks: HashMap<(String, String), u32>,
    byte_char: [char; 256],
    split: Regex,
    spaces: Regex,
    sot: u32,
    eot: u32,
}

fn bad(what: &str) -> Error {
    Error::Tokenizer(format!("tokenizer.json: {what}"))
}

/// GPT-2's reversible byte → printable unicode mapping.
fn byte_chars() -> [char; 256] {
    let mut map = ['\0'; 256];
    let mut extra = 0u32;
    for b in 0..=255u8 {
        let printable = matches!(b, b'!'..=b'~' | 0xA1..=0xAC | 0xAE..=0xFF);
        map[b as usize] = if printable {
            char::from(b)
        } else {
            extra += 1;
            char::from_u32(255 + extra).unwrap_or('\u{FFFD}')
        };
    }
    map
}

impl Tokenizer {
    pub(crate) fn from_json(bytes: &[u8]) -> Result<Self> {
        let v: Value = serde_json::from_slice(bytes).map_err(|source| Error::Json {
            what: "tokenizer.json".into(),
            source,
        })?;
        let model = &v["model"];
        if model["type"] != "BPE" || model["end_of_word_suffix"] != "</w>" {
            return Err(bad("not a CLIP BPE model"));
        }
        let vocab: HashMap<String, u32> = model["vocab"]
            .as_object()
            .ok_or_else(|| bad("no vocab"))?
            .iter()
            .filter_map(|(k, id)| Some((k.clone(), u32::try_from(id.as_u64()?).ok()?)))
            .collect();
        let merges = model["merges"].as_array().ok_or_else(|| bad("no merges"))?;
        let mut ranks = HashMap::with_capacity(merges.len());
        for (rank, m) in merges.iter().enumerate() {
            // "a b" (tokenizers < 0.20) or ["a", "b"]
            let pair = match m {
                Value::String(s) => s
                    .split_once(' ')
                    .map(|(a, b)| (a.to_string(), b.to_string())),
                Value::Array(a) => match (
                    a.first().and_then(Value::as_str),
                    a.get(1).and_then(Value::as_str),
                ) {
                    (Some(x), Some(y)) => Some((x.to_string(), y.to_string())),
                    _ => None,
                },
                _ => None,
            };
            let pair = pair.ok_or_else(|| bad("malformed merge"))?;
            ranks.entry(pair).or_insert(rank as u32);
        }
        let special = |content: &str| {
            v["added_tokens"]
                .as_array()
                .and_then(|a| a.iter().find(|t| t["content"] == content))
                .and_then(|t| t["id"].as_u64())
                .and_then(|id| u32::try_from(id).ok())
                .or_else(|| vocab.get(content).copied())
                .ok_or_else(|| bad(&format!("no {content}")))
        };
        let sot = special("<|startoftext|>")?;
        let eot = special("<|endoftext|>")?;
        let split = Regex::new(r"'s|'t|'re|'ve|'m|'ll|'d|\p{L}+|\p{N}|[^\s\p{L}\p{N}]+")
            .map_err(|e| Error::Tokenizer(e.to_string()))?;
        let spaces = Regex::new(r"\s+").map_err(|e| Error::Tokenizer(e.to_string()))?;
        Ok(Self {
            vocab,
            ranks,
            byte_char: byte_chars(),
            split,
            spaces,
            sot,
            eot,
        })
    }

    /// Token ids with the start and end tokens, not padded or truncated.
    pub(crate) fn encode(&self, text: &str) -> Vec<u32> {
        let nfc: String = text.nfc().collect();
        let norm = self.spaces.replace_all(&nfc, " ").to_lowercase();
        let mut ids = vec![self.sot];
        for piece in self.split.find_iter(&norm) {
            let word: String = piece
                .as_str()
                .bytes()
                .map(|b| self.byte_char[b as usize])
                .collect();
            for sym in self.bpe(&word) {
                ids.push(self.vocab.get(&sym).copied().unwrap_or(self.eot));
            }
        }
        ids.push(self.eot);
        ids
    }

    /// The model input: `CONTEXT` ids, truncated keeping the end token, padded with 0.
    pub(crate) fn encode_padded(&self, text: &str) -> Vec<i64> {
        let mut ids = self.encode(text);
        if ids.len() > CONTEXT {
            ids.truncate(CONTEXT - 1);
            ids.push(self.eot);
        }
        ids.resize(CONTEXT, 0);
        ids.into_iter().map(i64::from).collect()
    }

    fn bpe(&self, word: &str) -> Vec<String> {
        let mut syms: Vec<String> = word.chars().map(String::from).collect();
        if let Some(last) = syms.last_mut() {
            last.push_str("</w>");
        }
        loop {
            let best = syms
                .windows(2)
                .enumerate()
                .filter_map(|(i, w)| {
                    self.ranks
                        .get(&(w[0].clone(), w[1].clone()))
                        .map(|&r| (r, i))
                })
                .min();
            let Some((rank, _)) = best else { break };
            let mut out = Vec::with_capacity(syms.len());
            let mut i = 0;
            while i < syms.len() {
                if i + 1 < syms.len()
                    && self.ranks.get(&(syms[i].clone(), syms[i + 1].clone())) == Some(&rank)
                {
                    out.push(format!("{}{}", syms[i], syms[i + 1]));
                    i += 2;
                } else {
                    out.push(std::mem::take(&mut syms[i]));
                    i += 1;
                }
            }
            syms = out;
        }
        syms
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A miniature CLIP tokenizer.json in both merge formats.
    fn tiny(array_merges: bool) -> Tokenizer {
        let merges: Vec<Value> = [("c", "a"), ("ca", "t</w>"), ("h", "i</w>")]
            .iter()
            .map(|(a, b)| {
                if array_merges {
                    serde_json::json!([a, b])
                } else {
                    Value::String(format!("{a} {b}"))
                }
            })
            .collect();
        let json = serde_json::json!({
            "added_tokens": [{"id": 9, "content": "<|startoftext|>"}, {"id": 10, "content": "<|endoftext|>"}],
            "model": {"type": "BPE", "end_of_word_suffix": "</w>", "merges": merges,
                "vocab": {"c": 0, "a": 1, "t</w>": 2, "ca": 3, "cat</w>": 4, "h": 5, "i</w>": 6, "hi</w>": 7, "!</w>": 8}},
        });
        Tokenizer::from_json(json.to_string().as_bytes()).unwrap()
    }

    #[test]
    fn tiny_bpe_merges_by_rank() {
        for fmt in [false, true] {
            let t = tiny(fmt);
            assert_eq!(t.encode("Cat  HI!"), [9, 4, 7, 8, 10]);
            assert_eq!(t.encode(""), [9, 10]);
            let p = t.encode_padded(&"cat ".repeat(100));
            assert_eq!(p.len(), CONTEXT);
            assert_eq!((p[0], p[1], p[CONTEXT - 1]), (9, 4, 10));
            assert_eq!(t.encode_padded("hi")[..4], [9, 7, 10, 0]);
        }
    }

    #[test]
    fn byte_map_is_a_bijection() {
        let m = byte_chars();
        let set: std::collections::HashSet<char> = m.iter().copied().collect();
        assert_eq!(set.len(), 256);
        assert_eq!(m[b' ' as usize], '\u{120}'); // 'Ġ'
        assert_eq!(m[b'a' as usize], 'a');
    }

    /// The real MobileCLIP tokenizer.json when available (`WALLPICK_TOKENIZER`, or the e2e
    /// cache); ids from the official `tokenizers` 0.23.3 `Tokenizer.from_file(...).encode(s).ids`.
    #[test]
    fn real_tokenizer_matches_huggingface_ids() {
        let Some(path) = std::env::var_os("WALLPICK_TOKENIZER") else {
            eprintln!("WALLPICK_TOKENIZER not set; skipped (covered by the e2e test)");
            return;
        };
        let t = Tokenizer::from_json(&std::fs::read(path).unwrap()).unwrap();
        check_reference(&t);
    }

    pub(crate) fn check_reference(t: &Tokenizer) {
        let cases: &[(&str, &[u32])] = &[
            (
                "a photo of a cat",
                &[49406, 320, 1125, 539, 320, 2368, 49407],
            ),
            (
                "Dark minimalist wallpaper",
                &[49406, 3144, 26641, 10560, 49407],
            ),
            (
                "night landscape with mountains",
                &[49406, 930, 5727, 593, 5873, 49407],
            ),
            (
                "text, logos and watermarks",
                &[49406, 4160, 267, 24879, 537, 2505, 5466, 49407],
            ),
            (
                "it's 3 o'clock!",
                &[49406, 585, 568, 274, 334, 262, 6716, 256, 49407],
            ),
            (
                "Café  naïve\tcoöperate",
                &[49406, 15304, 1097, 35689, 563, 622, 7255, 703, 671, 49407],
            ),
            (
                "emoji 🌸 pink hair",
                &[49406, 16327, 10980, 3360, 2225, 49407],
            ),
            (
                "привет мир",
                &[
                    49406, 26302, 16370, 29503, 110, 22176, 141, 480, 38018, 16701, 141, 478, 49407,
                ],
            ),
            ("aaaaa don't   stop", &[49406, 31095, 847, 713, 1691, 49407]),
        ];
        for (text, ids) in cases {
            assert_eq!(t.encode(text), *ids, "{text:?}");
        }
    }
}
