//! Hidden edges: stroking a path without some of its segments.
//!
//! A shape's `hidden_edge_flags` say, for each command of its path, whether
//! that segment's stroke is left undrawn. [`Path::without_hidden_edges`] cuts
//! the path down to the segments that are drawn, for the stroke only: the
//! fill keeps the whole path, and the points stay where they are.

use glam::Vec2;

use crate::path::{Path, PathCmd};

impl Path {
    /// The segments of this path that are not hidden, as open pieces, with
    /// the width profile cut to match. `hidden` has one flag per command;
    /// when it does not (or nothing is hidden) the path comes back as it is.
    /// A closed subpath whose closing edge and every other edge are drawn is
    /// still closed, so its corners join.
    pub fn without_hidden_edges(&self, hidden: &[bool], widths: Option<&[f32]>) -> (Path, Option<Vec<f32>>) {
        if hidden.len() != self.cmds.len() || !hidden.iter().any(|h| *h) {
            return (self.clone(), widths.map(<[f32]>::to_vec));
        }
        let width = |n: usize| {
            widths
                .and_then(|w| w.get(n))
                .copied()
                .filter(|w| w.is_finite())
                .unwrap_or(1.0)
        };
        let mut cmds = Vec::with_capacity(self.cmds.len());
        let mut out_w = widths.map(|_| Vec::with_capacity(self.cmds.len()));
        let push_w = |out_w: &mut Option<Vec<f32>>, n: usize| {
            if let Some(w) = out_w {
                w.push(width(n));
            }
        };

        // the number of the next on-curve point, where the pen is and which
        // point that is, where this subpath began
        let mut next = 0usize;
        let (mut cur, mut cur_n) = (Vec2::ZERO, 0usize);
        let (mut first, mut first_n) = (Vec2::ZERO, 0usize);
        // whether the piece being drawn ends at the pen, and whether every
        // segment of this subpath so far has been drawn (so a close can join)
        let mut drawing = false;
        let mut whole = true;
        for (i, cmd) in self.cmds.iter().enumerate() {
            match *cmd {
                PathCmd::MoveTo(p) => {
                    (cur, cur_n) = (p, next);
                    (first, first_n) = (p, next);
                    next += 1;
                    drawing = false;
                    whole = true;
                }
                PathCmd::LineTo(p) | PathCmd::CubicTo(_, _, p) => {
                    let end_n = next;
                    next += 1;
                    if hidden[i] {
                        drawing = false;
                        whole = false;
                    } else {
                        if !drawing {
                            cmds.push(PathCmd::MoveTo(cur));
                            push_w(&mut out_w, cur_n);
                            drawing = true;
                        }
                        cmds.push(*cmd);
                        push_w(&mut out_w, end_n);
                    }
                    (cur, cur_n) = (p, end_n);
                }
                PathCmd::Close => {
                    if !hidden[i] {
                        if whole && drawing {
                            cmds.push(PathCmd::Close);
                        } else if cur != first {
                            // a gap earlier in this subpath: the closing edge is drawn on its own
                            if !drawing {
                                cmds.push(PathCmd::MoveTo(cur));
                                push_w(&mut out_w, cur_n);
                            }
                            cmds.push(PathCmd::LineTo(first));
                            push_w(&mut out_w, first_n);
                        }
                    }
                    (cur, cur_n) = (first, first_n);
                    drawing = false;
                }
            }
        }
        (Path { cmds }, out_w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Path {
        Path::new()
            .move_to(Vec2::new(0.0, 0.0))
            .line_to(Vec2::new(10.0, 0.0))
            .line_to(Vec2::new(10.0, 10.0))
            .line_to(Vec2::new(0.0, 10.0))
            .close()
    }

    #[test]
    fn nothing_hidden_is_the_path_itself() {
        let p = square();
        let (out, w) = p.without_hidden_edges(&[false; 5], None);
        assert_eq!(out, p);
        assert_eq!(w, None);
        // flags that do not fit the path are ignored
        assert_eq!(p.without_hidden_edges(&[true], None).0, p);
    }

    #[test]
    fn a_hidden_side_leaves_the_others_as_one_open_piece() {
        // hide the first side (command 1): the other three are one piece from its end round to its start
        let (out, _) = square().without_hidden_edges(&[false, true, false, false, false], None);
        assert_eq!(
            out.cmds,
            [
                PathCmd::MoveTo(Vec2::new(10.0, 0.0)),
                PathCmd::LineTo(Vec2::new(10.0, 10.0)),
                PathCmd::LineTo(Vec2::new(0.0, 10.0)),
                PathCmd::LineTo(Vec2::new(0.0, 0.0)),
            ]
        );
    }

    #[test]
    fn two_gaps_make_two_pieces() {
        // hide the second side (command 2): one piece before it, one after, joined by the close
        let (out, _) = square().without_hidden_edges(&[false, false, true, false, false], None);
        assert_eq!(
            out.cmds,
            [
                PathCmd::MoveTo(Vec2::new(0.0, 0.0)),
                PathCmd::LineTo(Vec2::new(10.0, 0.0)),
                PathCmd::MoveTo(Vec2::new(10.0, 10.0)),
                PathCmd::LineTo(Vec2::new(0.0, 10.0)),
                PathCmd::LineTo(Vec2::new(0.0, 0.0)),
            ]
        );
    }

    #[test]
    fn hiding_the_closing_edge_opens_the_shape() {
        let (out, _) = square().without_hidden_edges(&[false, false, false, false, true], None);
        assert_eq!(
            out.cmds,
            [
                PathCmd::MoveTo(Vec2::new(0.0, 0.0)),
                PathCmd::LineTo(Vec2::new(10.0, 0.0)),
                PathCmd::LineTo(Vec2::new(10.0, 10.0)),
                PathCmd::LineTo(Vec2::new(0.0, 10.0)),
            ]
        );
    }

    #[test]
    fn the_width_profile_follows_the_points_that_are_drawn() {
        // points 0..=3 with widths 1, 2, 3, 4; the side from point 1 to point 2 is hidden
        let widths = [1.0, 2.0, 3.0, 4.0];
        let (out, w) = square().without_hidden_edges(&[false, false, true, false, false], Some(&widths));
        // a piece 0 -> 1, then a piece 2 -> 3 -> 0
        assert_eq!(out.on_curve_points().len(), 5);
        assert_eq!(w, Some(vec![1.0, 2.0, 3.0, 4.0, 1.0]));
    }

    #[test]
    fn every_edge_hidden_leaves_nothing() {
        let (out, _) = square().without_hidden_edges(&[false, true, true, true, true], None);
        assert!(out.cmds.is_empty());
    }
}
