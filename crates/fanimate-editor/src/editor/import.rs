//! Importing pictures (PNG, JPEG), Photoshop files and OpenRaster files.
//!
//! A Photoshop file comes in as a group named after it, holding one image
//! layer per Photoshop layer where it was, and groups for its groups, so a
//! character drawn in layers is ready to rig.

use std::sync::Arc;

use fanimate_core::{Image, Layer, LayerId, LayerKind, MediaFile, Vec2};

use super::{Editor, StatusKind, labelled};

impl Editor {
    /// Import a picture file (`name` is its file name) as an image layer,
    /// centred on the canvas. One undo step.
    pub fn import_picture(&mut self, name: &str, bytes: Vec<u8>) -> Result<LayerId, String> {
        let format = image::guess_format(&bytes).map_err(|_| "not a picture Fanimate can read".to_owned())?;
        let ext = match format {
            image::ImageFormat::Png => "png",
            image::ImageFormat::Jpeg => "jpg",
            _ => return Err("only PNG and JPEG pictures can be imported".into()),
        };
        let size = image::load_from_memory(&bytes)
            .map(|img| Vec2::new(img.width() as f32, img.height() as f32))
            .map_err(|e| format!("the picture could not be read: {e}"))?;
        self.checkpoint("Import picture");
        let media = self.store_media(bytes, ext);
        let mut layer = labelled(
            self.doc
                .new_layer(stem(name), LayerKind::Image(Image::new(media, size.x, size.y))),
        );
        layer.transform.position.base = (self.doc.size() - size) / 2.0;
        let id = layer.id;
        self.place_imported(layer);
        self.set_status(StatusKind::Success, format!("Imported {}", file_name(name)));
        Ok(id)
    }

    /// Import a Photoshop file as a group of image layers. One undo step.
    pub fn import_psd(&mut self, name: &str, bytes: &[u8]) -> Result<LayerId, String> {
        // the reader can panic on damaged files; that must not take the
        // app, and the user's work, down with it
        let psd = std::panic::catch_unwind(|| psd::Psd::from_bytes(bytes))
            .map_err(|_| "the Photoshop file is damaged".to_owned())?
            .map_err(|e| format!("the Photoshop file could not be read: {e}"))?;
        let (w, h) = (psd.width(), psd.height());
        let mut pictures: Vec<(Option<u32>, Layer, Vec<u8>)> = Vec::new();
        for layer in psd.layers() {
            // the layer's pixels, clipped to the canvas (a layer may hang
            // off its edges)
            let (l, t) = (i64::from(layer.layer_left()), i64::from(layer.layer_top()));
            let clip = |v: i64, max: u32| v.clamp(0, i64::from(max)) as u32;
            let (left, top) = (clip(l, w), clip(t, h));
            let (right, bottom) = (
                clip(l + i64::from(layer.width()), w),
                clip(t + i64::from(layer.height()), h),
            );
            if right <= left || bottom <= top {
                continue;
            }
            let full = std::panic::catch_unwind(|| layer.rgba())
                .map_err(|_| "a Photoshop layer is damaged".to_owned())?;
            let png = crop_png(&full, w, left, top, right, bottom)?;
            let mut l = labelled(self.doc.new_layer(
                layer.name().to_owned(),
                LayerKind::Image(Image::new(
                    String::new(),
                    (right - left) as f32,
                    (bottom - top) as f32,
                )),
            ));
            l.visible = layer.visible();
            l.opacity.base = f32::from(layer.opacity()) / 255.0;
            l.transform.position.base = Vec2::new(left as f32, top as f32);
            pictures.push((layer.parent_id(), l, png));
        }
        if pictures.is_empty() {
            return Err("the Photoshop file has no layers with pixels".into());
        }
        self.checkpoint("Import Photoshop file");
        // groups, made on demand so empty ones are left out
        let mut groups: Vec<(u32, Layer, Option<u32>)> = Vec::new();
        for id in psd.group_ids_in_order() {
            if let Some(g) = psd.groups().get(id) {
                let layer = labelled(
                    self.doc
                        .new_layer(g.name().to_owned(), LayerKind::Group { children: vec![] }),
                );
                groups.push((*id, layer, g.parent_id()));
            }
        }
        let mut root = labelled(
            self.doc
                .new_layer(stem(name), LayerKind::Group { children: vec![] }),
        );
        let mut placed = 0;
        for (parent, mut layer, png) in pictures {
            let media = self.store_media(png, "png");
            if let LayerKind::Image(img) = &mut layer.kind {
                img.media = media;
            }
            let into = parent.and_then(|p| groups.iter_mut().find(|g| g.0 == p).map(|g| &mut g.1));
            match into.and_then(|g| g.kind.children_mut()) {
                Some(children) => children.push(layer),
                None => {
                    if let Some(c) = root.kind.children_mut() {
                        c.push(layer);
                    }
                }
            }
            placed += 1;
        }
        // nest groups inside their parents, innermost first
        while let Some(i) = groups
            .iter()
            .rposition(|g| g.2.is_some_and(|p| groups.iter().any(|o| o.0 == p)))
        {
            let (_, group, parent) = groups.remove(i);
            if let Some(target) = groups.iter_mut().find(|g| Some(g.0) == parent)
                && let Some(c) = target.1.kind.children_mut()
            {
                c.insert(0, group);
            }
        }
        if let Some(children) = root.kind.children_mut() {
            for (_, group, _) in groups {
                if group.kind.children().is_some_and(|c| !c.is_empty()) {
                    children.push(group);
                }
            }
        }
        let id = root.id;
        // the Photoshop canvas, placed at the centre of ours
        let offset = (self.doc.size() - Vec2::new(w as f32, h as f32)) / 2.0;
        root.transform.position.base = offset;
        self.place_imported(root);
        self.set_status(
            StatusKind::Success,
            format!("Imported {placed} layers from {}", file_name(name)),
        );
        Ok(id)
    }

    /// Import an OpenRaster file (`.ora`, saved by Krita, MyPaint, GIMP and
    /// others) as a group of image layers, groups kept, each layer where it
    /// was with its opacity and visibility. One undo step. What Fanimate
    /// cannot show (blend modes other than normal, layers that could not be
    /// read) is counted in the status message.
    pub fn import_ora(&mut self, name: &str, bytes: &[u8]) -> Result<LayerId, String> {
        let ora = fanimate_import::ora::parse(bytes)?;
        self.checkpoint("Import OpenRaster file");
        let mut root = labelled(
            self.doc
                .new_layer(stem(name), LayerKind::Group { children: vec![] }),
        );
        let children = self.ora_layers(&ora.nodes);
        if let Some(c) = root.kind.children_mut() {
            *c = children;
        }
        // the picture's canvas, placed at the centre of ours
        root.transform.position.base =
            (self.doc.size() - Vec2::new(ora.width as f32, ora.height as f32)) / 2.0;
        let id = root.id;
        self.place_imported(root);
        let mut message = format!("Imported {} layers from {}", ora.layers, file_name(name));
        let notes = ora.notes.phrases();
        if !notes.is_empty() {
            message = format!("{message} ({})", notes.join(", "));
        }
        self.set_status(
            if notes.is_empty() {
                StatusKind::Success
            } else {
                StatusKind::Info
            },
            message,
        );
        Ok(id)
    }

    /// Fanimate layers for `nodes` (bottom first, the order Fanimate draws in).
    fn ora_layers(&mut self, nodes: &[fanimate_import::ora::OraNode]) -> Vec<Layer> {
        use fanimate_import::ora::OraNode;
        let mut out = Vec::with_capacity(nodes.len());
        for node in nodes {
            match node {
                OraNode::Layer(l) => {
                    let media = self.store_media(l.png.clone(), "png");
                    let mut layer = labelled(self.doc.new_layer(
                        l.name.clone(),
                        LayerKind::Image(Image::new(media, l.width as f32, l.height as f32)),
                    ));
                    layer.visible = l.visible;
                    layer.opacity.base = l.opacity;
                    layer.transform.position.base = Vec2::new(l.x as f32, l.y as f32);
                    out.push(layer);
                }
                OraNode::Group {
                    name,
                    opacity,
                    visible,
                    children,
                } => {
                    let children = self.ora_layers(children);
                    let mut group = labelled(
                        self.doc
                            .new_layer(name.clone(), LayerKind::Group { children: vec![] }),
                    );
                    group.visible = *visible;
                    group.opacity.base = *opacity;
                    if let Some(c) = group.kind.children_mut() {
                        *c = children;
                    }
                    out.push(group);
                }
            }
        }
        out
    }

    /// Keep a picture's bytes in the document's media store; returns its
    /// name. The same picture is stored once.
    fn store_media(&mut self, bytes: Vec<u8>, ext: &str) -> String {
        let name = MediaFile::name_for(&bytes, ext);
        self.doc
            .media
            .entry(name.clone())
            .or_insert_with(|| Arc::new(MediaFile { bytes }));
        name
    }

    /// Put an imported layer above the selection (inside a selected group
    /// or bone layer), and select it.
    pub(super) fn place_imported(&mut self, layer: Layer) {
        let id = layer.id;
        let (parent, index) = match self.primary().and_then(|p| self.doc.find(p)) {
            Some(p) if p.kind.children().is_some() => (Some(p.id), p.kind.children().map_or(0, Vec::len)),
            _ => self.insertion_point(),
        };
        if !self.doc.insert(parent, index, layer.clone()) {
            self.doc.insert(None, self.doc.layers.len(), layer);
        }
        self.selection = vec![id];
        self.anchor = Some(id);
    }
}

/// The file name without its folders, for messages.
pub(super) fn file_name(name: &str) -> &str {
    name.rsplit(['/', '\\']).next().unwrap_or(name)
}

/// The file name without folders or extension, for a layer name.
pub(super) fn stem(name: &str) -> String {
    let file = file_name(name);
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
    if stem.is_empty() {
        "Picture".into()
    } else {
        stem.to_owned()
    }
}

/// The rectangle `left..right`, `top..bottom` of a `width`-wide RGBA
/// canvas, as a PNG file.
fn crop_png(
    rgba: &[u8],
    width: u32,
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
) -> Result<Vec<u8>, String> {
    let (w, h) = (right - left, bottom - top);
    let mut out = image::RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let i = (((top + y) * width + left + x) * 4) as usize;
            if let Some(px) = rgba.get(i..i + 4) {
                out.put_pixel(x, y, image::Rgba([px[0], px[1], px[2], px[3]]));
            }
        }
    }
    let mut bytes = Vec::new();
    out.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .map_err(|e| format!("a layer could not be stored: {e}"))?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Action;

    pub(crate) fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([200, 30, 30, 255]));
        let mut bytes = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn a_picture_becomes_a_centred_image_layer() {
        let mut ed = Editor::untitled();
        let id = ed.import_picture("C:\\art\\arm.png", png(40, 20)).unwrap();
        let layer = ed.doc.find(id).unwrap();
        assert_eq!(layer.name, "arm");
        let LayerKind::Image(img) = &layer.kind else {
            panic!()
        };
        assert_eq!((img.width, img.height), (40.0, 20.0));
        assert!(ed.doc.media.contains_key(&img.media));
        assert_eq!(layer.transform.position.base, Vec2::new(620.0, 350.0));
        assert!(fanimate_fnm::validate(&ed.doc).is_clean());
        ed.run(Action::Undo);
        assert!(ed.doc.find(id).is_none());
        // undo took the picture away too; the same picture twice is stored once
        assert!(ed.doc.media.is_empty());
        ed.import_picture("a.png", png(4, 4)).unwrap();
        ed.import_picture("b.png", png(4, 4)).unwrap();
        assert_eq!(ed.doc.media.len(), 1);
    }

    #[test]
    fn unreadable_files_are_refused_without_a_trace() {
        let mut ed = Editor::untitled();
        assert!(ed.import_picture("x.png", b"hello".to_vec()).is_err());
        assert!(ed.import_psd("x.psd", b"8BPS garbage").is_err());
        assert!(ed.history.undo_label().is_none());
    }

    #[test]
    fn crop_takes_the_layer_rectangle() {
        let mut rgba = vec![0u8; 4 * 4 * 4];
        rgba[(4 * 2 + 1) * 4] = 255;
        let png = crop_png(&rgba, 4, 1, 2, 3, 4).unwrap();
        let img = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(img.dimensions(), (2, 2));
        assert_eq!(img.get_pixel(0, 0)[0], 255);
    }

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/psd")
                .join(name),
        )
        .unwrap()
    }

    #[test]
    fn photoshop_layers_become_image_layers_in_a_group() {
        let mut ed = Editor::untitled();
        let id = ed
            .import_psd("two.psd", &fixture("two-layers-red-green-1x1.psd"))
            .unwrap();
        let root = ed.doc.find(id).unwrap();
        assert_eq!(root.name, "two");
        let children = root.kind.children().unwrap();
        assert_eq!(children.len(), 2);
        assert!(children.iter().all(|l| matches!(l.kind, LayerKind::Image(_))));
        assert!(
            fanimate_fnm::validate(&ed.doc).is_clean(),
            "{}",
            fanimate_fnm::validate(&ed.doc)
        );
        assert_eq!(ed.history.undo_label(), Some("Import Photoshop file"));
    }

    #[test]
    fn photoshop_groups_become_groups() {
        let mut ed = Editor::untitled();
        let id = ed
            .import_psd(
                "g.psd",
                &fixture("green-1x1-one-group-one-layer-inside-one-outside.psd"),
            )
            .unwrap();
        let children = ed.doc.find(id).unwrap().kind.children().unwrap().clone();
        assert!(children.iter().any(|l| matches!(l.kind, LayerKind::Group { .. })));
        assert!(children.iter().any(|l| matches!(l.kind, LayerKind::Image(_))));
    }

    #[test]
    fn stems_drop_folders_and_extensions() {
        assert_eq!(stem("/a/b/hero.psd"), "hero");
        assert_eq!(stem("noext"), "noext");
        assert_eq!(stem(".png"), "Picture");
        assert_eq!(file_name("C:\\art\\a.png"), "a.png");
    }

    /// An `.ora` with the given stack and pictures.
    fn ora_file(stack: &str, pictures: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut files: Vec<(&str, &[u8])> =
            vec![("mimetype", b"image/openraster"), ("stack.xml", stack.as_bytes())];
        for (name, bytes) in pictures {
            files.push((name, bytes));
        }
        fanimate_import::zip::write(&files).unwrap()
    }

    const ORA_STACK: &str = r#"<image w="40" h="20"><stack>
        <layer name="Lines" src="data/a.png" x="3" y="2" opacity="0.5"/>
        <stack name="Colour" visibility="hidden">
            <layer name="Skin" src="data/b.png" composite-op="svg:multiply"/>
        </stack>
    </stack></image>"#;

    #[test]
    fn an_openraster_file_becomes_a_group_of_image_layers_in_one_undo_step() {
        let mut ed = Editor::new(fanimate_core::Document::new("t", 100.0, 60.0, 24.0));
        let file = ora_file(
            ORA_STACK,
            &[("data/a.png", png(10, 8)), ("data/b.png", png(6, 6))],
        );
        let id = ed.import_ora("/art/hero.ora", &file).unwrap();
        let root = ed.doc.find(id).unwrap();
        assert_eq!(root.name, "hero");
        // the picture's canvas sits at the middle of ours
        assert_eq!(root.transform.position.base, Vec2::new(30.0, 20.0));
        let kids = root.kind.children().unwrap();
        // bottom first: the Colour group, then Lines on top
        assert_eq!(kids.len(), 2);
        assert_eq!((kids[0].name.as_str(), kids[0].visible), ("Colour", false));
        let skin = &kids[0].kind.children().unwrap()[0];
        assert_eq!(skin.name, "Skin");
        let lines = &kids[1];
        assert_eq!(lines.name, "Lines");
        assert_eq!(lines.opacity.base, 0.5);
        assert_eq!(lines.transform.position.base, Vec2::new(3.0, 2.0));
        let LayerKind::Image(img) = &lines.kind else {
            panic!("an image layer")
        };
        assert_eq!((img.width, img.height), (10.0, 8.0));
        assert!(
            ed.doc.media.contains_key(&img.media),
            "the picture is stored in the project"
        );
        // the status says what Fanimate could not show
        let (_, message) = ed.status.clone().unwrap();
        assert!(message.contains("Imported 2 layers from hero.ora"), "{message}");
        assert!(
            message.contains("1 layer with a blend mode shown as normal"),
            "{message}"
        );
        assert_eq!(ed.selection, vec![id]);
        // one undo step takes it all back
        ed.run(crate::Action::Undo);
        assert!(ed.doc.layers.is_empty());
        ed.run(crate::Action::Redo);
        assert_eq!(ed.doc.layers.len(), 1);
    }

    #[test]
    fn a_file_that_is_not_openraster_is_refused_without_a_trace() {
        let mut ed = Editor::new(fanimate_core::Document::new("t", 100.0, 60.0, 24.0));
        let rev = ed.revision();
        assert!(ed.import_ora("x.ora", b"PK nonsense").is_err());
        assert!(ed.import_ora("x.ora", b"").is_err());
        assert_eq!(ed.revision(), rev, "no undo step");
        assert!(ed.doc.layers.is_empty() && ed.doc.media.is_empty());
    }
}
