use axum::Router;
use tower_http::compression::CompressionLayer;
use tower_http::services::ServeDir;

use crate::routes;
use crate::state::AppState;

pub(crate) fn build_app(state: AppState) -> Router {
    let static_assets = ServeDir::new("client/dist")
        .precompressed_br()
        .precompressed_gzip();

    let app = Router::new()
        .route(
            "/api/territories",
            axum::routing::get(routes::api::get_territories),
        )
        .route(
            "/api/live/state",
            axum::routing::get(routes::api::get_live_state),
        )
        .route(
            "/api/guild/{name}",
            axum::routing::get(routes::api::get_guild),
        )
        .route(
            "/api/events",
            axum::routing::get(routes::sse::territory_events),
        )
        .route("/api/health", axum::routing::get(routes::api::health))
        .route("/api/metrics", axum::routing::get(routes::api::metrics))
        .route(
            "/api/history/at",
            axum::routing::get(routes::history::history_at),
        )
        .route(
            "/api/history/events",
            axum::routing::get(routes::history::history_events),
        )
        .route(
            "/api/history/bounds",
            axum::routing::get(routes::history::history_bounds),
        );

    app.layer(CompressionLayer::new())
        .fallback_service(static_assets)
        .with_state(state)
}
