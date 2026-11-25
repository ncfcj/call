use std::{net::SocketAddr, time::Duration};

use anyhow::Context;
use sqlx::postgres::PgPoolOptions;
use tracing_subscriber::EnvFilter;

use call_server::{config::Config, retention, routes, state::AppState, trace};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let log = tracing_subscriber::fmt().with_env_filter(
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("call_server=debug,tower_http=info")),
    );
    if std::env::var("LOG_FORMAT").is_ok_and(|format| format == "json") {
        log.json().init();
    } else {
        log.init();
    }

    let config = Config::from_env()?;

    let db = PgPoolOptions::new()
        .max_connections(5)
        // O Neon free desliga o compute ocioso: a primeira conexão pode demorar.
        .acquire_timeout(Duration::from_secs(10))
        .connect(&config.database_url)
        .await
        .context("não foi possível conectar ao Postgres (DATABASE_URL)")?;

    sqlx::migrate!("../migrations").run(&db).await.context("falha ao aplicar migrations")?;

    tokio::spawn(retention::run(db.clone(), config.message_ttl_days));

    let mut state = AppState::new(
        db,
        &config.jwt_secret,
        config.cookie_secure,
        config.web_origins.clone(),
        config.livekit.clone(),
        config.message_ttl_days,
    );
    state.client_ip_header = config.client_ip_header.clone();
    state.signup_code = config.signup_code.clone();
    let mut app = routes::router(state);
    if let Some(dir) = &config.web_dir {
        app = routes::serve_web(app, dir, &config.livekit.url)?;
    }
    let app = app.layer(config.cors_layer()).layer(trace::layer());

    // Um listener por endereço, todos com o mesmo app (o mesmo estado e o mesmo hub).
    let mut servers = tokio::task::JoinSet::new();
    for addr in &config.bind_addrs {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .with_context(|| format!("não foi possível escutar em {addr}"))?;
        tracing::info!("call-server ouvindo em http://{}", listener.local_addr()?);
        servers.spawn(
            axum::serve(listener, app.clone().into_make_service_with_connect_info::<SocketAddr>())
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
    let ctrl_c = tokio::signal::ctrl_c();
    // O Docker e o Render encerram com SIGTERM, que o processo 1 de um contêiner ignora por padrão.
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! { _ = ctrl_c => {}, _ = term.recv() => {} }
            }
            Err(_) => drop(ctrl_c.await),
        }
    }
    #[cfg(not(unix))]
    let _ = ctrl_c.await;
    tracing::info!("encerrando");
}
