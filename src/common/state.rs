use axum_keycloak_auth::instance::KeycloakAuthInstance;
use sea_orm::DatabaseConnection;
use std::sync::Arc;

use crate::archive::store::ArchiveStore;
use crate::common::auth::{DeviceTokenCache, new_device_token_cache};
use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub config: Config,
    /// `None` only in local and dev deployments; `main` refuses to start without it
    /// anywhere else.
    pub keycloak_auth_instance: Option<Arc<KeycloakAuthInstance>>,
    pub device_token_cache: DeviceTokenCache,
    /// The blob store, when the `S3_*` group is configured. `None` answers 503 on
    /// every `/archive` route.
    pub archive: Option<Arc<ArchiveStore>>,
}

impl AppState {
    #[must_use]
    pub fn new(
        db: DatabaseConnection,
        config: Config,
        keycloak_auth_instance: Option<Arc<KeycloakAuthInstance>>,
    ) -> Self {
        let device_token_cache = new_device_token_cache(config.token_cache_ttl_seconds);
        let archive = config
            .archive
            .as_ref()
            .map(|archive_config| Arc::new(ArchiveStore::new(archive_config)));
        Self {
            db,
            config,
            keycloak_auth_instance,
            device_token_cache,
            archive,
        }
    }
}
