use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

/// Library errors for discover / funnel / bind / emit.
/// The CLI maps these into [`anyhow::Error`] at the boundary.
#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error("failed to read `{path}`: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid JSON in `{path}`: {source}")]
    InvalidJson {
        path: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("{0}")]
    Msg(String),

    #[error("{path}: {message}")]
    Page { path: String, message: String },

    #[error("missing data source id `{id}` (no <script type=\"statica/data\" id=\"{id}\">)")]
    MissingData { id: String },

    #[error("missing fragment id `{id}` (no <link rel=\"statica/fragment\" id=\"{id}\">)")]
    MissingFragment { id: String },

    #[error("fragment `{id}` has no matching <template id=\"{id}\"> in `{path}`")]
    MissingTemplate { id: String, path: String },

    #[error("path not found: {path}")]
    PathNotFound { path: String },

    #[error("collection item missing field `{field}` required by route `[{field}]`")]
    MissingRouteField { field: String },

    #[error("duplicate collection value for `[{field}]`: `{value}`")]
    DuplicateRouteValue { field: String, value: String },

    #[error("data-each for `{id}` expected a JSON array")]
    ExpectedArray { id: String },

    #[error(
        "fragment `{id}` data-bind=`{prop}` is invalid (expected a JS identifier or destructure like `{{variant, href}}`)"
    )]
    InvalidBindProp { id: String, prop: String },

    #[error(
        "fragment `{id}` uses `{path}` but `{name}` is not bound — declare it in data-bind (e.g. data-bind=\"{name}\" or data-bind=\"{{{name}}}\")"
    )]
    UnboundTemplateVar {
        id: String,
        /// Full `${path}` as authored.
        path: String,
        /// Root name that must appear in `data-bind`.
        name: String,
    },
}

impl Error {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Msg(s.into())
    }

    pub fn page(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Page {
            path: path.into(),
            message: message.into(),
        }
    }

    pub fn read(path: impl Into<String>, source: std::io::Error) -> Self {
        Self::Read {
            path: path.into(),
            source,
        }
    }

    pub fn invalid_json(path: impl Into<String>, source: serde_json::Error) -> Self {
        Self::InvalidJson {
            path: path.into(),
            source,
        }
    }
}
