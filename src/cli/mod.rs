//! Inline terminal presentation and multiline editing.
mod editor;
mod screen;
mod style;
mod tools;
mod welcome;
pub(crate) use editor::read;
pub(crate) use screen::Screen;
pub(crate) use tools::{saved_list, saved_output};
