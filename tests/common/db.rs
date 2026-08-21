use std::path::Path;
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};

use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseConnection, Statement};
use sea_orm_migration::MigratorTrait;
use tokio::sync::OnceCell;

/// Migrated once per test binary, then cloned per test.
static TEMPLATE: OnceCell<String> = OnceCell::const_new();

static NEXT_CLONE: AtomicU64 = AtomicU64::new(0);

/// Small, because every test in flight holds its own pool against one server.
const POOL_MAX: u32 = 4;

/// A database of this test's own, cloned from the migrated template.
///
/// Nothing is shared between tests, so they run in parallel and need no cleanup. The
/// template is built from nothing, so the schema is always the one the migration
/// describes now rather than one an earlier edit left behind.
///
/// # Panics
///
/// Panics when `DATABASE_URL` is unset, or the clone or its migrations fail.
pub async fn setup_test_db() -> DatabaseConnection {
    let template = TEMPLATE.get_or_init(build_template).await;
    let name = format!(
        "drm_t_{}_{}",
        process::id(),
        NEXT_CLONE.fetch_add(1, Ordering::Relaxed)
    );

    let admin = connect(&maintenance_url()).await;
    // Postgres refuses a template that has a session on it, so the builder closed its own.
    exec(
        &admin,
        &format!("CREATE DATABASE {name} TEMPLATE {template}"),
    )
    .await;
    admin.close().await.ok();

    connect(&url_for(&name)).await
}

/// Create the per-binary template and bring the schema up on it.
async fn build_template() -> String {
    let name = format!("drm_tpl_{}", process::id());
    let admin = connect(&maintenance_url()).await;
    drop_abandoned(&admin).await;
    exec(&admin, &format!("DROP DATABASE IF EXISTS {name}")).await;
    exec(&admin, &format!("CREATE DATABASE {name}")).await;
    admin.close().await.ok();

    let db = connect(&url_for(&name)).await;
    migration::Migrator::up(&db, None)
        .await
        .expect("migrations apply to the template");
    // Every later CREATE DATABASE names this as its template, which needs it idle.
    db.close()
        .await
        .expect("the template connection closes before it is cloned");
    name
}

/// Drop databases left by a run whose process is gone.
///
/// The name carries the pid that made it, so a live sibling binary keeps its own.
async fn drop_abandoned(admin: &DatabaseConnection) {
    let rows = admin
        .query_all_raw(Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT datname FROM pg_database \
             WHERE datname LIKE 'drm_t\\_%' OR datname LIKE 'drm_tpl\\_%'"
                .to_string(),
        ))
        .await
        .expect("the database catalogue is readable");

    for row in rows {
        let name: String = row.try_get_by_index(0).expect("datname decodes");
        if owner_alive(&name) {
            continue;
        }
        exec(
            admin,
            &format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
        )
        .await;
    }
}

/// Whether the process named in a scratch database is still running.
fn owner_alive(name: &str) -> bool {
    let Some(pid) = name.split('_').nth(2) else {
        return true;
    };
    Path::new(&format!("/proc/{pid}")).exists()
}

fn base_url() -> String {
    dotenvy::dotenv().ok();
    std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for integration tests")
}

/// The server address with the database name replaced.
fn url_for(database: &str) -> String {
    let url = base_url();
    let (server, _) = url
        .rsplit_once('/')
        .expect("DATABASE_URL carries a database name");
    format!("{server}/{database}")
}

/// `postgres` itself, since CREATE DATABASE cannot run from the database being made.
fn maintenance_url() -> String {
    url_for("postgres")
}

async fn connect(url: &str) -> DatabaseConnection {
    let mut options = ConnectOptions::new(url.to_string());
    options.max_connections(POOL_MAX).min_connections(0);
    Database::connect(options)
        .await
        .unwrap_or_else(|e| panic!("connects to {url}: {e}"))
}

/// # Panics
///
/// Panics when the statement fails, naming the SQL.
pub async fn exec(db: &DatabaseConnection, sql: &str) {
    db.execute_raw(Statement::from_string(
        sea_orm::DatabaseBackend::Postgres,
        sql.to_string(),
    ))
    .await
    .unwrap_or_else(|e| panic!("SQL failed: {e}\nQuery: {sql}"));
}

/// A site as the console authors one: `device_id` null, so no device owns it.
#[allow(dead_code)]
pub async fn seed_site(db: &DatabaseConnection, id: &str, name: &str) {
    exec(
        db,
        &format!(
            "INSERT INTO site (id, name, description, created_at, updated_at) \
             VALUES ('{id}', '{name}', '', NOW(), NOW())"
        ),
    )
    .await;
}

/// A campaign as the console authors one: `device_id` null, so no device owns it.
#[allow(dead_code)]
pub async fn seed_campaign(db: &DatabaseConnection, id: &str, name: &str) {
    exec(
        db,
        &format!(
            "INSERT INTO campaign (id, name, description, created_at, updated_at) \
             VALUES ('{id}', '{name}', '', NOW(), NOW())"
        ),
    )
    .await;
}

/// Seed a connect code with a known secret, so a test can enrol without a login.
///
/// The enrolling device takes `device_name`, as it would from a minted code.
///
/// # Panics
///
/// Panics when the insert fails.
pub async fn seed_connect_code(
    db: &DatabaseConnection,
    minted_by: &str,
    device_name: &str,
) -> String {
    // Derived from the subject, so two seeds cannot collide on the unique hash.
    use std::fmt::Write as _;
    let secret = format!("{minted_by:_<32}").bytes().take(32).fold(
        String::with_capacity(64),
        |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        },
    );
    let hash = deepreefmap_api::common::tokens::sha256_hex(&secret);

    exec(
        db,
        &format!(
            "INSERT INTO connect_code \
             (id, code_hash, created_by, device_name, expires_at, created_at) \
             VALUES (gen_random_uuid(), '{hash}', '{minted_by}', '{device_name}', \
             NOW() + INTERVAL '1 hour', NOW())"
        ),
    )
    .await;

    secret
}

/// First column of the first row, for assertions that must read Postgres rather than the
/// API's own view of it.
///
/// # Panics
///
/// Panics when the query fails or returns nothing.
pub async fn one_value<T>(db: &DatabaseConnection, sql: &str) -> T
where
    T: sea_orm::TryGetable,
{
    let row = db
        .query_one_raw(Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            sql.to_string(),
        ))
        .await
        .unwrap_or_else(|e| panic!("SQL failed: {e}\nQuery: {sql}"))
        .unwrap_or_else(|| panic!("no row for: {sql}"));
    row.try_get_by_index(0)
        .unwrap_or_else(|e| panic!("column 0 does not decode: {e}\nQuery: {sql}"))
}
