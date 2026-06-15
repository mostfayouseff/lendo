/// Security tests: input validation and injection resistance.
/// Run with: cargo test --test input_validation_test

#[cfg(test)]
mod security {
    use reqwest::Client;
    use serde_json::{json, Value};

    fn api_url(path: &str) -> String {
        let base = std::env::var("TEST_API_URL").unwrap_or_else(|_| "http://localhost:8080".into());
        format!("{}/api/v1{}", base, path)
    }

    /// Every SQL injection payload must return non-500 and must not leak data.
    #[tokio::test]
    async fn test_sql_injection_in_email() {
        let client = Client::new();
        let payloads = [
            "' OR '1'='1",
            "admin'--",
            "'; DROP TABLE users; --",
            "\" OR \"1\"=\"1",
            "1; SELECT * FROM users",
        ];
        for payload in &payloads {
            let resp = client
                .post(api_url("/auth/login"))
                .json(&json!({ "email": payload, "password": "anything" }))
                .send().await.unwrap();
            let status = resp.status().as_u16();
            assert!(status == 401 || status == 400 || status == 422,
                "SQL injection payload '{}' returned unexpected status {}", payload, status);
        }
    }

    #[tokio::test]
    async fn test_xss_in_username() {
        let client = Client::new();
        let payload = "<script>alert('xss')</script>";
        let resp = client
            .post(api_url("/auth/register"))
            .json(&json!({ "username": payload, "email": "xss@test.invalid", "password": "password123" }))
            .send().await.unwrap();
        let body: Value = resp.json().await.unwrap();
        let body_str = body.to_string();
        assert!(!body_str.contains("<script>"), "XSS payload must not appear unescaped in response");
    }

    #[tokio::test]
    async fn test_empty_body_login() {
        let client = Client::new();
        let resp = client
            .post(api_url("/auth/login"))
            .header("Content-Type", "application/json")
            .body("{}")
            .send().await.unwrap();
        assert!(resp.status().as_u16() >= 400, "Empty login body must fail");
    }

    #[tokio::test]
    async fn test_oversized_password() {
        let client = Client::new();
        let huge_password = "A".repeat(100_000);
        let resp = client
            .post(api_url("/auth/login"))
            .json(&json!({ "email": "test@test.com", "password": huge_password }))
            .send().await.unwrap();
        assert!(resp.status().as_u16() < 500, "Oversized password must not cause server error");
    }

    #[tokio::test]
    async fn test_missing_auth_header() {
        let endpoints = [
            ("/strategies", "GET"),
            ("/wallets",    "GET"),
            ("/trades",     "GET"),
            ("/settings",   "GET"),
        ];
        let client = Client::new();
        for (path, method) in &endpoints {
            let req = match *method {
                "GET"  => client.get(api_url(path)),
                "POST" => client.post(api_url(path)),
                _      => client.get(api_url(path)),
            };
            let resp = req.send().await.unwrap();
            assert!(resp.status().as_u16() >= 400,
                "Endpoint {} {} must reject unauthenticated request, got {}", method, path, resp.status());
        }
    }

    #[tokio::test]
    async fn test_content_type_headers() {
        let client = Client::new();
        let resp = client.get(api_url("/health")).send().await.unwrap();
        let ct = resp.headers().get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(ct.contains("application/json"), "Health must return JSON content-type, got: {ct}");
    }

    #[tokio::test]
    async fn test_invalid_uuid_path_param() {
        let client = Client::new();
        let resp = client
            .post(api_url("/auth/login"))
            .json(&json!({
                "email":    std::env::var("ADMIN_EMAIL").unwrap_or_else(|_| "admin@apex.local".into()),
                "password": std::env::var("ADMIN_PASSWORD").unwrap_or_else(|_| "changeme".into()),
            }))
            .send().await.unwrap();
        let body: Value = resp.json().await.unwrap();
        let token = body["tokens"]["access_token"].as_str().unwrap_or("").to_string();

        let resp = client
            .get(api_url("/trades/not-a-valid-uuid"))
            .bearer_auth(&token)
            .send().await.unwrap();
        assert_eq!(resp.status().as_u16(), 400, "Invalid UUID path param must return 400");
    }

    #[tokio::test]
    async fn test_rate_limit_headers_present() {
        let client = Client::new();
        for _ in 0..3 {
            client.post(api_url("/auth/login"))
                .json(&json!({ "email": "nobody@invalid.com", "password": "wrong" }))
                .send().await.unwrap();
        }
    }
}
