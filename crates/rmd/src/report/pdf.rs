//! Vector drawing shared by the PDF reports and the schedule visualization.
//!
//! The Python code drew its pages with matplotlib. Here every page is laid out
//! as a small SVG document (coordinates in PDF points, origin top-left),
//! parsed by `usvg` against the system fonts and written into a multi-page PDF
//! with `krilla`. Text stays selectable, and characters missing from the main
//! font (Chinese task or habit names, symbols) fall back to CJK and symbol
//! fonts. Missing fonts never panic: text that cannot be shaped is skipped.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use krilla::Document;
use krilla::geom::Size;
use krilla::metadata::Metadata;
use krilla::page::PageSettings;
use krilla_svg::{SurfaceExt, SvgSettings};
use unicode_width::UnicodeWidthChar;
use usvg::fontdb::{Database, Family, ID, Query, Stretch, Style, Weight};

/// matplotlib's A4 landscape figure (`figsize=(11.69, 8.27)`), in points.
pub(crate) const A4_LANDSCAPE: (f32, f32) = (11.69 * 72.0, 8.27 * 72.0);
/// matplotlib's A4 portrait figure (`figsize=(8.27, 11.69)`), in points.
pub(crate) const A4_PORTRAIT: (f32, f32) = (8.27 * 72.0, 11.69 * 72.0);

/// matplotlib font weights.
pub(crate) const MEDIUM: u16 = 500;
pub(crate) const BOLD: u16 = 700;
pub(crate) const HEAVY: u16 = 800;

/// Latin text: the visualizer asked matplotlib for Arial/Helvetica/DejaVu Sans.
const SANS_FAMILIES: &[&str] = &[
    "Helvetica Neue",
    "Helvetica",
    "Arial",
    "DejaVu Sans",
    "Liberation Sans",
    "Noto Sans",
    "Nimbus Sans",
    "FreeSans",
    "Roboto",
    "Ubuntu",
    "Cantarell",
    "Segoe UI",
];

const MONO_FAMILIES: &[&str] =
    &["Menlo", "SF Mono", "Monaco", "DejaVu Sans Mono", "Liberation Mono", "Noto Sans Mono", "Consolas", "Courier New"];

/// Tried in order for characters the main font lacks: Chinese first (task and
/// habit names), then broad-coverage, symbol and emoji fonts.
const FALLBACK_FAMILIES: &[&str] = &[
    "PingFang SC",
    "Hiragino Sans GB",
    "Heiti SC",
    "STHeiti",
    "Noto Sans CJK SC",
    "Noto Sans SC",
    "Source Han Sans SC",
    "Source Han Sans CN",
    "WenQuanYi Zen Hei",
    "WenQuanYi Micro Hei",
    "Droid Sans Fallback",
    "Microsoft YaHei",
    "SimHei",
    "Arial Unicode MS",
    "DejaVu Sans",
    "Apple Symbols",
    "Noto Sans Symbols",
    "Noto Sans Symbols 2",
    "Symbola",
    "Segoe UI Symbol",
    "Apple Color Emoji",
    "Noto Color Emoji",
    "Segoe UI Emoji",
];

/// System fonts, scanned once per process.
fn system_fonts() -> Arc<Database> {
    static DB: OnceLock<Arc<Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = Database::new();
        db.load_system_fonts();
        Arc::new(db)
    })
    .clone()
}

fn has_family(db: &Database, name: &str) -> bool {
    db.faces().any(|face| face.families.iter().any(|(family, _)| family == name))
}

fn face_has_char(db: &Database, id: ID, c: char) -> bool {
    db.with_face_data(id, |data, index| {
        ttf_parser::Face::parse(data, index).ok().and_then(|face| face.glyph_index(c)).is_some()
    })
    .unwrap_or(false)
}

/// First preferred family installed, else any family that has Latin letters.
fn pick_family(db: &Database, preferred: &[&str]) -> Option<String> {
    if let Some(name) = preferred.iter().find(|name| has_family(db, name)) {
        return Some((*name).to_string());
    }
    db.faces()
        .filter(|face| face.style == Style::Normal && !face.families.is_empty())
        .find(|face| face_has_char(db, face.id, 'A'))
        .map(|face| face.families[0].0.clone())
}

type FallbackCache = Arc<Mutex<HashMap<(char, Vec<ID>), Option<ID>>>>;

/// usvg fallback: the preferred families first (same weight as the main
/// font), then usvg's default scan over every installed face.
fn fallback_selector(preferred: Arc<[String]>, cache: FallbackCache) -> usvg::FallbackSelectionFn<'static> {
    let scan_all = usvg::FontResolver::default_fallback_selector();
    Box::new(move |c, used, db| {
        let key = (c, used.to_vec());
        if let Some(found) = cache.lock().ok().and_then(|cache| cache.get(&key).copied()) {
            return found;
        }
        let base = used.first().and_then(|id| db.face(*id));
        let weight = base.map_or(Weight::NORMAL, |face| face.weight);
        let style = base.map_or(Style::Normal, |face| face.style);
        let mut found = preferred.iter().find_map(|family| {
            let families = [Family::Name(family)];
            let query = Query { families: &families, weight, stretch: Stretch::Normal, style };
            db.query(&query).filter(|id| !used.contains(id) && face_has_char(db, *id, c))
        });
        if found.is_none() && !used.is_empty() {
            found = scan_all(c, used, db);
        }
        if let Ok(mut cache) = cache.lock() {
            cache.insert(key, found);
        }
        found
    })
}

/// Horizontal advances of one font face, in ems, read as characters are
/// measured (`None`: the face has no glyph for the character).
struct FaceMetrics {
    notdef: f32,
    advances: HashMap<char, Option<f32>>,
}

/// Fonts for one document, with the metrics used to fit text into place.
pub(crate) struct Fonts {
    db: Arc<Database>,
    sans: String,
    mono: String,
    fallbacks: Arc<[String]>,
    fallback_cache: FallbackCache,
    select_fallback: usvg::FallbackSelectionFn<'static>,
    metrics: RefCell<HashMap<ID, Option<FaceMetrics>>>,
}

impl Fonts {
    /// Resolve the families to draw with from the installed system fonts.
    pub(crate) fn load() -> Self {
        Self::with_database(system_fonts())
    }

    /// Resolve the families to draw with from `db`.
    pub(crate) fn with_database(db: Arc<Database>) -> Self {
        let sans = pick_family(&db, SANS_FAMILIES).unwrap_or_else(|| "sans-serif".to_string());
        let mono = pick_family(&db, MONO_FAMILIES).unwrap_or_else(|| sans.clone());
        let fallbacks: Arc<[String]> =
            FALLBACK_FAMILIES.iter().filter(|name| has_family(&db, name)).map(|name| (*name).to_string()).collect();
        let fallback_cache = FallbackCache::default();
        let select_fallback = fallback_selector(fallbacks.clone(), fallback_cache.clone());
        Self { db, sans, mono, fallbacks, fallback_cache, select_fallback, metrics: RefCell::default() }
    }

    fn options(&self) -> usvg::Options<'static> {
        usvg::Options {
            fontdb: self.db.clone(),
            font_family: self.sans.clone(),
            font_resolver: usvg::FontResolver {
                select_font: usvg::FontResolver::default_font_selector(),
                select_fallback: fallback_selector(self.fallbacks.clone(), self.fallback_cache.clone()),
            },
            ..usvg::Options::default()
        }
    }

    fn family(&self, mono: bool) -> &str {
        if mono { &self.mono } else { &self.sans }
    }

    /// Advances of `chars` in the face `id`, in ems (`None` if unreadable).
    fn em_advances(&self, id: ID, chars: &[char]) -> Option<Vec<Option<f32>>> {
        let mut metrics = self.metrics.borrow_mut();
        let face = metrics
            .entry(id)
            .or_insert_with(|| {
                self.db.with_face_data(id, |data, index| {
                    let face = ttf_parser::Face::parse(data, index).ok()?;
                    let units = f32::from(face.units_per_em().max(1));
                    let notdef = f32::from(face.glyph_hor_advance(ttf_parser::GlyphId(0)).unwrap_or(0)) / units;
                    Some(FaceMetrics { notdef, advances: HashMap::new() })
                })?
            })
            .as_mut()?;
        let unknown: Vec<char> = chars.iter().copied().filter(|c| !face.advances.contains_key(c)).collect();
        if !unknown.is_empty() {
            self.db.with_face_data(id, |data, index| {
                let Ok(parsed) = ttf_parser::Face::parse(data, index) else {
                    return;
                };
                let units = f32::from(parsed.units_per_em().max(1));
                for c in unknown {
                    let advance =
                        parsed.glyph_index(c).map(|g| f32::from(parsed.glyph_hor_advance(g).unwrap_or(0)) / units);
                    face.advances.insert(c, advance);
                }
            });
        }
        Some(chars.iter().map(|c| face.advances.get(c).copied().flatten()).collect())
    }

    /// Width from the font metrics, choosing fonts the way usvg does when it
    /// lays the text out: the main face, then fallbacks for missing glyphs
    /// (a fallback that has every glyph takes over the whole text).
    fn measure(&self, text: &str, style: &TextStyle) -> Option<f32> {
        let family = self.family(style.mono);
        let named = [Family::Name(family), Family::Serif];
        let generic = [Family::SansSerif, Family::Serif];
        let families = if family == "sans-serif" { &generic } else { &named };
        let query = Query { families, weight: Weight(style.weight), stretch: Stretch::Normal, style: Style::Normal };
        let base = self.db.query(&query)?;
        let chars: Vec<char> = text.chars().collect();
        let mut advances = self.em_advances(base, &chars)?;
        let mut used = vec![base];
        let mut db = self.db.clone();
        while let Some(missing) = advances.iter().position(Option::is_none) {
            let Some(fallback) = (self.select_fallback)(chars[missing], &used, &mut db) else {
                break;
            };
            let Some(alternative) = self.em_advances(fallback, &chars) else {
                break;
            };
            if alternative.iter().all(Option::is_some) {
                advances = alternative;
                break;
            }
            for (advance, other) in advances.iter_mut().zip(alternative) {
                if advance.is_none() {
                    *advance = other;
                }
            }
            used.push(fallback);
        }
        let notdef = self.metrics.borrow().get(&base).and_then(|m| m.as_ref().map(|m| m.notdef)).unwrap_or(0.0);
        Some(advances.iter().map(|advance| advance.unwrap_or(notdef)).sum::<f32>() * style.size)
    }

    /// Advance width of `text` in points, as laid out on the page.
    pub(crate) fn width(&self, text: &str, style: &TextStyle) -> f32 {
        let text = clean_text(text);
        if text.is_empty() {
            return 0.0;
        }
        self.measure(&text, style).unwrap_or_else(|| estimate_width(&text, style.size))
    }

    /// `text` shortened with an ellipsis so it fits in `max_width`.
    pub(crate) fn fit(&self, text: &str, max_width: f32, style: &TextStyle) -> String {
        let text = clean_text(text);
        if self.width(&text, style) <= max_width {
            return text;
        }
        let chars: Vec<char> = text.chars().collect();
        let candidate = |n: usize| {
            let prefix: String = chars[..n].iter().collect();
            format!("{}…", prefix.trim_end())
        };
        let n = self.longest_fitting(chars.len(), max_width, style, &candidate);
        if n == 0 && self.width("…", style) > max_width { String::new() } else { candidate(n) }
    }

    /// Wrap `text` onto at most `max_lines` lines of `max_width`, breaking at
    /// spaces where possible; the last line ends with an ellipsis if cut.
    pub(crate) fn wrap(&self, text: &str, max_width: f32, max_lines: usize, style: &TextStyle) -> Vec<String> {
        let mut rest = clean_text(text).trim().to_string();
        let mut lines = Vec::new();
        while !rest.is_empty() && lines.len() < max_lines {
            if lines.len() + 1 == max_lines || self.width(&rest, style) <= max_width {
                lines.push(self.fit(&rest, max_width, style));
                break;
            }
            let chars: Vec<char> = rest.chars().collect();
            let mut n =
                self.longest_fitting(chars.len(), max_width, style, &|n| chars[..n].iter().collect::<String>()).max(1);
            // Prefer breaking after a space unless the text is CJK (breakable anywhere).
            let breaks_inside_word = n < chars.len() && !chars[n].is_whitespace() && !is_wide(chars[n - 1]);
            if breaks_inside_word && let Some(space) = chars[..n].iter().rposition(|c| c.is_whitespace()) {
                n = space.max(1);
            }
            lines.push(chars[..n].iter().collect::<String>().trim_end().to_string());
            rest = chars[n..].iter().collect::<String>().trim_start().to_string();
        }
        lines
    }

    /// Largest `n <= len` whose `candidate(n)` fits in `max_width` (binary search).
    fn longest_fitting(
        &self,
        len: usize,
        max_width: f32,
        style: &TextStyle,
        candidate: &dyn Fn(usize) -> String,
    ) -> usize {
        let (mut lo, mut hi) = (0, len);
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if self.width(&candidate(mid), style) <= max_width {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        lo
    }
}

fn is_wide(c: char) -> bool {
    c.width().unwrap_or(0) > 1
}

/// Rough width when no font could lay the text out.
fn estimate_width(text: &str, size: f32) -> f32 {
    text.chars().map(|c| c.width().unwrap_or(0) as f32 * 0.55 * size).sum()
}

/// Control characters (newlines, tabs) become spaces: every label is one line.
fn clean_text(text: &str) -> String {
    text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() => out.push(' '),
            // Not allowed in XML documents.
            '\u{FFFE}' | '\u{FFFF}' => out.push('\u{FFFD}'),
            c => out.push(c),
        }
    }
    out
}

/// Horizontal text alignment (matplotlib `ha`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Anchor {
    Start,
    Middle,
    End,
}

/// Vertical text alignment: `y` is the baseline, or the middle of the text
/// (matplotlib `va="center"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Baseline {
    Alphabetic,
    Central,
}

/// Font and placement of a piece of text.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextStyle<'c> {
    pub size: f32,
    pub weight: u16,
    pub color: &'c str,
    pub anchor: Anchor,
    pub baseline: Baseline,
    pub mono: bool,
}

impl TextStyle<'static> {
    /// Regular black left-aligned text at `size` points.
    pub(crate) const fn new(size: f32) -> Self {
        Self { size, weight: 400, color: "#000000", anchor: Anchor::Start, baseline: Baseline::Alphabetic, mono: false }
    }
}

impl<'c> TextStyle<'c> {
    pub(crate) const fn weight(self, weight: u16) -> Self {
        Self { weight, ..self }
    }

    pub(crate) const fn bold(self) -> Self {
        self.weight(BOLD)
    }

    pub(crate) const fn color<'d>(self, color: &'d str) -> TextStyle<'d> {
        TextStyle {
            color,
            size: self.size,
            weight: self.weight,
            anchor: self.anchor,
            baseline: self.baseline,
            mono: self.mono,
        }
    }

    pub(crate) const fn anchor(self, anchor: Anchor) -> Self {
        Self { anchor, ..self }
    }

    /// Centered horizontally.
    pub(crate) const fn centered(self) -> Self {
        self.anchor(Anchor::Middle)
    }

    /// `y` is the vertical middle of the text.
    pub(crate) const fn middle(self) -> Self {
        Self { baseline: Baseline::Central, ..self }
    }

    pub(crate) const fn mono(self) -> Self {
        Self { mono: true, ..self }
    }
}

fn write_text_element(out: &mut String, family: &str, x: f32, y: f32, text: &str, style: &TextStyle) {
    let anchor = match style.anchor {
        Anchor::Start => "start",
        Anchor::Middle => "middle",
        Anchor::End => "end",
    };
    let baseline = match style.baseline {
        Baseline::Alphabetic => "alphabetic",
        Baseline::Central => "central",
    };
    // A generic family stays unquoted; quoted, it would name a font.
    let family = if family == "sans-serif" {
        family.to_string()
    } else {
        format!("'{}'", xml_escape(&family.replace('\'', "")))
    };
    let _ = write!(
        out,
        r#"<text x="{x:.2}" y="{y:.2}" font-family="{family}" font-size="{size}" font-weight="{weight}" fill="{color}" text-anchor="{anchor}" dominant-baseline="{baseline}" xml:space="preserve">{text}</text>"#,
        size = style.size,
        weight = style.weight,
        color = style.color,
        text = xml_escape(text),
    );
}

/// A line's color, width, opacity and dash pattern.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Stroke<'c> {
    pub color: &'c str,
    pub width: f32,
    pub opacity: f32,
    pub dash: Option<&'c str>,
}

impl<'c> Stroke<'c> {
    pub(crate) const fn new(color: &'c str, width: f32) -> Self {
        Self { color, width, opacity: 1.0, dash: None }
    }

    pub(crate) const fn opacity(self, opacity: f32) -> Self {
        Self { opacity, ..self }
    }

    pub(crate) const fn dashed(self, dash: &'c str) -> Self {
        Self { dash: Some(dash), ..self }
    }

    fn attributes(&self) -> String {
        let mut out = format!(
            r#" fill="none" stroke="{}" stroke-width="{}" stroke-opacity="{}""#,
            self.color, self.width, self.opacity
        );
        if let Some(dash) = self.dash {
            let _ = write!(out, r#" stroke-dasharray="{dash}""#);
        }
        out
    }
}

/// One page being drawn: SVG elements in points, origin top-left.
pub(crate) struct Canvas<'f> {
    fonts: &'f Fonts,
    pub width: f32,
    pub height: f32,
    body: String,
}

impl<'f> Canvas<'f> {
    pub(crate) fn new(fonts: &'f Fonts, (width, height): (f32, f32)) -> Self {
        Self { fonts, width, height, body: String::new() }
    }

    pub(crate) fn fonts(&self) -> &'f Fonts {
        self.fonts
    }

    /// Filled rectangle.
    pub(crate) fn rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: &str, opacity: f32) {
        self.rounded_rect(x, y, width, height, 0.0, color, opacity);
    }

    /// Filled rectangle with rounded corners.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn rounded_rect(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        color: &str,
        opacity: f32,
    ) {
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        let _ = write!(
            self.body,
            r#"<rect x="{x:.2}" y="{y:.2}" width="{width:.2}" height="{height:.2}" rx="{radius:.2}" fill="{color}" fill-opacity="{opacity}"/>"#
        );
    }

    /// Filled circle.
    pub(crate) fn circle(&mut self, cx: f32, cy: f32, radius: f32, color: &str, opacity: f32) {
        self.ellipse(cx, cy, radius, radius, color, opacity);
    }

    /// Filled ellipse.
    pub(crate) fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, color: &str, opacity: f32) {
        if rx <= 0.0 || ry <= 0.0 {
            return;
        }
        let _ = write!(
            self.body,
            r#"<ellipse cx="{cx:.2}" cy="{cy:.2}" rx="{rx:.2}" ry="{ry:.2}" fill="{color}" fill-opacity="{opacity}"/>"#
        );
    }

    /// Straight line.
    pub(crate) fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, stroke: Stroke) {
        let _ =
            write!(self.body, r#"<line x1="{x1:.2}" y1="{y1:.2}" x2="{x2:.2}" y2="{y2:.2}"{}/>"#, stroke.attributes());
    }

    /// Open polyline with round joins (check marks).
    pub(crate) fn polyline(&mut self, points: &[(f32, f32)], stroke: Stroke) {
        let points: Vec<String> = points.iter().map(|(x, y)| format!("{x:.2},{y:.2}")).collect();
        let _ = write!(
            self.body,
            r#"<polyline points="{}" stroke-linecap="round" stroke-linejoin="round"{}/>"#,
            points.join(" "),
            stroke.attributes()
        );
    }

    /// Single-line text at (`x`, `y`).
    pub(crate) fn text(&mut self, x: f32, y: f32, text: &str, style: &TextStyle) {
        if text.is_empty() {
            return;
        }
        write_text_element(&mut self.body, self.fonts.family(style.mono), x, y, text, style);
    }

    fn to_svg(&self) -> String {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">{body}</svg>"#,
            w = self.width,
            h = self.height,
            body = self.body
        )
    }
}

/// Write `pages` as one PDF file at `path`.
pub(crate) fn write_pdf(path: &Path, title: &str, pages: &[Canvas]) -> anyhow::Result<()> {
    // Embedded (selectable) text first; if a font cannot be embedded, draw
    // the glyphs as outlines instead.
    let bytes = match render_pdf(title, pages, true) {
        Ok(bytes) => bytes,
        Err(_) => render_pdf(title, pages, false)?,
    };
    std::fs::write(path, bytes).map_err(|e| super::os_error(e, path))?;
    Ok(())
}

fn render_pdf(title: &str, pages: &[Canvas], embed_text: bool) -> anyhow::Result<Vec<u8>> {
    let mut document = Document::new();
    document.set_metadata(Metadata::new().title(title.to_string()).creator("Schedule Everything (rmd)".to_string()));
    for canvas in pages {
        let tree = usvg::Tree::from_str(&canvas.to_svg(), &canvas.fonts.options())
            .map_err(|e| anyhow::anyhow!("could not lay out the PDF page: {e}"))?;
        let size = Size::from_wh(canvas.width, canvas.height)
            .ok_or_else(|| anyhow::anyhow!("invalid page size {}x{}", canvas.width, canvas.height))?;
        let mut page = document.start_page_with(PageSettings::new(size));
        let mut surface = page.surface();
        surface.draw_svg(&tree, size, SvgSettings { embed_text, ..SvgSettings::default() });
        surface.finish();
        page.finish();
    }
    document.finish().map_err(|e| anyhow::anyhow!("could not write the PDF: {e:?}"))
}
