//! Reading the text of an SVG file, so it can come in as text layers.
//!
//! usvg is built without fonts, so it drops `<text>`; this reads the elements
//! straight from the XML. It takes what a title or a label needs: the words
//! (one line, or several by `<tspan>` lines), the font family, size and weight,
//! the fill colour, `text-anchor`, and the transform of the element and its
//! parents (where it stands, how big it is, how far it is turned). Text on a
//! path, text in `defs`, hidden text and text with no fill are left out and
//! counted. Nothing outside the file is read.

use fanimate_core::{Color, Vec2};
use usvg::roxmltree::{self, Node};

/// The most text elements read from one file.
pub const MAX_TEXTS: usize = 2_000;
/// The most characters kept of one text element.
pub const MAX_CHARS: usize = 20_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

/// One text element, in the page's units after the file's view box.
#[derive(Clone, Debug, PartialEq)]
pub struct SvgText {
    pub id: String,
    /// Lines joined with `\n`.
    pub content: String,
    /// Where the first line's baseline starts (or is centred on, or ends,
    /// by `anchor`), with every transform applied.
    pub at: Vec2,
    pub size: f32,
    /// The first family named that is not a generic one; empty if none.
    pub family: String,
    pub weight: f32,
    pub fill: Color,
    pub anchor: Anchor,
    /// The turn the element's transforms add up to, in radians, clockwise on screen.
    pub rotation: f32,
}

/// The text elements of `doc` and how many were left out. `page` is the size
/// usvg gave the drawing, which the view box is fitted into.
pub fn texts(doc: &roxmltree::Document, page: Vec2) -> (Vec<SvgText>, usize) {
    let view = view_transform(doc, page);
    let mut out = Vec::new();
    let mut skipped = 0;
    for node in doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "text")
    {
        if node.ancestors().any(|a| {
            a.is_element()
                && matches!(
                    a.tag_name().name(),
                    "defs" | "clipPath" | "mask" | "pattern" | "symbol" | "marker" | "filter"
                )
        }) {
            continue;
        }
        if out.len() >= MAX_TEXTS {
            skipped += 1;
            continue;
        }
        match read_text(node, &view) {
            Some(Some(t)) => out.push(t),
            Some(None) => {}
            None => skipped += 1,
        }
    }
    (out, skipped)
}

/// `Some(Some(text))` read, `Some(None)` nothing to show (hidden, empty, no
/// fill), `None` something Fanimate cannot do (text on a path).
fn read_text(node: Node, view: &Affine) -> Option<Option<SvgText>> {
    if node
        .descendants()
        .any(|d| d.is_element() && d.tag_name().name() == "textPath")
    {
        return None;
    }
    let hidden = prop(node, "display").is_some_and(|v| v == "none")
        || prop(node, "visibility").is_some_and(|v| v == "hidden" || v == "collapse");
    let fill = match prop(node, "fill").as_deref() {
        Some("none") => return Some(None),
        Some(v) => colour(v).unwrap_or(Color::BLACK),
        None => Color::BLACK,
    };
    if hidden {
        return Some(None);
    }
    let content = lines(node);
    if content.trim().is_empty() {
        return Some(None);
    }
    let mut m = Affine::IDENTITY;
    // parents first, so a child's transform acts inside its parent's
    let chain: Vec<Node> = node.ancestors().filter(|a| a.is_element()).collect();
    for a in chain.iter().rev() {
        if let Some(t) = a.attribute("transform") {
            m = m.then(&parse_transform(t));
        }
    }
    let m = view.then(&m);
    let x = number(node.attribute("x")).unwrap_or(0.0);
    let y = number(node.attribute("y")).unwrap_or(0.0);
    let (px, py) = m.apply(x, y);
    let scale = m.scale();
    let size = prop(node, "font-size").and_then(|v| length(&v)).unwrap_or(16.0) * scale;
    if !(px.is_finite() && py.is_finite() && size.is_finite()) || size <= 0.0 {
        return Some(None);
    }
    let family = prop(node, "font-family")
        .map(|f| {
            f.split(',')
                .map(|n| n.trim().trim_matches(['"', '\'']).trim().to_owned())
                .find(|n| {
                    !n.is_empty()
                        && !matches!(
                            n.to_ascii_lowercase().as_str(),
                            "sans-serif" | "serif" | "monospace" | "cursive" | "fantasy" | "system-ui"
                        )
                })
                .unwrap_or_default()
        })
        .unwrap_or_default();
    let weight = match prop(node, "font-weight").as_deref() {
        Some("bold") => 700.0,
        Some("bolder") => 800.0,
        Some("lighter") => 300.0,
        Some(v) => v.parse::<f32>().unwrap_or(400.0),
        None => 400.0,
    };
    let anchor = match prop(node, "text-anchor").as_deref() {
        Some("middle") => Anchor::Middle,
        Some("end") => Anchor::End,
        _ => Anchor::Start,
    };
    Some(Some(SvgText {
        id: node.attribute("id").unwrap_or_default().to_owned(),
        content: content.chars().take(MAX_CHARS).collect(),
        at: Vec2::new(px, py),
        size,
        family,
        weight,
        fill,
        anchor,
        rotation: m.rotation(),
    }))
}

/// The element's words: runs of white space become one space, and a
/// `<tspan>` that sets a new `y` or a `dy` (or an `x` after text) starts a line.
fn lines(node: Node) -> String {
    let mut lines: Vec<String> = vec![String::new()];
    for n in node.descendants().skip(1) {
        if n.is_text() {
            if let Some(t) = n.text() {
                lines.last_mut().expect("one line").push_str(t);
            }
        } else if n.is_element() && n.tag_name().name() == "tspan" {
            let moves = n.attribute("y").is_some()
                || n.attribute("dy")
                    .and_then(leading_number)
                    .is_some_and(|d| d != 0.0);
            if moves && !lines.last().expect("one line").trim().is_empty() {
                lines.push(String::new());
            }
        }
    }
    lines
        .iter()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(
            "
",
        )
}

/// The number at the start of a value, whatever its unit (`1.2em` is 1.2).
fn leading_number(v: &str) -> Option<f32> {
    let v = v.trim();
    // the longest prefix that is a number, so the e of 1.2em is not an exponent
    (1..=v.len())
        .rev()
        .filter(|end| v.is_char_boundary(*end))
        .find_map(|end| v[..end].parse::<f32>().ok())
        .filter(|n| n.is_finite())
}

/// A presentation property: the `style` attribute's declaration, else the
/// attribute, on the element or the nearest ancestor that has one.
fn prop(node: Node, name: &str) -> Option<String> {
    node.ancestors().filter(|a| a.is_element()).find_map(|a| {
        a.attribute("style")
            .and_then(|style| {
                style.split(';').find_map(|d| {
                    let (k, v) = d.split_once(':')?;
                    (k.trim() == name).then(|| v.trim().to_owned())
                })
            })
            .or_else(|| a.attribute(name).map(|v| v.trim().to_owned()))
    })
}

fn number(v: Option<&str>) -> Option<f32> {
    // a list of coordinates takes the first
    let first = v?.split([' ', ',']).find(|s| !s.is_empty())?;
    length(first)
}

/// `12`, `12px`, `9pt`, `1.5pc`, `2mm`, `1cm`, `1in`; other units are not understood.
fn length(v: &str) -> Option<f32> {
    let v = v.trim();
    let (end, n) = (1..=v.len())
        .rev()
        .filter(|end| v.is_char_boundary(*end))
        .find_map(|end| v[..end].parse::<f32>().ok().map(|n| (end, n)))?;
    let scale = match v[end..].trim() {
        "" | "px" => 1.0,
        "pt" => 96.0 / 72.0,
        "pc" => 16.0,
        "mm" => 96.0 / 25.4,
        "cm" => 96.0 / 2.54,
        "in" => 96.0,
        _ => return None,
    };
    let out = n * scale;
    out.is_finite().then_some(out)
}

fn colour(v: &str) -> Option<Color> {
    let v = v.trim();
    if let Some(inner) = v.strip_prefix("rgb(").and_then(|r| r.strip_suffix(')')) {
        let parts: Vec<f32> = inner
            .split([',', ' ', '/'])
            .filter(|s| !s.is_empty())
            .take(3)
            .map(|s| match s.strip_suffix('%') {
                Some(p) => p.parse::<f32>().map(|p| p * 2.55),
                None => s.parse::<f32>(),
            })
            .collect::<Result<_, _>>()
            .ok()?;
        return match parts[..] {
            [r, g, b] => Some(Color::rgb(
                (r / 255.0).clamp(0.0, 1.0),
                (g / 255.0).clamp(0.0, 1.0),
                (b / 255.0).clamp(0.0, 1.0),
            )),
            _ => None,
        };
    }
    if v.starts_with('#') {
        return Color::from_hex(v).ok();
    }
    Some(match v.to_ascii_lowercase().as_str() {
        "black" => Color::BLACK,
        "white" => Color::WHITE,
        "red" => Color::rgb(1.0, 0.0, 0.0),
        "green" => Color::rgb(0.0, 0.5, 0.0),
        "lime" => Color::rgb(0.0, 1.0, 0.0),
        "blue" => Color::rgb(0.0, 0.0, 1.0),
        "yellow" => Color::rgb(1.0, 1.0, 0.0),
        "orange" => Color::rgb(1.0, 0.647, 0.0),
        "purple" => Color::rgb(0.5, 0.0, 0.5),
        "gray" | "grey" => Color::rgb(0.5, 0.5, 0.5),
        "cyan" | "aqua" => Color::rgb(0.0, 1.0, 1.0),
        "magenta" | "fuchsia" => Color::rgb(1.0, 0.0, 1.0),
        _ => return None,
    })
}

/// A 2D affine map: x' = a x + c y + e, y' = b x + d y + f.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Affine {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}

impl Affine {
    const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// `self` after `inner`: a point goes through `inner`, then `self`.
    fn then(&self, inner: &Affine) -> Affine {
        // the map is "inner first", written as outer * inner
        Affine {
            a: self.a * inner.a + self.c * inner.b,
            b: self.b * inner.a + self.d * inner.b,
            c: self.a * inner.c + self.c * inner.d,
            d: self.b * inner.c + self.d * inner.d,
            e: self.a * inner.e + self.c * inner.f + self.e,
            f: self.b * inner.e + self.d * inner.f + self.f,
        }
    }

    fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }

    /// How much it scales lengths (the square root of its determinant).
    fn scale(&self) -> f32 {
        (self.a * self.d - self.b * self.c).abs().sqrt()
    }

    fn rotation(&self) -> f32 {
        self.b.atan2(self.a)
    }
}

/// Where the view box puts the drawing on a page of size `page`: scaled to
/// fit and centred, as the default `preserveAspectRatio` does.
fn view_transform(doc: &roxmltree::Document, page: Vec2) -> Affine {
    let Some(vb) = doc.root_element().attribute("viewBox") else {
        return Affine::IDENTITY;
    };
    let v: Vec<f32> = vb
        .split([' ', ','])
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    let [x, y, w, h] = v[..] else {
        return Affine::IDENTITY;
    };
    if !(w > 0.0 && h > 0.0 && page.x > 0.0 && page.y > 0.0) {
        return Affine::IDENTITY;
    }
    let s = (page.x / w).min(page.y / h);
    Affine {
        a: s,
        b: 0.0,
        c: 0.0,
        d: s,
        e: (page.x - w * s) / 2.0 - x * s,
        f: (page.y - h * s) / 2.0 - y * s,
    }
}

/// An SVG `transform` list: matrix, translate, scale, rotate, skewX, skewY.
fn parse_transform(list: &str) -> Affine {
    let mut m = Affine::IDENTITY;
    let mut rest = list.trim();
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim().trim_start_matches([',', ' ']);
        let Some(close) = rest[open..].find(')') else {
            break;
        };
        let args: Vec<f32> = rest[open + 1..open + close]
            .split([' ', ','])
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect();
        let t = match (name, &args[..]) {
            ("matrix", [a, b, c, d, e, f]) => Affine {
                a: *a,
                b: *b,
                c: *c,
                d: *d,
                e: *e,
                f: *f,
            },
            ("translate", [x]) => Affine {
                e: *x,
                ..Affine::IDENTITY
            },
            ("translate", [x, y]) => Affine {
                e: *x,
                f: *y,
                ..Affine::IDENTITY
            },
            ("scale", [s]) => Affine {
                a: *s,
                d: *s,
                ..Affine::IDENTITY
            },
            ("scale", [x, y]) => Affine {
                a: *x,
                d: *y,
                ..Affine::IDENTITY
            },
            ("rotate", [deg]) => rotation(deg.to_radians()),
            ("rotate", [deg, cx, cy]) => Affine {
                e: *cx,
                f: *cy,
                ..Affine::IDENTITY
            }
            .then(&rotation(deg.to_radians()))
            .then(&Affine {
                e: -*cx,
                f: -*cy,
                ..Affine::IDENTITY
            }),
            _ => Affine::IDENTITY,
        };
        // later transforms in the list act on the points first
        m = m.then(&t);
        rest = &rest[open + close + 1..];
    }
    if [m.a, m.b, m.c, m.d, m.e, m.f].iter().all(|v| v.is_finite()) {
        m
    } else {
        Affine::IDENTITY
    }
}

fn rotation(radians: f32) -> Affine {
    let (s, c) = radians.sin_cos();
    Affine {
        a: c,
        b: s,
        c: -s,
        d: c,
        e: 0.0,
        f: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(svg: &str, page: Vec2) -> (Vec<SvgText>, usize) {
        let doc = roxmltree::Document::parse(svg).unwrap();
        texts(&doc, page)
    }

    const PAGE: Vec2 = Vec2::new(200.0, 100.0);

    #[test]
    fn a_plain_text_is_read_with_its_place_size_and_colour() {
        let (t, skipped) = read(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
                <text id="title" x="10" y="40" font-size="24" font-family="'Inter', sans-serif"
                      font-weight="bold" fill="#ff0000" text-anchor="middle">  Hello
                   world </text></svg>"##,
            PAGE,
        );
        assert_eq!(skipped, 0);
        assert_eq!(t.len(), 1);
        let t = &t[0];
        assert_eq!((t.id.as_str(), t.content.as_str()), ("title", "Hello world"));
        assert_eq!(t.at, Vec2::new(10.0, 40.0));
        assert_eq!((t.size, t.family.as_str(), t.weight), (24.0, "Inter", 700.0));
        assert_eq!(t.fill, Color::rgb(1.0, 0.0, 0.0));
        assert_eq!(t.anchor, Anchor::Middle);
    }

    #[test]
    fn the_view_box_and_parent_transforms_scale_move_and_turn_it() {
        let (t, _) = read(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" viewBox="0 0 100 50">
                <g transform="translate(10 5) rotate(90)">
                  <text x="2" y="3" font-size="10">A</text></g></svg>"#,
            PAGE,
        );
        let t = &t[0];
        // (2,3) turned a quarter, moved by (10,5) = (7,7), then the view box doubles it
        assert!(
            (t.at.x - 14.0).abs() < 1e-3 && (t.at.y - 14.0).abs() < 1e-3,
            "{:?}",
            t.at
        );
        assert!((t.size - 20.0).abs() < 1e-3, "the size doubles: {}", t.size);
        assert!((t.rotation - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
    }

    #[test]
    fn style_declarations_and_inherited_properties_are_read() {
        let (t, _) = read(
            r##"<svg xmlns="http://www.w3.org/2000/svg">
                <g fill="blue" font-size="12pt" style="font-family: Georgia, serif">
                  <text x="0" y="0" style="fill:rgb(0,255,0);font-weight:600">x</text>
                  <text x="0" y="0">y</text></g></svg>"##,
            PAGE,
        );
        assert_eq!(t[0].fill, Color::rgb(0.0, 1.0, 0.0));
        assert_eq!(t[0].weight, 600.0);
        assert_eq!(t[0].family, "Georgia");
        assert!((t[0].size - 16.0).abs() < 1e-3, "12pt is 16 px");
        assert_eq!(t[1].fill, Color::rgb(0.0, 0.0, 1.0), "inherited from the group");
    }

    #[test]
    fn tspans_that_move_down_are_lines() {
        let (t, _) = read(
            r#"<svg xmlns="http://www.w3.org/2000/svg"><text x="0" y="20">
                <tspan x="0" dy="0">First</tspan><tspan x="0" dy="1.2em">Second</tspan>
                <tspan> and more</tspan></text></svg>"#,
            PAGE,
        );
        assert_eq!(t[0].content, "First\nSecond and more");
    }

    #[test]
    fn hidden_empty_unpainted_and_defs_text_is_not_imported_and_text_on_a_path_is_counted() {
        let (t, skipped) = read(
            r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">
                <defs><text>in defs</text></defs>
                <text display="none">hidden</text>
                <text visibility="hidden">hidden</text>
                <text fill="none">no paint</text>
                <text>   </text>
                <text><textPath xlink:href="#p">on a path</textPath></text>
                <text>kept</text></svg>"##,
            PAGE,
        );
        assert_eq!(t.iter().map(|t| t.content.as_str()).collect::<Vec<_>>(), ["kept"]);
        assert_eq!(skipped, 1, "only the text on a path is a loss");
    }

    #[test]
    fn hostile_values_never_panic_or_make_something_that_is_not_a_number() {
        for svg in [
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 0 0"><text x="1e999" y="NaN" font-size="-5">x</text></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="a b c d"><text transform="matrix(1e38 0 0 1e38 0 0)" font-size="1e38">x</text></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><text transform="rotate(" font-size="12px 3">x</text></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><text style=";;:::" fill="rgb(1,2)" font-weight="heavy">x</text></svg>"#,
        ] {
            let (t, _) = read(svg, PAGE);
            for t in &t {
                assert!(t.at.is_finite() && t.size.is_finite() && t.size > 0.0, "{t:?}");
                assert!(t.rotation.is_finite());
            }
        }
        // and a huge file of text stops at the limit
        let mut svg = String::from(r#"<svg xmlns="http://www.w3.org/2000/svg">"#);
        for _ in 0..MAX_TEXTS + 5 {
            svg.push_str("<text>a</text>");
        }
        svg.push_str("</svg>");
        let (t, skipped) = read(&svg, PAGE);
        assert_eq!((t.len(), skipped), (MAX_TEXTS, 5));
    }

    #[test]
    fn transform_lists_compose_in_order() {
        let m = parse_transform("translate(10, 0) scale(2)");
        // a point is scaled, then moved
        assert_eq!(m.apply(1.0, 1.0), (12.0, 2.0));
        let r = parse_transform("rotate(90 10 10)");
        let (x, y) = r.apply(20.0, 10.0);
        assert!((x - 10.0).abs() < 1e-4 && (y - 20.0).abs() < 1e-4, "{x} {y}");
        assert_eq!(parse_transform("nonsense(1 2)"), Affine::IDENTITY);
        assert_eq!(parse_transform(""), Affine::IDENTITY);
    }
}
