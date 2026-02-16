//! Lark (Feishu) channel implementation

use super::lark_types::*;
use super::traits::{Channel, ChannelMessage};
use async_trait::async_trait;

/// Lark channel — receives messages via webhook/event, sends via Messaging API
pub struct LarkChannel {
    // App Credentials
    app_id: String,
    app_secret: String,
    encrypt_key: Option<String>,
    verify_token: String,

    // Runtime
    access_token: Option<String>,
    token_expires_at: Option<i64>,

    // Security
    allowed_users: Vec<String>,

    // Client
    client: reqwest::Client,
}

impl LarkChannel {
    pub fn new(
        app_id: String,
        app_secret: String,
        encrypt_key: Option<String>,
        verify_token: String,
        allowed_users: Vec<String>,
    ) -> Self {
        Self {
            app_id,
            app_secret,
            encrypt_key,
            verify_token,
            access_token: None,
            token_expires_at: None,
            allowed_users,
            client: reqwest::Client::new(),
        }
    }

    /// Get or refresh tenant access token
    pub async fn get_access_token(&mut self) -> anyhow::Result<String> {
        // Check if current token is still valid (with 5 min buffer)
        if let (Some(token), Some(expires_at)) = (&self.access_token, self.token_expires_at) {
            let now = chrono::Utc::now().timestamp();
            if now < expires_at - 300 {
                return Ok(token.clone());
            }
        }

        // Fetch new token
        let body = serde_json::json!({
            "app_id": self.app_id,
            "app_secret": self.app_secret
        });

        let resp: LarkTokenResponse = self
            .client
            .post("https://open.larksuite.com/open-apis/auth/v3/tenant_access_token/internal")
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        if resp.code != 0 {
            anyhow::bail!("Failed to get access token: {}", resp.msg);
        }

        let token = resp
            .tenant_access_token
            .ok_or_else(|| anyhow::anyhow!("No token in response"))?;

        self.access_token = Some(token.clone());
        self.token_expires_at = Some(
            resp.expire.unwrap_or(7200) as i64 + chrono::Utc::now().timestamp(),
        );

        Ok(token)
    }

    /// Check if a user is allowed
    pub fn is_user_allowed(&self, open_id: &str) -> bool {
        self.allowed_users
            .iter()
            .any(|u| u == "*" || u == open_id)
    }

    /// Verify Lark event encryption (placeholder for now)
    pub fn verify_event_encryption(
        &self,
        _encrypt_key: &str,
        _ciphertext: &str,
    ) -> anyhow::Result<String> {
        // TODO: Implement proper AES-256-CBC decryption
        Ok(String::new())
    }

    /// Send text message to Lark
    pub async fn send_text(&mut self, user_id: &str, text: &str) -> anyhow::Result<()> {
        let token = self.get_access_token().await?;

        let content = serde_json::json!({
            "text": text
        });

        let body = LarkSendMessageRequest {
            receive_id_type: "open_id".into(),
            msg_type: "text".into(),
            receive_id: user_id.into(),
            content,
        };

        let resp: LarkSendMessageResponse = self
            .client
            .post("https://open.larksuite.com/open-apis/message/v4/send")
            .header("Authorization", format!("Bearer {}", token))
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        if resp.code != 0 {
            anyhow::bail!("Lark send failed: {}", resp.msg);
        }

        Ok(())
    }

    /// Get or fetch token without mutating self (for Channel trait)
    async fn get_or_fetch_token(&self, client: &mut reqwest::Client) -> anyhow::Result<String> {
        if let Some(token) = &self.access_token {
            return Ok(token.clone());
        }

        let body = serde_json::json!({
            "app_id": self.app_id,
            "app_secret": self.app_secret
        });

        let resp: LarkTokenResponse = client
            .post("https://open.larksuite.com/open-apis/auth/v3/tenant_access_token/internal")
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        if resp.code != 0 {
            anyhow::bail!("Failed to get access token: {}", resp.msg);
        }

        Ok(resp.tenant_access_token.unwrap_or_default())
    }
}

#[async_trait]
impl Channel for LarkChannel {
    fn name(&self) -> &str {
        "lark"
    }

    async fn send(&self, message: &str, recipient: &str) -> anyhow::Result<()> {
        let mut client = reqwest::Client::new();
        let token = self.get_or_fetch_token(&mut client).await?;

        let content = serde_json::json!({
            "text": message
        });

        let body = LarkSendMessageRequest {
            receive_id_type: "open_id".into(),
            msg_type: "text".into(),
            receive_id: recipient.into(),
            content,
        };

        let resp: LarkSendMessageResponse = client
            .post("https://open.larksuite.com/open-apis/message/v4/send")
            .header("Authorization", format!("Bearer {}", token))
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        if resp.code != 0 {
            anyhow::bail!("Lark send failed: {}", resp.msg);
        }

        Ok(())
    }

    async fn listen(&self, _tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        // Webhook-based: Gateway handles incoming messages
        // This waits indefinitely since we don't poll
        std::future::pending().await
    }

    async fn health_check(&self) -> bool {
        self.client
            .get("https://open.larksuite.com/open-apis/bot/v3/info")
            .header("Authorization", format!("Bearer {}", self.access_token.as_ref().unwrap_or(&String::new())))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}
