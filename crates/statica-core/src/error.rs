use thiserror::Error;

use crate::loc::Diagnostic;

pub type Result<T> = std::result::Result<T, Error>;

/// Library errors for discover / funnel / bind / emit.
/// The CLI maps these into [`anyhow::Error`] at the boundary.
///
/// Site authoring problems use [`Error::Diag`] (`file:line:column` + snippet).
#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    /// Authoring / site diagnostic with `file:line:column` (+ optional snippet).
    #[error(transparent)]
    Diag(#[from] Diagnostic),

    #[error("failed to read `{path}`: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

impl Error {
    pub fn msg(s: impl Into<String>) -> Self {
        // No file context — prefer [`Error::at`] / [`Error::at_file`] for site errors.
        Self::Diag(Diagnostic::at_file("<unknown>", s))
    }

    pub fn at(file: &str, source: &str, needles: &[&str], message: impl Into<String>) -> Self {
        Self::Diag(Diagnostic::at(file, source, needles, message))
    }

    pub fn at_file(file: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Diag(Diagnostic::at_file(file, message))
    }

    /// Attach page/file location when the error is not already a [`Diagnostic`].
    #[must_use]
    pub fn in_file(self, file: &str, source: &str) -> Self {
        self.in_file_at(file, source, &[])
    }

    /// Like [`Error::in_file`], but search `needles` for a better caret.
    #[must_use]
    pub fn in_file_at(self, file: &str, source: &str, needles: &[&str]) -> Self {
        match self {
            Self::Diag(d) => Self::Diag(d),
            other => Self::Diag(Diagnostic::at(file, source, needles, other.to_string())),
        }
    }

    pub fn read(path: impl Into<String>, source: std::io::Error) -> Self {
        Self::Read {
            path: path.into(),
            source,
        }
    }

    pub fn invalid_js_value(path: impl Into<String>, message: impl Into<String>) -> Self {
        let path = path.into();
        Self::Diag(Diagnostic::at_file(
            path,
            format!("invalid JS value: {}", message.into()),
        ))
    }
}
