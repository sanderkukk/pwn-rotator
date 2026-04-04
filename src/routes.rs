//! Axum HTTP route handlers.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{config::Config, rotator::{Rotator, RotatorError}};

/// Shared application state injected into every handler.
#[derive(Clone)]
pub struct AppState {
    pub rotator: Rotator,
    pub config: Config,
}

// ── error conversion ────────────────────────────────────────────────────────

pub(crate) struct ApiError(RotatorError);

impl From<RotatorError> for ApiError {
    fn from(e: RotatorError) -> Self {
        Self(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            RotatorError::OutOfRange(_) => StatusCode::UNPROCESSABLE_ENTITY,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = serde_json::json!({ "error": self.0.to_string() });
        (status, Json(body)).into_response()
    }
}

// ── response types ───────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct PositionResponse {
    pub azimuth: f32,
    pub elevation: f32,
}

#[derive(Serialize)]
pub struct AzimuthResponse {
    pub azimuth: f32,
}

#[derive(Serialize)]
pub struct ConfigResponse {
    pub device: String,
    pub baud_rate: u32,
    pub listen_addr: String,
}

// ── request types ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct RotateRequest {
    pub azimuth: u16,
    pub elevation: Option<u16>,
}

// ── handlers ─────────────────────────────────────────────────────────────────

/// `GET /status` – return current azimuth and elevation.
pub async fn get_status(State(state): State<AppState>) -> Result<Json<PositionResponse>, ApiError> {
    let (azimuth, elevation) = tokio::task::spawn_blocking(move || {
        state.rotator.get_position()
    })
    .await
    .expect("blocking task panicked")?;

    Ok(Json(PositionResponse { azimuth, elevation }))
}

/// `POST /rotate` – rotate to the requested azimuth (and optionally elevation).
///
/// Body: `{ "azimuth": 180 }` or `{ "azimuth": 180, "elevation": 45 }`
pub async fn post_rotate(
    State(state): State<AppState>,
    Json(payload): Json<RotateRequest>,
) -> Result<StatusCode, ApiError> {
    tokio::task::spawn_blocking(move || {
        if let Some(el) = payload.elevation {
            state.rotator.set_position(payload.azimuth, el)
        } else {
            state.rotator.set_azimuth(payload.azimuth)
        }
    })
    .await
    .expect("blocking task panicked")?;

    Ok(StatusCode::ACCEPTED)
}

/// `POST /stop` – stop all rotator movement.
pub async fn post_stop(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    tokio::task::spawn_blocking(move || state.rotator.stop())
        .await
        .expect("blocking task panicked")?;

    Ok(StatusCode::OK)
}

/// `GET /config` – return current runtime configuration (read-only).
pub async fn get_config(State(state): State<AppState>) -> Json<ConfigResponse> {
    Json(ConfigResponse {
        device: state.config.device.clone(),
        baud_rate: state.config.baud_rate,
        listen_addr: state.config.listen_addr.clone(),
    })
}
