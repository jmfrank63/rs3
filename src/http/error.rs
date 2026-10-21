//! HTTP error type.
//!
//! Every handler error becomes a JSON object `{"error": "<code>", "message": "<detail>"}`
//! with content type `application/json` and a status chosen per variant. The
//! codes are `not_found` (404), `malformed_json` (400), `invalid_input` (422),
//! `js_error` (422, the message is only the JS exception message),
//! `js_depth_limit` (422, the message names the limit and the PATCH key chain),
//! `js_timeout` (504, the message names the execution limit in milliseconds)
//! and `internal` (500, fixed message `internal error`; details go to stderr).
use actix_web::{HttpResponse, ResponseError, http::StatusCode};

use crate::js::ExecError;
use crate::store::StoreError;

/// Errors a handler can return; each maps to one status code and machine code.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error(transparent)]
    Store(#[from] StoreError),
    /// A stored script threw, failed to parse or left an op error uncaught.
    /// Carries only the JS exception message; the stack went to stderr.
    #[error("{0}")]
    JsError(String),
    /// A PATCH chain exceeded the nesting depth limit. Carries the message
    /// naming the limit and the key chain.
    #[error("{0}")]
    JsDepthLimit(String),
    /// A script ran past the execution timeout and was terminated. Carries
    /// the message naming the limit.
    #[error("{0}")]
    JsTimeout(String),
    /// A host fault. The body message is always the fixed `internal error`.
    #[error("internal error")]
    Internal,
}

impl From<ExecError> for ApiError {
    /// A script failure becomes 422 `js_error` with the JS exception message
    /// (the runner already wrote the stack to stderr). A depth stop becomes
    /// 422 `js_depth_limit` with its message (the op already logged it once).
    /// A timeout becomes 504 `js_timeout` with its message (the JS worker or
    /// the backstop already logged it once).
    /// A host fault is logged to stderr and becomes 500 `internal`, so host
    /// details never reach the client.
    fn from(e: ExecError) -> Self {
        match e {
            ExecError::Script { message } => ApiError::JsError(message),
            ExecError::DepthLimit { .. } => ApiError::JsDepthLimit(e.to_string()),
            ExecError::Timeout { .. } => ApiError::JsTimeout(e.to_string()),
            ExecError::Unavailable | ExecError::Internal(_) => {
                // `Internal` already displays with this prefix; log its bare detail.
                let detail = match e {
                    ExecError::Internal(m) => m,
                    other => other.to_string(),
                };
                eprintln!("error: JS host failure: {detail}");
                ApiError::Internal
            }
        }
    }
}

/// Wire shape of every error body.
#[derive(serde::Serialize)]
struct ErrorBody<'a> {
    error: &'a str,
    message: String,
}

impl ApiError {
    /// The machine-readable code placed in the `error` field.
    pub fn code(&self) -> &'static str {
        match self {
            ApiError::Store(StoreError::NotFound(_)) => "not_found",
            ApiError::Store(StoreError::MalformedJson(_)) => "malformed_json",
            ApiError::Store(StoreError::InvalidInput(_)) => "invalid_input",
            ApiError::JsError(_) => "js_error",
            ApiError::JsDepthLimit(_) => "js_depth_limit",
            ApiError::JsTimeout(_) => "js_timeout",
            ApiError::Internal => "internal",
        }
    }
}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        match self {
            ApiError::Store(StoreError::NotFound(_)) => StatusCode::NOT_FOUND,
            ApiError::Store(StoreError::MalformedJson(_)) => StatusCode::BAD_REQUEST,
            ApiError::Store(StoreError::InvalidInput(_)) => StatusCode::UNPROCESSABLE_ENTITY,
            ApiError::JsError(_) => StatusCode::UNPROCESSABLE_ENTITY,
            ApiError::JsDepthLimit(_) => StatusCode::UNPROCESSABLE_ENTITY,
            ApiError::JsTimeout(_) => StatusCode::GATEWAY_TIMEOUT,
            ApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code()).json(ErrorBody {
            error: self.code(),
            message: self.to_string(),
        })
    }
}
