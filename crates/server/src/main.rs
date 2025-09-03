use axum::Router;
use axum::routing::get;
use ferri_core::config::load_config;
use ferri_core::logger::init_logger;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = load_config()?;
    let _guards = init_logger(&cfg)?;

    let app = Router::new().route("/", get(|| async { "Hello, World!" }));
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", cfg.addr, cfg.port)).await?;

    axum::serve(listener, app).await?;

    Ok(())
}
