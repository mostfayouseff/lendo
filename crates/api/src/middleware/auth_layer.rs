use axum::{
    extract::State,
    http::Request,
    middleware::Next,
    response::Response,
};
use axum_extra::{
    headers::{authorization::Bearer, Authorization},
    TypedHeader,
};
use std::sync::Arc;

use crate::{error::ApiError, state::AppState};
use auth::middleware::AuthUser;

pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    mut request: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, ApiError> {
    let claims = state.jwt.validate_access(auth.token())
        .map_err(ApiError::Auth)?;
    request.extensions_mut().insert(AuthUser::from(claims));
    Ok(next.run(request).await)
}
