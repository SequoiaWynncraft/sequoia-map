use std::path::Path;

const WORKSPACE_MIGRATIONS_DIR: &str = "server/migrations";
const CRATE_MIGRATIONS_DIR: &str = "./migrations";

fn migrations_path() -> &'static Path {
    let workspace_path = Path::new(WORKSPACE_MIGRATIONS_DIR);
    if workspace_path.exists() {
        return workspace_path;
    }
    Path::new(CRATE_MIGRATIONS_DIR)
}

pub async fn run(pool: &sqlx::PgPool) -> Result<(), sqlx_core::migrate::MigrateError> {
    let migrator = sqlx_core::migrate::Migrator::new(migrations_path()).await?;
    migrator.run(pool).await
}
