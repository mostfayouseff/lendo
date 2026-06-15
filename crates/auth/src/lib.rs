pub mod error;
pub mod jwt;
pub mod middleware;
pub mod password;

pub use error::AuthError;
pub use jwt::{Claims, JwtConfig, TokenPair, TokenType};
pub use middleware::{require_auth, require_role, AuthUser};
pub use password::{hash_password, verify_password};
