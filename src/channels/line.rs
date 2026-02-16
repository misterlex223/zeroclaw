use super::traits::Channel;
use async_trait::async_trait;
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

/// LINE channel — receives messages via webhook, sends via Messaging API
pub struct LineChannel {
    channel_access_token: String,
    channel_secret: String,
    allowed_users: Vec<String>,
    client: reqwest::Client,
}

impl LineChannel {
    pub fn new(channel_access_token: String,
               channel_secret: String,
               allowed_users: Vec<String>) -> Self {
        Self {
            channel_access_token,
            channel_secret,
            allowed_users,
            client: reqwest::Client::new(),
        }
    }

    /// Verify LINE webhook signature using constant-time comparison
    pub fn verify_webhook_signature(&self, body: &[u8], signature: &str) -> bool {
        // Decode signature from base64
        let decoded_sig = match base64_decode(signature) {
            Ok(s) => s,
            Err(_) => return false,
        };

        // Create HMAC using channel_secret
        let mut mac = HmacSha256::new_from_slice(self.channel_secret.as_bytes()).unwrap();
        mac.update(body);
        let expected = mac.finalize().into_bytes();

        // Constant-time comparison to prevent timing attacks
        decoded_sig.len() == expected.len()
            && decoded_sig.ct_eq(&expected).into()
    }

    /// Check if a LINE user ID is in the allowlist
    pub fn is_user_allowed(&self, user_id: &str) -> bool {
        self.allowed_users.iter().any(|u| u == "*" || u == user_id)
    }

    /// Send reply message to LINE
    async fn send_reply(&self, reply_token: &str, messages: serde_json::Value) -> anyhow::Result<()> {
        let body = serde_json::json!({
            "replyToken": reply_token,
            "messages": messages
        });

        let resp = self.client
            .post("https://api.line.me/v2/bot/message/reply")
            .bearer_auth(&self.channel_access_token)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_body = resp.text().await.unwrap_or_default();
            anyhow::bail!("LINE reply API failed ({status}): {error_body}");
        }

        Ok(())
    }

    /// Send push message to LINE (proactive)
    async fn send_push(&self, to: &str, messages: serde_json::Value) -> anyhow::Result<()> {
        let body = serde_json::json!({
            "to": to,
            "messages": messages
        });

        let resp = self.client
            .post("https://api.line.me/v2/bot/message/push")
            .bearer_auth(&self.channel_access_token)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_body = resp.text().await.unwrap_or_default();
            anyhow::bail!("LINE push API failed ({status}): {error_body}");
        }

        Ok(())
    }
}

/// Quick reply item for LINE messages
pub struct QuickReplyItem {
    pub label: String,
    pub text: String,
}

impl LineChannel {
    /// Send a flex message
    pub async fn send_flex(&self,
                           to: &str,
                           alt_text: &str,
                           contents: &serde_json::Value) -> anyhow::Result<()> {
        let messages = serde_json::json!([{
            "type": "flex",
            "altText": alt_text,
            "contents": contents
        }]);
        self.send_push(to, messages).await
    }

    /// Send message with quick reply buttons
    pub async fn send_with_quick_reply(&self,
                                       to: &str,
                                       text: &str,
                                       items: Vec<QuickReplyItem>) -> anyhow::Result<()> {
        let quick_reply_items: Vec<serde_json::Value> = items.into_iter()
            .map(|item| serde_json::json!({
                "type": "action",
                "action": {
                    "type": "message",
                    "label": item.label,
                    "text": item.text
                }
            }))
            .collect();

        let messages = serde_json::json!([{
            "type": "text",
            "text": text,
            "quickReply": {
                "items": quick_reply_items
            }
        }]);
        self.send_push(to, messages).await
    }
}

/// Helper to decode base64 URL-safe (no padding)
fn base64_decode(input: &str) -> Result<Vec<u8>, base64::DecodeError> {
    use base64::{Engine as _, engine::general_purpose};
    general_purpose::URL_SAFE_NO_PAD.decode(input)
}

#[async_trait]
impl Channel for LineChannel {
    fn name(&self) -> &str {
        "line"
    }

    async fn send(&self, message: &str, recipient: &str) -> anyhow::Result<()> {
        // recipient is LINE User ID for push messages
        let messages = serde_json::json!([
            {
                "type": "text",
                "text": message
            }
        ]);
        self.send_push(recipient, messages).await
    }

    async fn listen(&self, _tx: tokio::sync::mpsc::Sender<super::traits::ChannelMessage>) -> anyhow::Result<()> {
        // Webhook-based: Gateway handles incoming messages
        // This waits indefinitely since we don't poll
        std::future::pending().await
    }

    async fn health_check(&self) -> bool {
        self.client
            .get("https://api.line.me/v2/bot/info")
            .bearer_auth(&self.channel_access_token)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_channel_name() {
        let ch = LineChannel::new("token".into(), "secret".into(), vec![]);
        assert_eq!(ch.name(), "line");
    }

    #[test]
    fn line_signature_verification_valid() {
        let channel_secret = "test_secret";
        let body = b"test_body";

        // Create valid signature
        let mut mac = HmacSha256::new_from_slice(channel_secret.as_bytes()).unwrap();
        mac.update(body);
        let signature = mac.finalize().into_bytes();
        let signature_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature);

        let ch = LineChannel::new("token".into(), channel_secret.into(), vec![]);
        assert!(ch.verify_webhook_signature(body, &signature_b64));
    }

    #[test]
    fn line_signature_verification_invalid() {
        let ch = LineChannel::new("token".into(), "secret".into(), vec![]);
        assert!(!ch.verify_webhook_signature(b"test_body", "invalid_signature"));
    }

    #[test]
    fn line_signature_verification_empty_body() {
        let channel_secret = "secret";
        let body = b"";

        let mut mac = HmacSha256::new_from_slice(channel_secret.as_bytes()).unwrap();
        mac.update(body);
        let signature = mac.finalize().into_bytes();
        let signature_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature);

        let ch = LineChannel::new("token".into(), channel_secret.into(), vec![]);
        assert!(ch.verify_webhook_signature(body, &signature_b64));
    }

    #[test]
    fn line_user_allowed_wildcard() {
        let ch = LineChannel::new("t".into(), "s".into(), vec!["*".into()]);
        assert!(ch.is_user_allowed("U123"));
        assert!(ch.is_user_allowed("any_user"));
    }

    #[test]
    fn line_user_allowed_specific() {
        let ch = LineChannel::new("t".into(), "s".into(), vec!["U111".into(), "U222".into()]);
        assert!(ch.is_user_allowed("U111"));
        assert!(ch.is_user_allowed("U222"));
        assert!(!ch.is_user_allowed("U333"));
    }

    #[test]
    fn line_user_denied_empty() {
        let ch = LineChannel::new("t".into(), "s".into(), vec![]);
        assert!(!ch.is_user_allowed("U123"));
    }

    #[test]
    fn line_user_exact_match() {
        let ch = LineChannel::new("t".into(), "s".into(), vec!["U123".into()]);
        assert!(ch.is_user_allowed("U123"));
        assert!(!ch.is_user_allowed("U1234"));
        assert!(!ch.is_user_allowed("U12"));
    }

    #[test]
    fn line_quick_reply_item_creation() {
        let item = QuickReplyItem {
            label: "Yes".to_string(),
            text: "yes".to_string(),
        };
        assert_eq!(item.label, "Yes");
        assert_eq!(item.text, "yes");
    }
}
