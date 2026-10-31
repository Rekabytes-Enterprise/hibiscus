//! Fullscreen terminal presentation. Inline CLI lives in crate::cli.
mod activity;
mod clipboard;
mod file_mentions;
mod markdown;
mod mermaid;
mod modal;
mod paint;
mod picker;
mod screen;
mod text_clipboard;
mod welcome;
pub(crate) use screen::Screen;
