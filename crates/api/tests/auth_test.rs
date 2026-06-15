/// Integration tests for authentication endpoints.
/// Run with: cargo test --test auth_test
/// Requires: DATABASE_URL, REDIS_URL, JWT_SECRET set in environment.

#[cfg(test)]
mod auth_integration {
    use reqwest::Client;
    use serde_json::{json, Value};

    fn api_url(path: &str) -> String {
        let base = std::env::var("TEST_API_URL").unwrap_or_else(|_| "http://localhost:8080".into());
        format!("{}/api/v1{}", base, path)
    }

    async fn login_admin() -> (Client, String) {
        let client = Client::new();
        let resp = client
            .post(api_url("/auth/login"))
            .json(&json!({
                "email":    std::env::var("ADMIN_EMAIL").unwrap_or_else(|_| "admin@apex.local".into()),
                "password": std::env::var("ADMIN_PASSWORD").unwrap_or_else(|_| "changeme".into()),
            }))
            .send()
            .await
            .expect("Login request failed");

        assert_eq!(resp.status(), 200, "Login should return 200");
        let body: Value = resp.json().await.unwrap();
        let token = body["tokens"]["access_token"].as_str().unwrap().to_string();
        (client, token)
    }

    #[tokio::test]
    async fn test_login_success() {
        let (_, token) = login_admin().await;
        assert!(!token.is_empty(), "Access token must not be empty");
    }

    #[tokio::test]
    async fn test_login_wrong_password() {
        let client = Client::new();
        let resp = client
            .post(api_url("/auth/login"))
            .json(&json!({ "email": "admin@apex.local", "password": "wrongpassword" }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 401, "Wrong password must return 401");
    }

    #[tokio::test]
    async fn test_login_unknown_email() {
        let client = Client::new();
        let resp = client
            .post(api_url("/auth/login"))
            .json(&json!({ "email": "nobody@nowhere.invalid", "password": "anything" }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 401);
    }

    #[tokio::test]
    async fn test_me_with_valid_token() {
        let (client, token) = login_admin().await;
        let resp = client
            .get(api_url("/users/me"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        assert!(body["id"].is_string());
        assert!(body["email"].is_string());
    }

    #[tokio::test]
    async fn test_protected_route_without_token() {
        let client = Client::new();
        let resp = client
            .get(api_url("/users/me"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400, "Missing bearer header returns 400");
    }

    #[tokio::test]
    async fn test_protected_route_with_invalid_token() {
        let client = Client::new();
        let resp = client
            .get(api_url("/users/me"))
            .bearer_auth("invalid.jwt.token")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 401);
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let client = Client::new();
        let resp = client.get(api_url("/health")).send().await.unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        assert!(body["status"].is_string());
    }

    #[tokio::test]
    async fn test_logout_revokes_session() {
        let (client, token) = login_admin().await;

        let login_resp: Value = client
            .post(api_url("/auth/login"))
            .json(&json!({
                "email":    std::env::var("ADMIN_EMAIL").unwrap_or_else(|_| "admin@apex.local".into()),
                "password": std::env::var("ADMIN_PASSWORD").unwrap_or_else(|_| "changeme".into()),
            }))
            .send().await.unwrap().json().await.unwrap();

        let refresh_token = login_resp["tokens"]["refresh_token"].as_str().unwrap();

        client
            .post(api_url("/auth/logout"))
            .bearer_auth(&token)
            .json(&json!({ "refresh_token": refresh_token }))
            .send().await.unwrap();

        let refresh_resp = client
            .post(api_url("/auth/refresh"))
            .json(&json!({ "refresh_token": refresh_token }))
            .send().await.unwrap();

        assert_eq!(refresh_resp.status(), 401, "Revoked refresh token must return 401");
    }
}
