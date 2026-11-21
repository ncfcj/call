use std::time::Duration;

use anyhow::Context;
use sqlx::postgres::PgPoolOptions;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use call_server::{config::Config, routes, state::AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("call_server=debug,tower_http=info")),
        )
        .init();

    let config = Config::from_env()?;

    let db = PgPoolOptions::new()
        .max_connections(5)
        // O Neon free desliga o compute ocioso: a primeira conexão pode demorar.
        .acquire_timeout(Duration::from_secs(10))
        .connect(&config.database_url)
        .await
        .context("não foi possível conectar ao Postgres (DATABASE_URL)")?;

    sqlx::migrate!("../migrations").run(&db).await.context("falha ao aplicar migrations")?;

    let state =
        AppState::new(db, &config.jwt_secret, config.cookie_secure, config.web_origins.clone());
    let app = routes::router(state).layer(config.cors_layer()).layer(TraceLayer::new_for_http());

    // Um listener por endereço, todos com o mesmo app (o mesmo estado e o mesmo hub).
    let mut servers = tokio::task::JoinSet::new();
    for addr in &config.bind_addrs {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .with_context(|| format!("não foi possível escutar em {addr}"))?;
        tracing::info!("call-server ouvindo em http://{}", listener.local_addr()?);
        servers.spawn(
            axum::serve(listener, app.clone())
                .with_graceful_shutdown(shutdown_signal())
                .into_future(),
        );
    }
    while let Some(result) = servers.join_next().await {
        result.context("servidor HTTP terminou com pânico")??;
    }
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("encerrando");
}
