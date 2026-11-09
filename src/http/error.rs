//! HTTP error type.
//!
//! Every handler error becomes a JSON object `{"error": "<code>", "message": "<detail>"}`
//! with content type `application/json` and a status chosen per variant. The
//! codes are `not_found` (404), `malformed_json` (400), `invalid_input` (422),
//! `js_error` (422, the message is only the JS exception message),
//! `js_depth_limit` (422, the message names the limit and the PATCH key chain),
//! `js_timeout` (504, the message names the execution limit in milliseconds)
//! `goals_unavailable` (503, fixed message: the server has no database for goals)
//! `unavailable` (503, fixed message: the cluster could not commit or confirm the request)
//! `fragments_unavailable` (503, fixed message: a sealed goal's stream could not be rebuilt from enough fragments)
//! `no_quorum` (503, fixed message `no quorum`: no leader or majority answered within the deadline)
//! `outcome_unknown` (503, fixed message `outcome unknown`: a write was handed on and not answered, so it may
//! still commit; check before retrying)
//! and `internal` (500, fixed message `internal error`; details go to stderr).
//! An unknown goal answers `not_found` (404, `goal not found: {id}`), and so does an unknown session
//! (`session not found: {id}`); so does a `/run` turn whose key holds no script (`script not found: {prompt}`). `turn_conflict` (409) refuses a turn that skips ahead or changes the
//! prompt it was started with.
use actix_web::{HttpResponse, ResponseError, http::StatusCode};

use crate::journal::JournalError;
use crate::js::ExecError;
use crate::raft::ClusterError;
use crate::store::StoreError;

/// Errors a handler can return; each maps to one status code and machine code.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// A client-visible store error. Storage failures never get here: `From`
    /// logs them and answers `Internal`.
    #[error(transparent)]
    Store(StoreError),
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
    /// The server runs without a database, so it has no goals.
    #[error("goals need a database; set STORAGE.PATH")]
    GoalsUnavailable,
    /// No goal has this id (as given in the path).
    #[error("goal not found: {0}")]
    GoalNotFound(String),
    /// No session has this id (as given in the path).
    #[error("session not found: {0}")]
    SessionNotFound(String),
    /// A `/run` turn names a key that holds no script (carries the prompt).
    #[error("script not found: {0}")]
    ScriptNotFound(String),
    /// The turn skips ahead of its session, or was started before with another prompt.
    #[error("turn does not follow the session or its prompt differs from the recorded one")]
    TurnConflict,
    /// The cluster has no leader or no quorum to answer.
    #[error("unavailable")]
    Unavailable,
    /// A sealed stream could not be rebuilt: too few valid fragments were reachable.
    #[error("fragments unavailable")]
    FragmentsUnavailable,
    /// No leader or quorum answered within the request deadline.
    #[error("no quorum")]
    NoQuorum,
    /// A write was handed to the cluster and not answered: it may commit later, so check before retrying.
    #[error("outcome unknown")]
    OutcomeUnknown,
    /// A host fault. The body message is always the fixed `internal error`.
    #[error("internal error")]
    Internal,
}

impl From<StoreError> for ApiError {
    /// A storage failure is a host fault: it is logged to stderr and becomes 500
    /// `internal`, so host details never reach the client.
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::Storage(detail) => {
                eprintln!("error: store failure: {detail}");
                ApiError::Internal
            }
            other => ApiError::Store(other),
        }
    }
}

impl From<ClusterError> for ApiError {
    /// The detail goes to stderr; the client only learns the cluster is unavailable.
    fn from(e: ClusterError) -> Self {
        eprintln!("error: cluster unavailable: {e}");
        match e {
            ClusterError::NoQuorum => ApiError::NoQuorum,
            ClusterError::OutcomeUnknown => ApiError::OutcomeUnknown,
            _ => ApiError::Unavailable,
        }
    }
}

impl From<JournalError> for ApiError {
    /// A journal failure is logged to stderr. A cluster that cannot answer becomes 503
    /// `unavailable`; anything else is a host fault and becomes 500 `internal`, so host
    /// details never reach the client.
    fn from(e: JournalError) -> Self {
        eprintln!("error: journal failure: {e}");
        match e {
            JournalError::Unavailable(_) => ApiError::Unavailable,
            JournalError::FragmentsUnavailable { .. } => ApiError::FragmentsUnavailable,
            _ => ApiError::Internal,
        }
    }
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
            ApiError::Store(StoreError::Storage(_)) | ApiError::Internal => "internal",
            ApiError::JsError(_) => "js_error",
            ApiError::JsDepthLimit(_) => "js_depth_limit",
            ApiError::JsTimeout(_) => "js_timeout",
            ApiError::GoalsUnavailable => "goals_unavailable",
            ApiError::Unavailable => "unavailable",
            ApiError::FragmentsUnavailable => "fragments_unavailable",
            ApiError::NoQuorum => "no_quorum",
            ApiError::OutcomeUnknown => "outcome_unknown",
            ApiError::GoalNotFound(_)
            | ApiError::SessionNotFound(_)
            | ApiError::ScriptNotFound(_) => "not_found",
            ApiError::TurnConflict => "turn_conflict",
        }
    }
}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        match self {
            ApiError::Store(StoreError::NotFound(_)) => StatusCode::NOT_FOUND,
            ApiError::Store(StoreError::MalformedJson(_)) => StatusCode::BAD_REQUEST,
            ApiError::Store(StoreError::InvalidInput(_)) => StatusCode::UNPROCESSABLE_ENTITY,
            ApiError::Store(StoreError::Storage(_)) => StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::JsError(_) => StatusCode::UNPROCESSABLE_ENTITY,
            ApiError::JsDepthLimit(_) => StatusCode::UNPROCESSABLE_ENTITY,
            ApiError::JsTimeout(_) => StatusCode::GATEWAY_TIMEOUT,
            ApiError::GoalsUnavailable
            | ApiError::Unavailable
            | ApiError::NoQuorum
            | ApiError::FragmentsUnavailable
            | ApiError::OutcomeUnknown => StatusCode::SERVICE_UNAVAILABLE,
            ApiError::GoalNotFound(_)
            | ApiError::SessionNotFound(_)
            | ApiError::ScriptNotFound(_) => StatusCode::NOT_FOUND,
            ApiError::TurnConflict => StatusCode::CONFLICT,
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
