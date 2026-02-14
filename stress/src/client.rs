use crate::config::SimConfig;
use anyhow::Result;
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct Friend {
    pub id: u64,
    pub affinity: f64,
}

#[derive(Clone, Debug)]
pub struct UserContext {
    pub id: u64,
    pub username: String,
    pub token: String,
    pub friends: Vec<Friend>,
    pub groups: Vec<u64>,
}

#[derive(Serialize, Deserialize)]
struct UserResponse {
    id: u64,
    username: String,
}

pub struct UserClient {
    http: HttpClient,
}

impl UserClient {
    pub fn new() -> Self {
        Self {
            http: HttpClient::new(),
        }
    }

    pub async fn register_and_login(&self idx: u32) -> Result<UserContext> {
        let username = format!("user_{}", idx);
        let password = "password123";
        let email = format!("user_{}@example.com", idx);

        let login_url = format!("{}/auth/login",);
        let mut resp = self
            .http
            .post(&login_url)
            .form(&[("username", username.as_str()), ("password", password)])
            .send()
            .await?;

        if !resp.status().is_success() {
            let reg_url = format!("{}/auth/register", );
            let reg_resp = self
                .http
                .post(&reg_url)
                .form(&[
                    ("username", username.as_str()),
                    ("password", password),
                    ("email", email.as_str()),
                    ("name", username.as_str()),
                ])
                .send()
                .await?;

            if !reg_resp.status().is_success() {
                return Err(anyhow::anyhow!(
                    "Registration failed: {}",
                    reg_resp.status()
                ));
            }

            resp = self
                .http
                .post(&login_url)
                .form(&[("username", username.as_str()), ("password", password)])
                .send()
                .await?;
        }

        if resp.status().is_success() {
            let cookie_value = resp
                .cookies()
                .find(|c| c.name() == "session")
                .map(|c| c.value().to_string())
                .ok_or_else(|| anyhow::anyhow!("No session cookie"))?;

            let user_data: UserResponse = resp.json().await?;

            return Ok(UserContext {
                id: user_data.id,
                username: user_data.username,
                token: cookie_value,
                friends: vec![],
                groups: vec![],
            });
        }

        Err(anyhow::anyhow!("Login failed"))
    }
}
