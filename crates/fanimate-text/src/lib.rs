//! Fanimate's text engine.
//!
//! Turns a string, a font and a few settings into positioned glyphs
//! ([`layout`]) and their outlines ([`Outliner`]). It knows nothing of
//! documents, layers or animation: `fanimate-core` calls it when it
//! evaluates a text layer, and draws the glyphs as ordinary shapes, so the
//! renderer, the exporters and the browser player need no text code.
//!
//! No IO and no UI, and everything is deterministic, so a title lays out the
//! same on every platform. The fonts it can use are the ones bundled in
//! [`fonts`].

pub mod fonts;
mod layout;
mod outline;

pub use layout::{Align, Direction, Feature, Glyph, Laid, MAX_CHARS, Params, SIZE_RANGE, layout};
pub use outline::{Outliner, Seg};
