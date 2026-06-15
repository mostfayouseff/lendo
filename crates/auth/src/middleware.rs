use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use axum_extra::{
    headers::{authorization::Bearer, Authorization},
    TypedHeader,
};
use uuid::Uuid;

use crate::{
    error::AuthError,
    jwt::{Claims, JwtConfig},
};
use db::models::user::UserRole;

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id:       Uuid,
    pub username: String,
    pub email:    String,
    pub role:     UserRole,
}

impl From<Claims> for AuthUser {
    fn from(c: Claims) -> Self {
        Self { id: c.sub, username: c.username, email: c.email, role: c.role }
    }
}

pub async fn require_auth(
    State(jwt): State<JwtConfig>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    mut request: Request,
    next: Next,
) -> Result<Response, AuthError> {
    let claims = jwt.validate_access(auth.token())?;
    request.extensions_mut().insert(AuthUser::from(claims));
    Ok(next.run(request).await)
}

pub fn require_role(user: &AuthUser, required: &UserRole) -> Result<(), AuthError> {
    let allowed = match required {
        UserRole::Viewer => true,
        UserRole::Trader => matches!(user.role, UserRole::Trader | UserRole::Admin),
        UserRole::Admin  => matches!(user.role, UserRole::Admin),
    };
    if allowed {
        Ok(())
    } else {
        Err(AuthError::InsufficientPermissions(format!("{:?}", required)))
    }
}
