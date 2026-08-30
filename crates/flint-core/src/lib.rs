//! Flint core — the judge layer (kernel-agnostic; no `punkgo-kernel`/`tokio` dep).
//!
//! The PROJECTOR (`projector`, `registry`, `merkle`, `claim`, `eval_threshold`)
//! is a deterministic PURE function of the kernel log — that purity is spec §13.1
//! ("recomputable on any machine") and is what `flint check` relies on.
//!
//! NOTE (codex P2-R2-1, honest scoping): the crate ALSO contains IO helpers —
//! `verifier` (shell-executes methods) and `content_store` (filesystem CAS). They
//! are NOT pure, and the projector's TRUST decision never calls them; they only
//! run methods / freeze artifacts for propose/verify/check. So "pure function of
//! the kernel log" describes the PROJECTOR, not the whole crate. The ledger
//! backend coupling lives in `flint-kernel`.

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
