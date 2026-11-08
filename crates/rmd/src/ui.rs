//! Terminal rendering helpers standing in for `rich`: console markup, word
//! wrapping, tables, panels, centred text, rules, and line prompts.
//! OWNER: CLI agent.
//!
//! - Markup such as `[bold red]text[/bold red]` (or `[/]` to close the last
//!   tag) is parsed the way rich parses it into a [`Text`] of styled characters.
//! - Output is styled with ANSI escape codes only when stdout is a terminal,
//!   `NO_COLOR` is unset and `TERM` is not `dumb` (`FORCE_COLOR` forces colour);
//!   otherwise it is plain text, exactly as rich renders when piped.
//! - Layout follows rich's algorithms (word wrapping, justification, table
//!   column widths, panels, alignment), so plain output matches the Python
//!   implementation's.
//!
//! ```no_run
//! use rmd::ui::{BoxStyle, Column, Console, Justify, Table, Text};
//!
//! let mut console = Console::stdout();
//! console.print("[bold green]✅ Saved[/bold green]");
//! let mut table = Table::new().title("[bold]Tasks[/bold]").box_style(BoxStyle::Rounded).expand(true);
//! table.add_column(Column::new("ID").justify(Justify::Right).style("dim"));
//! table.add_column(Column::new("Description"));
//! table.add_row([Text::from("1"), Text::raw("Write the report")]);
//! console.render(&table);
//! console.print_justified("[dim]Total tasks: 1[/dim]", Justify::Right);
//! ```

use std::collections::VecDeque;
use std::io::{IsTerminal, Write};
use std::sync::OnceLock;

use regex::Regex;
use unicode_width::UnicodeWidthChar;

mod colors;

// =============================================================================
// Styles and colours
// =============================================================================

const BOLD: u16 = 1;
const DIM: u16 = 1 << 1;
const ITALIC: u16 = 1 << 2;
const UNDERLINE: u16 = 1 << 3;
const BLINK: u16 = 1 << 4;
const BLINK2: u16 = 1 << 5;
const REVERSE: u16 = 1 << 6;
const CONCEAL: u16 = 1 << 7;
const STRIKE: u16 = 1 << 8;
const UNDERLINE2: u16 = 1 << 9;
const FRAME: u16 = 1 << 10;
const ENCIRCLE: u16 = 1 << 11;
const OVERLINE: u16 = 1 << 12;

/// SGR parameter for each attribute bit, in bit order.
const SGR_CODES: [&str; 13] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "21", "51", "52", "53"];

/// Attribute words accepted in style definitions (rich's `STYLE_ATTRIBUTES`).
const ATTRIBUTE_WORDS: [(&str, u16); 22] = [
    ("dim", DIM),
    ("d", DIM),
    ("bold", BOLD),
    ("b", BOLD),
    ("italic", ITALIC),
    ("i", ITALIC),
    ("underline", UNDERLINE),
    ("u", UNDERLINE),
    ("blink", BLINK),
    ("blink2", BLINK2),
    ("reverse", REVERSE),
    ("r", REVERSE),
    ("conceal", CONCEAL),
    ("c", CONCEAL),
    ("strike", STRIKE),
    ("s", STRIKE),
    ("underline2", UNDERLINE2),
    ("uu", UNDERLINE2),
    ("frame", FRAME),
    ("encircle", ENCIRCLE),
    ("overline", OVERLINE),
    ("o", OVERLINE),
];

fn attribute_bit(word: &str) -> Option<u16> {
    ATTRIBUTE_WORDS.iter().find(|(name, _)| *name == word).map(|(_, bit)| *bit)
}

/// How many colours the terminal supports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ColorSystem {
    /// The 16 standard ANSI colours.
    Standard,
    /// The 256-colour xterm palette.
    EightBit,
    /// 24-bit RGB.
    TrueColor,
}

/// A foreground or background colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Color {
    /// The terminal's default colour.
    Default,
    /// One of the 16 standard colours (0-15).
    Standard(u8),
    /// A colour from the 256-colour palette (16-255).
    EightBit(u8),
    /// A 24-bit colour.
    TrueColor(u8, u8, u8),
}

impl Color {
    /// Parse a colour the way rich does: a name (`red`, `bright_cyan`,
    /// `orange3`, ...), `#rrggbb`, `color(N)`, `rgb(r,g,b)`, or `default`.
    pub fn parse(definition: &str) -> Option<Color> {
        let name = definition.trim().to_lowercase();
        if name == "default" {
            return Some(Color::Default);
        }
        if let Ok(index) = colors::COLOR_NAMES.binary_search_by(|(n, _)| n.as_bytes().cmp(name.as_bytes())) {
            return Some(Color::from_number(colors::COLOR_NAMES[index].1));
        }
        if let Some(hex) = name.strip_prefix('#') {
            if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
                return Some(Color::TrueColor(channel(0)?, channel(2)?, channel(4)?));
            }
            return None;
        }
        if let Some(inner) = name.strip_prefix("color(").and_then(|s| s.strip_suffix(')')) {
            if inner.is_empty() || inner.len() > 3 || !inner.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            return inner.parse::<u8>().ok().map(Color::from_number);
        }
        if let Some(inner) = name.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
            let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
            if parts.len() != 3 {
                return None;
            }
            let channel = |s: &str| s.parse::<u8>().ok();
            return Some(Color::TrueColor(channel(parts[0])?, channel(parts[1])?, channel(parts[2])?));
        }
        None
    }

    fn from_number(number: u8) -> Color {
        if number < 16 { Color::Standard(number) } else { Color::EightBit(number) }
    }

    fn triplet(self) -> Option<(u8, u8, u8)> {
        match self {
            Color::Default => None,
            Color::Standard(n) => Some(colors::STANDARD_PALETTE[usize::from(n)]),
            Color::EightBit(n) => Some(eight_bit_triplet(n)),
            Color::TrueColor(r, g, b) => Some((r, g, b)),
        }
    }

    /// The closest colour the colour system can show.
    fn downgrade(self, system: ColorSystem) -> Color {
        match (self, system) {
            (Color::Default, _) | (Color::Standard(_), _) | (_, ColorSystem::TrueColor) => self,
            (Color::EightBit(_), ColorSystem::EightBit) => self,
            (Color::TrueColor(r, g, b), ColorSystem::EightBit) => {
                let (_, lightness, saturation) = rgb_to_hls(r, g, b);
                if saturation < 0.15 {
                    let gray = round_half_even(lightness * 25.0) as i64;
                    let number = match gray {
                        0 => 16,
                        25 => 231,
                        g => 231 + g,
                    };
                    return Color::EightBit(number.clamp(16, 255) as u8);
                }
                let six = |c: u8| {
                    let c = f64::from(c);
                    if c < 95.0 { c / 95.0 } else { 1.0 + (c - 95.0) / 40.0 }
                };
                let number =
                    16.0 + 36.0 * round_half_even(six(r)) + 6.0 * round_half_even(six(g)) + round_half_even(six(b));
                Color::EightBit(number.clamp(16.0, 255.0) as u8)
            }
            (color, ColorSystem::Standard) => {
                let (r1, g1, b1) = color.triplet().unwrap_or((0, 0, 0));
                let distance = |&(r2, g2, b2): &(u8, u8, u8)| {
                    let (r1, g1, b1, r2, g2, b2) =
                        (i64::from(r1), i64::from(g1), i64::from(b1), i64::from(r2), i64::from(g2), i64::from(b2));
                    let red_mean = (r1 + r2) / 2;
                    let (red, green, blue) = (r1 - r2, g1 - g2, b1 - b2);
                    (((512 + red_mean) * red * red) >> 8) + 4 * green * green + (((767 - red_mean) * blue * blue) >> 8)
                };
                let index = (0..16).min_by_key(|i| distance(&colors::STANDARD_PALETTE[*i])).unwrap_or(0);
                Color::Standard(index as u8)
            }
        }
    }

    fn ansi_codes(self, foreground: bool) -> String {
        match self {
            Color::Default => (if foreground { "39" } else { "49" }).to_string(),
            Color::Standard(n) => {
                let base = if foreground { 30 } else { 40 };
                let code = if n < 8 { base + u32::from(n) } else { base + 60 + u32::from(n - 8) };
                code.to_string()
            }
            Color::EightBit(n) => format!("{};5;{n}", if foreground { 38 } else { 48 }),
            Color::TrueColor(r, g, b) => format!("{};2;{r};{g};{b}", if foreground { 38 } else { 48 }),
        }
    }
}

fn eight_bit_triplet(number: u8) -> (u8, u8, u8) {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    match number {
        0..=15 => colors::EIGHT_BIT_LOW[usize::from(number)],
        16..=231 => {
            let index = usize::from(number - 16);
            (LEVELS[index / 36], LEVELS[(index / 6) % 6], LEVELS[index % 6])
        }
        _ => {
            let value = 8 + 10 * (number - 232);
            (value, value, value)
        }
    }
}

/// Python's `colorsys.rgb_to_hls` for 8-bit channels.
fn rgb_to_hls(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let (r, g, b) = (f64::from(r) / 255.0, f64::from(g) / 255.0, f64::from(b) / 255.0);
    let maxc = r.max(g).max(b);
    let minc = r.min(g).min(b);
    let lightness = (maxc + minc) / 2.0;
    if maxc == minc {
        return (0.0, lightness, 0.0);
    }
    let range = maxc - minc;
    let saturation = if lightness <= 0.5 { range / (maxc + minc) } else { range / (2.0 - maxc - minc) };
    (0.0, lightness, saturation)
}

/// Python's `round()` (ties to even).
fn round_half_even(value: f64) -> f64 {
    let rounded = value.round();
    if (value - value.trunc()).abs() == 0.5 { 2.0 * (value / 2.0).round() } else { rounded }
}

/// A text style: colours plus attributes (bold, dim, italic, ...).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Style {
    color: Option<Color>,
    bgcolor: Option<Color>,
    /// Attribute values, one bit per attribute.
    attributes: u16,
    /// Attributes this style sets (to true or false).
    set_attributes: u16,
}

impl Style {
    /// The empty style.
    pub const fn null() -> Style {
        Style { color: None, bgcolor: None, attributes: 0, set_attributes: 0 }
    }

    /// Parse a style definition such as `"bold red"`, `"italic dim"`,
    /// `"not bold"` or `"white on blue"`; `None` on a syntax error.
    pub fn parse(definition: &str) -> Option<Style> {
        let trimmed = definition.trim();
        let mut style = Style::null();
        if trimmed.is_empty() || trimmed == "none" {
            return Some(style);
        }
        let mut words = trimmed.split_whitespace();
        while let Some(original) = words.next() {
            let word = original.to_lowercase();
            match word.as_str() {
                "on" => style.bgcolor = Some(Color::parse(words.next()?)?),
                "not" => {
                    let bit = attribute_bit(words.next()?)?;
                    style.attributes &= !bit;
                    style.set_attributes |= bit;
                }
                "link" => {
                    words.next()?;
                }
                other => match attribute_bit(other) {
                    Some(bit) => {
                        style.attributes |= bit;
                        style.set_attributes |= bit;
                    }
                    None => style.color = Some(Color::parse(other)?),
                },
            }
        }
        Some(style)
    }

    /// Parse a definition, falling back to the null style (as rich does when
    /// rendering markup with an unknown style).
    pub fn get(definition: &str) -> Style {
        Style::parse(definition).unwrap_or_default()
    }

    /// `self + other`: `other` wins where both set something.
    pub fn combine(self, other: Style) -> Style {
        Style {
            color: other.color.or(self.color),
            bgcolor: other.bgcolor.or(self.bgcolor),
            attributes: (self.attributes & !other.set_attributes) | (other.attributes & other.set_attributes),
            set_attributes: self.set_attributes | other.set_attributes,
        }
    }

    /// Whether the style changes nothing.
    pub fn is_null(&self) -> bool {
        *self == Style::null()
    }

    fn background_only(self) -> Style {
        Style { bgcolor: self.bgcolor, ..Style::null() }
    }

    /// SGR parameters for the style (empty when it has no visible effect).
    fn sgr(&self, system: ColorSystem) -> String {
        let mut codes: Vec<String> = Vec::new();
        let attributes = self.attributes & self.set_attributes;
        for (bit, code) in SGR_CODES.iter().enumerate() {
            if attributes & (1 << bit) != 0 {
                codes.push((*code).to_string());
            }
        }
        if let Some(color) = self.color {
            codes.push(color.downgrade(system).ansi_codes(true));
        }
        if let Some(color) = self.bgcolor {
            codes.push(color.downgrade(system).ansi_codes(false));
        }
        codes.join(";")
    }

    /// `text` wrapped in this style's escape codes.
    pub fn paint(&self, text: &str, system: ColorSystem) -> String {
        let sgr = self.sgr(system);
        if sgr.is_empty() || text.is_empty() { text.to_string() } else { format!("\x1b[{sgr}m{text}\x1b[0m") }
    }
}

// =============================================================================
// Cell widths
// =============================================================================

/// Terminal cells taken by a character (rich's `get_character_cell_size`).
pub fn char_width(ch: char) -> usize {
    ch.width().unwrap_or(0)
}

/// Terminal cells taken by a string (rich's `cell_len`).
pub fn cell_len(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

// =============================================================================
// Text
// =============================================================================

/// How text lines are aligned within their width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Justify {
    /// Leave lines as they are (rich's "default").
    Default,
    /// Pad lines on the right to the full width.
    Left,
    /// Centre lines, padding both sides.
    Center,
    /// Pad lines on the left.
    Right,
}

/// What happens to text wider than the available width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overflow {
    /// Wrap long words onto the next line.
    Fold,
    /// Cut the text at the edge.
    Crop,
    /// Cut the text and end it with `…`.
    Ellipsis,
    /// Leave the text as it is.
    Ignore,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StyledChar {
    ch: char,
    style: Style,
}

/// One run of text in a single style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub style: Style,
}

/// A rendered line of segments.
pub type Line = Vec<Segment>;

/// Styled text (rich's `Text`): characters with their own styles plus a base
/// style that also covers padding added during layout.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Text {
    chars: Vec<StyledChar>,
    style: Style,
    justify: Option<Justify>,
    overflow: Option<Overflow>,
    no_wrap: Option<bool>,
}

fn tag_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(\\*)\[([a-z#/@][^\[]*?)\]").expect("valid markup regex"))
}

/// A markup tag's identity for matching `[/name]` against `[name]`.
#[derive(Clone, Debug, PartialEq, Eq)]
enum TagKey {
    Style(Style),
    Name(String),
}

fn tag_key(name: &str) -> TagKey {
    match Style::parse(name) {
        Some(style) => TagKey::Style(style),
        None => TagKey::Name(name.trim().to_lowercase()),
    }
}

impl Text {
    /// Empty text.
    pub fn new() -> Text {
        Text::default()
    }

    /// Unstyled text, taken literally (no markup).
    pub fn raw(text: &str) -> Text {
        let mut out = Text::new();
        out.push_str(text, Style::null());
        out
    }

    /// Text with a base style (rich's `Text(text, style=...)`), taken literally.
    pub fn styled(text: &str, style: &str) -> Text {
        let mut out = Text::raw(text);
        out.style = Style::get(style);
        out
    }

    /// Parse console markup (`[bold]...[/bold]`, `[/]`, `\[` escapes).
    pub fn from_markup(markup: &str) -> Text {
        let mut text = Text::new();
        let mut stack: Vec<(TagKey, Style)> = Vec::new();
        let current = |stack: &[(TagKey, Style)]| stack.iter().fold(Style::null(), |acc, (_, s)| acc.combine(*s));
        let push_plain = |text: &mut Text, plain: &str, style: Style| text.push_str(&plain.replace("\\[", "["), style);
        let mut position = 0;
        for caps in tag_regex().captures_iter(markup) {
            let whole = caps.get(0).expect("match");
            let escapes = caps.get(1).map_or("", |m| m.as_str());
            let tag_text = caps.get(2).map_or("", |m| m.as_str());
            if whole.start() > position {
                push_plain(&mut text, &markup[position..whole.start()], current(&stack));
            }
            position = whole.end();
            if !escapes.is_empty() {
                let (backslashes, escaped) = (escapes.len() / 2, escapes.len() % 2 == 1);
                if backslashes > 0 {
                    text.push_str(&"\\".repeat(backslashes), current(&stack));
                }
                if escaped {
                    push_plain(&mut text, &whole.as_str()[escapes.len()..], current(&stack));
                    continue;
                }
            }
            let (name, parameters) = match tag_text.split_once('=') {
                Some((name, parameters)) => (name, Some(parameters)),
                None => (tag_text, None),
            };
            if let Some(closing) = name.strip_prefix('/') {
                let closing = closing.trim();
                if closing.is_empty() {
                    stack.pop();
                } else {
                    let key = tag_key(closing);
                    if let Some(index) = stack.iter().rposition(|(k, _)| *k == key) {
                        stack.remove(index);
                    }
                }
            } else {
                let style = if name.starts_with('@') {
                    Style::null()
                } else {
                    match parameters {
                        Some(parameters) => Style::get(&format!("{name} {parameters}")),
                        None => Style::get(name),
                    }
                };
                stack.push((tag_key(name), style));
            }
        }
        if position < markup.len() {
            push_plain(&mut text, &markup[position..], current(&stack));
        }
        text
    }

    fn push_str(&mut self, text: &str, style: Style) {
        self.chars.extend(text.chars().map(|ch| StyledChar { ch, style }));
    }

    /// Append literal text in a style (rich's `Text.append(text, style)`).
    pub fn append(&mut self, text: &str, style: &str) -> &mut Text {
        self.push_str(text, Style::get(style));
        self
    }

    /// Append another text, keeping its styles (its base style included).
    pub fn append_text(&mut self, other: &Text) -> &mut Text {
        self.chars.extend(other.chars.iter().map(|c| StyledChar { ch: c.ch, style: other.style.combine(c.style) }));
        self
    }

    /// Set the base style.
    pub fn with_style(mut self, style: &str) -> Text {
        self.style = Style::get(style);
        self
    }

    /// Set how the text is justified when rendered.
    pub fn with_justify(mut self, justify: Justify) -> Text {
        self.justify = Some(justify);
        self
    }

    /// Set the overflow method.
    pub fn with_overflow(mut self, overflow: Overflow) -> Text {
        self.overflow = Some(overflow);
        self
    }

    /// Disable (or enable) word wrapping.
    pub fn with_no_wrap(mut self, no_wrap: bool) -> Text {
        self.no_wrap = Some(no_wrap);
        self
    }

    /// The text without styles.
    pub fn plain(&self) -> String {
        self.chars.iter().map(|c| c.ch).collect()
    }

    /// Number of characters.
    pub fn len(&self) -> usize {
        self.chars.len()
    }

    /// Whether the text has no characters.
    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    /// Terminal cells the text takes on one line.
    pub fn cell_len(&self) -> usize {
        self.chars.iter().map(|c| char_width(c.ch)).sum()
    }

    /// (longest word, longest line) in cells (rich's `Text.__rich_measure__`).
    fn measurement(&self) -> (usize, usize) {
        let plain = self.plain();
        let max = plain.split('\n').map(cell_len).max().unwrap_or(0);
        let min = plain.split_whitespace().map(cell_len).max().unwrap_or(max);
        (min, max)
    }

    fn slice(&self, start: usize, end: usize) -> Text {
        Text {
            chars: self.chars[start.min(self.chars.len())..end.min(self.chars.len())].to_vec(),
            style: self.style,
            justify: self.justify,
            overflow: self.overflow,
            no_wrap: self.no_wrap,
        }
    }

    /// Split at character offsets (rich's `Text.divide`).
    fn divide(&self, offsets: &[usize]) -> Vec<Text> {
        if offsets.is_empty() {
            return vec![self.clone()];
        }
        let mut lines = Vec::with_capacity(offsets.len() + 1);
        let mut start = 0;
        for &offset in offsets {
            lines.push(self.slice(start, offset));
            start = offset;
        }
        lines.push(self.slice(start, self.chars.len()));
        lines
    }

    /// Split on newlines, keeping blank lines (rich's `split(allow_blank=True)`).
    fn split_lines(&self) -> Vec<Text> {
        let mut lines = Vec::new();
        let mut start = 0;
        for (index, c) in self.chars.iter().enumerate() {
            if c.ch == '\n' {
                lines.push(self.slice(start, index));
                start = index + 1;
            }
        }
        lines.push(self.slice(start, self.chars.len()));
        lines
    }

    fn pad_left(&mut self, count: usize) {
        let pad = std::iter::repeat_n(StyledChar { ch: ' ', style: Style::null() }, count);
        self.chars.splice(0..0, pad);
    }

    fn pad_right(&mut self, count: usize) {
        self.chars.extend(std::iter::repeat_n(StyledChar { ch: ' ', style: Style::null() }, count));
    }

    fn rstrip(&mut self) {
        while self.chars.last().is_some_and(|c| c.ch.is_whitespace()) {
            self.chars.pop();
        }
    }

    /// Remove trailing whitespace beyond `size` characters.
    fn rstrip_end(&mut self, size: usize) {
        let length = self.chars.len();
        if length > size {
            let excess = length - size;
            let whitespace = self.chars.iter().rev().take_while(|c| c.ch.is_whitespace()).count();
            let crop = whitespace.min(excess);
            self.chars.truncate(length - crop);
        }
    }

    /// Crop or pad to exactly `total` cells (rich's `set_cell_size`).
    fn set_cell_size(&mut self, total: usize) {
        let mut width = 0;
        let mut kept = 0;
        for c in &self.chars {
            let w = char_width(c.ch);
            if width + w > total {
                break;
            }
            width += w;
            kept += 1;
        }
        let replaced_style = self.chars.get(kept).map(|c| c.style);
        self.chars.truncate(kept);
        if width < total {
            let style = replaced_style.unwrap_or_default();
            self.chars.extend(std::iter::repeat_n(StyledChar { ch: ' ', style }, total - width));
        }
    }

    /// Shorten to `max_width` cells (rich's `Text.truncate`).
    fn truncate(&mut self, max_width: usize, overflow: Overflow, pad: bool) {
        if overflow == Overflow::Ignore {
            return;
        }
        let length = self.cell_len();
        if length > max_width {
            if overflow == Overflow::Ellipsis {
                let style = self.chars.get(max_width.saturating_sub(1)).map(|c| c.style).unwrap_or_default();
                self.set_cell_size(max_width.saturating_sub(1));
                self.chars.push(StyledChar { ch: '…', style });
            } else {
                self.set_cell_size(max_width);
            }
        }
        if pad && length < max_width {
            self.pad_right(max_width - length);
        }
    }

    /// Word-wrap into lines of at most `width` cells (rich's `Text.wrap`).
    pub fn wrap(&self, width: usize, justify: Justify, overflow: Overflow, no_wrap: bool) -> Vec<Text> {
        let mut lines = Vec::new();
        for line in self.split_lines() {
            let mut new_lines = if no_wrap {
                vec![line]
            } else {
                let chars: Vec<char> = line.chars.iter().map(|c| c.ch).collect();
                let offsets = divide_line(&chars, width, overflow == Overflow::Fold);
                line.divide(&offsets)
            };
            for line in &mut new_lines {
                line.rstrip_end(width);
            }
            justify_lines(&mut new_lines, width, justify, overflow);
            for line in &mut new_lines {
                line.truncate(width, overflow, false);
            }
            lines.extend(new_lines);
        }
        lines
    }

    /// The text as segments, with the base style applied.
    pub fn segments(&self) -> Line {
        let mut line: Line = Vec::new();
        for c in &self.chars {
            let style = self.style.combine(c.style);
            match line.last_mut() {
                Some(last) if last.style == style => last.text.push(c.ch),
                _ => line.push(Segment { text: c.ch.to_string(), style }),
            }
        }
        line
    }
}

impl From<&str> for Text {
    /// Strings are console markup, as in rich.
    fn from(markup: &str) -> Text {
        Text::from_markup(markup)
    }
}

impl From<String> for Text {
    /// Strings are console markup, as in rich.
    fn from(markup: String) -> Text {
        Text::from_markup(&markup)
    }
}

/// `(start, end)` character spans of rich's words (`\s*\S+\s*`).
fn words(chars: &[char]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut position = 0;
    while position < chars.len() {
        let mut index = position;
        while index < chars.len() && chars[index].is_whitespace() {
            index += 1;
        }
        let word_start = index;
        while index < chars.len() && !chars[index].is_whitespace() {
            index += 1;
        }
        if index == word_start {
            break;
        }
        while index < chars.len() && chars[index].is_whitespace() {
            index += 1;
        }
        out.push((position, index));
        position = index;
    }
    out
}

fn chars_cell_len(chars: &[char]) -> usize {
    chars.iter().map(|c| char_width(*c)).sum()
}

/// Split a word into pieces of at most `width` cells (rich's `chop_cells`).
fn chop_cells(chars: &[char], width: usize) -> Vec<Vec<char>> {
    let mut lines: Vec<Vec<char>> = vec![Vec::new()];
    let mut total = 0;
    for &ch in chars {
        let w = char_width(ch);
        if total + w > width {
            lines.push(vec![ch]);
            total = w;
        } else {
            lines.last_mut().expect("at least one line").push(ch);
            total += w;
        }
    }
    lines
}

/// Character offsets at which to break a line to fit `width` cells (rich's `divide_line`).
fn divide_line(chars: &[char], width: usize, fold: bool) -> Vec<usize> {
    let mut breaks = Vec::new();
    let mut cell_offset: usize = 0;
    for (start, end) in words(chars) {
        let word = &chars[start..end];
        let trimmed_end = word.iter().rposition(|c| !c.is_whitespace()).map_or(0, |i| i + 1);
        let word_length = chars_cell_len(&word[..trimmed_end]);
        let remaining = width as i64 - cell_offset as i64;
        if remaining >= word_length as i64 {
            cell_offset += chars_cell_len(word);
        } else if word_length > width {
            if fold {
                let folded = chop_cells(word, width);
                let mut position = start;
                let count = folded.len();
                for (index, piece) in folded.iter().enumerate() {
                    if position != 0 {
                        breaks.push(position);
                    }
                    if index + 1 == count {
                        cell_offset = chars_cell_len(piece);
                    } else {
                        position += piece.len();
                    }
                }
            } else {
                if start != 0 {
                    breaks.push(start);
                }
                cell_offset = chars_cell_len(word);
            }
        } else if cell_offset != 0 && start != 0 {
            breaks.push(start);
            cell_offset = chars_cell_len(word);
        }
    }
    breaks
}

/// Justify wrapped lines in place (rich's `Lines.justify`).
fn justify_lines(lines: &mut [Text], width: usize, justify: Justify, overflow: Overflow) {
    match justify {
        Justify::Default => {}
        Justify::Left => {
            for line in lines {
                line.truncate(width, overflow, true);
            }
        }
        Justify::Center => {
            for line in lines {
                line.rstrip();
                line.truncate(width, overflow, false);
                let left = width.saturating_sub(line.cell_len()) / 2;
                line.pad_left(left);
                let right = width.saturating_sub(line.cell_len());
                line.pad_right(right);
            }
        }
        Justify::Right => {
            for line in lines {
                line.rstrip();
                line.truncate(width, overflow, false);
                let left = width.saturating_sub(line.cell_len());
                line.pad_left(left);
            }
        }
    }
}

// =============================================================================
// Renderables
// =============================================================================

/// Layout options passed down while rendering (rich's `ConsoleOptions`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// Available width in cells.
    pub width: usize,
    /// Justification for text that does not set its own.
    pub justify: Option<Justify>,
    /// Overflow for text that does not set its own.
    pub overflow: Option<Overflow>,
    /// Disable wrapping for text that does not set it.
    pub no_wrap: bool,
}

impl Options {
    /// Options for a given width.
    pub fn width(width: usize) -> Options {
        Options { width, justify: None, overflow: None, no_wrap: false }
    }

    fn with_width(self, width: usize) -> Options {
        Options { width, ..self }
    }
}

/// Something that can be laid out into lines.
pub trait Renderable {
    /// Render into lines of at most `options.width` cells.
    fn render(&self, options: &Options) -> Vec<Line>;

    /// (minimum, maximum) width in cells (rich's `__rich_measure__`).
    fn measure(&self, options: &Options) -> (usize, usize);
}

/// rich's `Measurement.get`: normalised and clamped to the available width.
fn measure(renderable: &dyn Renderable, options: &Options) -> (usize, usize) {
    let max_width = options.width;
    if max_width < 1 {
        return (0, 0);
    }
    let (minimum, maximum) = renderable.measure(options);
    let minimum = minimum.min(maximum);
    let (minimum, maximum) = (minimum.min(max_width), maximum.min(max_width));
    if maximum < 1 {
        return (0, 0);
    }
    (minimum.min(maximum), maximum)
}

impl Renderable for Text {
    fn render(&self, options: &Options) -> Vec<Line> {
        let justify = self.justify.or(options.justify).unwrap_or(Justify::Default);
        let overflow = self.overflow.or(options.overflow).unwrap_or(Overflow::Fold);
        let no_wrap = self.no_wrap.unwrap_or(options.no_wrap);
        self.wrap(options.width, justify, overflow, no_wrap).iter().map(Text::segments).collect()
    }

    fn measure(&self, _options: &Options) -> (usize, usize) {
        self.measurement()
    }
}

fn line_cell_len(line: &Line) -> usize {
    line.iter().map(|s| cell_len(&s.text)).sum()
}

/// Crop a line to `width` cells (a split wide character becomes a space).
fn crop_line(line: Line, width: usize) -> Line {
    let mut out: Line = Vec::new();
    let mut used = 0;
    for segment in line {
        if used >= width {
            break;
        }
        let length = cell_len(&segment.text);
        if used + length <= width {
            used += length;
            out.push(segment);
            continue;
        }
        let mut text = String::new();
        for ch in segment.text.chars() {
            let w = char_width(ch);
            if used + w > width {
                if used < width {
                    text.push(' ');
                }
                break;
            }
            text.push(ch);
            used += w;
        }
        out.push(Segment { text, style: segment.style });
        break;
    }
    out
}

/// rich's `Console.render_lines`: apply `style` under each segment's own
/// style, crop to the width, and optionally pad with spaces.
fn render_lines(renderable: &dyn Renderable, options: &Options, style: Style, pad: bool) -> Vec<Line> {
    renderable
        .render(options)
        .into_iter()
        .map(|line| {
            let line: Line =
                line.into_iter().map(|s| Segment { text: s.text, style: style.combine(s.style) }).collect();
            let mut line = crop_line(line, options.width);
            let length = line_cell_len(&line);
            if pad && length < options.width {
                line.push(Segment { text: " ".repeat(options.width - length), style });
            }
            line
        })
        .collect()
}

/// Pad content: `(top, right, bottom, left)` blank cells around it.
struct Padding<'a> {
    content: &'a dyn Renderable,
    padding: (usize, usize, usize, usize),
}

impl Renderable for Padding<'_> {
    fn render(&self, options: &Options) -> Vec<Line> {
        let (top, right, bottom, left) = self.padding;
        let width = options.width;
        let inner = options.with_width(width.saturating_sub(left + right));
        let lines = render_lines(self.content, &inner, Style::null(), true);
        let blank = || vec![Segment { text: " ".repeat(width), style: Style::null() }];
        let mut out: Vec<Line> = (0..top).map(|_| blank()).collect();
        for line in lines {
            let mut padded: Line = Vec::new();
            if left > 0 {
                padded.push(Segment { text: " ".repeat(left), style: Style::null() });
            }
            padded.extend(line);
            if right > 0 {
                padded.push(Segment { text: " ".repeat(right), style: Style::null() });
            }
            out.push(padded);
        }
        out.extend((0..bottom).map(|_| blank()));
        out
    }

    fn measure(&self, options: &Options) -> (usize, usize) {
        let (_, right, _, left) = self.padding;
        let extra = left + right;
        let max_width = options.width;
        if max_width < extra + 1 {
            return (max_width, max_width);
        }
        let (minimum, maximum) = measure(self.content, options);
        ((minimum + extra).min(max_width), (maximum + extra).min(max_width))
    }
}

// =============================================================================
// Boxes
// =============================================================================

/// Box-drawing styles for tables and panels (rich's `box` module).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoxStyle {
    Ascii,
    Square,
    Minimal,
    Simple,
    SimpleHead,
    Heavy,
    HeavyHead,
    Double,
    Rounded,
}

impl BoxStyle {
    /// rich's eight 4-character lines: top, head, head_row, mid, row, foot_row, foot, bottom.
    fn spec(self) -> &'static str {
        match self {
            BoxStyle::Ascii => "+--+| |||-+|| |||-+||-+|| ||+--+",
            BoxStyle::Square => "┌─┬┐│ ││├─┼┤│ ││├─┼┤├─┼┤│ ││└─┴┘",
            BoxStyle::Minimal => "  ╷   │ ╶─┼╴  │ ╶─┼╴╶─┼╴  │   ╵ ",
            BoxStyle::Simple => "         ──          ──         ",
            BoxStyle::SimpleHead => "         ──                     ",
            BoxStyle::Heavy => "┏━┳┓┃ ┃┃┣━╋┫┃ ┃┃┣━╋┫┣━╋┫┃ ┃┃┗━┻┛",
            BoxStyle::HeavyHead => "┏━┳┓┃ ┃┃┡━╇┩│ ││├─┼┤├─┼┤│ ││└─┴┘",
            BoxStyle::Double => "╔═╦╗║ ║║╠═╬╣║ ║║╠═╬╣╠═╬╣║ ║║╚═╩╝",
            BoxStyle::Rounded => "╭─┬╮│ ││├─┼┤│ ││├─┼┤├─┼┤│ ││╰─┴╯",
        }
    }

    /// Character `column` (0-3) of box line `row` (0-7).
    fn ch(self, row: usize, column: usize) -> char {
        self.spec().chars().nth(row * 4 + column).unwrap_or(' ')
    }

    /// A horizontal line: `row` 0 = top, 2 = below the header, 4 = between
    /// rows, 7 = bottom.
    fn line(self, row: usize, widths: &[usize], edge: bool) -> String {
        let (left, horizontal, cross, right) = (self.ch(row, 0), self.ch(row, 1), self.ch(row, 2), self.ch(row, 3));
        let mut out = String::new();
        if edge {
            out.push(left);
        }
        for (index, width) in widths.iter().enumerate() {
            out.extend(std::iter::repeat_n(horizontal, *width));
            if index + 1 < widths.len() {
                out.push(cross);
            }
        }
        if edge {
            out.push(right);
        }
        out
    }
}

// =============================================================================
// Table
// =============================================================================

/// A table column (rich's `Column`).
#[derive(Clone, Debug)]
pub struct Column {
    header: Text,
    justify: Justify,
    style: Style,
    header_style: Style,
    width: Option<usize>,
    no_wrap: bool,
    overflow: Overflow,
    cells: Vec<Text>,
}

impl Column {
    /// A column with a header (console markup).
    pub fn new(header: &str) -> Column {
        Column {
            header: Text::from_markup(header),
            justify: Justify::Left,
            style: Style::null(),
            header_style: Style::null(),
            width: None,
            no_wrap: false,
            overflow: Overflow::Ellipsis,
            cells: Vec::new(),
        }
    }

    /// How cells are justified (default left).
    pub fn justify(mut self, justify: Justify) -> Column {
        self.justify = justify;
        self
    }

    /// Style for the column's cells.
    pub fn style(mut self, style: &str) -> Column {
        self.style = Style::get(style);
        self
    }

    /// Extra style for the header cell.
    pub fn header_style(mut self, style: &str) -> Column {
        self.header_style = Style::get(style);
        self
    }

    /// Fixed content width in cells.
    pub fn width(mut self, width: usize) -> Column {
        self.width = Some(width);
        self
    }

    /// Never wrap the column's cells.
    pub fn no_wrap(mut self) -> Column {
        self.no_wrap = true;
        self
    }

    /// Overflow method for the column's cells (default ellipsis).
    pub fn overflow(mut self, overflow: Overflow) -> Column {
        self.overflow = overflow;
        self
    }
}

#[derive(Clone, Debug, Default)]
struct RowInfo {
    style: Style,
    end_section: bool,
}

/// A table (rich's `Table`), laid out with rich's column-width algorithm.
#[derive(Clone, Debug)]
pub struct Table {
    title: Option<Text>,
    columns: Vec<Column>,
    rows: Vec<RowInfo>,
    box_style: Option<BoxStyle>,
    header_style: Style,
    border_style: Style,
    show_header: bool,
    show_edge: bool,
    show_lines: bool,
    expand: bool,
}

impl Default for Table {
    fn default() -> Table {
        Table::new()
    }
}

impl Table {
    /// An empty table with rich's defaults (heavy-head box, bold headers).
    pub fn new() -> Table {
        Table {
            title: None,
            columns: Vec::new(),
            rows: Vec::new(),
            box_style: Some(BoxStyle::HeavyHead),
            header_style: Style::get("bold"),
            border_style: Style::null(),
            show_header: true,
            show_edge: true,
            show_lines: false,
            expand: false,
        }
    }

    /// Title (console markup) centred above the table, in italics.
    pub fn title(mut self, markup: &str) -> Table {
        self.title = Some(Text::from_markup(markup).with_style("italic"));
        self
    }

    /// Box style (`None` draws no borders).
    pub fn box_style(mut self, box_style: BoxStyle) -> Table {
        self.box_style = Some(box_style);
        self
    }

    /// Draw no borders.
    pub fn no_box(mut self) -> Table {
        self.box_style = None;
        self
    }

    /// Style for header cells.
    pub fn header_style(mut self, style: &str) -> Table {
        self.header_style = Style::get(style);
        self
    }

    /// Style for the borders.
    pub fn border_style(mut self, style: &str) -> Table {
        self.border_style = Style::get(style);
        self
    }

    /// Show or hide the header row.
    pub fn show_header(mut self, show: bool) -> Table {
        self.show_header = show;
        self
    }

    /// Draw lines between all rows.
    pub fn show_lines(mut self, show: bool) -> Table {
        self.show_lines = show;
        self
    }

    /// Expand to the full available width.
    pub fn expand(mut self, expand: bool) -> Table {
        self.expand = expand;
        self
    }

    /// Add a column.
    pub fn add_column(&mut self, column: Column) {
        let mut column = column;
        column.cells.resize(self.rows.len(), Text::new());
        self.columns.push(column);
    }

    /// Add a row of cells (missing cells are blank).
    pub fn add_row<I, T>(&mut self, cells: I)
    where
        I: IntoIterator<Item = T>,
        T: Into<Text>,
    {
        self.add_row_styled(cells, "");
    }

    /// Add a row with a style applied to every cell.
    pub fn add_row_styled<I, T>(&mut self, cells: I, style: &str)
    where
        I: IntoIterator<Item = T>,
        T: Into<Text>,
    {
        let mut cells = cells.into_iter().map(Into::into);
        for column in &mut self.columns {
            column.cells.push(cells.next().unwrap_or_default());
        }
        self.rows.push(RowInfo { style: Style::get(style), end_section: false });
    }

    /// Draw a separator after the last row added so far.
    pub fn add_section(&mut self) {
        if let Some(last) = self.rows.last_mut() {
            last.end_section = true;
        }
    }

    /// Number of data rows.
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    const PADDING: usize = 2;

    fn extra_width(&self) -> usize {
        let mut width = 0;
        if self.box_style.is_some() && self.show_edge {
            width += 2;
        }
        if self.box_style.is_some() {
            width += self.columns.len().saturating_sub(1);
        }
        width
    }

    fn cell_texts<'a>(&'a self, column: &'a Column) -> impl Iterator<Item = &'a Text> + 'a {
        self.show_header.then_some(&column.header).into_iter().chain(column.cells.iter())
    }

    fn measure_column(&self, column: &Column, max_width: usize) -> (usize, usize) {
        if max_width < 1 {
            return (0, 0);
        }
        if let Some(width) = column.width {
            let width = (width + Self::PADDING).min(max_width);
            return (width, width);
        }
        let options = Options::width(max_width);
        let mut minimum: Option<usize> = None;
        let mut maximum: Option<usize> = None;
        for text in self.cell_texts(column) {
            let padded = Padding { content: text, padding: (0, 1, 0, 1) };
            let (lo, hi) = measure(&padded, &options);
            minimum = Some(minimum.map_or(lo, |m| m.max(lo)));
            maximum = Some(maximum.map_or(hi, |m| m.max(hi)));
        }
        let minimum = minimum.unwrap_or(1).min(max_width);
        let maximum = maximum.unwrap_or(max_width).min(max_width);
        (minimum, maximum)
    }

    fn column_widths(&self, max_width: usize) -> Vec<usize> {
        let mut widths: Vec<usize> = self
            .columns
            .iter()
            .map(|c| self.measure_column(c, max_width).1)
            .map(|w| if w == 0 { 1 } else { w })
            .collect();
        let mut table_width: usize = widths.iter().sum();
        if table_width > max_width {
            let wrapable: Vec<bool> = self.columns.iter().map(|c| c.width.is_none() && !c.no_wrap).collect();
            widths = collapse_widths(&widths, &wrapable, max_width);
            table_width = widths.iter().sum();
            if table_width > max_width {
                let excess = table_width - max_width;
                widths = ratio_reduce(excess, &vec![1; widths.len()], &widths, &widths);
                table_width = widths.iter().sum();
            }
            widths = self.columns.iter().zip(&widths).map(|(c, w)| self.measure_column(c, *w).1).collect();
        }
        if table_width < max_width && self.expand {
            let pads = ratio_distribute(max_width - table_width, &widths);
            widths = widths.iter().zip(pads).map(|(w, p)| w + p).collect();
        }
        widths
    }

    fn render_body(&self, widths: &[usize]) -> Vec<Line> {
        let mut out: Vec<Line> = Vec::new();
        let border = self.border_style;
        let edge = self.show_edge;
        if let Some(b) = self.box_style
            && edge
        {
            out.push(vec![Segment { text: b.line(0, widths, true), style: border }]);
        }
        let header_count = usize::from(self.show_header);
        let total_rows = header_count + self.rows.len();
        for index in 0..total_rows {
            let header_row = self.show_header && index == 0;
            let first = index == 0;
            let last = index + 1 == total_rows;
            let row = if header_row { None } else { self.rows.get(index - header_count) };
            let row_style = row.map_or(Style::null(), |r| r.style);
            let mut cells: Vec<Vec<Line>> = Vec::new();
            for (column, width) in self.columns.iter().zip(widths) {
                let (text, cell_style) = if header_row {
                    (&column.header, self.header_style.combine(column.header_style))
                } else {
                    (&column.cells[index - header_count], column.style)
                };
                let options = Options {
                    width: *width,
                    justify: Some(column.justify),
                    overflow: Some(column.overflow),
                    no_wrap: column.no_wrap,
                };
                let padded = Padding { content: text, padding: (0, 1, 0, 1) };
                cells.push(render_lines(&padded, &options, cell_style.combine(row_style), true));
            }
            let height = cells.iter().map(Vec::len).max().unwrap_or(1).max(1);
            for ((cell, width), column) in cells.iter_mut().zip(widths).zip(&self.columns) {
                let style = if header_row {
                    self.header_style.combine(column.header_style)
                } else {
                    column.style.combine(row_style)
                };
                while cell.len() < height {
                    if header_row {
                        cell.insert(0, vec![Segment { text: " ".repeat(*width), style }]);
                    } else {
                        cell.push(vec![Segment { text: " ".repeat(*width), style }]);
                    }
                }
            }
            match self.box_style {
                Some(b) => {
                    let box_row = if first {
                        1
                    } else if last {
                        6
                    } else {
                        3
                    };
                    let (left, divider, right) = (b.ch(box_row, 0), b.ch(box_row, 2), b.ch(box_row, 3));
                    let divider_style =
                        if divider.is_whitespace() { row_style.background_only().combine(border) } else { border };
                    for line_no in 0..height {
                        let mut line: Line = Vec::new();
                        if edge {
                            line.push(Segment { text: left.to_string(), style: border });
                        }
                        for (index, cell) in cells.iter().enumerate() {
                            line.extend(cell[line_no].iter().cloned());
                            if index + 1 < cells.len() {
                                line.push(Segment { text: divider.to_string(), style: divider_style });
                            }
                        }
                        if edge {
                            line.push(Segment { text: right.to_string(), style: border });
                        }
                        out.push(line);
                    }
                    if first && self.show_header {
                        out.push(vec![Segment { text: b.line(2, widths, edge), style: border }]);
                    }
                    let end_section = row.is_some_and(|r| r.end_section);
                    if (self.show_lines || end_section) && !last && !header_row {
                        out.push(vec![Segment { text: b.line(4, widths, edge), style: border }]);
                    }
                }
                None => {
                    for line_no in 0..height {
                        out.push(cells.iter().flat_map(|cell| cell[line_no].iter().cloned()).collect());
                    }
                }
            }
        }
        if let Some(b) = self.box_style
            && edge
        {
            out.push(vec![Segment { text: b.line(7, widths, true), style: border }]);
        }
        out
    }
}

/// rich's `ratio_distribute`: share `total` in proportion to `ratios`.
fn ratio_distribute(total: usize, ratios: &[usize]) -> Vec<usize> {
    let mut total_ratio: f64 = ratios.iter().sum::<usize>() as f64;
    let mut total_remaining = total as f64;
    let mut out = Vec::with_capacity(ratios.len());
    for &ratio in ratios {
        let distributed = if total_ratio > 0.0 {
            (ratio as f64 * total_remaining / total_ratio).ceil().max(0.0)
        } else {
            total_remaining
        };
        out.push(distributed as usize);
        total_ratio -= ratio as f64;
        total_remaining -= distributed;
    }
    out
}

/// rich's `ratio_reduce`: take `total` away in proportion to `ratios`.
fn ratio_reduce(total: usize, ratios: &[usize], maximums: &[usize], values: &[usize]) -> Vec<usize> {
    let ratios: Vec<usize> = ratios.iter().zip(maximums).map(|(r, m)| if *m > 0 { *r } else { 0 }).collect();
    let mut total_ratio: f64 = ratios.iter().sum::<usize>() as f64;
    if total_ratio == 0.0 {
        return values.to_vec();
    }
    let mut total_remaining = total as f64;
    let mut out = Vec::with_capacity(values.len());
    for ((ratio, maximum), value) in ratios.iter().zip(maximums).zip(values) {
        if *ratio > 0 && total_ratio > 0.0 {
            let distributed =
                (*maximum as f64).min(round_half_even(*ratio as f64 * total_remaining / total_ratio)).max(0.0);
            out.push(value.saturating_sub(distributed as usize));
            total_remaining -= distributed;
            total_ratio -= *ratio as f64;
        } else {
            out.push(*value);
        }
    }
    out
}

/// rich's `Table._collapse_widths`: shrink the widest wrappable columns first.
fn collapse_widths(widths: &[usize], wrapable: &[bool], max_width: usize) -> Vec<usize> {
    let mut widths = widths.to_vec();
    let mut total: usize = widths.iter().sum();
    if !wrapable.iter().any(|w| *w) {
        return widths;
    }
    while total > 0 && total > max_width {
        let excess = total - max_width;
        let max_column = widths.iter().zip(wrapable).filter(|(_, w)| **w).map(|(w, _)| *w).max().unwrap_or(0);
        let second_max = widths
            .iter()
            .zip(wrapable)
            .map(|(w, allow)| if *allow && *w != max_column { *w } else { 0 })
            .max()
            .unwrap_or(0);
        let difference = max_column - second_max;
        let ratios: Vec<usize> =
            widths.iter().zip(wrapable).map(|(w, allow)| usize::from(*w == max_column && *allow)).collect();
        if ratios.iter().all(|r| *r == 0) || difference == 0 {
            break;
        }
        let max_reduce = vec![excess.min(difference); widths.len()];
        widths = ratio_reduce(excess, &ratios, &max_reduce, &widths);
        total = widths.iter().sum();
    }
    widths
}

impl Renderable for Table {
    fn render(&self, options: &Options) -> Vec<Line> {
        if self.columns.is_empty() {
            return vec![Vec::new()];
        }
        let extra = self.extra_width();
        let widths = self.column_widths(options.width.saturating_sub(extra));
        let table_width = widths.iter().sum::<usize>() + extra;
        let mut out = Vec::new();
        if let Some(title) = &self.title {
            let title_options = Options { width: table_width, justify: Some(Justify::Center), ..*options };
            out.extend(title.render(&title_options));
        }
        out.extend(self.render_body(&widths));
        out
    }

    fn measure(&self, options: &Options) -> (usize, usize) {
        let extra = self.extra_width();
        let max_width = options.width.saturating_sub(extra);
        let widths = self.column_widths(max_width);
        let minimum: usize = self.columns.iter().map(|c| self.measure_column(c, max_width).0).sum::<usize>() + extra;
        let maximum = widths.iter().sum::<usize>() + extra;
        (minimum.min(maximum), maximum)
    }
}

// =============================================================================
// Panel, Align, Rule
// =============================================================================

/// A bordered box around content (rich's `Panel`).
pub struct Panel {
    content: Box<dyn Renderable>,
    title: Option<Text>,
    style: Style,
    border_style: Style,
    box_style: BoxStyle,
    padding: (usize, usize, usize, usize),
    expand: bool,
}

impl Panel {
    /// A rounded panel that fills the width, with padding `(0, 1)`.
    pub fn new(content: impl Renderable + 'static) -> Panel {
        Panel {
            content: Box::new(content),
            title: None,
            style: Style::null(),
            border_style: Style::null(),
            box_style: BoxStyle::Rounded,
            padding: (0, 1, 0, 1),
            expand: true,
        }
    }

    /// Title (console markup) centred in the top border.
    pub fn title(mut self, markup: &str) -> Panel {
        self.title = Some(Text::from_markup(markup));
        self
    }

    /// Style for the whole panel (border and content).
    pub fn style(mut self, style: &str) -> Panel {
        self.style = Style::get(style);
        self
    }

    /// Style for the border.
    pub fn border_style(mut self, style: &str) -> Panel {
        self.border_style = Style::get(style);
        self
    }

    /// Box style.
    pub fn box_style(mut self, box_style: BoxStyle) -> Panel {
        self.box_style = box_style;
        self
    }

    /// Padding as (vertical, horizontal) cells.
    pub fn padding(mut self, vertical: usize, horizontal: usize) -> Panel {
        self.padding = (vertical, horizontal, vertical, horizontal);
        self
    }

    /// Fill the width (`true`, the default) or fit the content.
    pub fn expand(mut self, expand: bool) -> Panel {
        self.expand = expand;
        self
    }

    fn padded(&self) -> Padding<'_> {
        Padding { content: self.content.as_ref(), padding: self.padding }
    }
}

impl Renderable for Panel {
    fn render(&self, options: &Options) -> Vec<Line> {
        let padded = self.padded();
        let border = self.style.combine(self.border_style);
        let max_width = options.width;
        let title = self.title.as_ref().map(|t| {
            let mut title = t.clone();
            for c in &mut title.chars {
                if c.ch == '\n' {
                    c.ch = ' ';
                }
            }
            title.no_wrap = Some(true);
            title.pad_left(1);
            title.pad_right(1);
            for c in &mut title.chars {
                c.style = border.combine(c.style);
            }
            title
        });
        let mut child_width = if self.expand {
            max_width.saturating_sub(2)
        } else {
            measure(&padded, &options.with_width(max_width.saturating_sub(2))).1
        };
        if let Some(title) = &title {
            child_width = max_width.saturating_sub(2).min(child_width.max(title.cell_len() + 2));
        }
        let width = child_width + 2;
        let child_options = options.with_width(child_width);
        let lines = render_lines(&padded, &child_options, self.style, true);
        let b = self.box_style;
        let mut out: Vec<Line> = Vec::new();
        match title {
            Some(title) if width > 4 => {
                let mut title = title;
                title.truncate(width - 4, Overflow::Fold, false);
                let excess = (width - 4).saturating_sub(title.cell_len());
                let left = excess / 2;
                let horizontal = b.ch(0, 1).to_string();
                let mut line: Line =
                    vec![Segment { text: format!("{}{}", b.ch(0, 0), horizontal.repeat(1 + left)), style: border }];
                line.extend(title.segments());
                line.push(Segment {
                    text: format!("{}{}", horizontal.repeat(excess - left + 1), b.ch(0, 3)),
                    style: border,
                });
                out.push(line);
            }
            _ => out.push(vec![Segment { text: b.line(0, &[width - 2], true), style: border }]),
        }
        for line in lines {
            let mut row: Line = vec![Segment { text: b.ch(3, 0).to_string(), style: border }];
            row.extend(line);
            row.push(Segment { text: b.ch(3, 3).to_string(), style: border });
            out.push(row);
        }
        out.push(vec![Segment { text: b.line(7, &[width - 2], true), style: border }]);
        out
    }

    fn measure(&self, options: &Options) -> (usize, usize) {
        let padded = self.padded();
        let width = measure(&padded, &options.with_width(options.width.saturating_sub(2))).1 + 2;
        (width, width)
    }
}

/// Centre (or right-align) content within the width (rich's `Align`).
pub struct Align {
    content: Box<dyn Renderable>,
    justify: Justify,
}

impl Align {
    /// Centre content, padding both sides to the full width.
    pub fn center(content: impl Renderable + 'static) -> Align {
        Align { content: Box::new(content), justify: Justify::Center }
    }

    /// Right-align content.
    pub fn right(content: impl Renderable + 'static) -> Align {
        Align { content: Box::new(content), justify: Justify::Right }
    }
}

impl Renderable for Align {
    fn render(&self, options: &Options) -> Vec<Line> {
        let width = measure(self.content.as_ref(), options).1;
        let lines = render_lines(self.content.as_ref(), &options.with_width(width), Style::null(), true);
        let excess = options.width.saturating_sub(width);
        lines
            .into_iter()
            .map(|line| {
                let mut out: Line = Vec::new();
                let (left, right) = match self.justify {
                    Justify::Center => (excess / 2, excess - excess / 2),
                    Justify::Right => (excess, 0),
                    _ => (0, excess),
                };
                if left > 0 {
                    out.push(Segment { text: " ".repeat(left), style: Style::null() });
                }
                out.extend(line);
                if right > 0 {
                    out.push(Segment { text: " ".repeat(right), style: Style::null() });
                }
                out
            })
            .collect()
    }

    fn measure(&self, options: &Options) -> (usize, usize) {
        measure(self.content.as_ref(), options)
    }
}

/// A horizontal line across the width (rich's `Rule` without a title).
pub struct Rule {
    style: Style,
}

impl Rule {
    /// A rule drawn with `─` in a style.
    pub fn new(style: &str) -> Rule {
        Rule { style: Style::get(style) }
    }
}

impl Renderable for Rule {
    fn render(&self, options: &Options) -> Vec<Line> {
        vec![vec![Segment { text: "─".repeat(options.width), style: self.style }]]
    }

    fn measure(&self, options: &Options) -> (usize, usize) {
        (1, options.width)
    }
}

// =============================================================================
// Highlighting (rich's ReprHighlighter, the parts these messages use)
// =============================================================================

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// Spans rich's default highlighter (`ReprHighlighter`) colours: quoted
/// strings, URLs, `True`/`False`/`None`, and numbers.
fn highlight_spans(chars: &[char]) -> Vec<(usize, usize, &'static str)> {
    const URL_SCHEMES: [&str; 5] = ["https://", "http://", "file://", "wss://", "ws://"];
    let url_char = |c: char| c.is_ascii_alphanumeric() || "-$_+!`(),.?/;:&=%#~@".contains(c);
    let starts_with = |at: usize, word: &str| {
        let word: Vec<char> = word.chars().collect();
        chars.len() >= at + word.len() && chars[at..at + word.len()] == word[..]
    };
    let mut spans = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let prev = (i > 0).then(|| chars[i - 1]);
        let after_word = prev.is_some_and(is_word_char);
        if (c == '\'' || c == '"') && !after_word && prev != Some('\\') {
            let close = (i + 1..chars.len())
                .take_while(|j| chars[*j] != '\n')
                .find(|j| chars[*j] == c && chars[*j - 1] != '\\');
            if let Some(close) = close {
                spans.push((i, close + 1, "str"));
                i = close + 1;
                continue;
            }
        }
        if let Some(scheme) = URL_SCHEMES.iter().find(|scheme| starts_with(i, scheme)) {
            let mut end = i + scheme.chars().count();
            while end < chars.len() && url_char(chars[end]) {
                end += 1;
            }
            spans.push((i, end, "url"));
            i = end;
            continue;
        }
        if !after_word {
            let keyword =
                [("True", "bool_true"), ("False", "bool_false"), ("None", "none")].into_iter().find(|(word, _)| {
                    starts_with(i, word) && !chars.get(i + word.len()).copied().is_some_and(is_word_char)
                });
            if let Some((word, kind)) = keyword {
                spans.push((i, i + word.len(), kind));
                i += word.len();
                continue;
            }
            let digits_from = if c == '-' { i + 1 } else { i };
            if chars.get(digits_from).is_some_and(char::is_ascii_digit) {
                let mut end = digits_from;
                while end < chars.len() && chars[end].is_ascii_digit() {
                    end += 1;
                }
                if end + 1 < chars.len() && chars[end] == '.' && chars[end + 1].is_ascii_digit() {
                    end += 1;
                    while end < chars.len() && chars[end].is_ascii_digit() {
                        end += 1;
                    }
                }
                if !chars.get(end).copied().is_some_and(is_word_char) {
                    spans.push((i, end, "number"));
                    i = end;
                    continue;
                }
                // Not a whole number: skip the rest of this run of digits.
                i = end;
                continue;
            }
        }
        i += 1;
    }
    spans
}

/// Colour numbers, quoted strings, booleans and URLs the way rich's default
/// highlighter does for `console.print(str)` (colour output only).
fn highlight(text: &mut Text) {
    let chars: Vec<char> = text.chars.iter().map(|c| c.ch).collect();
    for (start, end, kind) in highlight_spans(&chars) {
        let style = Style::get(match kind {
            "str" => "not bold not italic green",
            "url" => "not bold not italic underline bright_blue",
            "bool_true" => "italic bright_green",
            "bool_false" => "italic bright_red",
            "none" => "italic magenta",
            _ => "bold not italic cyan",
        });
        for c in &mut text.chars[start..end] {
            c.style = c.style.combine(style);
        }
    }
}

// =============================================================================
// Prompts and Ctrl+C
// =============================================================================

/// Why a prompt returned no line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptError {
    /// Ctrl+C (Python's `KeyboardInterrupt`).
    Interrupted,
    /// End of input (Python's `EOFError`).
    Eof,
}

#[cfg(unix)]
mod signals {
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::PromptError;

    static CANCEL_MESSAGE: OnceLock<Vec<u8>> = OnceLock::new();
    static INTERRUPTED: AtomicBool = AtomicBool::new(false);

    extern "C" fn print_and_exit(_signal: libc::c_int) {
        if let Some(message) = CANCEL_MESSAGE.get() {
            // SAFETY: write(2) and _exit(2) are async-signal-safe.
            unsafe {
                libc::write(libc::STDOUT_FILENO, message.as_ptr().cast(), message.len());
                libc::_exit(1);
            }
        }
        // SAFETY: as above.
        unsafe { libc::_exit(1) };
    }

    extern "C" fn set_flag(_signal: libc::c_int) {
        INTERRUPTED.store(true, Ordering::SeqCst);
    }

    /// SAFETY: installs a handler that only touches async-signal-safe state.
    unsafe fn set_handler(handler: extern "C" fn(libc::c_int)) -> libc::sigaction {
        // SAFETY: zeroed sigaction structs are valid; the pointers are live.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = handler as *const () as libc::sighandler_t;
            action.sa_flags = 0;
            libc::sigemptyset(&mut action.sa_mask);
            let mut previous: libc::sigaction = std::mem::zeroed();
            libc::sigaction(libc::SIGINT, &action, &mut previous);
            previous
        }
    }

    pub(super) fn install_cancel_handler(message: &str) {
        let _ = CANCEL_MESSAGE.set(message.as_bytes().to_vec());
        // SAFETY: the handler only uses write(2) and _exit(2).
        unsafe {
            set_handler(print_and_exit);
        }
    }

    /// Read one line from the stdin descriptor, returning `Interrupted` on Ctrl+C.
    pub(super) fn read_line() -> Result<String, PromptError> {
        INTERRUPTED.store(false, Ordering::SeqCst);
        // SAFETY: the handler only stores to an atomic.
        let previous = unsafe { set_handler(set_flag) };
        let mut bytes: Vec<u8> = Vec::new();
        let result = loop {
            let mut byte = 0u8;
            // SAFETY: reading one byte into a live local buffer.
            let n = unsafe { libc::read(libc::STDIN_FILENO, (&mut byte as *mut u8).cast(), 1) };
            if n == 1 {
                if byte == b'\n' {
                    break Ok(());
                }
                bytes.push(byte);
                continue;
            }
            if n == 0 {
                break if bytes.is_empty() { Err(PromptError::Eof) } else { Ok(()) };
            }
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                if INTERRUPTED.load(Ordering::SeqCst) {
                    break Err(PromptError::Interrupted);
                }
                continue;
            }
            break Err(PromptError::Eof);
        };
        // SAFETY: restoring the handler that was installed before.
        unsafe {
            libc::sigaction(libc::SIGINT, &previous, std::ptr::null_mut());
        }
        result.map(|()| String::from_utf8_lossy(&bytes).into_owned())
    }
}

/// On Ctrl+C, print `message` to stdout and exit with status 1 (the Python
/// CLI's `KeyboardInterrupt` handling). Prompts still report
/// [`PromptError::Interrupted`] while they wait for input.
pub fn install_cancel_handler(message: &str) {
    #[cfg(unix)]
    signals::install_cancel_handler(message);
    #[cfg(not(unix))]
    let _ = message;
}

/// Read a line from standard input without the trailing newline.
fn read_stdin_line() -> Result<String, PromptError> {
    #[cfg(unix)]
    if std::io::stdin().is_terminal() {
        return signals::read_line();
    }
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(0) | Err(_) => Err(PromptError::Eof),
        Ok(_) => {
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }
            Ok(line)
        }
    }
}

// =============================================================================
// Console
// =============================================================================

enum Target {
    Stdout,
    Capture { out: String, err: String },
}

/// Where output goes, how wide it is, and whether it is styled (rich's `Console`).
pub struct Console {
    target: Target,
    color: Option<ColorSystem>,
    width: usize,
    inputs: Option<VecDeque<Result<String, PromptError>>>,
    interactive: Option<bool>,
}

fn env_nonempty(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty())
}

/// Colour support of stdout, or `None` for plain output.
pub fn detect_color_system() -> Option<ColorSystem> {
    if env_nonempty("NO_COLOR") {
        return None;
    }
    if !env_nonempty("FORCE_COLOR") && !std::io::stdout().is_terminal() {
        return None;
    }
    let term = std::env::var("TERM").unwrap_or_default().trim().to_lowercase();
    if term == "dumb" || term == "unknown" {
        return None;
    }
    let colorterm = std::env::var("COLORTERM").unwrap_or_default().trim().to_lowercase();
    if colorterm == "truecolor" || colorterm == "24bit" {
        return Some(ColorSystem::TrueColor);
    }
    let colors = term.rsplit('-').next().unwrap_or("");
    Some(if colors == "256color" || colors == "kitty" { ColorSystem::EightBit } else { ColorSystem::Standard })
}

/// Width for rendering: `COLUMNS`, else the terminal's width, else 80 (as rich).
pub fn detect_width() -> usize {
    if let Ok(columns) = std::env::var("COLUMNS")
        && !columns.is_empty()
        && columns.chars().all(|c| c.is_ascii_digit())
        && let Ok(width) = columns.parse::<usize>()
        && width > 0
    {
        return width;
    }
    let any_terminal =
        std::io::stdout().is_terminal() || std::io::stdin().is_terminal() || std::io::stderr().is_terminal();
    if any_terminal
        && let Ok((columns, _)) = crossterm::terminal::size()
        && columns > 0
    {
        return usize::from(columns);
    }
    80
}

impl Console {
    /// Console for standard output (width and colour detected).
    pub fn stdout() -> Console {
        Console {
            target: Target::Stdout,
            color: detect_color_system(),
            width: detect_width(),
            inputs: None,
            interactive: None,
        }
    }

    /// Console that records plain output in memory (for tests).
    pub fn capture(width: usize) -> Console {
        Console {
            target: Target::Capture { out: String::new(), err: String::new() },
            color: None,
            width,
            inputs: None,
            interactive: None,
        }
    }

    /// Answer prompts from a script instead of standard input; the console
    /// then counts as interactive. Each item is a line or a [`PromptError`].
    pub fn with_inputs<I>(mut self, inputs: I) -> Console
    where
        I: IntoIterator<Item = Result<String, PromptError>>,
    {
        self.inputs = Some(inputs.into_iter().collect());
        self.interactive = Some(true);
        self
    }

    /// Override whether standard input counts as an interactive terminal.
    pub fn with_interactive(mut self, interactive: bool) -> Console {
        self.interactive = Some(interactive);
        self
    }

    /// Use a colour system (or `None` for plain output).
    pub fn with_color(mut self, color: Option<ColorSystem>) -> Console {
        self.color = color;
        self
    }

    /// Rendering width in cells.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Whether output is styled with escape codes.
    pub fn is_color(&self) -> bool {
        self.color.is_some()
    }

    /// Whether standard input is an interactive terminal (`sys.stdin.isatty()`).
    pub fn is_interactive(&self) -> bool {
        self.interactive.unwrap_or_else(|| std::io::stdin().is_terminal())
    }

    /// Captured standard output (empty for a real console).
    pub fn output(&self) -> &str {
        match &self.target {
            Target::Capture { out, .. } => out,
            Target::Stdout => "",
        }
    }

    /// Captured standard error (empty for a real console).
    pub fn error_output(&self) -> &str {
        match &self.target {
            Target::Capture { err, .. } => err,
            Target::Stdout => "",
        }
    }

    fn write_out(&mut self, text: &str) {
        match &mut self.target {
            Target::Stdout => {
                let mut stdout = std::io::stdout().lock();
                let _ = stdout.write_all(text.as_bytes());
            }
            Target::Capture { out, .. } => out.push_str(text),
        }
    }

    /// Flush standard output.
    pub fn flush(&mut self) {
        if let Target::Stdout = self.target {
            let _ = std::io::stdout().flush();
        }
    }

    fn line_string(&self, line: &Line) -> String {
        match self.color {
            Some(system) => line.iter().map(|s| s.style.paint(&s.text, system)).collect(),
            None => line.iter().map(|s| s.text.as_str()).collect(),
        }
    }

    fn write_lines(&mut self, lines: &[Line]) {
        let mut text = String::new();
        for line in lines {
            text.push_str(&self.line_string(line));
            text.push('\n');
        }
        self.write_out(&text);
    }

    /// Python's `print(text)`: the text as is, plus a newline.
    pub fn println(&mut self, text: &str) {
        self.write_out(&format!("{text}\n"));
    }

    /// Python's `print(text, file=sys.stderr)`.
    pub fn eprintln(&mut self, text: &str) {
        match &mut self.target {
            Target::Stdout => {
                let mut stderr = std::io::stderr().lock();
                let _ = writeln!(stderr, "{text}");
            }
            Target::Capture { err, .. } => {
                err.push_str(text);
                err.push('\n');
            }
        }
    }

    /// Parse markup the way `console.print(str)` does: with rich's default
    /// highlighting (numbers, quoted strings, ...) when output is coloured.
    pub fn markup(&self, markup: &str) -> Text {
        let mut text = Text::from_markup(markup);
        if self.color.is_some() {
            highlight(&mut text);
        }
        text
    }

    /// rich's `console.print(markup)`: markup, wrapped at the console width.
    pub fn print(&mut self, markup: &str) {
        let text = self.markup(markup);
        self.render(&text);
    }

    /// rich's `console.print(markup, justify=...)`.
    pub fn print_justified(&mut self, markup: &str, justify: Justify) {
        let text = self.markup(markup);
        self.render_justified(&text, justify);
    }

    /// Print a [`Text`] (no markup or highlighting).
    pub fn print_text(&mut self, text: &Text) {
        self.render(text);
    }

    /// Print a [`Text`] with a justification.
    pub fn print_text_justified(&mut self, text: &Text, justify: Justify) {
        self.render_justified(text, justify);
    }

    /// rich's `console.print()`: an empty line.
    pub fn newline(&mut self) {
        self.write_out("\n");
    }

    /// rich's `console.rule(style=...)`.
    pub fn rule(&mut self, style: &str) {
        self.render(&Rule::new(style));
    }

    /// Print any renderable (table, panel, aligned text, ...).
    pub fn render(&mut self, renderable: &dyn Renderable) {
        let lines = render_lines(renderable, &Options::width(self.width), Style::null(), false);
        self.write_lines(&lines);
    }

    fn render_justified(&mut self, renderable: &dyn Renderable, justify: Justify) {
        let options = Options { justify: Some(justify), ..Options::width(self.width) };
        let lines = render_lines(renderable, &options, Style::null(), false);
        self.write_lines(&lines);
    }

    /// Lines a renderable produces at this console's width, as plain or
    /// styled strings (for callers that assemble output themselves).
    pub fn render_to_strings(&self, renderable: &dyn Renderable) -> Vec<String> {
        render_lines(renderable, &Options::width(self.width), Style::null(), false)
            .iter()
            .map(|line| self.line_string(line))
            .collect()
    }

    fn read_input(&mut self) -> Result<String, PromptError> {
        match &mut self.inputs {
            Some(inputs) => {
                let answer = inputs.pop_front().unwrap_or(Err(PromptError::Eof));
                if let (Target::Capture { out, .. }, Ok(line)) = (&mut self.target, &answer) {
                    out.push_str(line);
                    out.push('\n');
                }
                answer
            }
            None => read_stdin_line(),
        }
    }

    /// rich's `console.input(prompt)`: print the prompt (markup) without a
    /// newline, then read a line.
    pub fn input(&mut self, prompt_markup: &str) -> Result<String, PromptError> {
        let text = self.markup(prompt_markup);
        let lines = render_lines(&text, &Options::width(self.width), Style::null(), false);
        let rendered: Vec<String> = lines.iter().map(|line| self.line_string(line)).collect();
        self.write_out(&rendered.join("\n"));
        self.flush();
        self.read_input()
    }

    /// Python's `input(prompt)`: plain prompt, then read a line.
    pub fn input_plain(&mut self, prompt: &str) -> Result<String, PromptError> {
        self.write_out(prompt);
        self.flush();
        self.read_input()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_plain(renderable: &dyn Renderable, width: usize) -> String {
        let mut console = Console::capture(width);
        console.render(renderable);
        console.output().to_string()
    }

    #[test]
    fn markup_parses_nested_and_closing_tags() {
        let text = Text::from_markup("[bold red]a[dim]b[/dim]c[/bold red]d [Errno 2] \\[x] [/]");
        assert_eq!(text.plain(), "abcd [Errno 2] [x] ");
        let segments = text.segments();
        assert_eq!(segments[0].text, "a");
        assert_eq!(segments[0].style, Style::get("bold red"));
        assert_eq!(segments[1].style, Style::get("bold red dim"));
        assert_eq!(segments[2].style, Style::get("bold red"));
        assert!(segments[3].style.is_null());
        // Unknown styles are consumed as tags and render unstyled.
        assert_eq!(Text::from_markup("[nosuchstyle]x[/nosuchstyle]").plain(), "x");
        // Escaped backslashes before a tag.
        assert_eq!(Text::from_markup("\\\\[bold]x[/bold]").plain(), "\\x");
    }

    #[test]
    fn styles_parse_and_combine() {
        let style = Style::parse("bold not italic on blue orange3").unwrap();
        assert_eq!(style.sgr(ColorSystem::EightBit), "1;38;5;172;44");
        assert_eq!(Style::get("bright_white").sgr(ColorSystem::Standard), "97");
        assert_eq!(Style::get("#ff0000").sgr(ColorSystem::TrueColor), "38;2;255;0;0");
        assert_eq!(Style::get("#ff0000").sgr(ColorSystem::EightBit), "38;5;196");
        assert_eq!(Style::get("orange3").sgr(ColorSystem::Standard), "33");
        assert!(Style::parse("bold nosuchcolor").is_none());
        let combined = Style::get("bold red").combine(Style::get("not bold green"));
        assert_eq!(combined.sgr(ColorSystem::Standard), "32");
        assert_eq!(Style::get("dim strike").paint("x", ColorSystem::Standard), "\x1b[2;9mx\x1b[0m");
    }

    #[test]
    fn wrapping_matches_rich() {
        let mut console = Console::capture(80);
        console.print(
            "No plan yet. Ask your AI agent to plan today with the Schedule Everything plugin, or apply a plan with `rmd sync --apply plan.json`.",
        );
        assert_eq!(
            console.output(),
            "No plan yet. Ask your AI agent to plan today with the Schedule Everything \nplugin, or apply a plan with `rmd sync --apply plan.json`.\n"
        );
        let mut console = Console::capture(10);
        console.print("12345 abcdefghijklmnopqrstuvwyxz 12345");
        assert_eq!(console.output(), "12345 \nabcdefghij\nklmnopqrst\nuvwyxz \n12345\n");
    }

    #[test]
    fn justification_pads_like_rich() {
        let mut console = Console::capture(20);
        console.print_justified("[dim]Total: 3[/dim]", Justify::Right);
        console.print_text_justified(&Text::styled("Title", "bold"), Justify::Center);
        console.render(&Align::center(Text::from("📅 Week")));
        assert_eq!(console.output(), "            Total: 3\n       Title        \n      📅 Week       \n");
    }

    #[test]
    fn cell_widths_match_rich() {
        for (text, width) in
            [("📅", 2), ("⏭️", 1), ("☀️", 1), ("⚠️", 1), ("☕", 2), ("■", 1), ("整理", 4), ("🏳️", 1), ("—", 1)]
        {
            assert_eq!(cell_len(text), width, "{text}");
        }
    }

    #[test]
    fn expanded_table_matches_rich_layout() {
        let mut table = Table::new()
            .title("[bold]Current Task List[/bold]")
            .box_style(BoxStyle::Rounded)
            .header_style("bold cyan")
            .expand(true);
        table.add_column(Column::new("ID").justify(Justify::Right).style("dim").width(4).no_wrap());
        table.add_column(Column::new("Priority").no_wrap());
        table.add_column(Column::new("Description"));
        let rows = [
            ("1", "███████░░░ (7)", "⏳ Fix CI pipeline (8 days overdue)"),
            ("2", "██████░░░░ (6)", "⏳ Call the bank"),
            ("3", "█████████░ (9)", "Write grant report"),
            ("5", "█████░░░░░ (5)", "整理笔记"),
            ("6", "████████░░ (8)", "💤 Book flights (coming in 2 days)"),
        ];
        for (id, priority, description) in rows {
            table.add_row([
                Text::from(id),
                Text::from(format!("[red]{priority}[/red]")),
                Text::styled(description, "bold red"),
            ]);
        }
        let expected = [
            "                                         Current Task List                                          ",
            "╭──────────┬──────────────────────────┬────────────────────────────────────────────────────────────╮",
            "│       ID │ Priority                 │ Description                                                │",
            "├──────────┼──────────────────────────┼────────────────────────────────────────────────────────────┤",
            "│        1 │ ███████░░░ (7)           │ ⏳ Fix CI pipeline (8 days overdue)                        │",
            "│        2 │ ██████░░░░ (6)           │ ⏳ Call the bank                                           │",
            "│        3 │ █████████░ (9)           │ Write grant report                                         │",
            "│        5 │ █████░░░░░ (5)           │ 整理笔记                                                   │",
            "│        6 │ ████████░░ (8)           │ 💤 Book flights (coming in 2 days)                         │",
            "╰──────────┴──────────────────────────┴────────────────────────────────────────────────────────────╯",
        ];
        assert_eq!(render_plain(&table, 100), expected.join("\n") + "\n");
    }

    #[test]
    fn compact_table_and_title_fit_content() {
        let mut table = Table::new().title("[bold]Today's focus blocks[/bold]").box_style(BoxStyle::Rounded);
        table.add_column(Column::new("Time").justify(Justify::Right).style("cyan").no_wrap());
        table.add_column(Column::new("Block"));
        table.add_column(Column::new("Planned task"));
        table.add_row(["08:30", "potato", "[dim]—[/dim]"]);
        table.add_row(["09:30", "pomodoro", "[dim]—[/dim]"]);
        let expected = [
            "       Today's focus blocks        ",
            "╭───────┬──────────┬──────────────╮",
            "│  Time │ Block    │ Planned task │",
            "├───────┼──────────┼──────────────┤",
            "│ 08:30 │ potato   │ —            │",
            "│ 09:30 │ pomodoro │ —            │",
            "╰───────┴──────────┴──────────────╯",
        ];
        assert_eq!(render_plain(&table, 100), expected.join("\n") + "\n");
    }

    #[test]
    fn narrow_table_wraps_the_flexible_column() {
        let mut table = Table::new().box_style(BoxStyle::Rounded).expand(true);
        table.add_column(Column::new("ID").justify(Justify::Right).width(4).no_wrap());
        table.add_column(Column::new("Description"));
        table.add_row(["1", "a fairly long description that will not fit"]);
        let rendered = render_plain(&table, 30);
        assert_eq!(
            rendered,
            [
                "╭──────┬─────────────────────╮",
                "│   ID │ Description         │",
                "├──────┼─────────────────────┤",
                "│    1 │ a fairly long       │",
                "│      │ description that    │",
                "│      │ will not fit        │",
                "╰──────┴─────────────────────╯",
                "",
            ]
            .join("\n")
        );
    }

    #[test]
    fn panel_with_title_fits_content_when_not_expanded() {
        let panel = Panel::new(Text::from("[bold green]🔔 NOW:[/bold green]  potato at 10:00\n\n⏰ NEXT: x"))
            .title("[bold]Status[/bold]")
            .padding(1, 2)
            .expand(false);
        let expected = [
            "╭────────── Status ──────────╮",
            "│                            │",
            "│  🔔 NOW:  potato at 10:00  │",
            "│                            │",
            "│  ⏰ NEXT: x                │",
            "│                            │",
            "╰────────────────────────────╯",
        ];
        assert_eq!(render_plain(&panel, 100), expected.join("\n") + "\n");
    }

    #[test]
    fn expanded_panel_centres_aligned_content() {
        let panel = Panel::new(Align::center(Text::from("hello"))).style("yellow");
        assert_eq!(render_plain(&panel, 13), "╭───────────╮\n│   hello   │\n╰───────────╯\n");
    }

    #[test]
    fn simple_head_table_with_sections() {
        let mut table = Table::new().box_style(BoxStyle::SimpleHead).expand(true);
        table.add_column(Column::new("Time").justify(Justify::Right).width(5).no_wrap());
        table.add_column(Column::new("Activity"));
        table.add_section();
        table.add_row(["🌅", "Morning"]);
        table.add_row(["08:30", "potato"]);
        table.add_section();
        table.add_row(["🌆", "Evening"]);
        let rendered = render_plain(&table, 24);
        let expected = [
            "                        ",
            "     Time   Activity    ",
            " ────────────────────── ",
            "       🌅   Morning     ",
            "    08:30   potato      ",
            "                        ",
            "       🌆   Evening     ",
            "                        ",
            "",
        ];
        assert_eq!(rendered, expected.join("\n"));
    }

    #[test]
    fn scripted_inputs_echo_into_the_capture() {
        let mut console = Console::capture(80).with_inputs([Ok("yes".to_string()), Err(PromptError::Interrupted)]);
        assert!(console.is_interactive());
        assert_eq!(console.input("[bold]Name? [/bold]"), Ok("yes".to_string()));
        assert_eq!(console.input_plain("Again: "), Err(PromptError::Interrupted));
        assert_eq!(console.input("More: "), Err(PromptError::Eof));
        assert_eq!(console.output(), "Name? yes\nAgain: More: ");
    }

    #[test]
    fn highlighting_colours_numbers_and_strings_only_with_colour() {
        let mut console = Console::capture(80).with_color(Some(ColorSystem::Standard));
        console.print("Task 'x' has 3 items, v2 stays");
        assert_eq!(console.output(), "Task \x1b[32m'x'\x1b[0m has \x1b[1;36m3\x1b[0m items, v2 stays\n");
        let mut plain = Console::capture(80);
        plain.print("Task 'x' has 3 items");
        assert_eq!(plain.output(), "Task 'x' has 3 items\n");
    }

    #[test]
    fn highlighting_follows_rich_word_rules() {
        let spans = |text: &str| {
            let chars: Vec<char> = text.chars().collect();
            highlight_spans(&chars)
                .into_iter()
                .map(|(s, e, k)| (chars[s..e].iter().collect::<String>(), k))
                .collect::<Vec<_>>()
        };
        assert_eq!(spans("Oops! Task type name can't be empty. Let's try"), vec![]);
        assert_eq!(
            spans("Unknown 'xyz' at 10:00 (in 30m)"),
            vec![("'xyz'".into(), "str"), ("10".into(), "number"), ("00".into(), "number")]
        );
        assert_eq!(
            spans("2026-11-09 v2 1.5 -3 True None"),
            vec![
                ("2026".into(), "number"),
                ("11".into(), "number"),
                ("09".into(), "number"),
                ("1.5".into(), "number"),
                ("-3".into(), "number"),
                ("True".into(), "bool_true"),
                ("None".into(), "none"),
            ]
        );
        assert_eq!(spans("see https://example.com/x now"), vec![("https://example.com/x".into(), "url")]);
    }

    #[test]
    fn ratio_helpers_match_rich() {
        assert_eq!(ratio_distribute(10, &[6, 20, 8]), vec![2, 6, 2]);
        assert_eq!(ratio_reduce(3, &[1, 1], &[5, 5], &[10, 10]), vec![8, 9]);
        assert_eq!(round_half_even(2.5), 2.0);
        assert_eq!(round_half_even(3.5), 4.0);
    }
}
