//! `QTextDocument::toMarkdown()` of a document imported from Markdown: what a TextEdit shows
//! when its `textFormat` goes from `MarkdownText` to `PlainText`.
//!
//! Two halves, each following Qt 6.12: [`Importer`] builds the blocks QTextMarkdownImporter
//! builds from the parser's events (Qt parses with md4c in its GitHub dialect; here
//! pulldown-cmark reports the same structure), and [`Writer`] is QTextMarkdownWriter, line
//! for line, including what it gets wrong.

use pulldown_cmark::{CodeBlockKind, Event, LinkType, Options, Parser, Tag, TagEnd};

/// The Markdown Qt writes for a document imported from `md`, under a regular application font.
pub fn round_trip(md: &str) -> String {
    round_trip_in(md, false)
}

/// [`round_trip`] where the application font is bold to QFontInfo (`font_bold`): heavier than
/// QFont::Normal, as kdeglobals' Medium is. The writer reads a fragment's weight off
/// QFontInfo(QTextCharFormat::font()), a font resolved over the application's, so it then
/// finds every fragment bold and writes each block between `**`.
pub fn round_trip_in(md: &str, font_bold: bool) -> String {
    let mut imp = Importer::default();
    imp.import(md);
    let mut w = Writer {
        font_bold,
        last_list_indent: 1,
        ..Default::default()
    };
    w.write_frame(&imp.doc, &imp.doc.root, None);
    // The text is then read back with QTextDocument::toPlainText, which has no
    // non-breaking space.
    String::from_utf16_lossy(&w.out).replace('\u{a0}', " ")
}

/// QTextCharFormat, as far as the writer looks at it.
#[derive(Clone, Default, PartialEq, Debug)]
struct CharFmt {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    mono: bool,
    /// AnchorHref and the TextToolTip set with it.
    link: Option<(String, String)>,
    /// QTextImageFormat: name, ImageAltText, ImageTitle.
    image: Option<(String, String, String)>,
}

/// QTextBlockFormat. An `Option` is a property that may be absent (`hasProperty`).
#[derive(Clone, Default, PartialEq, Debug)]
struct BlockFmt {
    heading: usize,
    quote: Option<usize>,
    code_lang: Option<String>,
    fence: Option<char>,
    non_breakable: bool,
    /// BlockMarker: checked or not.
    marker: Option<bool>,
    indent: Option<usize>,
    hr: bool,
}

#[derive(Clone, Default, Debug)]
struct Block {
    fmt: BlockFmt,
    frags: Vec<(Vec<u16>, CharFmt)>,
    /// The QTextList it is an item of.
    list: Option<usize>,
}

impl Block {
    fn text_len(&self) -> usize {
        self.frags.iter().map(|f| f.0.len()).sum()
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum ListStyle {
    Disc,
    Circle,
    Square,
    Decimal,
}

/// QTextListFormat and the list's items (block indices), in order.
#[derive(Clone, Debug)]
struct List {
    indent: usize,
    style: ListStyle,
    suffix: char,
    start: i64,
    items: Vec<usize>,
}

/// A table cell's block: row, column, block.
type Cell = (usize, usize, usize);

/// A child of a frame: a block, or a table with its cells' blocks.
#[derive(Debug)]
enum Node {
    Block(usize),
    Table { cols: usize, cells: Vec<Cell> },
}

#[derive(Default, Debug)]
struct Doc {
    blocks: Vec<Block>,
    lists: Vec<List>,
    root: Vec<Node>,
}

impl Doc {
    /// QTextBlock::next() over the blocks outside tables, for `Writer::loose`.
    fn order(&self) -> Vec<usize> {
        let mut v = vec![];
        for n in &self.root {
            match n {
                Node::Block(b) => v.push(*b),
                Node::Table { cells, .. } => v.extend(cells.iter().map(|c| c.2)),
            }
        }
        v
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Normal,
    Br,
    SoftBr,
    Code,
    Html,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum BlockType {
    Other,
    Code,
    Td,
}

/// QTextMarkdownImporter: its members under their Qt names, its callbacks as methods.
#[derive(Default)]
struct Importer {
    doc: Doc,
    /// The block the cursor is in.
    cur: usize,
    /// The char format the cursor inserts with.
    fmt: CharFmt,
    span_stack: Vec<CharFmt>,
    list_stack: Vec<usize>,
    list_format: Option<List>,
    needs_insert_block: bool,
    needs_insert_list: bool,
    list_item: bool,
    marker: Option<bool>,
    quote_depth: usize,
    code_block: bool,
    code_lang: String,
    code_fence: Option<char>,
    block_type: Option<BlockType>,
    html_depth: i32,
    html: String,
    image: Option<(String, String)>,
    /// The table being filled: index in `doc.root`, row, column.
    table: Option<(usize, usize, usize)>,
    table_head: bool,
}

fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

impl Importer {
    fn import(&mut self, md: &str) {
        // A QTextDocument always has one block.
        self.doc.blocks.push(Block::default());
        self.doc.root.push(Node::Block(0));
        let mut opts = Options::empty();
        opts.insert(Options::ENABLE_TABLES);
        opts.insert(Options::ENABLE_STRIKETHROUGH);
        opts.insert(Options::ENABLE_TASKLISTS);
        let src = md.as_bytes();
        let at = |i: usize| src.get(i).copied().unwrap_or(0) as char;
        let mut code: Option<String> = None;
        let mut in_link = 0;
        for (ev, range) in Parser::new_ext(md, opts).into_offset_iter() {
            match ev {
                Event::Start(tag) => match tag {
                    Tag::Paragraph => {
                        self.block_type = Some(BlockType::Other);
                        self.needs_insert_block = true;
                    }
                    Tag::Heading { level, .. } => self.enter_heading(level as usize),
                    Tag::BlockQuote(_) => {
                        self.block_type = Some(BlockType::Other);
                        self.quote_depth += 1;
                    }
                    Tag::CodeBlock(kind) => {
                        self.block_type = Some(BlockType::Code);
                        self.code_block = true;
                        (self.code_lang, self.code_fence) = match kind {
                            CodeBlockKind::Fenced(info) => {
                                let lang = info.split_whitespace().next().unwrap_or("");
                                let fence = md[range.clone()].trim_start().chars().next();
                                (lang.to_string(), fence)
                            }
                            CodeBlockKind::Indented => (String::new(), None),
                        };
                        self.needs_insert_block = true;
                        code = Some(String::new());
                    }
                    Tag::List(start) => {
                        self.block_type = Some(BlockType::Other);
                        if self.needs_insert_list {
                            // A list nested in an empty list.
                            let l = self.create_list();
                            self.list_stack.push(l);
                        } else {
                            self.needs_insert_list = true;
                        }
                        let mut i = range.start;
                        while at(i).is_ascii_digit() {
                            i += 1;
                        }
                        let mark = at(i);
                        self.list_format = Some(List {
                            indent: self.list_stack.len() + 1,
                            style: match (start, mark) {
                                (Some(_), _) => ListStyle::Decimal,
                                (None, '*') => ListStyle::Circle,
                                (None, '+') => ListStyle::Square,
                                _ => ListStyle::Disc,
                            },
                            suffix: if start.is_some() { mark } else { '.' },
                            start: start.map_or(-1, |s| s as i64),
                            items: vec![],
                        });
                    }
                    Tag::Item => {
                        self.block_type = Some(BlockType::Other);
                        self.needs_insert_block = true;
                        self.list_item = true;
                        self.marker = None;
                    }
                    Tag::Table(_) => {
                        self.block_type = Some(BlockType::Other);
                        // QTextCursor::insertTable: the table, then an empty block after it.
                        self.doc.root.push(Node::Table {
                            cols: 0,
                            cells: vec![],
                        });
                        self.table = Some((self.doc.root.len() - 1, 0, 0));
                        self.doc.blocks.push(Block::default());
                        self.doc.root.push(Node::Block(self.doc.blocks.len() - 1));
                    }
                    Tag::TableHead => self.table_head = true,
                    Tag::TableRow => {
                        if let Some(t) = &mut self.table {
                            t.1 += 1;
                            t.2 = 0;
                        }
                    }
                    Tag::TableCell => {
                        self.block_type = Some(BlockType::Td);
                        self.enter_cell();
                    }
                    Tag::HtmlBlock => self.block_type = Some(BlockType::Other),
                    // MD_FLAG_UNDERLINE: `_` underlines, however many of them.
                    Tag::Emphasis | Tag::Strong if at(range.start) == '_' => {
                        self.enter_span(|f| f.underline = true)
                    }
                    Tag::Emphasis => self.enter_span(|f| f.italic = true),
                    Tag::Strong => self.enter_span(|f| f.bold = true),
                    Tag::Strikethrough => self.enter_span(|f| f.strike = true),
                    Tag::Link {
                        link_type,
                        dest_url,
                        title,
                        ..
                    } => {
                        in_link += 1;
                        let href = match link_type {
                            LinkType::Email => format!("mailto:{dest_url}"),
                            _ => dest_url.to_string(),
                        };
                        self.enter_span(|f| f.link = Some((href, title.to_string())));
                    }
                    Tag::Image {
                        dest_url, title, ..
                    } => {
                        in_link += 1;
                        self.image = Some((dest_url.to_string(), title.to_string()));
                        self.enter_span(|_| ());
                    }
                    _ => {}
                },
                Event::End(tag) => match tag {
                    TagEnd::Paragraph | TagEnd::Item => self.list_item = false,
                    TagEnd::Heading(_) => self.fmt = CharFmt::default(),
                    TagEnd::BlockQuote(_) => {
                        self.quote_depth -= 1;
                        self.needs_insert_block = true;
                    }
                    TagEnd::CodeBlock => {
                        // md4c reports a code block line by line, each newline on its own.
                        for line in code.take().unwrap_or_default().split_inclusive('\n') {
                            let text = line.strip_suffix('\n');
                            if !text.unwrap_or(line).is_empty() {
                                self.text(Kind::Code, text.unwrap_or(line));
                            }
                            if text.is_some() {
                                self.text(Kind::Code, "\n");
                            }
                        }
                        self.code_block = false;
                        self.code_lang.clear();
                        self.code_fence = None;
                        self.needs_insert_block = true;
                    }
                    TagEnd::List(_) => {
                        if self.needs_insert_list {
                            let l = self.create_list();
                            self.list_stack.push(l);
                        }
                        self.list_stack.pop();
                    }
                    TagEnd::Table => {
                        self.table = None;
                        // QTextCursor::End: the empty block after the table.
                        self.cur = self.doc.blocks.len() - 1;
                        if let Some(Node::Block(b)) = self.doc.root.last() {
                            self.cur = *b;
                        }
                    }
                    TagEnd::TableHead => self.table_head = false,
                    TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.leave_span(),
                    TagEnd::Link => {
                        in_link -= 1;
                        self.leave_span();
                    }
                    TagEnd::Image => {
                        in_link -= 1;
                        self.leave_span();
                        self.image = None;
                    }
                    _ => {}
                },
                Event::Text(t) => match &mut code {
                    Some(c) => c.push_str(&t),
                    None if in_link == 0 => self.text_with_autolinks(&t),
                    None => self.text(Kind::Normal, &t),
                },
                Event::Code(t) => {
                    self.enter_span(|f| f.mono = true);
                    self.text(Kind::Code, &t);
                    self.leave_span();
                }
                // A block of HTML ends with its line; QTextCursor::insertHtml drops that.
                Event::Html(t) => self.text(Kind::Html, t.trim_end_matches('\n')),
                Event::InlineHtml(t) => self.text(Kind::Html, &t),
                Event::SoftBreak => self.text(Kind::SoftBr, " "),
                Event::HardBreak => self.text(Kind::Br, "\n"),
                Event::Rule => {
                    self.block_type = Some(BlockType::Other);
                    let fmt = BlockFmt {
                        hr: true,
                        ..Default::default()
                    };
                    self.insert_block_with(fmt, None);
                    self.fmt = CharFmt::default();
                }
                Event::TaskListMarker(on) => self.marker = Some(on),
                _ => {}
            }
        }
    }

    /// MD_FLAG_PERMISSIVEAUTOLINKS: a bare URL in the text is a link to itself.
    fn text_with_autolinks(&mut self, t: &str) {
        let mut rest = t;
        loop {
            let found = ["http://", "https://", "ftp://", "www."]
                .iter()
                .filter_map(|p| rest.find(p))
                .min()
                .filter(|&i| !rest[..i].ends_with(|c: char| c.is_alphanumeric()));
            let Some(i) = found else { break };
            let tail = &rest[i..];
            let mut end = tail
                .find(|c: char| c.is_whitespace() || c == '<')
                .unwrap_or(tail.len());
            while tail[..end].ends_with(['.', ',', ':', ';', '!', '?', '"', '\'', '*', '_', '~']) {
                end -= 1;
            }
            let url = &tail[..end];
            if url
                .split_once("://")
                .map_or(url.len() <= 4, |(_, host)| host.is_empty())
            {
                break;
            }
            if i > 0 {
                self.text(Kind::Normal, &rest[..i]);
            }
            let href = if url.starts_with("www.") {
                format!("http://{url}")
            } else {
                url.to_string()
            };
            self.enter_span(|f| f.link = Some((href, String::new())));
            self.text(Kind::Normal, url);
            self.leave_span();
            rest = &tail[end..];
        }
        if !rest.is_empty() {
            self.text(Kind::Normal, rest);
        }
    }

    fn is_empty(&self) -> bool {
        self.doc.blocks.len() == 1 && self.doc.blocks[0].frags.is_empty()
    }

    /// QTextCursor::insertBlock after the cursor's block, in its frame.
    fn insert_block_with(&mut self, fmt: BlockFmt, list: Option<usize>) {
        self.doc.blocks.push(Block {
            fmt,
            frags: vec![],
            list,
        });
        let b = self.doc.blocks.len() - 1;
        let cur = self.cur;
        let in_cell = self.doc.root.iter_mut().find_map(|n| match n {
            Node::Table { cells, .. } => {
                let i = cells.iter().position(|c| c.2 == cur)?;
                let (r, c, _) = cells[i];
                cells.insert(i + 1, (r, c, b));
                Some(())
            }
            Node::Block(_) => None,
        });
        if in_cell.is_none() {
            let at = self
                .doc
                .root
                .iter()
                .position(|n| matches!(n, Node::Block(x) if *x == cur))
                .map_or(self.doc.root.len(), |i| i + 1);
            self.doc.root.insert(at, Node::Block(b));
        }
        if let Some(l) = list {
            self.doc.lists[l].items.push(b);
        }
        self.cur = b;
    }

    /// QTextCursor::createList: the cursor's block becomes the first item of a new list.
    fn create_list(&mut self) -> usize {
        let mut l = self.list_format.clone().expect("a list format");
        l.items = vec![self.cur];
        self.doc.lists.push(l);
        let id = self.doc.lists.len() - 1;
        self.doc.blocks[self.cur].list = Some(id);
        id
    }

    fn enter_heading(&mut self, level: usize) {
        self.block_type = Some(BlockType::Other);
        let fmt = BlockFmt {
            heading: level,
            ..Default::default()
        };
        self.needs_insert_block = false;
        if self.is_empty() {
            self.doc.blocks[self.cur].fmt = fmt;
        } else {
            self.insert_block_with(fmt, None);
        }
        // The heading's bold: the writer does not look at it.
        self.fmt = CharFmt::default();
    }

    fn enter_cell(&mut self) {
        let Some((t, row, col)) = self.table else {
            return;
        };
        let row = if self.table_head { 0 } else { row };
        self.doc.blocks.push(Block::default());
        let b = self.doc.blocks.len() - 1;
        if let Node::Table { cols, cells } = &mut self.doc.root[t] {
            *cols = (*cols).max(col + 1);
            cells.push((row, col, b));
        }
        self.cur = b;
        if let Some(t) = &mut self.table {
            t.2 += 1;
        }
    }

    fn enter_span(&mut self, set: impl FnOnce(&mut CharFmt)) {
        let mut f = self.span_stack.last().cloned().unwrap_or_default();
        set(&mut f);
        self.span_stack.push(f.clone());
        self.fmt = f;
    }

    fn leave_span(&mut self) {
        self.span_stack.pop();
        self.fmt = self.span_stack.last().cloned().unwrap_or_default();
    }

    /// cbText.
    fn text(&mut self, kind: Kind, text: &str) {
        if self.needs_insert_block {
            self.insert_block();
        }
        let mut s = text.to_string();
        match kind {
            Kind::Normal if self.html_depth != 0 => {
                self.html.push_str(&s);
                s.clear();
            }
            Kind::Html => {
                let b = s.as_bytes();
                for i in 0..b.len() {
                    if b[i] == b'<' && b.get(i + 1).is_some_and(|c| c.is_ascii_alphabetic()) {
                        self.html_depth += 1;
                    }
                    if b[i..].starts_with(b"/>") || b[i..].starts_with(b"</") {
                        self.html_depth -= 1;
                    }
                }
                self.html.push_str(&s);
                if self.html_depth == 0 {
                    let html = std::mem::take(&mut self.html);
                    self.insert_html(&html);
                    self.fmt = self.span_stack.last().cloned().unwrap_or_default();
                }
                s.clear();
            }
            _ => {}
        }
        if self.block_type == Some(BlockType::Code) && s == "\n" {
            self.needs_insert_block = true;
            s.clear();
        }
        if let Some((name, title)) = self.image.clone() {
            let fmt = CharFmt {
                image: Some((name, s, title)),
                ..Default::default()
            };
            self.insert_text("\u{fffc}", fmt);
            return;
        }
        if !s.is_empty() {
            self.insert_text(&s, self.fmt.clone());
        }
        if self.doc.blocks[self.cur].list.is_some() {
            self.doc.blocks[self.cur].fmt.indent = Some(0);
        }
    }

    /// QTextCursor::insertText: a newline in it starts a block of the same format.
    fn insert_text(&mut self, s: &str, fmt: CharFmt) {
        for (i, part) in s.split('\n').enumerate() {
            if i > 0 {
                let b = &self.doc.blocks[self.cur];
                self.insert_block_with(b.fmt.clone(), b.list);
            }
            if part.is_empty() {
                continue;
            }
            let frags = &mut self.doc.blocks[self.cur].frags;
            match frags.last_mut() {
                Some(last) if last.1 == fmt && fmt.image.is_none() => {
                    last.0.extend(part.encode_utf16())
                }
                _ => frags.push((utf16(part), fmt.clone())),
            }
        }
    }

    /// QTextCursor::insertHtml, for the inline tags a Markdown text carries: the character
    /// formats of `b`, `i`, `u`, `s` and their synonyms; other tags give their text only.
    fn insert_html(&mut self, html: &str) {
        let mut stack: Vec<(String, CharFmt)> = vec![];
        let mut rest = html;
        let base = self.fmt.clone();
        while !rest.is_empty() {
            let Some(lt) = rest.find('<') else {
                self.insert_html_text(rest, stack.last().map_or(&base, |s| &s.1));
                break;
            };
            self.insert_html_text(&rest[..lt], stack.last().map_or(&base, |s| &s.1));
            let Some(gt) = rest[lt..].find('>') else {
                break;
            };
            let tag = rest[lt + 1..lt + gt].trim().to_ascii_lowercase();
            rest = &rest[lt + gt + 1..];
            if let Some(name) = tag.strip_prefix('/') {
                if let Some(i) = stack.iter().rposition(|s| s.0 == name.trim()) {
                    stack.truncate(i);
                }
                continue;
            }
            let name = tag
                .split(|c: char| c.is_whitespace() || c == '/')
                .next()
                .unwrap_or("");
            let mut f = stack.last().map_or(&base, |s| &s.1).clone();
            match name {
                "b" | "strong" => f.bold = true,
                "i" | "em" | "cite" | "var" | "dfn" => f.italic = true,
                "u" => f.underline = true,
                "s" | "strike" | "del" => f.strike = true,
                _ => {}
            }
            if !tag.ends_with('/') {
                stack.push((name.to_string(), f));
            }
        }
    }

    fn insert_html_text(&mut self, s: &str, fmt: &CharFmt) {
        if s.is_empty() {
            return;
        }
        let mut t = String::new();
        for c in s.chars() {
            let c = if c.is_whitespace() { ' ' } else { c };
            if c != ' ' || !t.ends_with(' ') {
                t.push(c);
            }
        }
        let t = t
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"");
        self.insert_text(&t, fmt.clone());
    }

    /// insertBlock: a new block from the state the callbacks left.
    fn insert_block(&mut self) {
        let char_fmt = self.span_stack.last().cloned().unwrap_or_default();
        let mut fmt = BlockFmt::default();
        let mut list = None;
        if let (Some(&l), false, true) = (
            self.list_stack.last(),
            self.needs_insert_list,
            self.list_item,
        ) && let Some(&last) = self.doc.lists[l].items.last()
        {
            fmt = self.doc.blocks[last].fmt.clone();
        }
        if self.quote_depth > 0 {
            fmt.quote = Some(self.quote_depth);
        }
        if self.code_block {
            fmt.code_lang = Some(self.code_lang.clone());
            if let Some(f) = self.code_fence {
                fmt.non_breakable = true;
                fmt.fence = Some(f);
            }
        } else {
            fmt.code_lang = None;
            fmt.fence = None;
            fmt.non_breakable = false;
        }
        fmt.marker = self.marker;
        if !self.list_stack.is_empty() {
            fmt.indent = Some(self.list_stack.len());
        }
        if self.is_empty() {
            self.doc.blocks[self.cur].fmt = fmt;
        } else {
            self.insert_block_with(fmt, None);
        }
        self.fmt = char_fmt;
        if self.needs_insert_list {
            let l = self.create_list();
            self.list_stack.push(l);
        } else if let (Some(&l), true) = (self.list_stack.last(), self.list_item) {
            self.doc.lists[l].items.push(self.cur);
            list = Some(l);
        }
        if list.is_some() {
            self.doc.blocks[self.cur].list = list;
        }
        self.needs_insert_list = false;
        self.needs_insert_block = false;
        self.list_item = false;
    }
}

const COLUMN_LIMIT: i64 = 80;

/// QTextMarkdownWriter: its members under their Qt names. Lengths and columns count UTF-16
/// units, as QString does.
#[derive(Default)]
struct Writer {
    /// QFontInfo(QFont()).bold().
    font_bold: bool,
    out: Vec<u16>,
    line_prefix: Vec<u16>,
    code_block_fence: Vec<u16>,
    wrapped_line_indent: usize,
    last_list_indent: usize,
    line_prefix_written: bool,
    double_newline_written: bool,
    fenced_code_block: bool,
    indented_code_block: bool,
    /// listInfo's cache: whether a list is loose.
    loose: std::collections::HashMap<usize, bool>,
}

fn is_space(c: u16) -> bool {
    char::from_u32(c as u32).is_some_and(char::is_whitespace)
}

/// `\s` of an ASCII-only QRegularExpression.
fn re_space(c: u16) -> bool {
    matches!(c, 0x20 | 0x09..=0x0d)
}

/// `\w` of an ASCII-only QRegularExpression.
fn re_word(c: u16) -> bool {
    c < 128 && ((c as u8).is_ascii_alphanumeric() || c == b'_' as u16)
}

fn index_of(s: &[u16], from: usize, f: impl Fn(u16) -> bool) -> Option<usize> {
    (from..s.len()).find(|&i| f(s[i]))
}

/// nearestWordWrapIndex: the last whitespace before `before`.
fn nearest_word_wrap_index(s: &[u16], before: i64) -> i64 {
    let before = before.min(s.len() as i64);
    (0..before)
        .rev()
        .find(|&i| is_space(s[i as usize]))
        .unwrap_or(-1)
}

fn adjacent_backticks_count(s: &[u16]) -> usize {
    let tick = b'`' as u16;
    s.split(|&c| c != tick).map(<[u16]>::len).max().unwrap_or(0)
}

/// maybeEscapeFirstChar: a backslash before what would start a heading or a list item.
fn maybe_escape_first_char(s: &mut Vec<u16>) {
    let Some(first) = s.iter().copied().find(|&c| !is_space(c)) else {
        return;
    };
    if b"#*+-".iter().any(|&c| c as u16 == first) {
        let i = s.iter().position(|&c| c == first).unwrap_or(0);
        s.insert(i, b'\\' as u16);
        return;
    }
    // `\d+([\.)])\s` at the start.
    let digits = s
        .iter()
        .take_while(|&&c| c < 128 && (c as u8).is_ascii_digit())
        .count();
    let delim = s
        .get(digits)
        .is_some_and(|&c| c == b'.' as u16 || c == b')' as u16);
    if digits > 0 && delim && s.get(digits + 1).is_some_and(|&c| re_space(c)) {
        s.insert(digits, b'\\' as u16);
    }
}

/// escapeSpecialCharacters: every backslash doubled, then one before a special character
/// that prefixes a word, at most once between two runs of whitespace.
fn escape_special_characters(s: &mut Vec<u16>) {
    let bs = b'\\' as u16;
    let mut i = 0;
    while i < s.len() {
        if s[i] == bs {
            s.insert(i, bs);
            i += 1;
        }
        i += 1;
    }
    let special = |c: u16| b"<!*[`&".iter().any(|&x| x as u16 == c);
    // `[<!*[`&]+[/\w]`: the first match at or after `from`.
    let find = |s: &[u16], from: usize| {
        (from..s.len()).find(|&j| {
            let run = s[j..].iter().take_while(|&&c| special(c)).count();
            // The run may give characters back: any shorter run ends in a special too.
            run > 0
                && s.get(j + run)
                    .is_some_and(|&c| c == b'/' as u16 || re_word(c))
        })
    };
    let mut i = Some(0);
    while let Some(mut at) = i {
        if let Some(j) = find(s, at) {
            s.insert(j, bs);
            at = j + 3;
        }
        i = index_of(s, at, re_space).map(|k| k + 1);
    }
}

impl Writer {
    fn put(&mut self, s: &str) {
        self.out.extend(s.encode_utf16());
    }

    fn spaces(n: usize) -> Vec<u16> {
        vec![b' ' as u16; n]
    }

    fn set_line_prefix_for_block_quote(&mut self, level: usize) {
        self.line_prefix = utf16(&"> ".repeat(level));
    }

    /// listInfo: a list is loose when a block between its first and last item is no list
    /// item.
    fn loose(&mut self, doc: &Doc, list: usize) -> bool {
        if let Some(&l) = self.loose.get(&list) {
            return l;
        }
        let items = &doc.lists[list].items;
        let mut loose = false;
        if items.len() > 1 {
            let order = doc.order();
            let first = order.iter().position(|b| b == &items[0]).unwrap_or(0);
            for &next in &order[first + 1..] {
                if next == items[items.len() - 1] {
                    break;
                }
                if doc.blocks[next].list.is_none() {
                    loose = true;
                    break;
                }
            }
        }
        self.loose.insert(list, loose);
        loose
    }

    /// writeFrame: the root frame (`table` is `None`) or a table's cells.
    fn write_frame(&mut self, doc: &Doc, nodes: &[Node], table: Option<(usize, &[Cell])>) {
        let mut table_row: i64 = -1;
        let mut last_was_list = false;
        let mut widths = vec![];
        let cells: Vec<Node>;
        let nodes = match table {
            Some((cols, t)) => {
                widths = vec![0; cols];
                for &(_, col, b) in t {
                    // A cell's blocks follow each other.
                    let len: usize = t
                        .iter()
                        .filter(|c| {
                            c.1 == col && c.0 == t.iter().find(|x| x.2 == b).map_or(0, |x| x.0)
                        })
                        .map(|c| doc.blocks[c.2].text_len())
                        .sum();
                    widths[col] = widths[col].max(len);
                }
                cells = t.iter().map(|c| Node::Block(c.2)).collect();
                &cells[..]
            }
            None => nodes,
        };
        let default = BlockFmt::default();
        for (i, node) in nodes.iter().enumerate() {
            let b = match node {
                Node::Table { cols, cells } => {
                    self.write_frame(doc, &[], Some((*cols, cells)));
                    continue;
                }
                Node::Block(b) => *b,
            };
            let block = &doc.blocks[b];
            let format = &block.fmt;
            let next_format = match nodes.get(i + 1) {
                Some(Node::Block(n)) => &doc.blocks[*n].fmt,
                _ => &default,
            };
            let ending = i + 1 == nodes.len();
            let block_quote_indent = format.quote.unwrap_or(0);
            let next_block_quote_indent = next_format.quote.unwrap_or(0);
            let next_is_different = ending
                || next_format.indent.unwrap_or(0) != format.indent.unwrap_or(0)
                || next_format.code_lang != format.code_lang;
            let cell = table.map(|(_, t)| t[i]);
            if let Some((row, _, _)) = cell {
                if table_row < row as i64 {
                    if table_row == 0 {
                        self.put("\n");
                        for w in &widths {
                            self.put("|");
                            self.put(&"-".repeat(*w));
                        }
                        self.put("|");
                    }
                    self.put("\n|");
                    table_row = row as i64;
                }
            } else if block.list.is_none() && last_was_list {
                self.put("\n");
                self.line_prefix_written = false;
            }
            let ending_col = self.write_block(
                doc,
                b,
                table.is_none(),
                table.is_some() && table_row == 0,
                next_is_different && block.list.is_none(),
            );
            self.double_newline_written = false;
            if let Some((_, col, _)) = cell {
                let padding = widths[col] as i64 - ending_col;
                if padding > 0 {
                    self.out.extend(Self::spaces(padding as usize));
                }
                self.put("|");
            } else if self.fenced_code_block && ending {
                self.put("\n");
                let fence = [
                    &self.line_prefix[..],
                    &Self::spaces(self.wrapped_line_indent),
                    &self.code_block_fence,
                ]
                .concat();
                self.out.extend(fence);
                self.put("\n\n");
                self.code_block_fence.clear();
            } else if self.indented_code_block && next_is_different {
                self.put("\n\n");
            } else if ending_col > 0 {
                if block.list.is_some() || format.code_lang.is_some() {
                    self.put("\n");
                    if block.list.is_some() {
                        let prefix = self.line_prefix.clone();
                        self.out.extend(prefix);
                        self.line_prefix_written = true;
                    }
                } else {
                    self.put("\n");
                    if next_block_quote_indent < block_quote_indent {
                        self.set_line_prefix_for_block_quote(next_block_quote_indent);
                    }
                    let prefix = self.line_prefix.clone();
                    self.out.extend(prefix);
                    self.put("\n");
                    self.double_newline_written = true;
                }
            }
            last_was_list = block.list.is_some();
        }
        if table.is_some() {
            self.put("\n\n");
            self.double_newline_written = true;
        }
        self.loose.clear();
    }

    /// writeBlock: the column the block's last line ends at.
    fn write_block(
        &mut self,
        doc: &Doc,
        b: usize,
        mut wrap: bool,
        ignore_format: bool,
        ignore_empty: bool,
    ) -> i64 {
        let block = &doc.blocks[b];
        if block.text_len() == 0 && ignore_empty {
            return 0;
        }
        let fmt = &block.fmt;
        let mut missed_blank_code_block_line = false;
        let code_block = fmt.fence.is_some()
            || fmt.code_lang.as_ref().is_some_and(|l| !l.is_empty())
            || fmt.non_breakable;
        let block_quote_level = fmt.quote.unwrap_or(0);
        if self.fenced_code_block && !code_block {
            let line = [&self.line_prefix[..], &self.code_block_fence].concat();
            self.out.extend(line);
            self.put("\n");
            self.fenced_code_block = false;
            self.code_block_fence.clear();
            self.line_prefix_written = !self.line_prefix.is_empty();
        }
        self.line_prefix.clear();
        if fmt.heading == 0 && block_quote_level > 0 {
            self.set_line_prefix_for_block_quote(block_quote_level);
            if !self.line_prefix_written {
                let prefix = self.line_prefix.clone();
                self.out.extend(prefix);
                self.line_prefix_written = true;
            }
        }
        if let Some(l) = block.list {
            let list = &doc.lists[l];
            let level = list.indent;
            let start = if list.start >= 0 { list.start } else { 1 };
            let number = list
                .items
                .iter()
                .position(|&i| i == b)
                .map_or(-1, |i| i as i64)
                + start;
            let numeric = list.style == ListStyle::Decimal;
            let mut bullet = match list.style {
                ListStyle::Disc => "-",
                ListStyle::Circle => "*",
                ListStyle::Square => "+",
                ListStyle::Decimal => " ",
            }
            .to_string();
            self.wrapped_line_indent = if numeric { 4 } else { 2 };
            match fmt.marker {
                Some(true) => bullet += " [x]",
                Some(false) => bullet += " [ ]",
                None => {}
            }
            let indent_first_line = (level - 1) * if numeric { 4 } else { 2 };
            self.wrapped_line_indent += indent_first_line;
            if self.last_list_indent != level && !self.double_newline_written && self.loose(doc, l)
            {
                self.put("\n");
            }
            self.last_list_indent = level;
            let mut prefix = " ".repeat(indent_first_line);
            if numeric {
                let mut number = format!("{number}{} ", list.suffix);
                if number.len() == 3 {
                    number.push(' ');
                }
                prefix += &number;
            } else {
                prefix += &bullet;
                prefix.push(' ');
            }
            self.put(&prefix);
        } else if fmt.hr {
            self.put("- - -\n");
            return 0;
        } else if code_block {
            if fmt.quote.is_none() {
                missed_blank_code_block_line = true;
            }
            if !self.fenced_code_block {
                let fence = fmt.fence.unwrap_or('`').to_string().repeat(3);
                self.code_block_fence = utf16(&fence);
                if fmt.indent.is_some() {
                    self.code_block_fence = [
                        &Self::spaces(self.wrapped_line_indent)[..],
                        &self.code_block_fence,
                    ]
                    .concat();
                }
                let head = self.code_block_fence.clone();
                self.out.extend(head);
                self.put(fmt.code_lang.as_deref().unwrap_or(""));
                self.put("\n");
                let prefix = self.line_prefix.clone();
                self.out.extend(prefix);
                self.fenced_code_block = true;
            }
            wrap = false;
        } else if fmt.indent.unwrap_or(0) == 0 {
            self.wrapped_line_indent = 0;
            if fmt.code_lang.is_some() {
                self.line_prefix.extend(Self::spaces(4));
                self.indented_code_block = true;
            }
            if !self.line_prefix_written {
                let prefix = self.line_prefix.clone();
                self.out.extend(prefix);
                self.line_prefix_written = true;
            }
        }
        if fmt.heading > 0 {
            self.put(&"#".repeat(fmt.heading));
            self.put(" ");
            wrap = false;
        }
        let wrap_indent_string = [
            &self.line_prefix[..],
            &Self::spaces(self.wrapped_line_indent),
        ]
        .concat();
        let wrapped_line_indent = self.wrapped_line_indent as i64;
        let nl = b'\n' as u16;
        let tick = b'`' as u16;
        let mut col = wrap_indent_string.len() as i64;
        let (mut mono, mut bold, mut italic, mut underline, mut strike_out) =
            (false, false, false, false, false);
        let mut starts_or_ends_with_backtick = false;
        let mut ending_markers = false;
        let mut backticks = vec![tick];
        for (text, cf) in &block.frags {
            missed_blank_code_block_line = false;
            let mut fragment_text = text.clone();
            if !(self.fenced_code_block || self.indented_code_block) {
                escape_special_characters(&mut fragment_text);
                maybe_escape_first_char(&mut fragment_text);
            }
            if block.list.is_none() && fmt.indent.unwrap_or(0) > 0 {
                // <li>first line<p>continuation</p></li>
                self.out.extend(Self::spaces(self.wrapped_line_indent));
            }
            starts_or_ends_with_backtick |=
                fragment_text.first() == Some(&tick) || fragment_text.last() == Some(&tick);
            let break_before = |w: &mut Writer, col: &mut i64, len: usize| {
                if wrap && *col + len as i64 > COLUMN_LIMIT {
                    w.out.push(nl);
                    w.out.extend(&wrap_indent_string);
                    *col = wrapped_line_indent;
                }
            };
            if let Some((name, alt, title)) = &cf.image {
                let desc = if alt.is_empty() { "image" } else { alt };
                let mut s = format!("![{desc}]({name}");
                if !title.is_empty() {
                    s += &format!(" \"{title}\"");
                }
                s.push(')');
                let s = utf16(&s);
                break_before(self, &mut col, s.len());
                col += s.len() as i64;
                self.out.extend(s);
            } else if let Some((href, title)) = &cf.link {
                let text = String::from_utf16_lossy(&fragment_text);
                let scheme = href.split_once(':').is_some_and(|(s, _)| {
                    s.starts_with(|c: char| c.is_ascii_alphabetic())
                        && s.chars()
                            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
                });
                let s = if title.is_empty() && *href == text && scheme {
                    format!("<{href}>")
                } else if title.is_empty() {
                    format!("[{text}]({href})")
                } else {
                    // createLinkTitle: its blank lines dropped, its quotes escaped.
                    let lines: Vec<&str> = title
                        .split_inclusive('\n')
                        .filter(|l| !l.trim().is_empty())
                        .collect();
                    format!(
                        "[{text}]({href} \"{}\")",
                        lines.concat().replace('"', "\\\"")
                    )
                };
                let s = utf16(&s);
                break_before(self, &mut col, s.len());
                col += s.len() as i64;
                self.out.extend(s);
            } else {
                let mono_frag = cf.mono;
                let mut markers: Vec<u16> = vec![];
                if !ignore_format {
                    if mono_frag != mono && !self.indented_code_block && !self.fenced_code_block {
                        if mono_frag {
                            backticks = vec![tick; adjacent_backticks_count(&fragment_text) + 1];
                        }
                        markers.extend(&backticks);
                        if starts_or_ends_with_backtick {
                            markers.push(b' ' as u16);
                        }
                        mono = mono_frag;
                        ending_markers |= !mono;
                    }
                    if fmt.heading == 0 && !mono {
                        for (on, state, marker) in [
                            (cf.bold || self.font_bold, &mut bold, "**"),
                            (cf.italic, &mut italic, "*"),
                            (cf.strike, &mut strike_out, "~~"),
                            (cf.underline, &mut underline, "_"),
                        ] {
                            if on != *state {
                                markers.extend(marker.encode_utf16());
                                *state = on;
                                ending_markers |= !on;
                            }
                        }
                    }
                }
                let frag_len = fragment_text.len() as i64;
                if wrap && col + markers.len() as i64 * 2 + frag_len > COLUMN_LIMIT {
                    let mut i: i64 = 0;
                    let mut breaking_line = false;
                    while i < frag_len {
                        if col >= COLUMN_LIMIT {
                            self.out.append(&mut markers);
                            self.out.push(nl);
                            self.out.extend(&wrap_indent_string);
                            col = wrapped_line_indent;
                            while i < frag_len && is_space(fragment_text[i as usize]) {
                                i += 1;
                            }
                        }
                        let mut j = i + COLUMN_LIMIT - col;
                        if j < frag_len {
                            let wi = nearest_word_wrap_index(&fragment_text, j);
                            if wi < 0 {
                                j = frag_len;
                                if ending_markers {
                                    self.out.append(&mut markers);
                                }
                                self.out.push(nl);
                                self.out.extend(&wrap_indent_string);
                                col = wrapped_line_indent;
                            } else if wi >= i {
                                j = wi;
                                breaking_line = true;
                            }
                        } else {
                            j = frag_len;
                            breaking_line = false;
                        }
                        let mut subfrag = fragment_text
                            [(i.min(frag_len)) as usize..j.clamp(i, frag_len) as usize]
                            .to_vec();
                        if i == 0 {
                            self.out.extend(&markers);
                            col += markers.len() as i64;
                        }
                        if col == wrapped_line_indent {
                            maybe_escape_first_char(&mut subfrag);
                        }
                        let sub_len = subfrag.len() as i64;
                        self.out.extend(subfrag);
                        if breaking_line {
                            self.out.push(nl);
                            self.out.extend(&wrap_indent_string);
                            col = wrapped_line_indent;
                        } else {
                            col += sub_len;
                        }
                        i = j + 1;
                    }
                } else {
                    if !self.line_prefix_written && col == wrap_indent_string.len() as i64 {
                        let prefix = self.line_prefix.clone();
                        col += prefix.len() as i64;
                        self.out.extend(prefix);
                    }
                    col += (markers.len() + fragment_text.len()) as i64;
                    self.out.extend(markers);
                    self.out.extend(fragment_text);
                }
            }
        }
        if mono {
            if starts_or_ends_with_backtick {
                self.put(" ");
                col += 1;
            }
            col += backticks.len() as i64;
            self.out.extend(backticks);
        }
        for (on, marker) in [
            (bold, "**"),
            (italic, "*"),
            (underline, "_"),
            (strike_out, "~~"),
        ] {
            if on {
                self.put(marker);
                col += marker.len() as i64;
            }
        }
        if missed_blank_code_block_line {
            self.put("\n");
        }
        self.line_prefix_written = false;
        col
    }
}
