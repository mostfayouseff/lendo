use tower_http::cors::{Any, CorsLayer};
use axum::http::{HeaderValue, Method};

pub fn build_cors() -> CorsLayer {
    let origins = std::env::var("CORS_ALLOWED_ORIGINS")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());

    let mut layer = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS])
        .allow_headers(Any);

    for origin in origins.split(',') {
        if let Ok(val) = HeaderValue::from_str(origin.trim()) {
            layer = layer.allow_origin(val);
        }
    }
    layer
}
