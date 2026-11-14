# regress 0.12.0, patched

The crates.io release of [regress](https://github.com/ridiculousfish/regress) 0.12.0, used
through `[patch.crates-io]` in the workspace `Cargo.toml` and in `crates/iigen/check/Cargo.toml`,
with three ECMA-262 fixes for patterns without the u or v flag:

- `Canonicalize` (§22.2.2.7.3 steps 3-6): a non-ASCII character never canonicalizes to an ASCII
  one, so `/\u017f/i` does not match `s` and `/[a-z]/i` does not match U+212A KELVIN SIGN, and
  class escapes and character classes under `/i` expand with that rule, not with Unicode simple
  case folding (`src/unicode.rs`, `src/parse.rs`).
- `\u{...}` is an IdentityEscape followed by a quantifier (Annex B §B.1.2), not a code point
  escape (`src/parse.rs`).
- `\uD83D\uDE00` is two code units, not one code point (§22.2.1: only
  RegExpUnicodeEscapeSequence[+UnicodeMode] pairs surrogates), so `/[\ud83d\ude00]/` matches a
  lone `\ud83d` and `/[\ud83d\ude00-\ude02]/` is a class of U+D83D and the range
  U+DE00-U+DE02 (`src/parse.rs`).
- With `iu` a class is built as QV4's Yarr builds it: the ASCII part of a range takes only its
  ASCII case counterparts (`/[a-z]/iu` does not match U+017F or U+212A, `/[k]/iu` and `/\w/iu`
  do), and `\W` is the complement of `\w` (`src/parse.rs`). This one is Qt's, not for
  upstream.

Upstream: https://github.com/ridiculousfish/regress/issues/216 covers the first two (master as of
2026-11-02 still has both); the third is not reported there. Drop this directory and the two
`[patch.crates-io]` entries once a regress release has all three.
