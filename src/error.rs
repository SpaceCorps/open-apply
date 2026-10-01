//! Exit codes double as the machine-readable `code` field in the error envelope. An agent
//! branches on these, so they must stay stable.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ErrorCode {
    Ok = 0,
    Internal = 1,
    Usage = 2,
    NotFound = 3,
    Validation = 4,
    Network = 5,
    Guardrail = 6,
    Conflict = 7,
}

impl ErrorCode {
    pub fn name(self) -> &'static str {
        match self {
            ErrorCode::Ok => "ok",
            ErrorCode::Internal => "internal",
            ErrorCode::Usage => "usage",
            ErrorCode::NotFound => "not_found",
            ErrorCode::Validation => "validation",
            ErrorCode::Network => "network",
            ErrorCode::Guardrail => "guardrail",
            ErrorCode::Conflict => "conflict",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Error {
    pub code: ErrorCode,
    pub message: String,
    /// What to do next: a literal command or a plain instruction. Agents surface it verbatim.
    pub hint: Option<String>,
    /// Extra structured context (for example the id of the job a duplicate collided with).
    pub detail: Option<Box<serde_json::Value>>,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Error { code, message: message.into(), hint: None, detail: None }
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn detail(mut self, detail: serde_json::Value) -> Self {
        self.detail = Some(Box::new(detail));
        self
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Error::new(ErrorCode::Usage, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Error::new(ErrorCode::NotFound, message)
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Error::new(ErrorCode::Validation, message)
    }

    pub fn network(message: impl Into<String>) -> Self {
        Error::new(ErrorCode::Network, message)
    }

    pub fn guardrail(message: impl Into<String>) -> Self {
        Error::new(ErrorCode::Guardrail, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Error::new(ErrorCode::Conflict, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Error::new(ErrorCode::Internal, message)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::NotFound => Error::not_found(e.to_string()),
            _ => Error::internal(format!("io error: {e}")),
        }
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::internal(format!("database error: {e}"))
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::validation(format!("invalid JSON: {e}"))
    }
}

impl From<serde_norway::Error> for Error {
    fn from(e: serde_norway::Error) -> Self {
        Error::validation(format!("invalid YAML: {e}"))
    }
}
