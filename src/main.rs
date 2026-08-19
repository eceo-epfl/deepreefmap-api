use sea_orm::{ConnectOptions, ConnectionTrait, Database};
use sea_orm_migration::MigratorTrait;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::signal;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use axum_keycloak_auth::Url;
use axum_keycloak_auth::instance::{KeycloakAuthInstance, KeycloakConfig};

use deepreefmap_api::common::AppState;
use deepreefmap_api::config::Config;
use deepreefmap_api::routes;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,deepreefmap_api=debug,sqlx::query=warn".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Config::from_env()?;
    tracing::info!(
        deployment = ?config.deployment,
        host = %config.api_host,
        port = config.api_port,
        "Configuration loaded"
    );

    let mut db_opts = ConnectOptions::new(&config.database_url);
    db_opts
        .max_connections(config.db_max_connections)
        .min_connections(config.db_min_connections)
        .connect_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_mins(5))
        .sqlx_logging(false);
    let db = Database::connect(db_opts).await?;
    tracing::info!("Database connection pool established");

    // Migrations run under an advisory lock held on its own connection, so several
    // replicas booting together serialise instead of deadlocking on concurrent DDL.
    // Lock and unlock land on the same connection, so the lock cannot leak.
    tracing::info!("Running migrations...");
    let mut lock_opts = ConnectOptions::new(&config.database_url);
    lock_opts
        .max_connections(1)
        .min_connections(1)
        .connect_timeout(Duration::from_secs(5))
        .sqlx_logging(false);
    let lock_db = Database::connect(lock_opts).await?;
    lock_db
        .execute_unprepared("SELECT pg_advisory_lock(7429183056)")
        .await?;
    let migrated = migration::Migrator::up(&db, None).await;
    let _ = lock_db
        .execute_unprepared("SELECT pg_advisory_unlock(7429183056)")
        .await;
    lock_db.close().await?;
    migrated?;
    tracing::info!("Migrations completed");

    let keycloak_instance =
        if let (Some(url), Some(realm)) = (&config.keycloak_url, &config.keycloak_realm) {
            // Over plain HTTP an attacker on the path to Keycloak serves their own JWKS
            // and signs whatever role they like.
            assert!(
                !config.requires_keycloak() || url.starts_with("https://"),
                "SECURITY ERROR: KEYCLOAK_URL must be https in stage and prod deployments"
            );
            tracing::info!(url = %url, realm = %realm, "Initialising Keycloak");
            Some(Arc::new(KeycloakAuthInstance::new(
                KeycloakConfig::builder()
                    .server(Url::parse(url).expect("KEYCLOAK_URL must be a valid URL"))
                    .realm(realm.clone())
                    .build(),
            )))
        } else {
            assert!(
                !config.requires_keycloak(),
                "SECURITY ERROR: KEYCLOAK_URL and KEYCLOAK_REALM are required in \
             stage and prod deployments"
            );
            tracing::warn!("Keycloak not configured: no interactive login is available");
            None
        };

    if config.public_base_url.is_none() {
        // Not fatal: a server can serve reads and existing devices without it. But
        // no new device can be onboarded, which is worth saying loudly at boot
        // rather than leaving to a puzzled operator at mint time.
        tracing::warn!(
            "PUBLIC_BASE_URL is not set: connect codes cannot be minted, so no new \
             desktop client can be enrolled"
        );
    }

    if config.archive.is_some() {
        tracing::info!("Archive enabled");
    } else {
        tracing::warn!(
            "Archive disabled: set S3_URL, S3_BUCKET_ID, S3_ACCESS_KEY, S3_SECRET_KEY \
             and S3_PREFIX to enable /api/archive"
        );
    }

    let state = AppState::new(db, config.clone(), keycloak_instance);
    // No-op when the archive is disabled.
    deepreefmap_api::archive::reaper::spawn(state.clone());
    let app = routes::build_service(&state);

    let addr = config.bind_address();
    tracing::info!(address = %addr, "Starting server");
    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("Server shut down gracefully");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c().await.expect("Ctrl+C handler installs");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler installs")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => tracing::info!("Received Ctrl+C, shutting down..."),
        () = terminate => tracing::info!("Received SIGTERM, shutting down..."),
    }
}
