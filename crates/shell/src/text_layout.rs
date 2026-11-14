//! Where the lines of a text end, the way Qt's `QTextLine::layout_helper` decides it
//! (qtbase `src/gui/text/qtextlayout.cpp`): break opportunities come from the Unicode line
//! breaking algorithm as Qt runs it (`line_break`), breakable white space hangs past the
//! line's end and is not part of its width, and `WrapAnywhere` breaks between grapheme clusters.
//! Widths come from the caller, so one walk serves plain text and text made of styled spans.

use icu_segmenter::GraphemeClusterSegmenter;

/// `Text.wrapMode`, by what the line breaker does with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wrap {
    /// `Text.NoWrap`: lines end only at line separators.
    None,
    /// `Text.WordWrap`: only at break opportunities; a longer word overflows.
    Word,
    /// `Text.WrapAnywhere`: between any two grapheme clusters.
    Anywhere,
    /// `Text.Wrap`: at break opportunities, and anywhere within a word longer than the line.
    WordOrAnywhere,
}

impl Wrap {
    /// From the number QML gives `Text.wrapMode` (`QTextOption::WrapMode`).
    pub fn from_qml(mode: i32) -> Wrap {
        match mode {
            1 => Wrap::Word,
            3 => Wrap::Anywhere,
            4 => Wrap::WordOrAnywhere,
            _ => Wrap::None,
        }
    }
}

/// One line of a text, as byte offsets into it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Line {
    pub start: usize,
    /// End of what the line shows and measures: the white space it was broken at is past it.
    pub end: usize,
    /// Where the next line starts: past that white space, or past the line separator.
    pub next: usize,
    /// The line ends in a soft hyphen (U+00AD), which shows as a hyphen there.
    pub hyphen: bool,
    /// Ended by a line separator or the end of the text, not by wrapping.
    pub hard: bool,
}

pub const SOFT_HYPHEN: char = '\u{ad}';

/// `\n` and the Unicode line separator end a line.
pub fn is_separator(c: char) -> bool {
    matches!(c, '\n' | '\u{2028}')
}

/// White space a line may be broken at: `QChar::isSpace` without the no-break spaces.
fn breakable_space(c: char) -> bool {
    c.is_whitespace() && !matches!(c, '\u{a0}' | '\u{2007}' | '\u{202f}')
}

/// Break opportunities, grapheme cluster boundaries and the boundaries of shaped clusters,
/// per byte offset.
pub struct Breaks {
    line: Vec<bool>,
    grapheme: Vec<bool>,
    cluster: Vec<bool>,
}

impl Breaks {
    /// `objects` are the byte ranges of inline objects (images): each is a cluster of its own
    /// with an opportunity on both sides, whatever characters stand for it.
    pub fn new(text: &str, objects: &[(usize, usize)]) -> Breaks {
        let mut line = crate::line_break::line_breaks(text);
        let mut grapheme = vec![false; text.len() + 1];
        for b in GraphemeClusterSegmenter::new().segment_str(text) {
            grapheme[b] = true;
        }
        for &(a, b) in objects {
            for flags in [&mut line, &mut grapheme] {
                flags[a] = true;
                flags[b] = true;
                flags[a + 1..b].fill(false);
            }
        }
        let mut cluster: Vec<bool> = (0..=text.len())
            .map(|at| text.is_char_boundary(at))
            .collect();
        for &(a, b) in objects {
            cluster[a + 1..b].fill(false);
        }
        Breaks {
            line,
            grapheme,
            cluster,
        }
    }

    /// A line ends only where `shaped` says a shaped cluster starts, not inside a ligature:
    /// QTextLine::layout_helper looks for an opportunity after every cluster of glyphs.
    /// Elision is by grapheme cluster whatever the shaping (QTextEngine::elidedText).
    /// `item` as in `line_break::tailor`: the opportunities inside the Thai and the Myanmar
    /// items become the ones Qt finds there, and a syllable of an Indic, a Tibetan and a Khmer
    /// item one character. `text` and `objects` as in `new`.
    pub fn keep(
        &mut self,
        text: &str,
        objects: &[(usize, usize)],
        shaped: &dyn Fn(usize) -> bool,
        item: &dyn Fn(usize) -> Option<[u8; 4]>,
    ) {
        let end = self.cluster.len() - 1;
        for (at, boundary) in self.cluster.iter_mut().enumerate().take(end).skip(1) {
            *boundary = *boundary && shaped(at);
        }
        crate::line_break::tailor(text, item, &mut self.line, &mut self.grapheme);
        for &(a, b) in objects {
            for flags in [&mut self.line, &mut self.grapheme] {
                flags[a] = true;
                flags[b] = true;
                flags[a + 1..b].fill(false);
            }
        }
    }

    /// The end of the shaped cluster that starts at `pos`.
    fn next_cluster(&self, pos: usize) -> usize {
        (pos + 1..self.cluster.len())
            .find(|&k| self.cluster[k])
            .unwrap_or(self.cluster.len() - 1)
    }

    /// The end of the grapheme cluster that starts at `pos`.
    pub fn next_grapheme(&self, pos: usize) -> usize {
        (pos + 1..self.grapheme.len())
            .find(|&k| self.grapheme[k])
            .unwrap_or(self.grapheme.len() - 1)
    }

    /// The start of the grapheme cluster that ends at `pos`.
    pub fn prev_grapheme(&self, pos: usize) -> usize {
        (0..pos).rev().find(|&k| self.grapheme[k]).unwrap_or(0)
    }
}

/// The line that starts at `from`. `measure(a, b, hyphen)` is the width of `text[a..b]` laid
/// out from the start of a line, plus a hyphen's when `hyphen`; `overhang(a, b)` is how far
/// the last glyph of `text[a..b]` reaches past its advance (its negative right bearing), which
/// has to fit the line too.
pub fn line_at(
    text: &str,
    breaks: &Breaks,
    from: usize,
    wrap: Wrap,
    width: f64,
    measure: &dyn Fn(usize, usize, bool) -> f64,
    overhang: &dyn Fn(usize, usize) -> f64,
) -> Line {
    let line = walk(text, breaks, from, wrap, width, measure, overhang);
    // QTextOption::WrapAtWordBoundaryOrAnywhere: a line still wider than allowed is laid out
    // again breaking anywhere.
    let wide = measure(line.start, line.end, line.hyphen) + overhang(line.start, line.end);
    if wrap == Wrap::WordOrAnywhere && wide > width {
        return walk(text, breaks, from, Wrap::Anywhere, width, measure, overhang);
    }
    line
}

fn walk(
    text: &str,
    breaks: &Breaks,
    from: usize,
    wrap: Wrap,
    width: f64,
    measure: &dyn Fn(usize, usize, bool) -> f64,
    overhang: &dyn Fn(usize, usize) -> f64,
) -> Line {
    let char_at = |p: usize| text[p..].chars().next();
    let ends_in_hyphen = |end: usize| end > from && text[from..end].ends_with(SOFT_HYPHEN);
    // What the line holds for sure ends at `committed`; `spaces` is the end of the white space
    // after it, which joins the line only with the text that follows it.
    let (mut committed, mut spaces, mut pos) = (from, from, from);
    let found = |end: usize, next: usize, hard: bool| Line {
        start: from,
        end,
        next,
        hyphen: ends_in_hyphen(end),
        hard,
    };
    let full = |committed: usize, pos: usize, hyphen: bool| {
        committed > from
            && wrap != Wrap::None
            && measure(from, pos, hyphen) + overhang(from, pos) > width
    };
    // Around a tab the line is measured without the overhang of the glyph before it.
    let wide = |committed: usize, pos: usize| {
        committed > from && wrap != Wrap::None && measure(from, pos, false) > width
    };
    loop {
        let Some(c) = char_at(pos) else {
            // The end of the text: white space there counts when it fits.
            if !full(committed, pos, false) {
                committed = pos;
            }
            return found(committed, text.len(), true);
        };
        if is_separator(c) {
            return found(committed, pos + c.len_utf8(), true);
        }
        if c == '\t' {
            // A full line ends before the tab, which then opens the next line. Otherwise the
            // white space before the tab joins the line, and the tab is white space as wide
            // as the way to the next tab stop.
            if wide(committed, pos) {
                return found(committed, pos, false);
            }
            committed = pos;
            pos += 1;
            if wide(committed, pos) {
                return found(committed, pos, false);
            }
            (committed, spaces) = (pos, pos);
        } else if breakable_space(c) {
            // White space joins the line a cluster of glyphs at a time, with the marks and
            // joiners shaped onto it.
            while let Some(c) = char_at(pos).filter(|&c| breakable_space(c) && c != '\t')
                && !is_separator(c)
            {
                pos = breaks.next_cluster(pos);
            }
            spaces = pos;
        } else {
            if wrap != Wrap::None
                && spaces > committed
                && measure(from, spaces, false) - measure(from, committed, false) > width
            {
                return found(committed, spaces, false);
            }
            loop {
                pos = breaks.next_cluster(pos);
                if char_at(pos).is_none_or(breakable_space)
                    || breaks.line[pos]
                    || wrap == Wrap::Anywhere && breaks.grapheme[pos]
                {
                    break;
                }
            }
            if full(committed, pos, ends_in_hyphen(pos)) {
                return found(committed, spaces, false);
            }
            (committed, spaces) = (pos, pos);
        }
    }
}

/// The lines of a text that does not wrap, found without the Unicode tables.
fn hard_line(text: &str, from: usize) -> Line {
    let rest = &text[from..];
    let (len, next) = match rest.find(is_separator) {
        // White space before a line separator is not part of the line.
        Some(k) => (
            rest[..k].trim_end_matches(breakable_space).len(),
            k + rest[k..].chars().next().map_or(0, char::len_utf8),
        ),
        None => (rest.len(), rest.len()),
    };
    Line {
        start: from,
        end: from + len,
        next: from + next,
        hyphen: rest[..len].ends_with(SOFT_HYPHEN),
        hard: true,
    }
}

/// What bounds the lines of a Text: the inputs of QQuickTextPrivate::setupTextLayout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub wrap: Wrap,
    /// The width lines are wrapped and elided at.
    pub width: f64,
    /// `Text.elide` (ElideLeft 0, ElideRight 1, ElideMiddle 2) of a text that is one line.
    pub elide: Option<u32>,
    /// ElideRight with a height or a `maximumLineCount` to cut the lines at: the last line
    /// shown is elided.
    pub multiline_elide: bool,
    /// The height the lines are cut at when `multiline_elide`; infinite when the item's
    /// height is not set.
    pub height: f64,
    /// `Text.maximumLineCount`.
    pub max_lines: Option<usize>,
    /// Elided with no width, or no height, to show anything in.
    pub hidden: bool,
}

impl Bounds {
    /// Lines that end only at line separators.
    pub const NATURAL: Bounds = Bounds::wrapped(Wrap::None, 0.0);

    /// Lines wrapped at `width`, all of them shown in full.
    pub const fn wrapped(wrap: Wrap, width: f64) -> Bounds {
        Bounds {
            wrap,
            width,
            elide: None,
            multiline_elide: false,
            height: f64::INFINITY,
            max_lines: None,
            hidden: false,
        }
    }
}

/// How a shown line is cut off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cut {
    /// The text is this one line, wider than allowed: elided in this `Text.elide` mode.
    Width(u32),
    /// More lines follow that are not shown: elided on the right.
    Rest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub line: Line,
    pub cut: Option<Cut>,
}

pub struct Flow {
    /// The lines shown.
    pub rows: Vec<Row>,
    /// `Text.truncated`: lines were cut off or elided.
    pub truncated: bool,
    /// The lines up to `maximumLineCount`, whatever the height: the implicit height is theirs.
    pub lines: Vec<Line>,
    /// The boundaries the lines were broken at, when they were needed.
    pub breaks: Option<Breaks>,
    /// QTextLayout::maximumWidth once the lines are laid out: the implicit width.
    pub max_width: f64,
    /// The last row is elided because the line after it does not fit the height.
    pub height_cut: bool,
}

/// QTextEngine::maxWidth as QTextLine::layout_helper adds it up: the widest paragraph, each
/// the sum of its lines with the white space they were broken at, as wide as they were laid
/// out (so a tab counts from the start of its line, and a soft hyphen that ends a line counts).
/// Past the `max_lines` lines QQuickText shows, it lays out unwrapped the rest of the paragraph
/// and the paragraphs after it until it has counted `max_lines` of them.
fn max_width(
    text: &str,
    lines: &[Line],
    max_lines: Option<usize>,
    measure: &dyn Fn(usize, usize, bool) -> f64,
    natural: &dyn Fn(&Line) -> f64,
) -> f64 {
    let (mut widest, mut paragraph) = (0.0_f64, 0.0);
    for l in lines {
        if l.hard {
            // White space at the end of the text that did not fit the line still counts.
            let hanging = if l.next >= text.len() && !text.ends_with(is_separator) {
                measure(l.start, text.len(), false) - measure(l.start, l.end, false)
            } else {
                0.0
            };
            widest = widest.max(paragraph + natural(l) + hanging);
            paragraph = 0.0;
        } else {
            paragraph += measure(l.start, l.end, l.hyphen) + measure(l.start, l.next, false)
                - measure(l.start, l.end, false);
            widest = widest.max(paragraph);
        }
    }
    let (Some(max), Some(last)) = (max_lines, lines.last()) else {
        return widest;
    };
    let counted = 1 + lines.iter().filter(|l| l.hard).count();
    if lines.len() < max || last.next >= text.len() || counted >= max {
        return widest;
    }
    let mut more = max - counted;
    let mut at = last.next;
    if !last.hard {
        let rest = hard_line(text, at);
        widest = widest.max(paragraph + natural(&rest));
        at = rest.next;
    } else if !text[at..].starts_with(is_separator) {
        more += 1;
    }
    while more > 0 && (at < text.len() || text.ends_with(is_separator)) {
        let line = hard_line(text, at);
        widest = widest.max(natural(&line));
        if line.next >= text.len() {
            break;
        }
        at = line.next;
        more -= 1;
    }
    widest
}

/// What `flow` asks of the shaped text: `shaped` as in `Breaks::keep`, `measure` and
/// `overhang` as in `line_at`, `natural` and `height` a line's QTextLine::naturalTextWidth and
/// height.
#[derive(Clone, Copy)]
pub struct Measures<'a> {
    pub shaped: &'a dyn Fn(usize) -> bool,
    pub item: &'a dyn Fn(usize) -> Option<[u8; 4]>,
    pub measure: &'a dyn Fn(usize, usize, bool) -> f64,
    pub overhang: &'a dyn Fn(usize, usize) -> f64,
    pub natural: &'a dyn Fn(&Line) -> f64,
    pub height: &'a dyn Fn(&Line) -> f64,
}

/// The lines of a Text and which of them it shows: the line loop of
/// QQuickTextPrivate::setupTextLayout. `objects` as in `Breaks::new`, `measure` as in
/// `overhang` as in `line_at`; `natural` and `height` are a line's QTextLine::naturalTextWidth
/// and height; `shaped` and `item` as in `Breaks::keep`.
pub fn flow(text: &str, objects: &[(usize, usize)], bounds: &Bounds, by: &Measures) -> Flow {
    let Measures {
        shaped,
        item,
        measure,
        overhang,
        natural,
        height,
    } = *by;
    // With `multiline_elide` the last line allowed is not broken at words: it is elided.
    let no_break_last = bounds.multiline_elide
        && matches!(bounds.wrap, Wrap::Word | Wrap::WordOrAnywhere)
        && bounds.max_lines.is_some();
    let breaks = (bounds.wrap != Wrap::None).then(|| {
        let mut breaks = Breaks::new(text, objects);
        breaks.keep(text, objects, shaped, item);
        breaks
    });
    let mut lines: Vec<Line> = Vec::new();
    loop {
        let from = lines.last().map_or(0, |l| l.next);
        let line = match &breaks {
            Some(breaks) => {
                let wrap = if no_break_last && Some(lines.len() + 1) == bounds.max_lines {
                    Wrap::Anywhere
                } else {
                    bounds.wrap
                };
                line_at(text, breaks, from, wrap, bounds.width, measure, overhang)
            }
            None => hard_line(text, from),
        };
        // A text that ends in a line separator has one more, empty, line after it.
        let last = line.next >= text.len() && (from >= text.len() || !text.ends_with(is_separator));
        lines.push(line);
        if last || Some(lines.len()) == bounds.max_lines {
            break;
        }
    }
    let mut flow = Flow {
        rows: Vec::new(),
        truncated: bounds.hidden,
        max_width: max_width(text, &lines, bounds.max_lines, measure, natural),
        height_cut: false,
        lines,
        breaks,
    };
    if bounds.hidden {
        return flow;
    }
    let mut used = 0.0;
    for (n, line) in flow.lines.iter().enumerate() {
        used += height(line);
        // The line that does not fit the height is dropped and the one before it elided.
        if bounds.multiline_elide && used > bounds.height && n > 0 {
            flow.truncated = true;
            flow.height_cut = true;
            flow.rows[n - 1].cut = Some(Cut::Rest);
            break;
        }
        let mut row = Row {
            line: *line,
            cut: None,
        };
        if line.next >= text.len() {
            if let Some(mode) = bounds.elide
                && n == 0
                && natural(line) > bounds.width
            {
                flow.truncated = true;
                row.cut = Some(Cut::Width(mode));
            }
            flow.rows.push(row);
            break;
        }
        if Some(n + 1) == bounds.max_lines {
            flow.truncated = true;
            if bounds.multiline_elide {
                row.cut = Some(Cut::Rest);
            }
        }
        flow.rows.push(row);
    }
    flow
}

/// QTextEngine::elidedText: what is kept of the line `text[start..to]` for an ellipsis
/// `ellipsis` wide to fit `width` with it, as (the end of the head kept, the start of the tail
/// kept); None when the ellipsis alone is too wide. `mode` is `Text.elide` (ElideLeft 0,
/// ElideRight 1, ElideMiddle 2); `width_of(a, b)` is the width of `text[a..b]` as shaped in the
/// text. Clusters are taken while they are narrower than the room left of the ellipsis, and the
/// one that reaches it is dropped; so is the last cluster of the line.
pub fn elide(
    breaks: &Breaks,
    mode: u32,
    start: usize,
    to: usize,
    width: f64,
    ellipsis: f64,
    width_of: &dyn Fn(usize, usize) -> f64,
) -> Option<(usize, usize)> {
    let available = width - ellipsis;
    if available < 0.0 {
        return None;
    }
    let (mut head, mut tail) = (0.0, 0.0);
    let (mut next_left, mut next_right) = (start, to);
    loop {
        let (left, right) = (next_left, next_right);
        if mode != 0 {
            next_left = breaks.next_grapheme(left);
            head += width_of(left, next_left);
        }
        if mode != 1 {
            next_right = breaks.prev_grapheme(right);
            tail += width_of(next_right, right);
        }
        if next_left >= to || next_right <= start || head + tail >= available {
            return Some(match mode {
                0 => (start, right),
                1 => (left, to),
                _ => (left, right),
            });
        }
    }
}

/// What QTextEngine::elidedText puts between the head `text[start..head]` and the tail
/// `text[tail..to]` that `elide` keeps of a line: `ellipsis`, joined (U+200D) to a letter that
/// was joined to the one cut off, so that it keeps its shape, and on the side cut off the
/// bidirectional control characters of what is cut, which stay.
pub fn ellipsis_between(
    text: &str,
    mode: u32,
    (start, to): (usize, usize),
    (head, tail): (usize, usize),
    ellipsis: &str,
) -> String {
    const ZWJ: &str = "\u{200d}";
    let controls = |part: &str| -> String {
        let keep =
            |c: &char| matches!(*c as u32, 0x202a..=0x202e | 0x200e..=0x200f | 0x2066..=0x2069);
        part.chars().filter(keep).collect()
    };
    let before = match qsg::text::next_char_joins(&text[head..]) {
        true if mode != 0 => ZWJ,
        _ => "",
    };
    let after = match qsg::text::prev_char_joins(&text[..tail]) {
        true if mode != 1 => ZWJ,
        _ => "",
    };
    match mode {
        0 => format!("{}{ellipsis}{after}", controls(&text[start..tail])),
        1 => format!("{before}{ellipsis}{}", controls(&text[head..to])),
        _ => format!("{before}{ellipsis}{after}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character 10 wide, a tab to the next multiple of 80, a soft hyphen nothing.
    fn width(s: &str, hyphen: bool) -> f64 {
        let mut x = 0.0;
        for c in s.chars() {
            match c {
                '\t' => x = ((x / 80.0_f64).floor() + 1.0) * 80.0,
                SOFT_HYPHEN | '\r' | '\u{200b}' => {}
                _ => x += 10.0,
            }
        }
        x + if hyphen { 10.0 } else { 0.0 }
    }

    fn laid(text: &str, wrap: Wrap, w: f64) -> Vec<String> {
        let measure = |a: usize, b: usize, h: bool| width(&text[a..b], h);
        let bounds = Bounds::wrapped(wrap, w);
        let by = Measures {
            shaped: &|_| true,
            item: &|_| None,
            measure: &measure,
            overhang: &|_, _| 0.0,
            natural: &|_| 0.0,
            height: &|_| 20.0,
        };
        flow(text, &[], &bounds, &by)
            .lines
            .iter()
            .map(|l| {
                format!(
                    "{}{}",
                    &text[l.start..l.end],
                    if l.hyphen { "-" } else { "" }
                )
            })
            .collect()
    }

    #[test]
    fn breaks_at_unicode_line_break_opportunities() {
        // Finding 5: not only at spaces, `/` and `-`.
        assert_eq!(
            laid("中文文本需要换行", Wrap::Word, 40.0),
            ["中文文本", "需要换行"]
        );
        assert_eq!(
            laid("zero\u{200b}width", Wrap::Word, 60.0),
            ["zero\u{200b}", "width"]
        );
        assert_eq!(laid("ab\tcd\tef", Wrap::Word, 100.0), ["ab\tcd", "ef"]);
        // A line that is already too wide ends before the tab, which is the next line.
        assert_eq!(
            laid("abcdefghijkl\tefg", Wrap::Word, 100.0),
            ["abcdefghijkl", "\t", "efg"]
        );
        assert_eq!(laid("aaaa—bbbb", Wrap::Word, 60.0), ["aaaa—", "bbbb"]);
        assert_eq!(laid("aaaa|bbbb", Wrap::Word, 60.0), ["aaaa|", "bbbb"]);
        assert_eq!(
            laid("path?with=query&and", Wrap::Word, 100.0),
            ["path?", "with=query&and"]
        );
        assert_eq!(
            laid("soft\u{ad}hyphen", Wrap::Word, 60.0),
            ["soft\u{ad}-", "hyphen"]
        );
        assert_eq!(
            laid("no\u{a0}break here", Wrap::Word, 80.0),
            ["no\u{a0}break", "here"]
        );
    }

    #[test]
    fn white_space_at_a_break_is_not_part_of_the_line() {
        // Finding 7.
        assert_eq!(
            laid("alpha beta \ngamma", Wrap::Word, 100.0),
            ["alpha beta", "gamma"]
        );
        assert_eq!(laid("alpha  beta", Wrap::Word, 60.0), ["alpha", "beta"]);
        assert_eq!(laid("  alpha beta", Wrap::Word, 80.0), ["  alpha", "beta"]);
        assert_eq!(laid("alpha \n", Wrap::None, 0.0), ["alpha", ""]);
        assert_eq!(laid("alpha ", Wrap::None, 0.0), ["alpha "]);
    }

    #[test]
    fn wrap_anywhere_fills_each_line() {
        // Finding 8.
        assert_eq!(
            laid("delta epsilon zeta", Wrap::Anywhere, 50.0),
            ["delta", "epsil", "on ze", "ta"]
        );
        assert_eq!(
            laid("delta epsilon zeta", Wrap::WordOrAnywhere, 50.0),
            ["delta", "epsil", "on", "zeta"]
        );
        assert_eq!(
            laid("delta epsilon", Wrap::Word, 50.0),
            ["delta", "epsilon"]
        );
    }
}
