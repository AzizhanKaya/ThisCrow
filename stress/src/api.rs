use anyhow::Result;
use reqwest::header::SET_COOKIE;
use reqwest::{Client, Response};
use serde::Serialize;
use std::net::IpAddr;

#[derive(Serialize)]
pub struct RegisterPayload {
    pub username: String,
    pub name: String,
    pub password: String,
    pub email: String,
}

#[derive(Serialize)]
pub struct LoginPayload {
    pub username: String,
    pub password: String,
}

#[derive(Clone)]
pub struct ApiClient {
    client: Client,
    api_url: String,
}

impl ApiClient {
    pub fn new(api_url: String, local_ip: IpAddr) -> anyhow::Result<Self> {
        let client = Client::builder().local_address(local_ip).build()?;
        Ok(Self { client, api_url })
    }

    pub async fn register_user(&self, user_index: usize) -> Result<Option<String>> {
        let username = format!("stress_{}", user_index);
        let payload = RegisterPayload {
            username: username,
            name: format!("Stress User {}", user_index),
            password: "password123".to_string(),
            email: format!("{user_index}@example.com"),
        };

        let body = rmp_serde::to_vec_named(&payload)?;

        let res = self
            .client
            .post(format!("{}/api/auth/register", self.api_url))
            .header("Content-Type", "application/msgpack")
            .body(body)
            .send()
            .await?;

        if res.status().is_success() {
            let token = self.extract_session_token(&res);
            let _ = res.bytes().await;
            Ok(token)
        } else {
            anyhow::bail!(
                "Failed to register user {}: {:?}",
                user_index,
                res.text().await
            );
        }
    }

    pub async fn login_user(&self, user_index: usize) -> Result<Option<String>> {
        let username = format!("stress_{}", user_index);
        let payload = LoginPayload {
            username: username,
            password: "password123".to_string(),
        };

        let body = rmp_serde::to_vec_named(&payload)?;

        let res = self
            .client
            .post(format!("{}/api/auth/login", self.api_url))
            .header("Content-Type", "application/msgpack")
            .body(body)
            .send()
            .await?;

        if res.status().is_success() {
            let token = self.extract_session_token(&res);
            let _ = res.bytes().await;
            Ok(token)
        } else {
            anyhow::bail!(
                "Failed to login user {}: {:?}",
                user_index,
                res.text().await
            );
        }
    }

    fn extract_session_token(&self, response: &Response) -> Option<String> {
        let cookies: Vec<_> = response
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .collect();

        cookies
            .iter()
            .find(|c| c.starts_with("session="))
            .map(|cookie| cookie.split(';').next().unwrap().to_string())
    }
}
