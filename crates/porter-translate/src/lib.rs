//! Generates Horizon native mod bundles from scanned Java mods.
//!
//! [`bundle::generate`] is the rule-based part of the translator (milestone M1): it
//! writes a horizon-mixins bundle for a client mod, with Horizon's hook points wired
//! to a `runtime.rs` module of stubs, and a `PORTING.md` checklist of everything
//! left. Translating method bodies (milestone M4) comes later.

pub mod bundle;
