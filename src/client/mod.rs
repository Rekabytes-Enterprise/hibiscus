//! The terminal's side of the engine connection: the in-process worker client,
//! bounded transport, run state, diagnostics and user-facing errors.
pub(crate) mod bridge;
pub(crate) mod diagnostics;
pub(crate) mod embed;
pub(crate) mod error;
mod run;
pub(crate) mod transport;

#[cfg(feature = "fuzzing")]
pub(crate) mod fuzzing;
