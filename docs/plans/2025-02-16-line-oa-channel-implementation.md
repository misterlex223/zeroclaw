# LINE OA Channel Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add LINE Official Account as a new channel to ZeroClaw, enabling users to interact with the AI assistant via LINE messaging platform.

**Architecture:** Webhook-based integration where LINE platform pushes events to ZeroClaw Gateway, which validates signatures, parses events, and forwards messages through the existing Channel message bus. Replies are sent via LINE Messaging API.

**Tech Stack:** Rust, tokio async runtime, reqwest HTTP client, serde JSON, hmac-sha256 for signature verification

---

## Task 1: Add LINE Configuration Schema

**Files:**
- Modify: `src/config/schema.rs`

**Step 1: Add LineConfig struct after WhatsAppConfig (around line 762)**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineConfig {
    /// Channel Access Token from LINE Developers Console
    pub channel_access_token: String,
    /// Channel Secret for webhook signature verification
    pub channel_secret: String,
    /// Allowed LINE User IDs (use "*" for all users)
    #[serde(default)]
    pub allowed_users: Vec<String>,
}
```

**Step 2: Add `line` field to ChannelsConfig struct (around line 676-687)**

Update `ChannelsConfig` struct to include:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelsConfig {
    pub cli: bool,
    pub telegram: Option<TelegramConfig>,
    pub discord: Option<DiscordConfig>,
    pub slack: Option<SlackConfig>,
    pub webhook: Option<WebhookConfig>,
    pub line: Option<LineConfig>,  // ADD THIS
    pub imessage: Option<IMessageConfig>,
    pub matrix: Option<MatrixConfig>,
    pub whatsapp: Option<WhatsAppConfig>,
    pub email: Option<crate::channels::email_channel::EmailConfig>,
    pub irc: Option<IrcConfig>,
}
```

**Step 3: Update ChannelsConfig Default impl (around line 689-704)**

```rust
impl Default for ChannelsConfig {
    fn default() -> Self {
        Self {
            cli: true,
            telegram: None,
            discord: None,
            slack: None,
            webhook: None,
            line: None,  // ADD THIS
            imessage: None,
            matrix: None,
            whatsapp: None,
            email: None,
            irc: None,
        }
    }
}
```

**Step 4: Update mod.rs exports (around line 3-8)**

```rust
pub use schema::{
    AutonomyConfig, BrowserConfig, ChannelsConfig, ComposioConfig, Config, DiscordConfig,
    DockerRuntimeConfig, GatewayConfig, HeartbeatConfig, IMessageConfig, IdentityConfig,
    LineConfig,  // ADD THIS
    MatrixConfig, MemoryConfig, ModelRouteConfig, ObservabilityConfig, ReliabilityConfig,
    RuntimeConfig, SecretsConfig, SlackConfig, TelegramConfig, TunnelConfig, WebhookConfig,
};
```

**Step 5: Add tests for LineConfig serde**

Add to `src/config/schema.rs` tests section (before closing `#[cfg(test)]`):

```rust
#[test]
fn line_config_serde() {
    let lc = LineConfig {
        channel_access_token: "test_token".into(),
        channel_secret: "test_secret".into(),
        allowed_users: vec!["U123".into(), "U456".into()],
    };
    let json = serde_json::to_string(&lc).unwrap();
    let parsed: LineConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.channel_access_token, "test_token");
    assert_eq!(parsed.allowed_users.len(), 2);
}

#[test]
fn line_config_toml_roundtrip() {
    let lc = LineConfig {
        channel_access_token: "tok".into(),
        channel_secret: "sec".into(),
        allowed_users: vec!["*".into()],
    };
    let toml_str = toml::to_string(&lc).unwrap();
    let parsed: LineConfig = toml::from_str(&toml_str).unwrap();
    assert_eq!(parsed.channel_access_token, "tok");
    assert_eq!(parsed.allowed_users, vec!["*"]);
}

#[test]
fn channels_config_with_line() {
    let c = ChannelsConfig {
        cli: true,
        telegram: None,
        discord: None,
        slack: None,
        webhook: None,
        line: Some(LineConfig {
            channel_access_token: "tok".into(),
            channel_secret: "sec".into(),
            allowed_users: vec!["U123".into()],
        }),
        imessage: None,
        matrix: None,
        whatsapp: None,
        email: None,
        irc: None,
    };
    let toml_str = toml::to_string_pretty(&c).unwrap();
    let parsed: ChannelsConfig = toml::from_str(&toml_str).unwrap();
    assert!(parsed.line.is_some());
    assert_eq!(parsed.line.unwrap().allowed_users, vec!["U123"]);
}

#[test]
fn channels_config_default_has_no_line() {
    let c = ChannelsConfig::default();
    assert!(c.line.is_none());
}
```

**Step 6: Run tests to verify**

```bash
cargo test -p zeroclaw line_config --lib
```

Expected: All tests PASS

**Step 7: Commit**

```bash
git add src/config/schema.rs src/config/mod.rs
git commit -m "feat(config): add LINE channel configuration schema"
```

---

## Task 2: Create LINE Channel Module

**Files:**
- Create: `src/channels/line.rs`
- Modify: `src/channels/mod.rs`

**Step 1: Create the line.rs file with basic structure**

Create `src/channels/line.rs`:

```rust
use super::traits::{Channel, ChannelMessage};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

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

    /// Verify LINE webhook signature
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

        // Constant-time comparison
        decoded_sig.len() == expected.len()
            && decoded_sig.iter().zip(expected.iter()).all(|(a, b)| a == b)
    }

    /// Check if a LINE user ID is in the allowlist
    fn is_user_allowed(&self, user_id: &str) -> bool {
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

    async fn listen(&self, _tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
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
}
```

**Step 2: Add line module to mod.rs**

Update `src/channels/mod.rs`:

```rust
pub mod cli;
pub mod discord;
pub mod email_channel;
pub mod imessage;
pub mod irc;
pub mod line;  // ADD THIS
pub mod matrix;
pub mod slack;
pub mod telegram;
pub mod traits;
pub mod whatsapp;

pub use cli::CliChannel;
pub use discord::DiscordChannel;
pub use email_channel::EmailChannel;
pub use imessage::IMessageChannel;
pub use irc::IrcChannel;
pub use line::LineChannel;  // ADD THIS
pub use matrix::MatrixChannel;
pub use slack::SlackChannel;
pub use telegram::TelegramChannel;
pub use traits::Channel;
pub use whatsapp::WhatsAppChannel;
```

**Step 3: Add dependencies to Cargo.toml if not present**

Check `Cargo.toml` for these dependencies (add if missing):

```toml
[dependencies]
# ... existing dependencies ...
hmac = "0.12"
sha2 = "0.10"
base64 = "0.22"
```

**Step 4: Run tests**

```bash
cargo test -p zeroclaw line --lib
```

Expected: All tests PASS

**Step 5: Commit**

```bash
git add src/channels/line.rs src/channels/mod.rs Cargo.toml
git commit -m "feat(channels): add LINE channel implementation"
```

---

## Task 3: Integrate LINE Channel into Channel Manager

**Files:**
- Modify: `src/channels/mod.rs`

**Step 1: Add LINE to handle_command List (around line 291-307)**

Update the channel list in `handle_command`:

```rust
crate::ChannelCommands::List => {
    println!("Channels:");
    println!("  ✅ CLI (always available)");
    for (name, configured) in [
        ("Telegram", config.channels_config.telegram.is_some()),
        ("Discord", config.channels_config.discord.is_some()),
        ("Slack", config.channels_config.slack.is_some()),
        ("LINE", config.channels_config.line.is_some()),  // ADD THIS
        ("Webhook", config.channels_config.webhook.is_some()),
        ("iMessage", config.channels_config.imessage.is_some()),
        ("Matrix", config.channels_config.matrix.is_some()),
        ("WhatsApp", config.channels_config.whatsapp.is_some()),
        ("Email", config.channels_config.email.is_some()),
        ("IRC", config.channels_config.irc.is_some()),
    ] {
        println!("  {} {name}", if configured { "✅" } else { "❌" });
    }
    println!("\nTo start channels: zeroclaw channel start");
    println!("To check health:    zeroclaw channel doctor");
    println!("To configure:      zeroclaw onboard");
    Ok(())
}
```

**Step 2: Add LINE to doctor_channels (around line 344-431)**

Add after Telegram check:

```rust
if let Some(ref line_cfg) = config.channels_config.line {
    channels.push((
        "LINE",
        Arc::new(LineChannel::new(
            line_cfg.channel_access_token.clone(),
            line_cfg.channel_secret.clone(),
            line_cfg.allowed_users.clone(),
        )),
    ));
}
```

**Step 3: Add LINE to start_channels (around line 551-617)**

Add after Telegram check:

```rust
if let Some(ref line_cfg) = config.channels_config.line {
    channels.push(Arc::new(LineChannel::new(
        line_cfg.channel_access_token.clone(),
        line_cfg.channel_secret.clone(),
        line_cfg.allowed_users.clone(),
    )));
}
```

**Step 4: Run tests**

```bash
cargo test -p zeroclaw channels --lib
```

Expected: All tests PASS

**Step 5: Commit**

```bash
git add src/channels/mod.rs
git commit -m "feat(channels): integrate LINE into channel manager"
```

---

## Task 4: Add LINE Webhook Types and Handler

**Files:**
- Create: `src/channels/line_webhook.rs`
- Modify: `src/channels/mod.rs`

**Step 1: Create webhook types module**

Create `src/channels/line_webhook.rs`:

```rust
use serde::{Deserialize, Serialize};

/// LINE webhook request body
#[derive(Debug, Deserialize)]
pub struct LineWebhook {
    pub destination: String,
    pub events: Vec<WebhookEvent>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookEvent {
    #[serde(rename = "type")]
    pub event_type: WebhookEventType,
    pub mode: String,
    pub timestamp: i64,
    pub source: WebhookSource,
    #[serde(default)]
    pub message: Option<WebhookMessage>,
    pub reply_token: Option<String>,
    #[serde(default)]
    pub postback: Option<WebhookPostback>,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WebhookEventType {
    Message,
    Postback,
    Beacon,
    MemberJoined,
    MemberLeft,
    Follow,
    Unfollow,
    AccountLink,
    Things,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
pub struct WebhookSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub user_id: String,
    #[serde(default)]
    pub group_id: Option<String>,
    #[serde(default)]
    pub room_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookMessage {
    #[serde(rename = "type")]
    pub message_type: String,
    pub id: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub file_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookPostback {
    pub data: String,
    #[serde(default)]
    pub params: Option<PostbackParams>,
}

#[derive(Debug, Deserialize)]
pub struct PostbackParams {
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub time: Option<String>,
    #[serde(default)]
    pub datetime: Option<String>,
}

/// Message object for sending
#[derive(Debug, Serialize)]
pub struct LineMessage {
    #[serde(rename = "type")]
    pub message_type: String,
    pub text: String,
}

impl LineMessage {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            message_type: "text".to_string(),
            text: text.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webhook_parse_message_event() {
        let json = r#"{
            "destination": "U123",
            "events": [{
                "type": "message",
                "mode": "active",
                "timestamp": 1234567890,
                "source": {
                    "type": "user",
                    "userId": "Uabc123"
                },
                "message": {
                    "type": "text",
                    "id": "msg_id",
                    "text": "Hello"
                },
                "replyToken": "reply_token"
            }]
        }"#;

        let webhook: LineWebhook = serde_json::from_str(json).unwrap();
        assert_eq!(webhook.destination, "U123");
        assert_eq!(webhook.events.len(), 1);
        assert_eq!(webhook.events[0].event_type, WebhookEventType::Message);
        assert_eq!(webhook.events[0].source.user_id, "Uabc123");
        assert_eq!(webhook.events[0].message.as_ref().unwrap().text, Some("Hello".to_string()));
    }

    #[test]
    fn line_message_serialization() {
        let msg = LineMessage::text("test");
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        assert!(json.contains("\"text\":\"test\""));
    }
}
```

**Step 2: Export webhook types from mod.rs**

Add to `src/channels/mod.rs`:

```rust
pub mod line_webhook;
pub use line_webhook::{LineMessage, LineWebhook, WebhookEvent, WebhookEventType};
```

**Step 3: Run tests**

```bash
cargo test -p zeroclaw line_webhook --lib
```

Expected: All tests PASS

**Step 4: Commit**

```bash
git add src/channels/line_webhook.rs src/channels/mod.rs
git commit -m "feat(channels): add LINE webhook types"
```

---

## Task 5: Add LINE Webhook Endpoint to Gateway

**Files:**
- Modify: `src/gateway/mod.rs`

**Step 1: Add LINE webhook handler**

Find the gateway handler section and add after existing webhook handlers:

```rust
use crate::channels::line_webhook::{LineWebhook, WebhookEventType};
use crate::channels::{ChannelMessage, LineChannel};
use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use bytes::Bytes;

/// Handle LINE webhook events
pub async fn handle_line_webhook(
    State(state): State<Arc<GatewayState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, Error> {
    // Get LINE channel from config
    let line_channel = state.line_channel.as_ref().ok_or_else(|| {
        Error::Configuration("LINE channel not configured".to_string())
    })?;

    // Verify signature
    let signature = headers
        .get("x-line-signature")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| Error::Unauthorized("Missing X-Line-Signature".to_string()))?;

    if !line_channel.verify_webhook_signature(&body, signature) {
        return Err(Error::Unauthorized("Invalid signature".to_string()));
    }

    // Parse webhook
    let webhook: LineWebhook = serde_json::from_slice(&body)
        .map_err(|e| Error::BadRequest(format!("Invalid webhook JSON: {e}")))?;

    // Process events
    for event in webhook.events {
        if event.event_type != WebhookEventType::Message {
            continue;
        }

        let Some(msg) = event.message else {
            continue;
        };

        let Some(text) = msg.text else {
            continue;
        };

        // Check if user is allowed
        if !line_channel.is_user_allowed(&event.source.user_id) {
            tracing::warn!("LINE: ignoring message from unauthorized user: {}", event.source.user_id);
            continue;
        }

        let channel_msg = ChannelMessage {
            id: Uuid::new_v4().to_string(),
            sender: event.source.user_id.clone(),
            content: text,
            channel: "line".to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        // Send to message bus
        if state.message_tx.send(channel_msg).await.is_err() {
            tracing::error!("Failed to send LINE message to bus");
        }
    }

    Ok(StatusCode::OK)
}
```

**Step 2: Add LINE channel to GatewayState**

Update GatewayState struct:

```rust
pub struct GatewayState {
    // ... existing fields ...
    pub line_channel: Option<Arc<LineChannel>>,
    pub message_tx: tokio::sync::mpsc::Sender<crate::channels::traits::ChannelMessage>,
}
```

**Step 3: Add LINE route to router**

Add to the router setup:

```rust
// LINE webhook endpoint
.route("/webhook/line", post(handle_line_webhook))
```

**Step 4: Update gateway initialization to include LINE channel**

When creating GatewayState, add:

```rust
let line_channel = config.channels_config.line.as_ref().map(|cfg| {
    Arc::new(LineChannel::new(
        cfg.channel_access_token.clone(),
        cfg.channel_secret.clone(),
        cfg.allowed_users.clone(),
    )) as Arc<LineChannel>
});

let state = Arc::new(GatewayState {
    // ... existing fields ...
    line_channel,
    // ...
});
```

**Step 5: Add tests for webhook signature verification**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_signature_extraction() {
        let mut headers = HeaderMap::new();
        headers.insert("x-line-signature", "test_sig");

        let signature = headers
            .get("x-line-signature")
            .and_then(|h| h.to_str().ok());

        assert_eq!(signature, Some("test_sig"));
    }
}
```

**Step 6: Run tests**

```bash
cargo test -p zeroclaw gateway --lib
```

Expected: All tests PASS

**Step 7: Commit**

```bash
git add src/gateway/mod.rs
git commit -m "feat(gateway): add LINE webhook endpoint"
```

---

## Task 6: Add Onboarding Support for LINE

**Files:**
- Modify: `src/onboard/mod.rs` or relevant onboarding file

**Step 1: Add LINE to onboarding prompts**

Find the channel configuration section in onboarding and add LINE prompts:

```rust
// Configure LINE channel
let line_enabled = confirm("Enable LINE Official Account channel?").await?;
if line_enabled {
    let channel_access_token = text("LINE Channel Access Token:")
        .with_validator(|s| {
            if s.is_empty() {
                Err("Channel Access Token is required".to_string())
            } else {
                Ok(())
            }
        })
        .ask()?;

    let channel_secret = text("LINE Channel Secret:")
        .with_validator(|s| {
            if s.is_empty() {
                Err("Channel Secret is required".to_string())
            } else {
                Ok(())
            }
        })
        .ask()?;

    let allowed_users_input = text("Allowed LINE User IDs (comma-separated, or * for all):")
        .default("*")
        .ask()?;

    let allowed_users: Vec<String> = allowed_users_input
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    config.channels_config.line = Some(LineConfig {
        channel_access_token,
        channel_secret,
        allowed_users,
    });
}
```

**Step 2: Run onboarding to test**

```bash
cargo run -- onboard
```

Expected: Onboarding includes LINE channel option

**Step 3: Commit**

```bash
git add src/onboard/mod.rs
git commit -m "feat(onboard): add LINE channel configuration"
```

---

## Task 7: Add Rich Message Support (Optional Enhancement)

**Files:**
- Modify: `src/channels/line.rs`

**Step 1: Add Flex Message support**

```rust
use serde_json::Value;

impl LineChannel {
    /// Send a flex message
    pub async fn send_flex(&self,
                           to: &str,
                           alt_text: &str,
                           contents: &Value) -> anyhow::Result<()> {
        let messages = serde_json::json!([{
            "type": "flex",
            "altText": alt_text,
            "contents": contents
        }]);
        self.send_push(to, messages).await
    }
}
```

**Step 2: Add Quick Reply support**

```rust
pub struct QuickReplyItem {
    pub label: String,
    pub text: String,
}

impl LineChannel {
    /// Send message with quick reply buttons
    pub async fn send_with_quick_reply(&self,
                                       to: &str,
                                       text: &str,
                                       items: Vec<QuickReplyItem>) -> anyhow::Result<()> {
        let quick_reply_items: Vec<Value> = items.into_iter()
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
```

**Step 3: Add tests for rich messages**

```rust
#[tokio::test]
async fn test_flex_message_serialization() {
    let flex_contents = serde_json::json!({
        "type": "bubble",
        "body": {
            "type": "box",
            "contents": [{
                "type": "text",
                "text": "Hello"
            }]
        }
    });

    // Verify serialization structure
    assert!(flex_contents["type"] == "bubble");
}

#[test]
fn test_quick_reply_item_creation() {
    let item = QuickReplyItem {
        label: "Yes".to_string(),
        text: "yes".to_string(),
    };
    assert_eq!(item.label, "Yes");
}
```

**Step 4: Run tests**

```bash
cargo test -p zeroclaw line --lib
```

Expected: All tests PASS

**Step 5: Commit**

```bash
git add src/channels/line.rs
git commit -m "feat(channels): add LINE Flex Message and Quick Reply support"
```

---

## Task 8: Documentation and Examples

**Files:**
- Create: `docs/line-channel.md`

**Step 1: Create LINE Channel documentation**

Create `docs/line-channel.md`:

```markdown
# LINE Official Account Channel

## Overview

The LINE channel enables ZeroClaw to receive and send messages through LINE Official Accounts using the LINE Messaging API.

## Setup

### 1. Create LINE OA Channel

1. Go to [LINE Developers Console](https://developers.line.biz/console/)
2. Create a new provider and channel (Messaging API)
3. Get your **Channel Access Token** (long-lived)
4. Get your **Channel Secret**

### 2. Configure Webhook

1. Set webhook URL to: `https://your-domain/webhook/line`
2. Enable "Use webhook"
3. Disable "Auto-reply messages" for bot behavior

### 3. Configure ZeroClaw

Run `zeroclaw onboard` and select LINE channel, or edit config:

```toml
[channels_config.line]
channel_access_token = "your_channel_access_token"
channel_secret = "your_channel_secret"
allowed_users = ["U1234567890"]  # or "*" for all users
```

### 4. Get Your User ID

Send a message to your LINE OA and check logs, or use the CLI:

```bash
zeroclaw channel doctor
```

## Usage

### Start the channel

```bash
zeroclaw channel start
```

### Send a message

Just send a text message to your LINE OA.

### Rich Messages (programmatic)

```rust
// Quick Reply
line_channel.send_with_quick_reply(
    user_id,
    "Choose an option:",
    vec![
        QuickReplyItem { label: "Yes".into(), text: "yes".into() },
        QuickReplyItem { label: "No".into(), text: "no".into() },
    ]
).await?;

// Flex Message
let flex = serde_json::json!({
    "type": "bubble",
    "body": {
        "type": "box",
        "contents": [{"type": "text", "text": "Hello!"}]
    }
});
line_channel.send_flex(user_id, "Alt text", &flex).await?;
```

## Troubleshooting

### Webhook not receiving messages

1. Check your tunnel is running: `zeroclaw tunnel status`
2. Verify webhook URL in LINE Developers Console
3. Check `X-Line-Signature` header is being received

### "Invalid signature" error

- Verify `channel_secret` matches LINE Developers Console
- Check signature isn't being modified by proxies/load balancers

### "Unauthorized user" in logs

- Add your LINE User ID to `allowed_users` in config
- Or use "*" to allow all users (development only)
```

**Step 2: Update README**

Add to main README channels section:

```markdown
### LINE Official Account

- Webhook-based message reception
- Text messages, Flex Messages, Quick Reply
- User ID allowlist for access control
- [Full documentation](docs/line-channel.md)
```

**Step 3: Commit**

```bash
git add docs/line-channel.md README.md
git commit -m "docs: add LINE channel documentation"
```

---

## Task 9: Integration Testing

**Files:**
- Create: `tests/integration_line_channel.rs`

**Step 1: Create integration test**

Create `tests/integration_line_channel.rs`:

```rust
use zeroclaw::channels::{LineChannel, ChannelMessage};
use zeroclaw::channels::line_webhook::{LineWebhook, LineMessage};

#[tokio::test]
async fn line_channel_signature_roundtrip() {
    let secret = "test_secret_for_integration";
    let body = b"integration_test_body";

    let channel = LineChannel::new(
        "dummy_token".into(),
        secret.into(),
        vec!["*".into()],
    );

    // Create signature
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;

    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body);
    let signature = mac.finalize().into_bytes();
    let signature_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature);

    // Verify
    assert!(channel.verify_webhook_signature(body, &signature_b64));
}

#[tokio::test]
async fn line_channel_user_allowlist() {
    let channel = LineChannel::new(
        "token".into(),
        "secret".into(),
        vec!["U123".into(), "U456".into()],
    );

    assert!(channel.is_user_allowed("U123"));
    assert!(channel.is_user_allowed("U456"));
    assert!(!channel.is_user_allowed("U789"));
    assert!(!channel.is_user_allowed(""));
}

#[tokio::test]
async fn line_webhook_parsing() {
    let webhook_json = r#"{
        "destination": "U123",
        "events": [{
            "type": "message",
            "mode": "active",
            "timestamp": 1234567890,
            "source": {
                "type": "user",
                "userId": "Uabc123"
            },
            "message": {
                "type": "text",
                "id": "msg_id",
                "text": "Test message"
            },
            "replyToken": "reply_token"
        }]
    }"#;

    let webhook: LineWebhook = serde_json::from_str(webhook_json).unwrap();
    assert_eq!(webhook.events.len(), 1);
    assert_eq!(webhook.events[0].source.user_id, "Uabc123");
}

#[tokio::test]
async fn line_message_builder() {
    let msg = LineMessage::text("Hello, LINE!");
    let json = serde_json::to_string(&msg).unwrap();

    assert!(json.contains("\"type\":\"text\""));
    assert!(json.contains("\"text\":\"Hello, LINE!\""));
}
```

**Step 2: Run integration tests**

```bash
cargo test --test integration_line_channel
```

Expected: All tests PASS

**Step 3: Commit**

```bash
git add tests/integration_line_channel.rs
git commit -m "test: add LINE channel integration tests"
```

---

## Task 10: Final Verification and Cleanup

**Files:**
- Multiple

**Step 1: Run full test suite**

```bash
cargo test --all
```

Expected: All tests PASS

**Step 2: Check compilation**

```bash
cargo build --release
```

Expected: Binary compiles without warnings

**Step 3: Verify channel list**

```bash
cargo run -- channel list
```

Expected: LINE appears in channel list

**Step 4: Final commit**

```bash
git add .
git commit -m "feat(channels): complete LINE OA channel implementation"
```

**Step 5: Create release notes**

Add to CHANGELOG or release notes:

```markdown
## Added

- LINE Official Account channel support
  - Webhook-based message reception
  - Text, Flex Message, and Quick Reply support
  - User ID allowlist for access control
  - Signature verification for security
```

---

## Implementation Notes

### Dependencies Required
- `hmac = "0.12"`
- `sha2 = "0.10"`
- `base64 = "0.22"`
- `uuid = "1"` (already present)

### LINE API Endpoints Used
- `POST https://api.line.me/v2/bot/message/reply` - Reply to webhook event
- `POST https://api.line.me/v2/bot/message/push` - Send proactive message
- `GET https://api.line.me/v2/bot/info` - Health check / bot info

### Webhook Flow
1. LINE sends POST to `/webhook/line` with `X-Line-Signature` header
2. Gateway validates signature using `channel_secret`
3. Events parsed, user allowlist checked
4. Valid messages forwarded to message bus
5. LLM response sent back via LINE API

### Security Considerations
- Always verify webhook signatures
- Use allowlist for user access control
- Encrypt tokens in config (existing `secrets.encrypt` option)
- Consider rate limiting for public deployments
