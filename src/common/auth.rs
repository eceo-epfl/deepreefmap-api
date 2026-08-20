//! Keycloak JWTs and device tokens, resolved to one identity.

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use chrono::Utc;
use moka::future::Cache;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use std::time::Duration;
use uuid::Uuid;

use crate::common::AppState;
use crate::common::tokens::{sha256_hex, split_device_token, verify_secret};
use crate::error::AppError;
use crate::routes::private::devices::model as device_model;

type KcStatus =
    axum_keycloak_auth::KeycloakAuthStatus<Role, axum_keycloak_auth::decode::ProfileAndEmail>;

/// Realm roles this API understands. Anything else grants nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    Member,
    Administrator,
    Unknown(String),
}

impl axum_keycloak_auth::role::Role for Role {}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Member => f.write_str("deepreefmap-member"),
            Self::Administrator => f.write_str("deepreefmap-admin"),
            Self::Unknown(role) => f.write_fmt(format_args!("Unknown role: {role}")),
        }
    }
}

impl From<String> for Role {
    fn from(value: String) -> Self {
        match value.as_str() {
            "deepreefmap-member" => Self::Member,
            "deepreefmap-admin" => Self::Administrator,
            _ => Self::Unknown(value),
        }
    }
}

impl Role {
    /// Whether the role is membership. A valid login alone is not.
    #[must_use]
    pub fn grants_access(&self) -> bool {
        matches!(self, Self::Member | Self::Administrator)
    }
}

/// What authored a request: a person, or an installation.
///
/// Carries no subject for a device, so attribution and authorisation cannot fall back
/// to whoever enrolled it.
#[derive(Debug, Clone, Copy)]
pub enum Origin<'a> {
    Human { sub: &'a str },
    Device { device_id: Uuid, name: &'a str },
}

/// Who is making the request.
#[derive(Debug, Clone)]
pub enum AuthContext {
    Keycloak {
        sub: String,
        email: Option<String>,
        roles: Vec<Role>,
    },
    Device {
        device_id: Uuid,
        device_name: String,
    },
}

impl AuthContext {
    /// The caller, as one of the two things it can be.
    #[must_use]
    pub fn origin(&self) -> Origin<'_> {
        match self {
            Self::Keycloak { sub, .. } => Origin::Human { sub },
            Self::Device {
                device_id,
                device_name,
            } => Origin::Device {
                device_id: *device_id,
                name: device_name,
            },
        }
    }

    /// The Keycloak subject, when a person is calling. `None` for a device.
    #[must_use]
    pub fn human_subject(&self) -> Option<&str> {
        match self {
            Self::Keycloak { sub, .. } => Some(sub),
            Self::Device { .. } => None,
        }
    }

    #[must_use]
    pub fn is_admin(&self) -> bool {
        match self {
            Self::Keycloak { roles, .. } => roles.contains(&Role::Administrator),
            Self::Device { .. } => false,
        }
    }
}

/// Validated device tokens, keyed by SHA-256 of the raw bearer value so a wrong
/// secret cannot hit an entry.
pub type DeviceTokenCache = Cache<String, device_model::Model>;

#[must_use]
pub fn new_device_token_cache(ttl_seconds: u64) -> DeviceTokenCache {
    Cache::builder()
        .max_capacity(1_000)
        .time_to_live(Duration::from_secs(ttl_seconds.max(1)))
        .build()
}

/// Keep the realm roles and drop the client ones.
///
/// `KeycloakRole::role()` unwraps either variant identically, so taking both would let any
/// client in the realm define a role named `deepreefmap-admin` and have it granted here.
fn realm_roles(roles: &[axum_keycloak_auth::role::KeycloakRole<Role>]) -> Vec<Role> {
    roles
        .iter()
        .filter_map(|role| match role {
            axum_keycloak_auth::role::KeycloakRole::Realm { role } => Some(role.clone()),
            axum_keycloak_auth::role::KeycloakRole::Client { .. } => None,
        })
        .collect()
}

/// Resolve the request's identity from a Keycloak JWT or a device token.
///
/// Sits behind the Keycloak layer in pass-through mode and is the only gate on the
/// routes it wraps.
pub async fn auth_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    // Read the claims out before the mutable borrow below.
    let keycloak = match request.extensions().get::<KcStatus>() {
        Some(axum_keycloak_auth::KeycloakAuthStatus::Success(token)) => {
            let roles = realm_roles(&token.roles);
            let raw_email = token.extra.email.email.trim();
            let email = (!raw_email.is_empty()).then(|| raw_email.to_string());
            Some((token.subject.clone(), email, roles, token.issuer.clone()))
        }
        // A missing or failed JWT may still be a device token.
        _ => None,
    };

    if let Some((sub, email, roles, issuer)) = keycloak {
        let accepted = state.config.expected_issuers();
        if !accepted.is_empty() && !accepted.iter().any(|a| a == &issuer) {
            tracing::warn!(%issuer, ?accepted, "Rejected a JWT from another issuer");
            return AppError::Unauthorized("Token issued by another realm".to_string())
                .into_response();
        }
        if !roles.iter().any(Role::grants_access) {
            // Distinct from a 401, which would send an entitled-looking user hunting
            // for a credential problem.
            tracing::info!(%sub, "Keycloak login without a deepreefmap role");
            return AppError::Forbidden("no_deepreefmap_role".to_string()).into_response();
        }
        request
            .extensions_mut()
            .insert(AuthContext::Keycloak { sub, email, roles });
        return next.run(request).await;
    }

    let bearer = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .map(String::from);

    if let Some(raw_token) = bearer
        && let Some(device) = validate_device_token(&state, &raw_token).await
    {
        request.extensions_mut().insert(AuthContext::Device {
            device_id: device.id,
            device_name: device.name.clone(),
        });
        return next.run(request).await;
    }

    AppError::Unauthorized("Authentication required".to_string()).into_response()
}

async fn validate_device_token(state: &AppState, raw_token: &str) -> Option<device_model::Model> {
    // Before the cache, the database, or argon2.
    split_device_token(raw_token)?;

    let cache_key = sha256_hex(raw_token);
    if let Some(cached) = state.device_token_cache.get(&cache_key).await {
        if cached.revoked_at.is_some() {
            state.device_token_cache.invalidate(&cache_key).await;
            return None;
        }
        touch_last_seen(&state.db, cached.id);
        return Some(cached);
    }

    let (prefix, secret) = split_device_token(raw_token)?;
    let device = device_model::Entity::find()
        .filter(device_model::Column::TokenPrefix.eq(prefix))
        .filter(device_model::Column::RevokedAt.is_null())
        .one(&state.db)
        .await
        .ok()??;

    if !verify_secret(secret, &device.token_hash) {
        return None;
    }

    state
        .device_token_cache
        .insert(cache_key, device.clone())
        .await;
    touch_last_seen(&state.db, device.id);
    Some(device)
}

/// Best-effort `last_seen_at` bump. Must never fail the request it observes.
fn touch_last_seen(db: &DatabaseConnection, device_id: Uuid) {
    let db = db.clone();
    tokio::spawn(async move {
        let update = device_model::ActiveModel {
            id: Set(device_id),
            last_seen_at: Set(Some(Utc::now())),
            ..Default::default()
        };
        let _ = update.update(&db).await;
    });
}

/// Refuse a delete to anyone but an administrator.
///
/// A tombstone cascades in meaning, since deleting a site orphans its transects in every
/// client that has already pulled them. Non-delete methods pass through, so members keep
/// authoring survey metadata.
pub async fn require_admin_delete(request: Request, next: Next) -> Response {
    if request.method() != axum::http::Method::DELETE {
        return next.run(request).await;
    }
    require_admin(
        request,
        next,
        "Deleting requires the deepreefmap-admin role",
        "Devices cannot delete: push a row with deleted_at set to /api/sync/push",
    )
    .await
}

async fn require_admin(
    request: Request,
    next: Next,
    human_denial: &str,
    device_denial: &str,
) -> Response {
    match request.extensions().get::<AuthContext>() {
        Some(context) if context.is_admin() => next.run(request).await,
        Some(AuthContext::Device { .. }) => {
            AppError::Forbidden(device_denial.to_string()).into_response()
        }
        Some(AuthContext::Keycloak { .. }) => {
            AppError::Forbidden(human_denial.to_string()).into_response()
        }
        None => AppError::Unauthorized("Authentication required".to_string()).into_response(),
    }
}

/// Require a signed-in person, not a device.
///
/// Guards credential minting and device naming: a device must not enrol further devices
/// nor rewrite the name it is attributed under.
pub async fn require_human(request: Request, next: Next) -> Response {
    human_only(request, next, "This endpoint requires an interactive login").await
}

/// Require a device token, not a person.
///
/// Guards the ingest. A push binds provenance from the credential and resolves conflicts
/// on the client's clock, so a person reaching it would hold both write and delete over
/// every row the console authored.
pub async fn require_device(request: Request, next: Next) -> Response {
    match request.extensions().get::<AuthContext>() {
        Some(AuthContext::Device { .. }) => next.run(request).await,
        Some(AuthContext::Keycloak { .. }) => AppError::Forbidden(
            "Pushing is for enrolled devices: edit these rows through the CRUD routes".to_string(),
        )
        .into_response(),
        None => AppError::Unauthorized("Authentication required".to_string()).into_response(),
    }
}

/// Refuse the generated CRUD routes to a device.
///
/// A device's capability set is the sync protocol. Reaching the console's routes would let
/// a token act with a member's rights.
pub async fn deny_device_crud(request: Request, next: Next) -> Response {
    human_only(
        request,
        next,
        "Devices speak only the sync protocol: use /api/sync/push and /api/sync/pull",
    )
    .await
}

async fn human_only(request: Request, next: Next, device_denial: &str) -> Response {
    match request.extensions().get::<AuthContext>() {
        Some(AuthContext::Keycloak { .. }) => next.run(request).await,
        Some(AuthContext::Device { .. }) => {
            AppError::Forbidden(device_denial.to_string()).into_response()
        }
        None => AppError::Unauthorized("Authentication required".to_string()).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum_keycloak_auth::role::KeycloakRole;

    #[test]
    fn test_client_roles_are_not_realm_roles() {
        let roles = vec![
            KeycloakRole::Realm { role: Role::Member },
            KeycloakRole::Client {
                client: "some-other-app".to_string(),
                role: Role::Administrator,
            },
        ];
        assert_eq!(realm_roles(&roles), vec![Role::Member]);
    }

    #[test]
    fn test_realm_roles_survive() {
        let roles = vec![KeycloakRole::Realm {
            role: Role::Administrator,
        }];
        assert_eq!(realm_roles(&roles), vec![Role::Administrator]);
    }
}
