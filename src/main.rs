mod cache;
mod calendar;
mod config;
mod error;
mod filter;

use crate::cache::Cache;
use crate::calendar::CalendarService;
use crate::config::Config;
use crate::error::Error;
use axum::Router;
use axum::extract::{RawQuery, State};
use axum::http::{HeaderName, HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;
use tracing_subscriber::EnvFilter;

#[derive(Clone)]
struct AppState {
    service: CalendarService,
    root_url: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (config, config_path) = Config::load()?;

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(if config.debug { "debug" } else { "info" })),
        )
        .init();

    if let Some(path) = &config_path {
        tracing::info!("loaded configuration from {}", path.display());
    } else {
        tracing::info!(
            "no {} found; using built-in configuration defaults",
            crate::config::DEFAULT_CONFIG_PATH
        );
    }

    let host = config.host();
    let cache = Cache::new(config.redis_url());
    tracing::info!(
        "Redis caching is {}",
        if cache.enabled() {
            "enabled"
        } else {
            "disabled"
        }
    );

    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .user_agent(concat!("ical-proxy/", env!("CARGO_PKG_VERSION")))
        .build()?;

    let state = AppState {
        service: CalendarService::new(http, cache),
        root_url: config.root_url,
    };

    let app = Router::new()
        .route("/", get(root))
        .fallback(fallback)
        .with_state(state)
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .layer(CompressionLayer::new().gzip(true))
        .layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::mirror_request())
                .allow_methods([
                    Method::GET,
                    Method::HEAD,
                    Method::POST,
                    Method::OPTIONS,
                    Method::PUT,
                    Method::DELETE,
                    Method::PATCH,
                ])
                .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
                .allow_credentials(true),
        )
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-engine"),
            HeaderValue::from_static("axum"),
        ));

    let listener = tokio::net::TcpListener::bind((host.as_str(), config.port)).await?;
    let address = listener.local_addr()?;
    tracing::info!("listening on {address}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn root(State(state): State<AppState>, uri: Uri, RawQuery(query): RawQuery) -> Response {
    let query = query.unwrap_or_default();
    let parameters: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();

    let url = parameters
        .iter()
        .find(|(key, _)| key == "url")
        .map(|(_, value)| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_owned);

    let Some(url) = url else {
        return redirect(&format!("{}/generator", state.root_url));
    };

    let ttl = parameters
        .iter()
        .find(|(key, _)| key == "ttl")
        .and_then(|(_, value)| value.parse().ok())
        .unwrap_or(3600);

    match state.service.get_calendar_filtered(&url, &query, ttl).await {
        Ok(body) => (
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/calendar"),
            )],
            body,
        )
            .into_response(),
        Err(Error::Http { status, body }) => (status, body).into_response(),
        Err(Error::NotFound) => (
            StatusCode::NOT_FOUND,
            format!("404: {} not found", uri.path()),
        )
            .into_response(),
    }
}

async fn fallback(uri: Uri) -> Response {
    (
        StatusCode::NOT_FOUND,
        format!("404: {} not found", uri.path()),
    )
        .into_response()
}

fn redirect(location: &str) -> Response {
    let location =
        HeaderValue::from_str(location).unwrap_or_else(|_| HeaderValue::from_static("/"));
    (StatusCode::FOUND, [(header::LOCATION, location)]).into_response()
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!("failed to listen for Ctrl+C: {error}");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => tracing::error!("failed to listen for SIGTERM: {error}"),
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}
