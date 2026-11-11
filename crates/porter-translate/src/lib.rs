//! Generates Horizon mods from scanned Java mods.
//!
//! [`bundle::generate`] is the rule-based part of the translator: a horizon-mixins
//! bundle (a native mod) with Horizon's hook points wired to a `runtime.rs` module of
//! stubs (milestone M1), a content mod with the Java mod's blocks, items and
//! resources (`porter-content`), block entities as saved state with tick stubs
//! ([`block_entity`], milestone M2), and a `PORTING.md` checklist of everything left.
//! [`ai`] translates the functions left to port with an AI model, checking each
//! with the compiler (milestone M4).

pub mod ai;
pub mod api;
pub mod block_entity;
pub mod blocks;
pub mod bundle;
pub mod events;
pub mod machine;
pub mod netconf;
pub mod refused;
pub mod screens;
pub mod statics;
/// The content mod, from the `porter-content` crate.
pub use porter_content as content;
