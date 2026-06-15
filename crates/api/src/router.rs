use axum::{
    middleware,
    routing::{get, post, put},
    Router,
};
use std::sync::Arc;
use tower_http::{
    services::{ServeDir, ServeFile},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};

use crate::{
    handlers::{
        auth, bot, flash_loans, health, monitoring, opportunities, risk,
        settings, strategies, tokens, trades, users, wallets,
    },
    middleware::{auth_layer::auth_middleware, cors::build_cors, metrics::track_metrics},
    state::AppState,
    ws::ws_handler,
};

pub fn build(state: Arc<AppState>) -> Router {
    let public_routes = Router::new()
        .route("/health",        get(health::health))
        .route("/ready",         get(health::ready))
        .route("/metrics",       get(monitoring::metrics))
        .route("/auth/login",    post(auth::login))
        .route("/auth/refresh",  post(auth::refresh))
        .route("/auth/register", post(auth::register));

    let authed_routes = Router::new()
        .route("/auth/logout",                   post(auth::logout))
        .route("/users/me",                      get(users::me))
        .route("/users",                         get(users::list))
        .route("/users/:id",                     put(users::update))
        .route("/users/me/password",             post(users::change_password))
        .route("/wallets",                       get(wallets::list).post(wallets::create))
        .route("/wallets/:id",                   get(wallets::get).put(wallets::update).delete(wallets::delete))
        .route("/wallets/:id/activate",          post(wallets::activate))
        .route("/tokens",                        get(tokens::list).post(tokens::create))
        .route("/tokens/active",                 get(tokens::list_active))
        .route("/tokens/:id",                    get(tokens::get).put(tokens::update).delete(tokens::delete))
        .route("/tokens/:id/status/:status",     post(tokens::set_status))
        .route("/strategies",                    get(strategies::list).post(strategies::create))
        .route("/strategies/:id",                get(strategies::get).put(strategies::update).delete(strategies::delete))
        .route("/strategies/:id/start",          post(strategies::start))
        .route("/strategies/:id/pause",          post(strategies::pause))
        .route("/trades",                        get(trades::list))
        .route("/trades/summary",                get(trades::summary))
        .route("/trades/:id",                    get(trades::get))
        .route("/opportunities",                 get(opportunities::list))
        .route("/opportunities/recent",          get(opportunities::list_recent))
        .route("/settings",                      get(settings::get_all).post(settings::set))
        .route("/settings/bot",                  get(settings::get_bot_settings).put(settings::update_bot_settings))
        .route("/bot/status",                    get(bot::status))
        .route("/bot/command",                   post(bot::command))
        .route("/risk/rules",                    get(risk::list).post(risk::create))
        .route("/risk/rules/:id",                put(risk::update).delete(risk::delete))
        .route("/flash-loans/providers",         get(flash_loans::providers))
        .route("/flash-loans/quote",             post(flash_loans::quote))
        .route("/monitoring/overview",           get(monitoring::overview))
        .route("/monitoring/events",             get(monitoring::system_events))
        .route("/monitoring/events/:id/resolve", post(monitoring::resolve_event))
        .layer(middleware::from_fn_with_state(state.clone(), auth_middleware));

    let frontend = ServeDir::new("frontend/dist")
        .fallback(ServeFile::new("frontend/dist/index.html"));

    Router::new()
        .route("/ws", get(ws_handler))
        .nest("/api/v1", public_routes.merge(authed_routes))
        .fallback_service(frontend)
        .layer(middleware::from_fn(track_metrics))
        .layer(build_cors())
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::new(std::time::Duration::from_secs(30)))
        .with_state(state)
}
