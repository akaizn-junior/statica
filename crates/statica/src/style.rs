//! Terminal colors via `owo-colors` (respects TTY / `NO_COLOR` / `FORCE_COLOR`).

use owo_colors::{OwoColorize, Stream};

pub fn success(s: impl AsRef<str>) -> String {
    let s = s.as_ref();
    format!("{}", s.if_supports_color(Stream::Stderr, |t| t.green()))
}

pub fn warn(s: impl AsRef<str>) -> String {
    let s = s.as_ref();
    format!("{}", s.if_supports_color(Stream::Stderr, |t| t.yellow()))
}

pub fn error(s: impl AsRef<str>) -> String {
    let s = s.as_ref();
    format!("{}", s.if_supports_color(Stream::Stderr, |t| t.red()))
}

pub fn accent(s: impl AsRef<str>) -> String {
    let s = s.as_ref();
    format!("{}", s.if_supports_color(Stream::Stderr, |t| t.cyan()))
}

pub fn dim(s: impl AsRef<str>) -> String {
    let s = s.as_ref();
    format!("{}", s.if_supports_color(Stream::Stderr, |t| t.dimmed()))
}

pub fn bold(s: impl AsRef<str>) -> String {
    let s = s.as_ref();
    format!("{}", s.if_supports_color(Stream::Stderr, |t| t.bold()))
}

pub fn bold_stdout(s: impl AsRef<str>) -> String {
    let s = s.as_ref();
    format!("{}", s.if_supports_color(Stream::Stdout, |t| t.bold()))
}
