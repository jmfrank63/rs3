//! HTTP error type.
//!
//! Every handler error becomes a JSON object `{"error": "<code>", "message": "<detail>"}`
//! with content type `application/json` and a status chosen per variant. The
//! codes are `not_found` (404), `malformed_json` (400), `invalid_input` (422)
//! and `internal` (500, fixed message `internal error`).
use actix_web::{HttpResponse, ResponseError, http::StatusCode};

use crate::js::ExecError;
use crate::store::StoreError;

/// Errors a handler can return; each maps to one status code and machine code.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("internal error")]
    Internal,
}

impl From<ExecError> for ApiError {
    /// Log the JS failure to stderr and answer the fixed 500 body.
    fn from(e: ExecError) -> Self {
        eprintln!("error: JS execution failed: {e}");
        ApiError::Internal
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
