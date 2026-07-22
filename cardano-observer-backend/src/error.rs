use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// API error rendered as the standard JSON error envelope:
/// `{ "status_code": ..., "error": ..., "message": ... }`.
#[derive(Debug)]
pub enum ApiError {
    BadRequest(String),
    NotFound,
    Internal(anyhow::Error),
}

impl ApiError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        ApiError::BadRequest(msg.into())
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        ApiError::Internal(e.into())
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError::Internal(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, error, message) = match self {
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, "Bad Request", msg),
            ApiError::NotFound => (
                StatusCode::NOT_FOUND,
                "Not Found",
                "The requested component has not been found.".to_string(),
            ),
            ApiError::Internal(e) => {
                tracing::error!("internal error: {e:#}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal Server Error",
                    "An unexpected response was received from the backend.".to_string(),
                )
            }
        };
        (
            status,
            Json(json!({
                "status_code": status.as_u16(),
                "error": error,
                "message": message,
            })),
        )
            .into_response()
    }
}
