//! Shared terminal services and frontend selection.
pub(crate) mod display;
pub(crate) mod format;
mod frontend;
pub(crate) mod input;
pub(crate) mod messages;
pub(crate) mod mode;
pub(crate) mod paste;
pub(crate) use frontend::Frontend;
pub(crate) mod terminal;
pub(crate) mod text;
pub(crate) mod tool;
pub(crate) mod welcome;
