//! Which characters of a text make up emoji sequences, as QTextEngine::itemize finds them
//! (Qt 6.12) with the emoji segmenter it bundles (google/emoji-segmenter 0.4.0): such
//! characters get an item of their own, set in the emoji font.

use crate::emoji_data::RANGES;

/// The segmenter's character categories that QTextEngine::itemize hands out.
#[derive(Clone, Copy, PartialEq)]
enum Category {
    TextPresentation,
    EmojiPresentation,
    ModifierBase,
    Modifier,
    RegionalIndicator,
    KeycapBase,
    EnclosingKeycap,
    EnclosingCircleBackslash,
    Zwj,
    Vs15,
    Vs16,
    TagBase,
    TagSequence,
    TagTerm,
    Other,
}

fn category(c: char) -> Category {
    use Category::*;
    match c as u32 {
        0x20E3 => EnclosingKeycap,
        0x20E0 => EnclosingCircleBackslash,
        0xFE0E => Vs15,
        0xFE0F => Vs16,
        0x200D => Zwj,
        0x1F3F4 => TagBase,
        0xE007F => TagTerm,
        0xE0030..=0xE0039 | 0xE0061..=0xE007A => TagSequence,
        0x1F1E6..=0x1F1FF => RegionalIndicator,
        0x30..=0x39 | 0x23 | 0x2A => KeycapBase,
        cp => {
            let at = RANGES.partition_point(|r| r.1 < cp);
            match RANGES.get(at).filter(|r| r.0 <= cp).map(|r| r.2) {
                Some(0) => ModifierBase,
                Some(1) => Modifier,
                Some(2) => EmojiPresentation,
                Some(_) => TextPresentation,
                None => Other,
            }
        }
    }
}

/// The segmenter's `any_emoji`.
fn any_emoji(c: Category) -> bool {
    use Category::*;
    matches!(
        c,
        TextPresentation | EmojiPresentation | KeycapBase | ModifierBase | TagBase
    )
}

/// The length of the longest emoji sequence that `c` starts with (the segmenter's
/// `emoji_run`), or 0.
fn emoji_run(c: &[Category]) -> usize {
    use Category::*;
    let at = |k: usize| c.get(k).copied();
    let first = c[0];
    let mut longest = 0;
    if matches!(first, EmojiPresentation | TagBase | ModifierBase) {
        longest = 1;
    }
    if any_emoji(first) && matches!(at(1), Some(Vs16 | EnclosingCircleBackslash))
        || first == ModifierBase && at(1) == Some(Modifier)
        || first == RegionalIndicator && at(1) == Some(RegionalIndicator)
    {
        longest = 2;
    }
    if first == KeycapBase && at(1) == Some(Vs16) && at(2) == Some(EnclosingKeycap) {
        longest = 3;
    }
    if first == TagBase {
        let tags = c[1..].iter().take_while(|&&t| t == TagSequence).count();
        if tags > 0 && at(1 + tags) == Some(TagTerm) {
            longest = longest.max(tags + 2);
        }
    }
    // emoji_zwj_sequence: elements joined by ZWJ, each an emoji with its variation selector
    // or modifier.
    let element = |k: usize| match (at(k), at(k + 1)) {
        (Some(e), Some(Vs16)) if any_emoji(e) => 2,
        (Some(ModifierBase), Some(Modifier)) => 2,
        (Some(e), _) if any_emoji(e) => 1,
        _ => 0,
    };
    let mut end = element(0);
    if end > 0 {
        let mut joined = false;
        while at(end) == Some(Zwj) && element(end + 1) > 0 {
            end += 1 + element(end + 1);
            joined = true;
        }
        if joined {
            longest = longest.max(end);
        }
    }
    longest
}

/// Per byte of `text`: 0 outside an emoji sequence, 1 where one starts, 2 inside one. The
/// segmenter takes the longest sequence it finds at a character; an emoji followed by the
/// text variation selector, alone or as a keycap, is text.
pub(crate) fn sequences(text: &str) -> Vec<u8> {
    use Category::*;
    let mut marks = vec![0; text.len()];
    if text.is_ascii() {
        return marks;
    }
    let chars: Vec<(usize, Category)> =
        text.char_indices().map(|(b, c)| (b, category(c))).collect();
    let categories: Vec<Category> = chars.iter().map(|c| c.1).collect();
    let mut k = 0;
    while k < chars.len() {
        let c = &categories[k..];
        let text_run = match (c[0], c.get(1), c.get(2)) {
            (KeycapBase, Some(Vs15), Some(EnclosingKeycap)) => 3,
            (e, Some(Vs15), _) if any_emoji(e) => 2,
            _ => 1,
        };
        let emoji = emoji_run(c);
        if emoji >= text_run {
            let end = chars.get(k + emoji).map_or(text.len(), |c| c.0);
            marks[chars[k].0..end].fill(2);
            marks[chars[k].0] = 1;
            k += emoji;
        } else {
            k += text_run;
        }
    }
    marks
}

#[cfg(test)]
mod tests {
    use super::sequences;

    fn emoji(text: &str) -> Vec<bool> {
        let marks = sequences(text);
        text.char_indices().map(|(b, _)| marks[b] > 0).collect()
    }

    #[test]
    fn a_lone_modifier_or_regional_indicator_is_text() {
        assert_eq!(emoji("\u{2757}\u{1F3FB}"), [true, false]);
        assert_eq!(emoji("\u{261D}\u{1F3FB}"), [true, true]);
        assert_eq!(emoji("\u{1F1E6}"), [false]);
        assert_eq!(emoji("\u{1F1E6}\u{1F1E7}\u{1F1E8}"), [true, true, false]);
    }

    #[test]
    fn the_variation_selector_picks_the_presentation() {
        assert_eq!(emoji("\u{263A}"), [false]);
        assert_eq!(emoji("\u{263A}\u{FE0F}"), [true, true]);
        assert_eq!(emoji("\u{2757}\u{FE0E}"), [false, false]);
        assert_eq!(emoji("1\u{FE0F}\u{20E3}"), [true, true, true]);
        assert_eq!(emoji("1\u{20E3}"), [false, false]);
        assert_eq!(emoji("1\u{FE0E}\u{20E3}"), [false, false, false]);
    }

    #[test]
    fn joiners_and_tags_stay_in_the_sequence() {
        assert_eq!(
            emoji("\u{1F468}\u{200D}\u{1F469}\u{200D}a"),
            [true, true, true, false, false]
        );
        assert_eq!(
            emoji("\u{1F3F4}\u{E0067}\u{E0062}\u{E007F}"),
            [true, true, true, true]
        );
        assert_eq!(emoji("\u{263A}\u{20E0}"), [true, true]);
    }
}
