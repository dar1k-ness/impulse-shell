//! Every hand-numbered bit flag in `crates/` is declared inside `bits::bit_flags!`, which fails
//! the build on two flags with one bit. A flag here is an integer constant whose value is a
//! single bit and which is either written as a shift (`1 << n`) or combined or tested as a mask
//! (`x & FLAG`, `A | B`, `x |= FLAG`, `!FLAG`). A flag written as a plain number and only
//! passed whole or compared with `==` is not found.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Constants that pass for a flag by that rule and are not one: `(file, name, why)`.
const NOT_FLAGS: &[(&str, &str, &str)] = &[
    (
        "crates/qml/src/colorscheme.rs",
        "MAX_NOTE",
        "a size in bytes (1 MiB)",
    ),
    (
        "crates/qml/src/dbusvar.rs",
        "MAX_VALUES",
        "a count of values",
    ),
    (
        "crates/qml/src/eval.rs",
        "MAX_DENSE_ARRAY",
        "a count of array elements",
    ),
    (
        "crates/qml/src/layout_model/mod.rs",
        "MAX_PX",
        "a length in pixels",
    ),
    (
        "crates/qml/src/value.rs",
        "SPARSE_GAP",
        "a count of array holes",
    ),
    (
        "crates/qml/tests/xhr_equiv.rs",
        "MIB",
        "a size in bytes (1 MiB)",
    ),
    (
        "crates/qsg/src/svg.rs",
        "MAX_LAYER_PIXELS",
        "a count of pixels",
    ),
    (
        "crates/qsg/src/svg/picture.rs",
        "ONE",
        "1.0 in 16.16 fixed point",
    ),
    (
        "crates/qsg/src/text.rs",
        "SUB0",
        "the first font index of the scripts' own stacks",
    ),
    (
        "crates/qsg/src/text.rs",
        "SUB_STRIDE",
        "a count of font indices, those of one script's stack",
    ),
    (
        "crates/qsg/src/triangulate.rs",
        "QTRIANGULATOR_MAX",
        "the bound of a coordinate in fixed point",
    ),
    (
        "crates/shell/src/gpu.rs",
        "STREAM_SIZE",
        "a size in bytes (1 MiB)",
    ),
    ("crates/shell/src/heap.rs", "M", "a size in bytes (1 MiB)"),
    (
        "crates/shell/src/ipc.rs",
        "MAX_REQUEST",
        "a size in bytes (1 MiB)",
    ),
    (
        "crates/shell/src/ipc.rs",
        "MAX_PENDING_OUT",
        "a size in bytes (1 MiB)",
    ),
    (
        "crates/shell/src/main.rs",
        "LINE",
        "a size in bytes (64 KiB)",
    ),
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every `.rs` file under `p` outside build output.
fn sources(p: &Path, out: &mut Vec<PathBuf>) {
    if !p.is_dir() {
        if p.extension().is_some_and(|e| e == "rs") {
            out.push(p.to_path_buf());
        }
        return;
    }
    if p.file_name().is_some_and(|n| n == "target") {
        return;
    }
    let mut entries: Vec<PathBuf> = std::fs::read_dir(p)
        .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for e in entries {
        sources(&e, out);
    }
}

fn word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// `text` with `//` comments blanked, so that a name in a comment is not a use.
fn code(text: &str) -> String {
    text.lines()
        .map(|l| l.find("//").map_or(l, |i| &l[..i]))
        .collect::<Vec<_>>()
        .join("\n")
}

/// An integer literal (`0x40`, `0b100`, `8u32`, `1_000`), or None.
fn literal(v: &str) -> Option<u128> {
    let v = v.replace('_', "");
    let (digits, radix) = match v.get(..2) {
        Some("0x") => (&v[2..], 16),
        Some("0b") => (&v[2..], 2),
        Some("0o") => (&v[2..], 8),
        _ => (&v[..], 10),
    };
    let end = digits
        .find(|c: char| !c.is_digit(radix))
        .unwrap_or(digits.len());
    let suffix = &digits[end..];
    let int = suffix.is_empty()
        || (suffix.starts_with(['i', 'u'])
            && matches!(&suffix[1..], "8" | "16" | "32" | "64" | "128" | "size"));
    if !int {
        return None;
    }
    u128::from_str_radix(&digits[..end], radix).ok()
}

/// How the value of a constant is written, if it is one bit.
#[derive(PartialEq)]
enum Bit {
    /// `1 << n`
    Shift,
    /// A literal power of two.
    Number,
}

fn single_bit(v: &str) -> Option<Bit> {
    let v = v.trim().trim_start_matches('(').trim_end_matches(')');
    if let Some((one, shift)) = v.split_once("<<") {
        let bit = literal(one.trim()) == Some(1) && literal(shift.trim()).is_some();
        return bit.then_some(Bit::Shift);
    }
    literal(v)
        .is_some_and(|n| n.is_power_of_two())
        .then_some(Bit::Number)
}

/// `const NAME: T = value;` on one line, with any visibility: the name and the value.
fn constant(line: &str) -> Option<(&str, &str)> {
    let mut l = line.trim_start();
    if let Some(rest) = l.strip_prefix("pub") {
        l = rest.trim_start();
        if l.starts_with('(') {
            l = l[l.find(')')? + 1..].trim_start();
        }
    }
    let l = l.strip_prefix("const ")?;
    let (name, rest) = l.split_once(':')?;
    let (_, value) = rest.split_once('=')?;
    let value = value.trim_end().strip_suffix(';')?;
    let name = name.trim();
    (!name.is_empty() && name.bytes().all(word)).then_some((name, value))
}

/// Whether `at` is between the parentheses of a `matches!( … )`, on its line or a later one.
fn in_matches(text: &str, at: usize) -> bool {
    let Some(open) = text[..at].rfind("matches!(") else {
        return false;
    };
    let mut depth = 0i32;
    for c in text[open..at].bytes() {
        depth += i32::from(c == b'(') - i32::from(c == b')');
    }
    depth > 0
}

/// Whether `text` uses `name` as a mask: next to a binary `&`, `|` or `^`, or under `!`.
fn masked(text: &str, name: &str) -> bool {
    let b = text.as_bytes();
    let mut from = 0;
    while let Some(i) = text[from..].find(name) {
        let (start, end) = (from + i, from + i + name.len());
        from = end;
        if (start > 0 && word(b[start - 1])) || (end < b.len() && word(b[end])) {
            continue;
        }
        // The operator after the name.
        let after = text[end..].trim_start().as_bytes();
        let binary = |s: &[u8]| {
            !s.is_empty() && matches!(s[0], b'&' | b'|' | b'^') && s.get(1) != Some(&s[0])
        };
        // `A | B =>`, `n @ (A | B) if ..` and `matches!(x, A | B)` are alternatives of a
        // pattern, not a mask.
        let alternatives = in_matches(text, start) || {
            let mut rest = text[end..].trim_start();
            loop {
                if rest.starts_with("=>") || rest.starts_with("if ") {
                    break true;
                }
                let next = rest.trim_start_matches(|c: char| c.is_alphanumeric() || c == '_');
                let next = next
                    .strip_prefix("::")
                    .or(next.strip_prefix('|'))
                    .or(next.strip_prefix(')'))
                    .unwrap_or(next);
                if next.len() == rest.len() {
                    break false;
                }
                rest = next.trim_start();
            }
        };
        if binary(after) && !(after[0] == b'|' && alternatives) {
            return true;
        }
        // `x & SIZE - 1` masks with another number, not with the constant.
        if matches!(after.first(), Some(b'-' | b'+' | b'*' | b'/')) {
            continue;
        }
        // The operator before the name, skipping a path (`scene::RICH_BOLD`).
        let mut p = start;
        while p >= 2 && &b[p - 2..p] == b"::" {
            p -= 2;
            while p > 0 && word(b[p - 1]) {
                p -= 1;
            }
        }
        let before = text[..p].trim_end().as_bytes();
        let Some((&op, head)) = before.split_last() else {
            continue;
        };
        let prev = head.last().copied();
        let hit = match op {
            b'!' => true,
            b'|' => prev != Some(op) && !alternatives,
            b'^' => true,
            // `x |= FLAG`, not `x == FLAG` or `x != FLAG`.
            b'=' => matches!(prev, Some(b'&' | b'|' | b'^')),
            // `x & FLAG`, not a reference (`f(&FLAG)`) and not `&&`.
            b'&' => {
                let left = String::from_utf8_lossy(head);
                left.trim_end()
                    .bytes()
                    .last()
                    .is_some_and(|c| word(c) || c == b')' || c == b']')
                    && prev != Some(b'&')
            }
            _ => false,
        };
        if hit {
            return true;
        }
    }
    false
}

/// The flags declared outside `bit_flags!` in the files under `dir`: `(file, line, name)`.
/// A private constant is looked for in its own file, a `pub` one in every file of its crate.
fn outside(dir: &Path, base: &Path) -> Vec<(String, usize, String)> {
    let mut paths = Vec::new();
    sources(dir, &mut paths);
    let files: Vec<(String, String)> = paths
        .iter()
        .map(|p| {
            let rel = p.strip_prefix(base).unwrap().to_string_lossy().into_owned();
            (rel, code(&std::fs::read_to_string(p).unwrap()))
        })
        .collect();
    let krate = |rel: &str| rel.split('/').take(2).collect::<Vec<_>>().join("/");
    let mut found = Vec::new();
    for (rel, text) in &files {
        // Brace depth inside a `bit_flags! { … }` block, 0 outside one.
        let mut depth = 0i32;
        for (n, line) in text.lines().enumerate() {
            let opens = depth == 0 && line.contains("bit_flags!");
            if opens || depth > 0 {
                depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;
                continue;
            }
            let Some((name, value)) = constant(line) else {
                continue;
            };
            let Some(bit) = single_bit(value) else {
                continue;
            };
            let public = line.trim_start().starts_with("pub");
            let used = || {
                files.iter().any(|(other, t)| {
                    (other == rel || (public && krate(other) == krate(rel))) && masked(t, name)
                })
            };
            if bit == Bit::Shift || used() {
                found.push((rel.clone(), n + 1, name.to_string()));
            }
        }
    }
    found
}

#[test]
fn every_bit_flag_is_declared_in_the_macro() {
    let root = root();
    let found = outside(&root.join("crates"), &root);
    let excused = |f: &str, n: &str| NOT_FLAGS.iter().any(|(ef, en, _)| *ef == f && *en == n);
    let stray: Vec<String> = found
        .iter()
        .filter(|(f, _, n)| !excused(f, n))
        .map(|(f, l, n)| format!("{f}:{l} {n}"))
        .collect();
    assert!(
        stray.is_empty(),
        "bit flags declared outside bits::bit_flags! (wrap the set in it, or list a constant \
         that is not a flag in NOT_FLAGS with the reason):\n{}",
        stray.join("\n")
    );
    let seen: BTreeSet<(&str, &str)> = found
        .iter()
        .map(|(f, _, n)| (f.as_str(), n.as_str()))
        .collect();
    let stale: Vec<_> = NOT_FLAGS
        .iter()
        .filter(|(f, n, _)| !seen.contains(&(*f, *n)))
        .collect();
    assert!(
        stale.is_empty(),
        "NOT_FLAGS entries nothing matches: {stale:?}"
    );
}

#[test]
fn the_scan_tells_a_flag_from_a_code() {
    let dir = std::env::temp_dir().join(format!("bits-outside-{}", std::process::id()));
    let src = dir.join("crates/x/src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("a.rs"),
        "const A: u32 = 1 << 3;\n\
         const B: u8 = 0x40;\n\
         pub const C: u32 = 2;\n\
         const CODE: u32 = 4;\n\
         const ONE: u32 = 1;\n\
         const TWO: u32 = 2;\n\
         const SIZE: usize = 64;\n\
         const REF: u8 = 2;\n\
         const THREE: u32 = 3;\n\
         const N: u32 = 256;\n\
         const D: u32 = 1 << 2;\n\
         const E: u32 = 16;\n\
         const WIDE: u32 = (1 << 9) - 1;\n\
         bits::bit_flags! {\n\
         const IN: u32 = 1;\n\
         }\n\
         fn f(x: u32, y: u8) -> bool {\n\
             let _ = [0u8; SIZE];\n\
             g(&REF);\n\
             match x { ONE | TWO => {} CODE | TWO if y > 0 => {} _ => {} }\n\
             match x { n @ (CODE | TWO)\n\
                 if n > y.into() => {} _ => {} }\n\
             x & A != 0 && y | B != 0 && x == CODE && x >= THREE | 1 && x & IN != 0 && x & N - 1 != 0\n\
                 && matches!(x, CODE | ONE)\n\
                 && matches!(\n\
                     x,\n\
                     CODE\n\
                         | TWO\n\
                 )\n\
                 && x | E != 0\n\
         }\n",
    )
    .unwrap();
    std::fs::write(
        src.join("b.rs"),
        "fn h(x: u32) -> u32 { x & !crate::a::C }\n",
    )
    .unwrap();
    let found = outside(&dir.join("crates"), &dir);
    std::fs::remove_dir_all(&dir).unwrap();
    let names: Vec<&str> = found.iter().map(|(_, _, n)| n.as_str()).collect();
    assert_eq!(names, ["A", "B", "C", "D", "E"]);
}
