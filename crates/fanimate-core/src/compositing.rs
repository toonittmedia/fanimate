//! Compositing: how a layer's picture is combined with the others.
//!
//! Most layers are drawn straight onto what is under them. These settings need
//! a layer or a group drawn on its own first and laid over the rest:
//!
//! - **Blend** (multiply, screen, add): the way the picture mixes with what is under it.
//! - **Mask** (a layer that is a mask, adding to it or subtracting from it): the
//!   mask layers of a group together decide where its other layers show.
//!   Mask layers are not drawn themselves. A layer marked **unmasked** is not
//!   held back by its group's mask.
//! - **Clip** (clip to the layer below): the layer shows only where the layer
//!   under it (its base) does, as a clipping mask in a paint program. A base and the
//!   layers clipped to it are one picture.
//! - **Flatten** (a group): draw the group as one picture, then fade it as one,
//!   so shapes that overlap inside it do not show through each other.
//!
//! None of it is needed for a plain document, which evaluates and draws exactly
//! as before. The evaluator turns it into [`Scope`]s: pictures that are drawn
//! on their own and then laid over their parent. Every draw item says which
//! scope it belongs to and how it is laid down in it ([`Ink`]).

use serde::{Deserialize, Serialize};

/// How a layer's picture mixes with what is under it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Blend {
    /// Laid over, as paint is.
    #[default]
    Normal,
    /// Darkens: each colour is multiplied with the one under it.
    Multiply,
    /// Lightens: the opposite of multiply.
    Screen,
    /// Adds the colours, for glows and light.
    Add,
}

impl Blend {
    pub const ALL: [Blend; 4] = [Self::Normal, Self::Multiply, Self::Screen, Self::Add];

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Multiply => "Multiply",
            Self::Screen => "Screen",
            Self::Add => "Add",
        }
    }
}

/// Whether a layer is part of its group's mask, and how.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaskRole {
    /// An ordinary layer, drawn.
    #[default]
    None,
    /// Not drawn: what it covers is let through.
    Add,
    /// Not drawn: what it covers is held back, out of what the other mask layers let through.
    Subtract,
}

impl MaskRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Not a mask",
            Self::Add => "Add to mask",
            Self::Subtract => "Subtract from mask",
        }
    }
}

/// A layer's compositing settings. All off by default, and then not written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Composite {
    #[serde(skip_serializing_if = "is_normal")]
    pub blend: Blend,
    #[serde(skip_serializing_if = "is_no_mask")]
    pub mask: MaskRole,
    /// Not held back by the group's mask.
    #[serde(skip_serializing_if = "is_false")]
    pub unmasked: bool,
    /// Show only where the layer below does.
    #[serde(skip_serializing_if = "is_false")]
    pub clip: bool,
    /// For a group (or a switch or bone layer): draw it as one picture, then fade it as one.
    #[serde(skip_serializing_if = "is_false")]
    pub flatten: bool,
}

fn is_normal(b: &Blend) -> bool {
    *b == Blend::Normal
}
fn is_no_mask(m: &MaskRole) -> bool {
    *m == MaskRole::None
}
fn is_false(b: &bool) -> bool {
    !*b
}

impl Composite {
    /// Nothing is set: the layer is drawn straight onto what is under it.
    pub fn is_plain(&self) -> bool {
        *self == Self::default()
    }
}

/// A picture of its own: drawn separately, then laid over its parent (or, for
/// a mask, used to let the pictures that name it through).
pub type ScopeId = u32;

/// The picture everything is drawn into unless a scope says otherwise.
pub const ROOT_SCOPE: ScopeId = 0;

/// How a draw item is laid down in its scope.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ink {
    /// Over what is already there.
    #[default]
    Over,
    /// Only where what is already there is drawn, a clipped layer: the picture
    /// keeps its own coverage and the clipped layer takes its colour there.
    Atop,
    /// Takes away what is there: a mask layer that subtracts.
    Erase,
}

/// What a scope is for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScopeKind {
    /// The whole picture. Always scope 0.
    Root,
    /// A picture laid over its parent with `opacity`, mixed as `blend`, and
    /// held to where `mask` (a [`ScopeKind::Mask`] scope) lets it through.
    Layer {
        opacity: f32,
        blend: Blend,
        mask: Option<ScopeId>,
    },
    /// What a group's mask layers cover; never laid over anything.
    Mask,
}

/// A scope and the one it is drawn into. A scope's id is greater than its
/// parent's, and its items come after the items drawn before it was opened.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scope {
    pub parent: ScopeId,
    pub kind: ScopeKind,
}

impl Scope {
    pub const ROOT: Scope = Scope {
        parent: ROOT_SCOPE,
        kind: ScopeKind::Root,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_settings_are_not_written_and_every_other_one_reads_back() {
        let plain = Composite::default();
        assert!(plain.is_plain());
        assert_eq!(serde_json::to_string(&plain).unwrap(), "{}");
        let all = Composite {
            blend: Blend::Multiply,
            mask: MaskRole::Subtract,
            unmasked: true,
            clip: true,
            flatten: true,
        };
        let text = serde_json::to_string(&all).unwrap();
        assert_eq!(
            text,
            r#"{"blend":"multiply","mask":"subtract","unmasked":true,"clip":true,"flatten":true}"#
        );
        assert_eq!(serde_json::from_str::<Composite>(&text).unwrap(), all);
        // a file with nothing in it, or one setting, is read with the rest off
        assert!(serde_json::from_str::<Composite>("{}").unwrap().is_plain());
        let one: Composite = serde_json::from_str(r#"{"clip":true}"#).unwrap();
        assert!(one.clip && one.blend == Blend::Normal && !one.is_plain());
    }
}
