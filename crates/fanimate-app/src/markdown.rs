//! A small Markdown reader and viewer, for the release notes (What's New and
//! the update dialog).
//!
//! It reads what those notes use: headings, paragraphs, bulleted and
//! numbered lists (nested by indentation), tables, fenced code, rules, and
//! within text **bold**, *italic*, `code` and [links](https://...). Anything
//! else is shown as plain text, never dropped. Parsing is separate from
//! drawing so it is tested without a window.

use eframe::egui::{self, Margin, RichText, Stroke, Ui};

use crate::theme::Palette;

/// A run of text with one style.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub link: Option<String>,
}

impl Span {
    fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Heading(u8, Vec<Span>),
    Paragraph(Vec<Span>),
    /// A list item: its nesting level, its number if the list is numbered.
    Item {
        depth: usize,
        number: Option<usize>,
        text: Vec<Span>,
    },
    Code(String),
    Table {
        header: Vec<Vec<Span>>,
        rows: Vec<Vec<Vec<Span>>>,
    },
    Rule,
}

/// Text with inline styles: `**bold**`, `*italic*` or `_italic_`, `` `code` ``
/// and `[text](url)`. A backslash escapes the next character.
pub fn inline(s: &str) -> Vec<Span> {
    let chars: Vec<char> = s.chars().collect();
    let mut spans: Vec<Span> = Vec::new();
    let mut plain = String::new();
    let flush = |plain: &mut String, spans: &mut Vec<Span>| {
        if !plain.is_empty() {
            spans.push(Span::plain(std::mem::take(plain)));
        }
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // the text up to the next `end` after position `from`, if any
        let find = |from: usize, end: &[char]| -> Option<usize> {
            (from..chars.len().saturating_sub(end.len() - 1)).find(|&j| chars[j..].starts_with(end))
        };
        match c {
            '\\' if i + 1 < chars.len() => {
                plain.push(chars[i + 1]);
                i += 2;
            }
            '`' => match find(i + 1, &['`']) {
                Some(end) => {
                    flush(&mut plain, &mut spans);
                    spans.push(Span {
                        text: chars[i + 1..end].iter().collect(),
                        code: true,
                        ..Span::default()
                    });
                    i = end + 1;
                }
                None => {
                    plain.push(c);
                    i += 1;
                }
            },
            '*' if chars.get(i + 1) == Some(&'*') => match find(i + 2, &['*', '*']) {
                Some(end) if end > i + 2 => {
                    flush(&mut plain, &mut spans);
                    // bold may hold links and code
                    let inner: String = chars[i + 2..end].iter().collect();
                    for mut span in inline(&inner) {
                        span.bold = true;
                        spans.push(span);
                    }
                    i = end + 2;
                }
                _ => {
                    plain.push(c);
                    i += 1;
                }
            },
            '*' | '_' => {
                // not inside a word for `_` (snake_case stays as it is)
                let word_inside = c == '_'
                    && i > 0
                    && chars[i - 1].is_alphanumeric()
                    && chars.get(i + 1).is_some_and(|n| n.is_alphanumeric());
                match find(i + 1, &[c]) {
                    Some(end) if end > i + 1 && !word_inside && !chars[i + 1].is_whitespace() => {
                        flush(&mut plain, &mut spans);
                        let inner: String = chars[i + 1..end].iter().collect();
                        for mut span in inline(&inner) {
                            span.italic = true;
                            spans.push(span);
                        }
                        i = end + 1;
                    }
                    _ => {
                        plain.push(c);
                        i += 1;
                    }
                }
            }
            '[' => {
                let close = find(i + 1, &[']']);
                let link = close
                    .filter(|&end| chars.get(end + 1) == Some(&'('))
                    .and_then(|end| {
                        let url_end = find(end + 2, &[')'])?;
                        Some((end, url_end))
                    });
                match link {
                    Some((end, url_end)) => {
                        flush(&mut plain, &mut spans);
                        let url: String = chars[end + 2..url_end].iter().collect();
                        let label: String = chars[i + 1..end].iter().collect();
                        for mut span in inline(&label) {
                            span.link = Some(url.clone());
                            spans.push(span);
                        }
                        i = url_end + 1;
                    }
                    None => {
                        plain.push(c);
                        i += 1;
                    }
                }
            }
            _ => {
                plain.push(c);
                i += 1;
            }
        }
    }
    flush(&mut plain, &mut spans);
    spans
}

/// The cells of a table row, without the outer bars.
fn cells(line: &str) -> Vec<String> {
    let line = line.trim();
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = line.strip_suffix('|').unwrap_or(line);
    // a backslash-escaped bar belongs to the cell
    let mut out = Vec::new();
    let mut cell = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'|') {
            cell.push('|');
            chars.next();
        } else if c == '|' {
            out.push(std::mem::take(&mut cell).trim().to_owned());
        } else {
            cell.push(c);
        }
    }
    out.push(cell.trim().to_owned());
    out
}

fn is_table_rule(line: &str) -> bool {
    let t = line.trim();
    t.contains('-') && t.contains('|') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

/// The list marker at the start of `line` (after its indentation):
/// the text after it, and the number for a numbered item.
fn list_marker(line: &str) -> Option<(Option<usize>, &str)> {
    let t = line.trim_start();
    if let Some(rest) = t
        .strip_prefix("- ")
        .or_else(|| t.strip_prefix("* "))
        .or_else(|| t.strip_prefix("+ "))
    {
        return Some((None, rest));
    }
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && digits <= 3 {
        let rest = t[digits..].strip_prefix(". ")?;
        return Some((Some(t[..digits].parse().ok()?), rest));
    }
    None
}

/// Read Markdown into blocks.
pub fn parse(md: &str) -> Vec<Block> {
    let lines: Vec<&str> = md.lines().collect();
    let mut blocks = Vec::new();
    let mut paragraph = String::new();
    let flush = |paragraph: &mut String, blocks: &mut Vec<Block>| {
        if !paragraph.trim().is_empty() {
            blocks.push(Block::Paragraph(inline(paragraph.trim())));
        }
        paragraph.clear();
    };
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim();
        if t.starts_with("```") || t.starts_with("~~~") {
            flush(&mut paragraph, &mut blocks);
            let mut code = Vec::new();
            i += 1;
            while i < lines.len()
                && !lines[i].trim().starts_with("```")
                && !lines[i].trim().starts_with("~~~")
            {
                code.push(lines[i]);
                i += 1;
            }
            blocks.push(Block::Code(code.join("\n")));
            i += 1;
            continue;
        }
        if t.is_empty() {
            flush(&mut paragraph, &mut blocks);
            i += 1;
            continue;
        }
        if t.starts_with('#') {
            let level = t.chars().take_while(|c| *c == '#').count();
            if level <= 6 && t[level..].starts_with(' ') {
                flush(&mut paragraph, &mut blocks);
                blocks.push(Block::Heading(level as u8, inline(t[level..].trim())));
                i += 1;
                continue;
            }
        }
        if t.len() >= 3 && t.chars().all(|c| c == '-' || c == '*' || c == '_') && !t.contains(' ') {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Rule);
            i += 1;
            continue;
        }
        if t.starts_with('|') && lines.get(i + 1).is_some_and(|l| is_table_rule(l)) {
            flush(&mut paragraph, &mut blocks);
            let header: Vec<_> = cells(t).iter().map(|c| inline(c)).collect();
            i += 2;
            let mut rows = Vec::new();
            while i < lines.len() && lines[i].trim().starts_with('|') {
                rows.push(cells(lines[i]).iter().map(|c| inline(c)).collect());
                i += 1;
            }
            blocks.push(Block::Table { header, rows });
            continue;
        }
        if let Some((number, rest)) = list_marker(line) {
            flush(&mut paragraph, &mut blocks);
            let indent = line.len() - line.trim_start().len();
            let mut text = rest.trim().to_owned();
            i += 1;
            // lines indented under the item continue it
            while i < lines.len() {
                let next = lines[i];
                let next_indent = next.len() - next.trim_start().len();
                if next.trim().is_empty() || list_marker(next).is_some() || next_indent <= indent {
                    break;
                }
                text.push(' ');
                text.push_str(next.trim());
                i += 1;
            }
            blocks.push(Block::Item {
                depth: indent / 2,
                number,
                text: inline(&text),
            });
            continue;
        }
        if !paragraph.is_empty() {
            paragraph.push(' ');
        }
        paragraph.push_str(t);
        i += 1;
    }
    flush(&mut paragraph, &mut blocks);
    blocks
}

/// Draw `spans` as flowing text.
fn show_spans(ui: &mut Ui, p: &Palette, spans: &[Span], size: f32, strong: bool) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for span in spans {
            let mut text = RichText::new(&span.text).size(size).color(p.text);
            if span.bold || strong {
                text = text.strong();
            }
            if span.italic {
                text = text.italics();
            }
            if span.code {
                text = text.monospace().background_color(p.bg).size(size - 0.5);
            }
            match &span.link {
                Some(url) => {
                    let r = ui.link(text).on_hover_text(url);
                    if r.clicked() {
                        crate::links::open(url);
                    }
                }
                None => {
                    ui.label(text);
                }
            }
        }
    });
}

/// The width of each column of a table in `available` points with `gap`
/// between columns: shared out by the longest text in each, none narrower
/// than a word or two.
fn column_widths(header: &[Vec<Span>], rows: &[Vec<Vec<Span>>], available: f32, gap: f32) -> Vec<f32> {
    let columns = header
        .len()
        .max(rows.iter().map(Vec::len).max().unwrap_or(0))
        .max(1);
    let length =
        |cell: Option<&Vec<Span>>| cell.map_or(0, |c| c.iter().map(|s| s.text.chars().count()).sum());
    let longest: Vec<usize> = (0..columns)
        .map(|i| {
            rows.iter()
                .map(|r| length(r.get(i)))
                .chain(std::iter::once(length(header.get(i))))
                .max()
                .unwrap_or(0)
                .max(4)
        })
        .collect();
    let room = (available - gap * (columns - 1) as f32).max(60.0 * columns as f32);
    let total: usize = longest.iter().sum();
    // a column never needs more than its longest text takes
    longest
        .iter()
        .map(|n| {
            (room * *n as f32 / total as f32)
                .max(60.0)
                .min(*n as f32 * 8.0 + 8.0)
        })
        .collect()
}

/// Draw the blocks, filling the width.
pub fn show(ui: &mut Ui, p: &Palette, blocks: &[Block]) {
    for (n, block) in blocks.iter().enumerate() {
        match block {
            Block::Heading(level, spans) => {
                let size = match level {
                    1 => 22.0,
                    2 => 17.5,
                    3 => 14.5,
                    _ => 13.0,
                };
                if n > 0 {
                    ui.add_space(if *level <= 2 { 12.0 } else { 8.0 });
                }
                show_spans(ui, p, spans, size, true);
                if *level == 2 {
                    ui.separator();
                }
                ui.add_space(2.0);
            }
            Block::Paragraph(spans) => {
                show_spans(ui, p, spans, 13.0, false);
                ui.add_space(6.0);
            }
            Block::Item { depth, number, text } => {
                ui.horizontal_top(|ui| {
                    ui.add_space(8.0 + *depth as f32 * 16.0);
                    let marker = number.map_or_else(|| "•".to_owned(), |n| format!("{n}."));
                    ui.label(RichText::new(marker).color(p.text_muted));
                    ui.vertical(|ui| show_spans(ui, p, text, 13.0, false));
                });
                ui.add_space(2.0);
            }
            Block::Code(code) => {
                egui::Frame::NONE
                    .fill(p.bg)
                    .stroke(Stroke::new(1.0, p.border))
                    .corner_radius(4)
                    .inner_margin(Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(code).monospace().size(12.0).color(p.text));
                    });
                ui.add_space(6.0);
            }
            Block::Table { header, rows } => {
                const GAP: f32 = 16.0;
                // each column gets a share of the width by how much text it
                // holds, so a long cell wraps at its own column and the table
                // fills the dialog (a wrapped cell in a grid otherwise sizes
                // itself to a sliver)
                let widths = column_widths(header, rows, ui.available_width(), GAP);
                egui::Grid::new(("markdown-table", n))
                    .striped(true)
                    .spacing([GAP, 5.0])
                    .show(ui, |ui| {
                        for (cell, width) in header.iter().zip(&widths) {
                            ui.scope(|ui| {
                                ui.set_width(*width);
                                show_spans(ui, p, cell, 12.5, true);
                            });
                        }
                        ui.end_row();
                        for row in rows {
                            for (cell, width) in row.iter().zip(&widths) {
                                ui.scope(|ui| {
                                    ui.set_width(*width);
                                    show_spans(ui, p, cell, 12.5, false);
                                });
                            }
                            ui.end_row();
                        }
                    });
                ui.add_space(6.0);
            }
            Block::Rule => {
                ui.separator();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(spans: &[Span]) -> Vec<&str> {
        spans.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn table_columns_share_the_width_by_their_text() {
        let cell = |t: &str| vec![Span::plain(t)];
        let header = vec![cell("Platform"), cell("File")];
        let rows = vec![vec![
            cell("Windows 10 and 11"),
            cell(
                "Fanimate-0.2.3-windows-x64-setup.exe (installs for your account, no administrator rights needed)",
            ),
        ]];
        let widths = column_widths(&header, &rows, 680.0, 16.0);
        assert_eq!(widths.len(), 2);
        // the long column is wider, and nothing is a sliver
        assert!(
            widths[1] > widths[0] && widths.iter().all(|w| *w >= 60.0),
            "{widths:?}"
        );
        // together they fit
        assert!(widths.iter().sum::<f32>() + 16.0 <= 680.0 + 0.5, "{widths:?}");
        // a narrow window still leaves every column readable
        assert!(
            column_widths(&header, &rows, 100.0, 16.0)
                .iter()
                .all(|w| *w >= 60.0)
        );
        // no table, no columns to size: one default column rather than a crash
        assert_eq!(column_widths(&[], &[], 300.0, 16.0).len(), 1);
    }

    #[test]
    fn inline_styles_are_read() {
        let spans = inline("a **bold** and *slanted* and `code` here");
        assert_eq!(
            texts(&spans),
            ["a ", "bold", " and ", "slanted", " and ", "code", " here"]
        );
        assert!(spans[1].bold && spans[3].italic && spans[5].code);
        assert!(!spans[0].bold && !spans[2].italic);
    }

    #[test]
    fn links_keep_their_address_and_may_be_bold() {
        let spans = inline("see [the docs](https://example.org/a_b) now");
        assert_eq!(texts(&spans), ["see ", "the docs", " now"]);
        assert_eq!(spans[1].link.as_deref(), Some("https://example.org/a_b"));
        let bold = inline("**[x](https://y.z)**");
        assert!(bold[0].bold && bold[0].link.is_some());
    }

    #[test]
    fn stray_marks_stay_as_text() {
        assert_eq!(texts(&inline("2 * 3 = 6")), ["2 * 3 = 6"]);
        assert_eq!(texts(&inline("snake_case_name")), ["snake_case_name"]);
        assert_eq!(texts(&inline("an unclosed `tick")), ["an unclosed `tick"]);
        assert_eq!(texts(&inline("[not a link] (x)")), ["[not a link] (x)"]);
        assert_eq!(texts(&inline(r"a \*star\*")), ["a *star*"]);
        assert!(inline("").is_empty());
    }

    #[test]
    fn blocks_are_found() {
        let md = "# Title\n\nSome text\nover two lines.\n\n## Part\n\n- one\n- two\n  continued\n    - nested\n\n1. first\n2. second\n\n---\n\n```\ncode here\n```\n";
        let blocks = parse(md);
        assert!(matches!(&blocks[0], Block::Heading(1, s) if s[0].text == "Title"));
        assert!(matches!(&blocks[1], Block::Paragraph(s) if s[0].text == "Some text over two lines."));
        assert!(matches!(&blocks[2], Block::Heading(2, _)));
        assert!(matches!(
            &blocks[3],
            Block::Item {
                depth: 0,
                number: None,
                ..
            }
        ));
        match &blocks[4] {
            Block::Item { text, .. } => assert_eq!(text[0].text, "two continued"),
            other => panic!("{other:?}"),
        }
        assert!(matches!(&blocks[5], Block::Item { depth: 2, .. }));
        assert!(matches!(&blocks[6], Block::Item { number: Some(1), .. }));
        assert!(matches!(&blocks[7], Block::Item { number: Some(2), .. }));
        assert_eq!(blocks[8], Block::Rule);
        assert_eq!(blocks[9], Block::Code("code here".into()));
    }

    #[test]
    fn tables_have_a_header_and_rows() {
        let md = "| Platform | File |\n|---|---|\n| Windows | `a.exe` |\n| macOS | `b.dmg` \\| x |\n\nafter";
        let blocks = parse(md);
        let Block::Table { header, rows } = &blocks[0] else {
            panic!("{blocks:?}");
        };
        assert_eq!(header.len(), 2);
        assert_eq!(rows.len(), 2);
        assert!(rows[0][1][0].code);
        assert_eq!(rows[1][1][1].text, " | x");
        assert!(matches!(&blocks[1], Block::Paragraph(_)));
    }

    #[test]
    fn what_is_not_understood_is_still_shown() {
        let blocks = parse("<div>raw html</div>\n\n> a quote");
        assert_eq!(blocks.len(), 2);
        assert!(matches!(&blocks[0], Block::Paragraph(s) if s[0].text == "<div>raw html</div>"));
        assert!(matches!(&blocks[1], Block::Paragraph(s) if s[0].text == "> a quote"));
    }

    #[test]
    fn the_shipped_release_notes_all_read() {
        for (version, notes) in crate::whatsnew::NOTES {
            let blocks = parse(notes);
            assert!(!blocks.is_empty(), "{version}");
            assert!(
                blocks.iter().any(|b| matches!(b, Block::Heading(..))),
                "{version} has no headings"
            );
        }
    }
}
