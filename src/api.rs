//! The HTTP API's request and response bodies and its typed errors.
//!
//! The rows they carry are `blotter_core::sync` types, the same the app
//! merges. These envelopes move into `blotter-core::sync` too once the app
//! has its client (Blotter #80), so both sides build against one copy.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

/// Every request but `hello` carries this side's sync protocol and schema.
pub const HEADER_PROTOCOL: &str = "x-blotter-protocol";
pub const HEADER_SCHEMA: &str = "x-blotter-schema";

/// The versions that must match for a device and the server to sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Versions {
    pub protocol: u32,
    pub schema: u32,
}

impl Versions {
    pub const SERVER: Versions = Versions {
        protocol: blotter_core::sync::SYNC_PROTOCOL,
        schema: blotter_core::sync::registry::SYNC_SCHEMA,
    };
}

/// `GET /v1/hello`, answered without authentication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub name: String,
    /// Blotter Sync's version.
    pub version: String,
    /// The Blotter release whose core this server was built with.
    pub blotter_core: String,
    pub server_id: String,
    #[serde(flatten)]
    pub versions: Versions,
    pub epoch: String,
    /// Whether a pairing code is waiting to be used.
    pub pairing_open: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The device's sync protocol differs from the server's.
    ProtocolMismatch,
    /// The device's sync schema differs from the server's.
    SchemaMismatch,
    /// No token, or one the server doesn't know.
    Unauthorized,
    /// This device's token was revoked.
    Revoked,
    /// No pairing code is open (`blotter-sync pair` makes one).
    PairingClosed,
    /// The pairing code is wrong.
    BadCode,
    /// The pairing code has expired or was used up.
    CodeExpired,
    /// Too many attempts from this address; try again shortly.
    RateLimited,
    /// The server's planner was replaced: bootstrap again.
    StaleEpoch,
    /// The request itself is malformed.
    Invalid,
    /// Something went wrong on the server.
    Internal,
}

impl ErrorCode {
    fn status(self) -> StatusCode {
        match self {
            ErrorCode::ProtocolMismatch | ErrorCode::SchemaMismatch => StatusCode::CONFLICT,
            ErrorCode::StaleEpoch => StatusCode::GONE,
            ErrorCode::Unauthorized | ErrorCode::Revoked => StatusCode::UNAUTHORIZED,
            ErrorCode::PairingClosed | ErrorCode::BadCode | ErrorCode::CodeExpired => {
                StatusCode::FORBIDDEN
            }
            ErrorCode::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            ErrorCode::Invalid => StatusCode::BAD_REQUEST,
            ErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

/// Every refusal's body. Version mismatches carry the server's versions, so
/// the app can say which side to update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<Versions>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError(pub ErrorBody);

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self(ErrorBody {
            code,
            message: message.into(),
            server: None,
        })
    }

    pub fn code(&self) -> ErrorCode {
        self.0.code
    }

    pub fn internal(err: impl std::fmt::Display) -> Self {
        tracing::error!("{err}");
        Self::new(
            ErrorCode::Internal,
            "the server ran into a problem; see its log",
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0.code.status(), Json(self.0)).into_response()
    }
}
