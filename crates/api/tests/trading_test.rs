/// Integration tests for trading endpoints.
/// Run with: cargo test --test trading_test

#[cfg(test)]
mod trading_integration {
    use reqwest::Client;
    use serde_json::{json, Value};

    fn api_url(path: &str) -> String {
        let base = std::env::var("TEST_API_URL").unwrap_or_else(|_| "http://localhost:8080".into());
        format!("{}/api/v1{}", base, path)
    }

    async fn auth_client() -> (Client, String) {
        let client = Client::new();
        let resp: Value = client
            .post(api_url("/auth/login"))
            .json(&json!({
                "email":    std::env::var("ADMIN_EMAIL").unwrap_or_else(|_| "admin@apex.local".into()),
                "password": std::env::var("ADMIN_PASSWORD").unwrap_or_else(|_| "changeme".into()),
            }))
            .send().await.unwrap().json().await.unwrap();
        let token = resp["tokens"]["access_token"].as_str().unwrap().to_string();
        (client, token)
    }

    #[tokio::test]
    async fn test_list_trades_empty() {
        let (client, token) = auth_client().await;
        let resp = client.get(api_url("/trades")).bearer_auth(&token).send().await.unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        assert!(body.is_array());
    }

    #[tokio::test]
    async fn test_trade_summary() {
        let (client, token) = auth_client().await;
        let resp = client.get(api_url("/trades/summary")).bearer_auth(&token).send().await.unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        assert!(body["total_trades"].is_number());
        assert!(body["win_rate"].is_number());
    }

    #[tokio::test]
    async fn test_list_strategies() {
        let (client, token) = auth_client().await;
        let resp = client.get(api_url("/strategies")).bearer_auth(&token).send().await.unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        assert!(body.is_array());
    }

    #[tokio::test]
    async fn test_create_and_delete_strategy() {
        let (client, token) = auth_client().await;

        let create_resp: Value = client
            .post(api_url("/strategies"))
            .bearer_auth(&token)
            .json(&json!({
                "name":                  "Test Strategy",
                "strategy_type":         "cross_dex",
                "min_profit_lamports":   10000,
                "max_position_lamports": 1000000000,
                "max_slippage_bps":      50,
                "max_hops":              4,
                "flash_loan_enabled":    false,
                "dex_whitelist":         [],
                "token_whitelist":       [],
            }))
            .send().await.unwrap().json().await.unwrap();

        let id = create_resp["id"].as_str().expect("Strategy ID must be present");
        assert!(!id.is_empty());

        let delete_resp = client
            .delete(api_url(&format!("/strategies/{id}")))
            .bearer_auth(&token)
            .send().await.unwrap();
        assert_eq!(delete_resp.status(), 200);
    }

    #[tokio::test]
    async fn test_list_tokens() {
        let (client, token) = auth_client().await;
        let resp = client.get(api_url("/tokens")).bearer_auth(&token).send().await.unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        assert!(body.is_array());
    }

    #[tokio::test]
    async fn test_list_opportunities() {
        let (client, token) = auth_client().await;
        let resp = client.get(api_url("/opportunities")).bearer_auth(&token).send().await.unwrap();
        assert_eq!(resp.status(), 200);
    }

    #[tokio::test]
    async fn test_flash_loan_providers() {
        let (client, token) = auth_client().await;
        let resp = client.get(api_url("/flash-loans/providers")).bearer_auth(&token).send().await.unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        assert!(body["providers"].is_array());
        assert!(body["providers"].as_array().unwrap().len() >= 3);
    }

    #[tokio::test]
    async fn test_flash_loan_quote_solend() {
        let (client, token) = auth_client().await;
        let resp = client
            .post(api_url("/flash-loans/quote"))
            .bearer_auth(&token)
            .json(&json!({
                "provider":      "solend",
                "borrow_mint":   "So11111111111111111111111111111111111111112",
                "borrow_amount": 1000000000,
            }))
            .send().await.unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        assert!(body["fee_bps"].is_number());
        assert!(body["repay_amount"].is_number());
        assert!(body["repay_amount"].as_u64().unwrap() > 1000000000);
    }

    #[tokio::test]
    async fn test_dashboard_overview() {
        let (client, token) = auth_client().await;
        let resp = client.get(api_url("/monitoring/overview")).bearer_auth(&token).send().await.unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        assert!(body["total_trades"].is_number());
        assert!(body["bot_running"].is_boolean());
    }

    #[tokio::test]
    async fn test_bot_status() {
        let (client, token) = auth_client().await;
        let resp = client.get(api_url("/bot/status")).bearer_auth(&token).send().await.unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        assert!(body["running"].is_boolean());
        assert!(body["mode"].is_string());
    }
}
