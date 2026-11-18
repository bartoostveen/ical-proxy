use axum::http::StatusCode;
use std::fmt;

/// Errors that can be turned into an HTTP response by the server.
#[derive(Debug)]
pub enum Error {
    /// An error response with an explicit status code and body.
    Http { status: StatusCode, body: String },
    /// The requested calendar could not be fetched.
    NotFound,
}

impl Error {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::Http {
            status: StatusCode::BAD_REQUEST,
            body: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http { body, .. } => f.write_str(body),
            Self::NotFound => f.write_str("not found"),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
