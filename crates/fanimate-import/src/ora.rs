//! OpenRaster (`.ora`), the layered picture format Krita, MyPaint, GIMP and
//! others save: a ZIP holding `stack.xml`, which lists the layers, and one PNG
//! per layer.
//!
//! This reads the stack into a tree of layers and groups with each layer's
//! picture, position, opacity and visibility, and leaves building Fanimate
//! layers to the editor. What a layer can say that Fanimate cannot show is
//! counted in [`Ora::notes`] rather than dropped silently: blend modes other
//! than normal, and layers whose picture is missing or cannot be read.
//! Never panics: a damaged archive, a hostile stack (too deep, too many
//! layers) or a picture that is too large is an error or a note.
//!
//! Layers come out bottom first, the order Fanimate draws in (the file lists
//! the top layer first).

use image::ImageReader;

use crate::Notes;
use crate::zip::Archive;

/// The most layers one file may have.
pub const MAX_LAYERS: usize = 500;
/// How deeply groups may nest.
const MAX_DEPTH: usize = 16;
/// The largest `stack.xml`, in bytes.
const MAX_STACK: usize = 4 * 1024 * 1024;
/// The largest picture side and pixel count of one layer.
const MAX_SIDE: u32 = 16_384;
const MAX_PIXELS: u64 = 100_000_000;

const BLEND: (&str, &str) = (
    "layer with a blend mode shown as normal",
    "layers with blend modes shown as normal",
);
const MISSING: (&str, &str) = (
    "layer left out (its picture is missing or unreadable)",
    "layers left out (their pictures are missing or unreadable)",
);
const TOO_MANY: (&str, &str) = (
    "layer left out (too many layers)",
    "layers left out (too many layers)",
);

/// One picture layer.
#[derive(Debug, Clone, PartialEq)]
pub struct OraLayer {
    pub name: String,
    /// The PNG file as it was in the archive.
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Where the layer's top left corner is in the image.
    pub x: i32,
    pub y: i32,
    pub opacity: f32,
    pub visible: bool,
}

/// A layer or a group of them.
#[derive(Debug, Clone, PartialEq)]
pub enum OraNode {
    Layer(OraLayer),
    Group {
        name: String,
        opacity: f32,
        visible: bool,
        /// Bottom first.
        children: Vec<OraNode>,
    },
}

/// An OpenRaster file, read.
#[derive(Debug)]
pub struct Ora {
    /// The image's size.
    pub width: u32,
    pub height: u32,
    /// The top level, bottom first.
    pub nodes: Vec<OraNode>,
    pub notes: Notes,
    /// How many picture layers there are, groups not counted.
    pub layers: usize,
}

/// Read an `.ora` file.
pub fn parse(bytes: &[u8]) -> Result<Ora, String> {
    if !crate::zip::is_zip(bytes) {
        return Err("that is not an OpenRaster file (it is not a ZIP archive)".to_owned());
    }
    let archive = Archive::read(bytes)?;
    if archive.size("stack.xml").is_none_or(|n| n > MAX_STACK) {
        return Err("the OpenRaster file has no readable stack.xml".to_owned());
    }
    let xml = archive
        .get("stack.xml")
        .and_then(Result::ok)
        .ok_or("the OpenRaster file has no readable stack.xml")?;
    let xml = String::from_utf8(xml).map_err(|_| "stack.xml is not text".to_owned())?;
    let options = roxmltree::ParsingOptions {
        allow_dtd: false,
        nodes_limit: 100_000,
        ..Default::default()
    };
    let doc = roxmltree::Document::parse_with_options(&xml, options)
        .map_err(|e| format!("stack.xml could not be read: {e}"))?;
    let image = doc.root_element();
    if image.tag_name().name() != "image" {
        return Err("stack.xml is not an OpenRaster image".to_owned());
    }
    let side = |name: &str| {
        image
            .attribute(name)
            .and_then(|v| v.trim().parse::<u32>().ok())
            .filter(|n| (1..=MAX_SIDE).contains(n))
    };
    let (Some(width), Some(height)) = (side("w"), side("h")) else {
        return Err(format!(
            "the image size in stack.xml is missing or not between 1 and {MAX_SIDE}"
        ));
    };
    let mut reader = Reader {
        archive: &archive,
        notes: Notes::default(),
        layers: 0,
    };
    let root = image
        .children()
        .find(|c| c.is_element() && c.tag_name().name() == "stack")
        .ok_or("stack.xml has no layers")?;
    let nodes = reader.children(root, 1);
    if reader.layers == 0 {
        return Err("the OpenRaster file has no layers that could be read".to_owned());
    }
    Ok(Ora {
        width,
        height,
        nodes,
        notes: reader.notes,
        layers: reader.layers,
    })
}

struct Reader<'a> {
    archive: &'a Archive<'a>,
    notes: Notes,
    layers: usize,
}

fn number(node: roxmltree::Node<'_, '_>, name: &str) -> Option<f64> {
    node.attribute(name)
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|n| n.is_finite())
}

fn opacity(node: roxmltree::Node<'_, '_>) -> f32 {
    number(node, "opacity").map_or(1.0, |o| o.clamp(0.0, 1.0) as f32)
}

fn visible(node: roxmltree::Node<'_, '_>) -> bool {
    node.attribute("visibility")
        .is_none_or(|v| !v.trim().eq_ignore_ascii_case("hidden"))
}

/// A name for a layer with none: files may leave it out.
fn name_of(node: roxmltree::Node<'_, '_>, fallback: &str) -> String {
    let name = node.attribute("name").unwrap_or("").trim();
    if name.is_empty() {
        fallback.to_owned()
    } else {
        name.chars().take(200).collect()
    }
}

impl Reader<'_> {
    /// The layers and stacks inside `stack`, bottom first.
    fn children(&mut self, stack: roxmltree::Node<'_, '_>, depth: usize) -> Vec<OraNode> {
        let mut out = Vec::new();
        // the file lists the top layer first
        let items: Vec<_> = stack.children().filter(roxmltree::Node::is_element).collect();
        for node in items.into_iter().rev() {
            match node.tag_name().name() {
                "stack" if depth < MAX_DEPTH => {
                    let children = self.children(node, depth + 1);
                    if !children.is_empty() {
                        out.push(OraNode::Group {
                            name: name_of(node, "Group"),
                            opacity: opacity(node),
                            visible: visible(node),
                            children,
                        });
                    }
                }
                "stack" => {
                    // nested too deeply: its layers are left out
                    for _ in node.descendants().filter(|d| d.tag_name().name() == "layer") {
                        self.notes.add(MISSING);
                    }
                }
                "layer" => {
                    if self.layers >= MAX_LAYERS {
                        self.notes.add(TOO_MANY);
                    } else if let Some(layer) = self.layer(node) {
                        self.layers += 1;
                        out.push(OraNode::Layer(layer));
                    } else {
                        self.notes.add(MISSING);
                    }
                }
                _ => {}
            }
        }
        out
    }

    fn layer(&mut self, node: roxmltree::Node<'_, '_>) -> Option<OraLayer> {
        let src = node.attribute("src")?;
        let png = self.archive.get(src)?.ok()?;
        // the size, from the file's header, before anything is decoded
        let (width, height) = ImageReader::new(std::io::Cursor::new(&png))
            .with_guessed_format()
            .ok()?
            .into_dimensions()
            .ok()?;
        if width == 0
            || height == 0
            || width > MAX_SIDE
            || height > MAX_SIDE
            || u64::from(width) * u64::from(height) > MAX_PIXELS
            || !png.starts_with(b"\x89PNG")
        {
            return None;
        }
        if node
            .attribute("composite-op")
            .is_some_and(|op| !matches!(op.trim(), "" | "svg:src-over"))
        {
            self.notes.add(BLEND);
        }
        let place = |name: &str| number(node, name).map_or(0, |n| n.clamp(-1.0e6, 1.0e6).round() as i32);
        Some(OraLayer {
            name: name_of(node, "Layer"),
            png,
            width,
            height,
            x: place("x"),
            y: place("y"),
            opacity: opacity(node),
            visible: visible(node),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG of one colour.
    pub(crate) fn png(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba(rgba));
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    /// A file with the given `stack.xml` and pictures.
    pub(crate) fn ora(stack: &str, pictures: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut files: Vec<(&str, &[u8])> =
            vec![("mimetype", b"image/openraster"), ("stack.xml", stack.as_bytes())];
        for (name, bytes) in pictures {
            files.push((name, bytes));
        }
        crate::zip::write(&files).unwrap()
    }

    const STACK: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<image w="64" h="32">
  <stack>
    <layer name="Lines" src="data/lines.png" x="4" y="-3" opacity="0.5"/>
    <stack name="Colour" visibility="hidden" opacity="0.8">
      <layer name="Skin" src="data/skin.png" composite-op="svg:multiply"/>
      <layer name="" src="data/bg.png" visibility="hidden"/>
    </stack>
    <layer name="Paper" src="data/paper.png" composite-op="svg:src-over"/>
  </stack>
</image>"#;

    fn pictures() -> Vec<(&'static str, Vec<u8>)> {
        vec![
            ("data/lines.png", png(10, 8, [0, 0, 0, 255])),
            ("data/skin.png", png(6, 6, [250, 200, 170, 255])),
            ("data/bg.png", png(2, 2, [1, 2, 3, 255])),
            ("data/paper.png", png(64, 32, [255, 255, 255, 255])),
        ]
    }

    #[test]
    fn a_stack_becomes_layers_and_groups_bottom_first() {
        let read = parse(&ora(STACK, &pictures())).unwrap();
        assert_eq!((read.width, read.height, read.layers), (64, 32, 4));
        // the file lists top first, so the bottom (Paper) comes first here
        let [
            OraNode::Layer(paper),
            OraNode::Group {
                name,
                opacity,
                visible,
                children,
            },
            OraNode::Layer(lines),
        ] = read.nodes.as_slice()
        else {
            panic!("{:?}", read.nodes);
        };
        assert_eq!(paper.name, "Paper");
        assert_eq!((paper.width, paper.height), (64, 32));
        assert_eq!((name.as_str(), *opacity, *visible), ("Colour", 0.8, false));
        assert_eq!(children.len(), 2);
        let OraNode::Layer(unnamed) = &children[0] else {
            panic!()
        };
        assert_eq!(
            (unnamed.name.as_str(), unnamed.visible),
            ("Layer", false),
            "a missing name gets one"
        );
        let OraNode::Layer(skin) = &children[1] else {
            panic!()
        };
        assert_eq!(skin.name, "Skin");
        assert_eq!((lines.x, lines.y, lines.opacity), (4, -3, 0.5));
        assert_eq!((lines.width, lines.height), (10, 8));
        assert!(lines.visible && lines.png.starts_with(b"\x89PNG"));
        // the multiply layer is counted, normal is not
        assert_eq!(
            read.notes.phrases(),
            ["1 layer with a blend mode shown as normal"]
        );
    }

    #[test]
    fn layers_that_cannot_be_read_are_counted_and_the_rest_come_in() {
        let stack = r#"<image w="8" h="8"><stack>
            <layer name="a" src="data/a.png"/>
            <layer name="gone" src="data/gone.png"/>
            <layer name="notpng" src="data/notpng.png"/>
            <layer name="nosrc"/>
        </stack></image>"#;
        let read = parse(&ora(
            stack,
            &[
                ("data/a.png", png(2, 2, [9, 9, 9, 255])),
                ("data/notpng.png", b"plain text".to_vec()),
            ],
        ))
        .unwrap();
        assert_eq!(read.layers, 1);
        assert_eq!(
            read.notes.phrases(),
            ["3 layers left out (their pictures are missing or unreadable)"]
        );
    }

    #[test]
    fn a_file_that_is_not_openraster_says_so() {
        assert!(parse(b"hello").unwrap_err().contains("not a ZIP"));
        let no_stack = crate::zip::write(&[("mimetype", b"image/openraster".as_slice())]).unwrap();
        assert!(parse(&no_stack).unwrap_err().contains("stack.xml"));
        let bad = |stack: &str| parse(&ora(stack, &pictures())).unwrap_err();
        assert!(bad("not xml <").contains("stack.xml"));
        assert!(bad("<picture/>").contains("not an OpenRaster"));
        assert!(bad("<image w=\"0\" h=\"5\"><stack/></image>").contains("size"));
        assert!(bad("<image w=\"99999\" h=\"5\"><stack/></image>").contains("size"));
        assert!(bad("<image w=\"5\" h=\"5\"/>").contains("no layers"));
        assert!(bad("<image w=\"5\" h=\"5\"><stack/></image>").contains("no layers"));
        // a DTD (entity tricks) is refused
        assert!(
            bad("<!DOCTYPE image [<!ENTITY a \"b\">]><image w=\"5\" h=\"5\"><stack/></image>")
                .contains("stack.xml")
        );
    }

    #[test]
    fn hostile_stacks_are_limited() {
        // 600 layers: only 500 come in, the rest are counted
        let mut stack = String::from("<image w=\"4\" h=\"4\"><stack>");
        for i in 0..600 {
            stack.push_str(&format!("<layer name=\"l{i}\" src=\"data/a.png\"/>"));
        }
        stack.push_str("</stack></image>");
        let read = parse(&ora(&stack, &[("data/a.png", png(1, 1, [0, 0, 0, 255]))])).unwrap();
        assert_eq!(read.layers, MAX_LAYERS);
        assert_eq!(read.notes.phrases(), ["100 layers left out (too many layers)"]);
        // groups nested 40 deep: the layers below 16 levels are left out
        let deep = format!(
            "<image w=\"4\" h=\"4\"><stack><layer name=\"top\" src=\"data/a.png\"/>{}<layer name=\"deep\" src=\"data/a.png\"/>{}</stack></image>",
            "<stack name=\"g\">".repeat(40),
            "</stack>".repeat(40)
        );
        let read = parse(&ora(&deep, &[("data/a.png", png(1, 1, [0, 0, 0, 255]))])).unwrap();
        assert_eq!(read.layers, 1);
        assert_eq!(read.notes.phrases().len(), 1);
        // odd attribute values do not panic and are held in range
        let odd = r#"<image w="4" h="4"><stack><layer src="data/a.png" x="NaN" y="1e999" opacity="9" visibility="???"/></stack></image>"#;
        let read = parse(&ora(odd, &[("data/a.png", png(1, 1, [0, 0, 0, 255]))])).unwrap();
        let OraNode::Layer(l) = &read.nodes[0] else {
            panic!()
        };
        assert_eq!((l.x, l.y, l.opacity, l.visible), (0, 0, 1.0, true));
        // a picture too large for a layer is left out (a header that claims it)
        let mut big = png(2, 2, [0, 0, 0, 255]);
        big[16..20].copy_from_slice(&200_000u32.to_be_bytes());
        let stack = r#"<image w="4" h="4"><stack><layer name="big" src="data/big.png"/></stack></image>"#;
        assert!(parse(&ora(stack, &[("data/big.png", big)])).is_err());
    }

    #[test]
    fn damaged_archives_never_panic() {
        let good = ora(STACK, &pictures());
        // cut anywhere, or with a byte flipped, it is an error or a read, never a panic
        let mut state = 0x1234_5678_9ABC_DEF1u64;
        for n in 0..300 {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let mut copy = good.clone();
            if n % 2 == 0 {
                copy.truncate((state >> 33) as usize % good.len());
            } else {
                let at = (state >> 33) as usize % good.len();
                copy[at] ^= 0xFF;
            }
            let _ = parse(&copy);
        }
    }
}
