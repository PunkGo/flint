//! Flint core — the judge layer: the signed rule Canon, the verdict engine, the trust
//! set, and the redacted receipt log. Kernel-agnostic and dependency-light (no async
//! runtime, no daemon) — it rides commodity harness hooks rather than owning a process.
//!
//! The enforcement path is deliberately small, and PIP-0001 freezes it against growth
//! toward a memory system: [`canon`] parses and validates the signed rule set, [`trust`]
//! decides whose signature counts, [`touchstone`] judges one action against the active
//! rules, [`striker`] compiles those rules out to each harness, and [`obslog`] writes the
//! redacted receipt. [`harness`], [`glob`] and [`config`] serve that path;
//! [`pit`] and [`memory`] are the capture side, which is knowledge and never judged.
//!
//! Four modules are a dormant outer ring, present but gaining no callers: [`forge`]
//! (evidence tiers), [`verifier`] (shell-runs a rule\'s falsifier method), [`content_store`]
//! (a filesystem CAS serving it), and [`model_veto`] (veto-only by construction — a model
//! may never be the judge). They perform IO; no verdict consults them.
pub mod budget;
pub mod canon;
pub mod config;
pub mod content_store;
pub mod forge;
pub mod glob;
pub mod harness;
pub mod memory;
pub mod model_veto;
pub mod obslog;
pub mod pit;
pub mod striker;
pub mod touchstone;
pub mod trust;
pub mod verifier;
