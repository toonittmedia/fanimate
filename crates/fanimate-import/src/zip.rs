//! ZIP archives, the container of `.moho` and `.fla` files. The reader and
//! writer are shared with the single-file `.fnm` format and live in
//! [`fanimate_fnm::zip`].

pub use fanimate_fnm::zip::{Archive, is_zip, write};
