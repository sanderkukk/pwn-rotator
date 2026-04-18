//! Axum HTTP route handlers.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::{rotator::{Rotator, RotatorError}};

/// Shared application state injected into every handler.
#[derive(Clone)]
pub struct AppState {
    pub rotator: Rotator,
}

// ── error conversion ──────────────────────────────────────────────────────────

#[derive(Debug)]
pub(crate) struct ApiError(RotatorError);

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            RotatorError::OutOfRange(_) => StatusCode::UNPROCESSABLE_ENTITY,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(serde_json::json!({ "error": self.0.to_string() }))).into_response()
    }
}

impl From<RotatorError> for ApiError {
    fn from(e: RotatorError) -> Self {
        Self(e)
    }
}

// ── response / request types ──────────────────────────────────────────────────

#[derive(Serialize)]
pub struct PositionResponse {
    pub azimuth: f32,
    pub elevation: f32,
}

#[derive(Deserialize)]
pub struct RotateRequest {
    pub azimuth: u16,
    pub elevation: Option<u16>,
}

// ── handlers ──────────────────────────────────────────────────────────────────

/// `GET /status` – return current azimuth and elevation.
pub async fn get_status(
    State(state): State<AppState>,
) -> Result<Json<PositionResponse>, ApiError> {
    let (azimuth, elevation) = state.rotator.get_position().await?;
    Ok(Json(PositionResponse { azimuth, elevation }))
}

/// `POST /rotate` – rotate to the requested azimuth (and optionally elevation).
///
/// Body: `{ "azimuth": 180 }` or `{ "azimuth": 180, "elevation": 45 }`
pub async fn post_rotate(
    State(state): State<AppState>,
    Json(payload): Json<RotateRequest>,
) -> Result<StatusCode, ApiError> {
    if let Some(el) = payload.elevation {
        state.rotator.set_position(payload.azimuth, el).await?;
    } else {
        state.rotator.set_azimuth(payload.azimuth).await?;
    }
    Ok(StatusCode::ACCEPTED)
}

/// `POST /stop` – stop all rotator movement.
pub async fn post_stop(
    State(state): State<AppState>,
) -> Result<StatusCode, ApiError> {
    state.rotator.stop().await?;
    Ok(StatusCode::OK)
}
