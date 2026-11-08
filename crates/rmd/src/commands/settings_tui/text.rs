//! Rich-style text for the settings editor: styled lines built with
//! [`Text::append`], Rich's word wrapping and ellipsis truncation, and a
//! bordered [`Panel`] drawn with ratatui the way Rich draws a `Panel`.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Padding, Paragraph};
use unicode_width::UnicodeWidthChar;

/// A run of text with one style.
pub type Segment = (String, Style);

/// Styled text split into lines, like Rich `Text`: a trailing newline leaves
/// an empty last line.
#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    lines: Vec<Vec<Segment>>,
}

impl Default for Text {
    fn default() -> Self {
        Self { lines: vec![Vec::new()] }
    }
}

impl Text {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append text (which may contain newlines) in one style.
    pub fn append(&mut self, text: &str, style: Style) {
        for (index, part) in text.split('\n').enumerate() {
            if index > 0 {
                self.lines.push(Vec::new());
            }
            if !part.is_empty() {
                self.lines.last_mut().expect("text has a line").push((part.to_string(), style));
            }
        }
    }

    /// Append already-styled segments (without newlines).
    pub fn append_segments(&mut self, segments: Vec<Segment>) {
        self.lines.last_mut().expect("text has a line").extend(segments);
    }

    /// The text without styles (Rich `Text.plain`).
    pub fn plain(&self) -> String {
        self.lines
            .iter()
            .map(|line| line.iter().map(|(text, _)| text.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn lines(&self) -> &[Vec<Segment>] {
        &self.lines
    }
}

/// Cell widths of `chars`. An emoji variation selector (U+FE0F) after a
/// narrow non-ASCII symbol makes it two cells wide, as ratatui measures it.
fn char_widths(chars: &[char]) -> Vec<usize> {
    let mut widths: Vec<usize> = Vec::with_capacity(chars.len());
    for (index, c) in chars.iter().enumerate() {
        let widened = *c == '\u{FE0F}' && index > 0 && !chars[index - 1].is_ascii() && widths[index - 1] == 1;
        widths.push(if widened { 1 } else { c.width().unwrap_or(0) });
    }
    widths
}

fn flatten(segments: &[Segment]) -> Vec<(char, Style)> {
    segments.iter().flat_map(|(text, style)| text.chars().map(move |c| (c, *style))).collect()
}

fn group(chars: &[(char, Style)]) -> Vec<Segment> {
    let mut segments: Vec<Segment> = Vec::new();
    for (c, style) in chars {
        match segments.last_mut() {
            Some((text, last)) if last == style => text.push(*c),
            _ => segments.push((c.to_string(), *style)),
        }
    }
    segments
}

/// Rich `Text.truncate(max_width, overflow="ellipsis")` for one line.
pub fn truncate_ellipsis(segments: Vec<Segment>, max_width: usize) -> Vec<Segment> {
    let chars = flatten(&segments);
    let widths = char_widths(&chars.iter().map(|(c, _)| *c).collect::<Vec<_>>());
    if widths.iter().sum::<usize>() <= max_width {
        return segments;
    }
    // `set_cell_size(plain, max_width - 1) + "…"`: the ellipsis takes the style
    // of the text it replaces; a wide character that does not fit is padded.
    let target = max_width.saturating_sub(1);
    let mut used = 0;
    let fitting = widths
        .iter()
        .take_while(|width| {
            used += **width;
            used <= target
        })
        .count();
    let used: usize = widths[..fitting].iter().sum();
    let style = chars.get(fitting).or(chars.last()).map_or(Style::new(), |(_, style)| *style);
    let mut kept = chars[..fitting].to_vec();
    kept.extend(std::iter::repeat_n((' ', style), target - used));
    kept.push(('…', style));
    group(&kept)
}

/// Start indexes and lengths of Rich's words (`\s*\S+\s*`) in `chars`.
fn words(chars: &[char]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut position = 0;
    while position < chars.len() {
        let start = position;
        let mut end = start;
        while end < chars.len() && chars[end].is_whitespace() {
            end += 1;
        }
        if end == chars.len() {
            break;
        }
        while end < chars.len() && !chars[end].is_whitespace() {
            end += 1;
        }
        while end < chars.len() && chars[end].is_whitespace() {
            end += 1;
        }
        found.push((start, end - start));
        position = end;
    }
    found
}

/// Rich `chop_cells`: lengths of the pieces of at most `width` cells.
fn chop_cells(widths: &[usize], width: usize) -> Vec<usize> {
    let mut lengths = vec![0];
    let mut total = 0;
    for cell in widths {
        if total + cell > width {
            lengths.push(1);
            total = *cell;
        } else {
            *lengths.last_mut().expect("lengths has a piece") += 1;
            total += cell;
        }
    }
    lengths
}

/// Rich `divide_line(text, width, fold=True)`: offsets where wrapped lines start.
fn divide_line(chars: &[char], widths: &[usize], width: usize) -> Vec<usize> {
    let mut breaks = Vec::new();
    let mut cell_offset = 0;
    for (start, length) in words(chars) {
        let word = &chars[start..start + length];
        let word_widths = &widths[start..start + length];
        let trimmed = word.iter().rposition(|c| !c.is_whitespace()).map_or(0, |last| last + 1);
        let word_length: usize = word_widths[..trimmed].iter().sum();
        let word_cells: usize = word_widths.iter().sum();
        if cell_offset + word_length <= width {
            cell_offset += word_cells;
        } else if word_length > width {
            // Fold a word that fits on no line across several.
            let pieces = chop_cells(word_widths, width);
            let mut piece_start = start;
            for (index, piece) in pieces.iter().enumerate() {
                if piece_start > 0 {
                    breaks.push(piece_start);
                }
                if index + 1 == pieces.len() {
                    cell_offset = widths[piece_start..start + length].iter().sum();
                } else {
                    piece_start += piece;
                }
            }
        } else if cell_offset > 0 && start > 0 {
            breaks.push(start);
            cell_offset = word_cells;
        }
    }
    breaks
}

/// Rich `Text.wrap` for one line: word wrap to `width` cells, folding words
/// that are too long and dropping whitespace that overflows.
pub fn wrap_line(segments: &[Segment], width: usize) -> Vec<Vec<Segment>> {
    let chars = flatten(segments);
    let plain: Vec<char> = chars.iter().map(|(c, _)| *c).collect();
    let widths = char_widths(&plain);
    let mut offsets = divide_line(&plain, &widths, width);
    offsets.push(chars.len());
    let mut start = 0;
    let mut lines = Vec::new();
    for end in offsets {
        let end = end.max(start);
        let mut piece = chars[start..end].to_vec();
        let piece_widths = &widths[start..end];
        start = end;
        // `rstrip_end(width)`: trailing whitespace beyond the width goes.
        if piece.len() > width {
            let excess = piece.len() - width;
            let whitespace = piece.iter().rev().take_while(|(c, _)| c.is_whitespace()).count();
            piece.truncate(piece.len() - whitespace.min(excess));
        }
        // `truncate(width, overflow="fold")`: crop anything still too wide.
        let mut used = 0;
        let fitting = piece_widths[..piece.len()]
            .iter()
            .take_while(|cell| {
                used += **cell;
                used <= width
            })
            .count();
        piece.truncate(fitting);
        lines.push(group(&piece));
    }
    lines
}

/// A bordered box with a centred title, like Rich's `Panel`.
#[derive(Clone, Debug, PartialEq)]
pub struct Panel {
    pub title: String,
    pub text: Text,
    pub border: Color,
    /// Rich's `padding=(vertical, horizontal)`.
    pub padding: (u16, u16),
}

impl Panel {
    /// The content lines wrapped to a panel `width` columns wide.
    pub fn content_lines(&self, width: u16) -> Vec<Line<'static>> {
        let inner = usize::from(width.saturating_sub(2 + 2 * self.padding.1)).max(1);
        self.text
            .lines()
            .iter()
            .flat_map(|line| wrap_line(line, inner))
            .map(|line| Line::from(line.into_iter().map(|(text, style)| Span::styled(text, style)).collect::<Vec<_>>()))
            .collect()
    }

    /// Rows the panel occupies at `width` columns, borders and padding included.
    pub fn height(&self, width: u16) -> usize {
        self.content_lines(width).len() + 2 + 2 * usize::from(self.padding.0)
    }

    /// Draw the panel at the top of the frame, full width and as tall as its
    /// content (cropped to the screen).
    pub fn draw(&self, frame: &mut Frame) {
        let area = frame.area();
        let lines = self.content_lines(area.width);
        let height = u16::try_from(lines.len() + 2 + 2 * usize::from(self.padding.0)).unwrap_or(u16::MAX);
        let border = Style::new().fg(self.border);
        let (vertical, horizontal) = self.padding;
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(border)
            .title(Line::from(format!(" {} ", self.title)).centered())
            .title_style(border)
            .padding(Padding::new(horizontal, horizontal, vertical, vertical));
        frame.render_widget(Paragraph::new(lines).block(block), Rect { height: height.min(area.height), ..area });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Terminal cells taken by `text`.
    fn cell_len(text: &str) -> usize {
        char_widths(&text.chars().collect::<Vec<_>>()).iter().sum()
    }

    fn plain(lines: &[Vec<Segment>]) -> Vec<String> {
        lines.iter().map(|line| line.iter().map(|(text, _)| text.as_str()).collect()).collect()
    }

    #[test]
    fn append_splits_lines_like_rich() {
        let mut text = Text::new();
        text.append("  a\n\n", Style::new());
        text.append("b", Style::new());
        assert_eq!(text.plain(), "  a\n\nb");
        assert_eq!(text.lines().len(), 3);
    }

    #[test]
    fn wraps_at_words_and_folds_long_words() {
        let line = vec![("  [↑↓] Move  [Enter] Edit  [a] Add  [d] Delete".to_string(), Style::new())];
        assert_eq!(plain(&wrap_line(&line, 46)), vec!["  [↑↓] Move  [Enter] Edit  [a] Add  [d] Delete"]);
        assert_eq!(plain(&wrap_line(&line, 30)), vec!["  [↑↓] Move  [Enter] Edit  [a]", "Add  [d] Delete"]);
        let long = vec![("ab cdefghij".to_string(), Style::new())];
        assert_eq!(plain(&wrap_line(&long, 4)), vec!["ab ", "cdef", "ghij"]);
        assert_eq!(plain(&wrap_line(&[], 4)), vec![""]);
    }

    #[test]
    fn truncates_with_an_ellipsis_in_the_cut_style() {
        let bold = Style::new().fg(Color::Yellow);
        let line = vec![("  key ".to_string(), Style::new()), ("value".to_string(), bold)];
        assert_eq!(truncate_ellipsis(line.clone(), 20), line);
        assert_eq!(
            truncate_ellipsis(line.clone(), 8),
            vec![("  key ".to_string(), Style::new()), ("v…".to_string(), bold)]
        );
        assert_eq!(truncate_ellipsis(line, 4), vec![("  k…".to_string(), Style::new())]);
        let wide = vec![("ab漢字".to_string(), Style::new())];
        assert_eq!(truncate_ellipsis(wide, 4), vec![("ab …".to_string(), Style::new())]);
    }

    #[test]
    fn emoji_with_variation_selector_takes_two_cells() {
        assert_eq!(cell_len("⚙️  General Settings"), 20);
        assert_eq!(cell_len("📋  Task Scheduling"), 19);
        assert_eq!(cell_len("⚙  Settings"), 11);
    }
}
