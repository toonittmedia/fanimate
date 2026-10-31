//! Captions as layers: reading an SRT or WebVTT file into a switch layer of
//! text layers, and writing the cues of such a layer back out.
//!
//! A caption is one text layer, and a switch layer (which already shows one
//! child at a time, chosen by name on keys) shows the right one at the right
//! time, so a project with captions needs no new layer kind and no new part
//! of the file format. Each caption is an ordinary text layer: it can be
//! restyled, given a text animator, or edited by hand, and a template can
//! style the lot by restyling one.

use fanimate_core::captions::{self, Cue};
use fanimate_core::scene::switch_choice;
use fanimate_core::{Color, Stroke};
use fanimate_core::{Frame, Interpolation, LayerId, LayerKind, Text, TextAlign, Vec2};

use super::{Editor, labelled};

/// What importing captions made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptionImport {
    /// The switch layer holding the captions.
    pub layer: LayerId,
    pub cues: usize,
    /// Cues left out of the file for a bad time or no words.
    pub skipped: usize,
    /// Cues past the limit that were not read.
    pub cut_off: usize,
}

/// The name of the child shown between captions.
const BLANK: &str = "Blank";

impl Editor {
    /// The document frame a time in milliseconds falls on.
    fn frame_at_ms(&self, ms: u64) -> Frame {
        let frames = (ms as f64 * f64::from(self.doc.fps) / 1000.0).round();
        self.doc
            .start
            .saturating_add(frames.min(f64::from(i32::MAX / 2)) as i32)
    }

    /// The time in milliseconds a document frame starts at.
    fn ms_at_frame(&self, frame: Frame) -> u64 {
        let frames = i64::from(frame) - i64::from(self.doc.start);
        (frames.max(0) as f64 * 1000.0 / f64::from(self.doc.fps.max(1.0))).round() as u64
    }

    /// Read an SRT or WebVTT file into a new switch layer named `name`: one
    /// text layer per caption, white with a dark outline, centred low on the
    /// canvas, shown from the frame it starts on to the frame it ends on. One
    /// undo step. The error says what is wrong with the file; a document
    /// that is playing refuses.
    pub fn import_captions(&mut self, source: &str, name: &str) -> Result<CaptionImport, String> {
        if self.editing_blocked() {
            return Err("Pause the animation to import captions.".to_owned());
        }
        let parsed = captions::parse(source).map_err(|e| e.to_string())?;
        if parsed.cues.is_empty() {
            return Err("None of the captions in that file could be read.".to_owned());
        }
        let size = (self.doc.height / 16.0).clamp(8.0, 400.0);
        let (width, left, top) = (self.doc.width * 0.8, self.doc.width * 0.1, self.doc.height * 0.78);
        self.checkpoint("Import captions");

        let mut children = Vec::with_capacity(parsed.cues.len() + 1);
        let mut keys: Vec<fanimate_core::anim::Key<String>> = Vec::new();
        let mut ends = Vec::new();
        for (n, cue) in parsed.cues.iter().enumerate() {
            let child_name = format!("Caption {}", n + 1);
            let mut text = Text::new(cue.text.clone());
            let look = text.look_mut();
            look.size = size;
            look.width = Some(width);
            look.align = TextAlign::Center;
            look.fill = Color::WHITE;
            look.stroke = Some(Stroke {
                color: Color::BLACK,
                width: size / 8.0,
            });
            let mut layer = labelled(self.doc.new_layer(child_name.clone(), LayerKind::Text(text)));
            layer.transform.position.base = Vec2::new(left, top);
            children.push(layer);

            let start = self.frame_at_ms(cue.start_ms);
            // a caption ends when the next begins if they overlap, and lasts a frame at least
            let mut end = self.frame_at_ms(cue.end_ms).max(start.saturating_add(1));
            if let Some(next) = parsed.cues.get(n + 1) {
                end = end.min(self.frame_at_ms(next.start_ms));
            }
            keys.push(key(start, child_name));
            ends.push(end);
        }
        // blanks first, then the starts, so a caption that starts as another ends wins
        let all_last = ends.iter().copied().max().unwrap_or(self.doc.end);
        let mut all = Vec::new();
        for end in ends {
            fanimate_core::anim::set_key_in(&mut all, end, BLANK.to_owned(), Interpolation::Step);
        }
        for k in keys {
            fanimate_core::anim::set_key_in(&mut all, k.frame, k.value, Interpolation::Step);
        }
        children.push(labelled(self.doc.new_layer(BLANK, LayerKind::vector(vec![]))));

        let wanted = if name.trim().is_empty() {
            "Captions"
        } else {
            name.trim()
        };
        let mut taken = false;
        self.doc.walk(|l| taken |= l.name == wanted);
        let layer_name = if taken {
            self.unused_name(wanted)
        } else {
            wanted.to_owned()
        };
        let layer = labelled(self.doc.new_layer(
            layer_name,
            LayerKind::Switch {
                children,
                switch: all,
            },
        ));
        let id = layer.id;
        // the animation has to run to the last caption, or the end of the file would cut it off
        let last_frame = all_last;
        self.doc.end = self.doc.end.max(last_frame);
        let (parent, index) = self.insertion_point();
        self.doc.insert(parent, index, layer);
        self.selection = vec![id];
        self.anchor = Some(id);
        Ok(CaptionImport {
            layer: id,
            cues: parsed.cues.len(),
            skipped: parsed.skipped,
            cut_off: parsed.cut_off,
        })
    }

    /// The switch layer that holds `child` as one of its layers, if there is one.
    fn switch_holding(&self, child: LayerId) -> Option<LayerId> {
        let mut found = None;
        self.doc.walk(|l| {
            if let LayerKind::Switch { children, .. } = &l.kind
                && children.iter().any(|c| c.id == child)
            {
                found = Some(l.id);
            }
        });
        found
    }

    /// How many other captions `Restyle All Captions` would change when the
    /// text layer `id` is the model: the other text layers of its switch layer.
    pub fn restylable_captions(&self, id: LayerId) -> usize {
        let Some(switch) = self.switch_holding(id) else {
            return 0;
        };
        match self.doc.find(switch).map(|l| &l.kind) {
            Some(LayerKind::Switch { children, .. }) => children
                .iter()
                .filter(|c| c.id != id && matches!(c.kind, LayerKind::Text(_)))
                .count(),
            _ => 0,
        }
    }

    /// Make every other caption in the switch layer that holds `model` look like
    /// it: the font and everything else about how the words are set, the
    /// animators, and where it stands, but not the words. One undo step.
    /// Returns how many captions changed.
    pub fn restyle_captions(&mut self, model: LayerId) -> Result<usize, String> {
        if self.editing_blocked() {
            return Err("Pause the animation to restyle captions.".to_owned());
        }
        let Some(LayerKind::Text(text)) = self.doc.find(model).map(|l| &l.kind) else {
            return Err("Select the caption to copy the look from.".to_owned());
        };
        let (look, animators) = (text.look().clone(), text.animators.clone());
        let transform = self.doc.find(model).map(|l| l.transform.clone());
        let Some(switch) = self.switch_holding(model) else {
            return Err("That text layer is not one of a set of captions.".to_owned());
        };
        let others: Vec<LayerId> = match self.doc.find(switch).map(|l| &l.kind) {
            Some(LayerKind::Switch { children, .. }) => children
                .iter()
                .filter(|c| c.id != model && matches!(c.kind, LayerKind::Text(_)))
                .map(|c| c.id)
                .collect(),
            _ => Vec::new(),
        };
        if others.is_empty() {
            return Err("There are no other captions to restyle.".to_owned());
        }
        // only the captions that differ are touched, so a set that already
        // matches leaves no undo step
        let differing: Vec<LayerId> = others
            .into_iter()
            .filter(|id| {
                let Some(layer) = self.doc.find(*id) else {
                    return false;
                };
                let LayerKind::Text(text) = &layer.kind else {
                    return false;
                };
                let mut next = look.clone();
                next.content = text.look().content.clone();
                next != *text.look()
                    || text.animators != animators
                    || transform.as_ref().is_some_and(|t| *t != layer.transform)
            })
            .collect();
        if differing.is_empty() {
            return Ok(0);
        }
        self.checkpoint("Restyle captions");
        for id in &differing {
            let Some(layer) = self.doc.find_mut(*id) else {
                continue;
            };
            if let LayerKind::Text(text) = &mut layer.kind {
                let mut next = look.clone();
                next.content = text.look().content.clone();
                *text.look_mut() = next;
                text.animators = animators.clone();
            }
            if let Some(t) = &transform {
                layer.transform = t.clone();
            }
        }
        Ok(differing.len())
    }

    /// The captions a switch layer shows, with the times they show at: each
    /// key that names a text layer with words in it, until the next key (or the
    /// end of the animation). `None` when the layer is not a switch layer.
    pub fn captions_of(&self, id: LayerId) -> Option<Vec<Cue>> {
        let Some(LayerKind::Switch { children, switch }) = self.doc.find(id).map(|l| &l.kind) else {
            return None;
        };
        let last = self.doc.end.saturating_add(1);
        let mut cues = Vec::new();
        for (n, key) in switch.iter().enumerate() {
            let Some(shown) = switch_choice(children, std::slice::from_ref(key), key.frame as f32) else {
                continue;
            };
            let LayerKind::Text(text) = &children[shown].kind else {
                continue;
            };
            let words = text.look().content.trim();
            if words.is_empty() || !children[shown].name.eq_ignore_ascii_case(&key.value) {
                continue;
            }
            let end = switch.get(n + 1).map_or(last, |next| next.frame);
            cues.push(Cue {
                start_ms: self.ms_at_frame(key.frame),
                end_ms: self.ms_at_frame(end.max(key.frame)),
                text: words.to_owned(),
            });
        }
        Some(cues)
    }

    /// The captions of a switch layer as SubRip (`webvtt` false) or WebVTT
    /// text. `None` when the layer is not a switch layer or shows no words.
    pub fn export_captions(&self, id: LayerId, webvtt: bool) -> Option<String> {
        let cues = self.captions_of(id).filter(|c| !c.is_empty())?;
        Some(if webvtt {
            captions::to_vtt(&cues)
        } else {
            captions::to_srt(&cues)
        })
    }
}

fn key(frame: Frame, value: String) -> fanimate_core::anim::Key<String> {
    fanimate_core::anim::Key {
        frame,
        value,
        interp: Interpolation::Step,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Action;
    use fanimate_core::Document;

    const SRT: &str =
        "1\n00:00:01,000 --> 00:00:02,000\nHello\n\n2\n00:00:03,000 --> 00:00:04,500\nSecond caption\n\n";

    fn editor() -> Editor {
        Editor::new(Document::new("t", 1920.0, 1080.0, 24.0))
    }

    fn captions_of_import(ed: &mut Editor) -> Vec<LayerId> {
        let made = ed.import_captions(SRT, "Dialogue").unwrap();
        let LayerKind::Switch { children, .. } = &ed.doc.find(made.layer).unwrap().kind else {
            panic!("a switch layer");
        };
        children
            .iter()
            .filter(|c| matches!(c.kind, LayerKind::Text(_)))
            .map(|c| c.id)
            .collect()
    }

    #[test]
    fn restyling_copies_the_look_the_animators_and_the_place_but_not_the_words() {
        let mut ed = editor();
        let ids = captions_of_import(&mut ed);
        assert_eq!(ids.len(), 2);
        let (model, other) = (ids[0], ids[1]);
        ed.edit_text(model, "Style", |l| {
            l.size = 90.0;
            l.family = "JetBrains Mono".to_owned();
            l.fill = Color::rgb(1.0, 0.8, 0.0);
        });
        ed.add_text_animator(model, fanimate_core::TextPreset::FadeIn);
        ed.doc.find_mut(model).unwrap().transform.position.base = Vec2::new(40.0, 900.0);
        assert_eq!(ed.restylable_captions(model), 1);

        let words_before = ed.text(other).unwrap().look().content.clone();
        ed.selection = vec![model];
        ed.run(Action::RestyleCaptions);
        let (m, o) = (ed.text(model).unwrap(), ed.text(other).unwrap());
        assert_eq!(o.look().size, 90.0);
        assert_eq!(o.look().family, "JetBrains Mono");
        assert_eq!(o.look().fill, m.look().fill);
        assert_eq!(o.animators, m.animators);
        assert_eq!(o.look().content, words_before, "the words stay");
        assert_ne!(o.look().content, m.look().content);
        assert_eq!(
            ed.doc.find(other).unwrap().transform.position.base,
            Vec2::new(40.0, 900.0)
        );

        // already alike: nothing to do, and no undo step is made
        assert_eq!(ed.restyle_captions(model), Ok(0));

        // one undo step puts the other caption back
        ed.run(Action::Undo);
        assert_ne!(ed.text(other).unwrap().look().size, 90.0);
        assert!(ed.text(other).unwrap().animators.is_empty());
    }

    #[test]
    fn restyling_says_what_is_wrong_and_changes_nothing() {
        let mut ed = editor();
        // a text layer that is not in a set of captions
        ed.add_text_layer(Vec2::new(10.0, 10.0), "Alone");
        let alone = ed.selected_text().unwrap();
        assert!(ed.restyle_captions(alone).is_err());
        assert_eq!(ed.restylable_captions(alone), 0);
        assert!(!ed.can_run(Action::RestyleCaptions));
        // a layer that is not text at all
        let made = ed
            .import_captions("1\n00:00:01,000 --> 00:00:02,000\nOnly\n\n", "One")
            .unwrap();
        assert!(ed.restyle_captions(made.layer).is_err());
        // one caption alone has no others to restyle
        let LayerKind::Switch { children, .. } = &ed.doc.find(made.layer).unwrap().kind else {
            panic!("a switch layer");
        };
        let only = children[0].id;
        assert!(
            ed.restyle_captions(only)
                .unwrap_err()
                .contains("no other captions")
        );
        // not while playing
        let ids = captions_of_import(&mut ed);
        ed.playing = true;
        assert!(ed.restyle_captions(ids[0]).is_err());
    }

    #[test]
    fn captions_become_text_layers_shown_at_their_times_in_one_undo_step() {
        let mut ed = editor();
        let made = ed.import_captions(SRT, "Dialogue").unwrap();
        assert_eq!((made.cues, made.skipped), (2, 0));
        let layer = ed.doc.find(made.layer).unwrap();
        assert_eq!(layer.name, "Dialogue");
        let LayerKind::Switch { children, switch } = &layer.kind else {
            panic!("a switch layer");
        };
        assert_eq!(children.len(), 3, "two captions and the blank");
        assert_eq!(
            children.last().unwrap().name,
            BLANK,
            "the blank is on top, so it shows before the first"
        );
        // 24 fps, frames count from 1: 1 s is frame 25, 2 s is 49, 3 s is 73, 4.5 s is 109
        let frames: Vec<(i32, &str)> = switch.iter().map(|k| (k.frame, k.value.as_str())).collect();
        assert_eq!(
            frames,
            [(25, "Caption 1"), (49, BLANK), (73, "Caption 2"), (109, BLANK)]
        );
        // and what shows, frame by frame
        let shown = |f: f32| switch_choice(children, switch, f).map(|i| children[i].name.as_str());
        assert_eq!(shown(1.0), Some(BLANK));
        assert_eq!(shown(30.0), Some("Caption 1"));
        assert_eq!(shown(60.0), Some(BLANK));
        assert_eq!(shown(80.0), Some("Caption 2"));
        assert_eq!(ed.selection, vec![made.layer]);
        ed.run(Action::Undo);
        assert!(ed.doc.layers.is_empty());
        ed.run(Action::Redo);
        assert_eq!(ed.doc.layers.len(), 1);
    }

    #[test]
    fn each_caption_is_an_ordinary_styled_text_layer() {
        let mut ed = editor();
        let made = ed.import_captions(SRT, "").unwrap();
        let LayerKind::Switch { children, .. } = &ed.doc.find(made.layer).unwrap().kind else {
            panic!();
        };
        let LayerKind::Text(text) = &children[0].kind else {
            panic!("text");
        };
        assert_eq!(text.look().content, "Hello");
        assert_eq!(text.look().align, TextAlign::Center);
        assert_eq!(text.look().width, Some(1536.0));
        assert!(text.look().stroke.is_some());
        assert_eq!(ed.doc.find(made.layer).unwrap().name, "Captions");
        assert!(ed.doc.end >= 109, "the animation runs to the last caption");
        // a second import does not take the first one's name
        let again = ed.import_captions(SRT, "").unwrap();
        assert_eq!(ed.doc.find(again.layer).unwrap().name, "Captions 1");
    }

    #[test]
    fn a_caption_that_overlaps_the_next_ends_where_the_next_begins() {
        let mut ed = editor();
        let src = "00:00:01,000 --> 00:00:05,000\nLong one\n\n00:00:02,000 --> 00:00:03,000\nInterrupts\n";
        let made = ed.import_captions(src, "x").unwrap();
        let LayerKind::Switch { switch, .. } = &ed.doc.find(made.layer).unwrap().kind else {
            panic!();
        };
        let frames: Vec<(i32, &str)> = switch.iter().map(|k| (k.frame, k.value.as_str())).collect();
        // the first is cut at 2 s (frame 49) by the second, which then ends at 3 s (frame 73)
        assert_eq!(frames, [(25, "Caption 1"), (49, "Caption 2"), (73, BLANK)]);
    }

    #[test]
    fn a_caption_shorter_than_a_frame_still_shows_for_one() {
        let mut ed = editor();
        let made = ed
            .import_captions("00:00:01,000 --> 00:00:01,001\nBlink\n", "x")
            .unwrap();
        let LayerKind::Switch { switch, .. } = &ed.doc.find(made.layer).unwrap().kind else {
            panic!();
        };
        assert_eq!(switch.iter().map(|k| k.frame).collect::<Vec<_>>(), [25, 26]);
    }

    #[test]
    fn what_is_imported_is_exported_with_the_times_it_has_in_frames() {
        let mut ed = editor();
        let made = ed.import_captions(SRT, "x").unwrap();
        let srt = ed.export_captions(made.layer, false).unwrap();
        assert_eq!(
            srt,
            "1\n00:00:01,000 --> 00:00:02,000\nHello\n\n2\n00:00:03,000 --> 00:00:04,500\nSecond caption\n\n"
        );
        assert!(
            ed.export_captions(made.layer, true)
                .unwrap()
                .starts_with("WEBVTT")
        );
        // editing a caption's words changes what is exported
        let LayerKind::Switch { children, .. } = &ed.doc.find(made.layer).unwrap().kind else {
            panic!();
        };
        let first = children[0].id;
        ed.edit_text(first, "Edit caption", |l| l.content = "Changed".into());
        assert!(ed.export_captions(made.layer, false).unwrap().contains("Changed"));
    }

    #[test]
    fn a_layer_that_is_not_captions_exports_nothing() {
        let mut ed = editor();
        let text = ed.add_text_layer(Vec2::ZERO, "x").unwrap();
        assert!(ed.captions_of(text).is_none());
        assert!(ed.export_captions(text, false).is_none());
        assert!(ed.captions_of(LayerId(999)).is_none());
        let empty = ed.doc.new_layer(
            "s",
            LayerKind::Switch {
                children: vec![],
                switch: vec![],
            },
        );
        let id = empty.id;
        ed.doc.layers.push(empty);
        assert!(ed.export_captions(id, false).is_none(), "no words");
    }

    #[test]
    fn bad_files_and_a_playing_animation_are_refused_without_changing_anything() {
        let mut ed = editor();
        assert!(ed.import_captions("no captions here", "x").is_err());
        assert!(
            ed.import_captions("00:00:05,000 --> 00:00:01,000\nbackwards\n", "x")
                .is_err()
        );
        assert!(ed.doc.layers.is_empty());
        ed.playing = true;
        assert!(ed.import_captions(SRT, "x").is_err());
        assert!(ed.doc.layers.is_empty());
    }

    #[test]
    fn captions_save_and_reopen_as_they_were() {
        let mut ed = editor();
        let made = ed.import_captions(SRT, "x").unwrap();
        let dir = std::env::temp_dir().join(format!("fanimate-captions-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("c.fnm");
        fanimate_fnm::save(&ed.doc, &file).unwrap();
        let (doc, report) = fanimate_fnm::load(&file).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert!(report.is_clean(), "{report}");
        let mut reopened = Editor::new(doc);
        reopened.doc.start = ed.doc.start;
        assert_eq!(
            reopened.export_captions(made.layer, false),
            ed.export_captions(made.layer, false)
        );
    }
}
