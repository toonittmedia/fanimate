//! Models for Fanimate's optional AI tools, and the rules a model must meet.
//!
//! This crate is the model side of workstream 7: it decides **which models
//! may exist** (the policy, decision D2-6), checks them (signed, checksummed
//! packages) and keeps them (the models folder). It does not run them: the
//! runtime is a trait with no implementation bundled yet (decision D2-5), and
//! the classical tools (line clean-up, inbetween assist, auto-rig) need no
//! model at all.
//!
//! - [`policy`]: permissive licences only, documented provenance, commissioned
//!   or donated data, no scraped art; one file, `models/policy.json`, read by
//!   this crate and by `scripts/models.py`.
//! - [`manifest`]: what a model says about itself.
//! - [`package`]: `.fnmodel` files, signed with the same Ed25519 scheme as
//!   updates; [`package::verify`] refuses anything that fails a check.
//! - [`store`]: install, list, remove and load, re-checking on every load.
//! - [`runtime`]: the trait, and `NoRuntime`.
//!
//! No UI, GPU or project code lives here, and nothing here touches the
//! network: a model is a file the user chooses to install.

pub mod manifest;
pub mod package;
pub mod policy;
pub mod runtime;
pub mod store;

pub use manifest::{ModelManifest, Provenance, Task};
pub use package::{
    ModelError, Verified, hex, key_from_hex, pack, public_key, seed_from_base64, sha256_hex, verify,
};
pub use policy::{Consent, check_licence};
pub use runtime::{NoRuntime, Runtime, Session, Tensor};
pub use store::{Installed, Loaded, ModelStore, user_folder};
