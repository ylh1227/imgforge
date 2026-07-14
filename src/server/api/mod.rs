//! axum REST 控制面路由。

mod routes;

use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use uuid::Uuid;

use crate::remote::contract::{RemoteApiErrorBody, RemoteApiErrorCode};
use crate::server::state::AppState;
use crate::server::storage::StoreError;

pub use routes::build_router;

pub fn app(state: AppState) -> Router {
    build_router(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}

pub fn new_request_id() -> String {
    Uuid::new_v4().to_string()
}

pub fn request_id_from_headers(headers: &HeaderMap) -> String {
    headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(new_request_id)
}

pub struct ApiError {
    pub status: StatusCode,
    pub body: RemoteApiErrorBody,
}

impl ApiError {
    pub fn from_store(err: StoreError, request_id: impl Into<String>) -> Self {
        let status = match &err {
            StoreError::NotFound(_) => StatusCode::NOT_FOUND,
            StoreError::Conflict(_) => StatusCode::CONFLICT,
            StoreError::Validation(_) => StatusCode::BAD_REQUEST,
            StoreError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self {
            status,
            body: err.into_api().request_id(request_id),
        }
    }

    pub fn unauthorized(request_id: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            body: RemoteApiErrorBody::new(RemoteApiErrorCode::Unauthorized, "unauthorized")
                .request_id(request_id),
        }
    }

    pub fn forbidden(request_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            body: RemoteApiErrorBody::new(RemoteApiErrorCode::Forbidden, message)
                .request_id(request_id),
        }
    }

    pub fn rate_limited(request_id: impl Into<String>) -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            body: RemoteApiErrorBody::new(RemoteApiErrorCode::RateLimited, "rate limited")
                .request_id(request_id),
        }
    }

    pub fn other(
        status: StatusCode,
        code: RemoteApiErrorCode,
        message: impl Into<String>,
        request_id: impl Into<String>,
    ) -> Self {
        Self {
            status,
            body: RemoteApiErrorBody::new(code, message).request_id(request_id),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut resp = (self.status, Json(self.body.clone())).into_response();
        if let Some(id) = &self.body.request_id {
            if let Ok(v) = HeaderValue::from_str(id) {
                resp.headers_mut().insert("x-request-id", v);
            }
        }
        resp
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
