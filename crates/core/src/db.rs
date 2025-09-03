use std::io;
use std::path::Path;
use std::time::Duration;

use crate::config::Config;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Pool, Sqlite};

/// Create a *lazy* SQLite pool from config and make sure the on-disk path exists.
///
/// This does **not** touch the database yet; connections are opened on first use.
/// Call [`bootstrap_db`] once at startup (in an async context) to run migrations.
pub fn init_db(cfg: &Config) -> io::Result<Pool<Sqlite>> {
    // Work with the configured path (supports both PathBuf and String via AsRef<Path>).
    let db_path = &cfg.db_path;
    let path: &Path = db_path.as_ref();

    // Special-case in-memory DB for tests/dev: ":memory:".
    let in_memory = path.as_os_str() == ":memory:";

    if !in_memory {
        // Ensure parent directory exists so SQLite can create the file.
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("failed to create DB dir {}: {e}", parent.display()),
                )
            })?;
        }
    }

    // Build connection options.
    let connect_opts = if in_memory {
        // `sqlite::memory:` via builder API
        let mut opts = SqliteConnectOptions::new()
            .journal_mode(SqliteJournalMode::Wal) // WAL is a no-op for in-memory but harmless
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5)) // 5 seconds
            .foreign_keys(true);
        // Use an in-memory filename shortcut understood by sqlx
        // (equivalent to filename(":memory:"))
        opts = opts.filename(":memory:");
        opts
    } else {
        SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5))
            .foreign_keys(true)
    };

    // Size this conservatively; SQLite benefits from fewer writers.
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_lazy_with(connect_opts);

    Ok(pool)
}

/// Run pending SQLx migrations against the provided pool.
/// Call this once during startup *after* [`init_db`].
pub async fn bootstrap_db(pool: &Pool<Sqlite>) -> io::Result<()> {
    // Run migrations located under `crates/<this-crate>/migrations`.
    // Adjust the path if your migrations live elsewhere.
    sqlx::migrate!("../../migrations")
        .run(pool)
        .await
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("DB migrations error: {e}")))?;

    Ok(())
}
