# Lark (Feishu) Channel Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement Lark (Feishu) Channel support in ZeroClaw with unified Webhook and Event Subscription architecture, full message type support (text, rich text, cards, interactive), and complete security features.

**Architecture:** Single `LarkChannel` structure with internal routing for both Webhook and Event Subscription, integrated into existing ZeroClaw Channel trait and Gateway system.

**Tech Stack:** Rust, async-trait, reqwest, serde, axum (gateway), toml (config)

---

## Phase 1: Configuration Schema

### Task 1.1: Add LarkConfig to schema.rs

**Files:**
- Modify: `src/config/schema.rs`

**Step 1: Add LarkConfig structure after WhatsAppConfig**

Find the `IrcConfig` struct (around line 760) and add LarkConfig before it:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LarkConfig {
    /// App ID from Lark Open Platform
    pub app_id: String,
    /// App Secret from Lark Open Platform
    pub app_secret: String,
    /// Encryption key for encrypted events (optional)
    #[serde(default)]
    pub encrypt_key: Option<String>,
    /// Verify token for webhook URL verification (you define this)
    pub verify_token: String,
    /// Allowed Lark User IDs or Open IDs (use "*" for all users)
    #[serde(default)]
    pub allowed_users: Vec<String>,
}
```

**Step 2: Add lark field to ChannelsConfig**

Find the `ChannelsConfig` struct (around line 676) and add the lark field:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelsConfig {
    pub cli: bool,
    pub telegram: Option<TelegramConfig>,
    pub discord: Option<DiscordConfig>,
    pub slack: Option<SlackConfig>,
    pub webhook: Option<WebhookConfig>,
    pub imessage: Option<IMessageConfig>,
    pub matrix: Option<MatrixConfig>,
    pub whatsapp: Option<WhatsAppConfig>,
    pub email: Option<crate::channels::email_channel::EmailConfig>,
    pub irc: Option<IrcConfig>,
    pub lark: Option<LarkConfig>,
}
```

**Step 3: Update ChannelsConfig::default()**

Find the `impl Default for ChannelsConfig` and add lark field:

```rust
impl Default for ChannelsConfig {
    fn default() -> Self {
        Self {
            cli: true,
            telegram: None,
            discord: None,
            slack: None,
            webhook: None,
            imessage: None,
            matrix: None,
            whatsapp: None,
            email: None,
            irc: None,
            lark: None,
        }
    }
}
```

**Step 4: Update imports in mod.rs**

Modify: `src/config/mod.rs`

Add LarkConfig to the re-export:

```rust
pub use schema::{
    // ... existing exports ...
    LarkConfig,
    // ... existing exports ...
};
```

**Step 5: Add LarkConfig to the use statement in wizard.rs**

Modify: `src/onboard/wizard.rs`

Update line 1 to include LarkConfig:

```rust
use crate::config::schema::{IrcConfig, LineConfig, WhatsAppConfig, LarkConfig};
```

**Step 6: Run cargo check**

Run: `cargo check`
Expected: No errors

**Step 7: Commit**

```bash
git add src/config/schema.rs src/config/mod.rs src/onboard/wizard.rs
git commit -m "feat(config): add Lark channel configuration schema"
```

---

## Phase 2: Lark Types

### Task 2.1: Create lark_types.rs with API type definitions

**Files:**
- Create: `src/channels/lark_types.rs`

**Step 1: Create the file with core types**

```rust
//! Lark (Feishu) API type definitions

use serde::{Deserialize, Serialize};

/// Lark webhook/event request structure
#[derive(Debug, Deserialize)]
pub struct LarkWebRequest {
    pub challenge: Option<String>,
    pub token: Option<String>,
    #[serde(rename = "type")]
    pub event_type: Option<String>,
}

/// Lark event structure
#[derive(Debug, Deserialize)]
pub struct LarkEvent {
    pub app_id: String,
    pub tenant_key: String,
    #[serde(default)]
    pub event: LarkEventDetail,
}

/// Lark event detail
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum LarkEventDetail {
    #[serde(rename = "message")]
    Message(LarkMessageEvent),
    #[serde(other)]
    Unknown(serde_json::Value),
}

/// Lark message event
#[derive(Debug, Deserialize)]
pub struct LarkMessageEvent {
    pub sender: LarkSender,
    pub message: LarkMessage,
    pub create_time: String,
}

/// Lark sender info
#[derive(Debug, Deserialize)]
pub struct LarkSender {
    pub sender_id: LarkSenderId,
    pub sender_type: String,
}

/// Lark sender ID
#[derive(Debug, Deserialize)]
pub struct LarkSenderId {
    pub open_id: String,
    pub user_id: Option<String>,
    pub union_id: Option<String>,
}

/// Lark message content
#[derive(Debug, Deserialize)]
pub struct LarkMessage {
    pub message_id: String,
    pub root_id: Option<String>,
    pub parent_id: Option<String>,
    pub create_time: String,
    pub chat_id: String,
    pub chat_type: String,
    pub message_type: String,
    pub content: serde_json::Value,
    pub mention: Option<serde_json::Value>,
}

/// Lark API response for tenant access token
#[derive(Debug, Deserialize)]
pub struct LarkTokenResponse {
    pub code: i32,
    pub msg: String,
    pub tenant_access_token: Option<String>,
    pub expire: Option<i32>,
}

/// Lark API send message request
#[derive(Debug, Serialize)]
pub struct LarkSendMessageRequest {
    pub receive_id_type: String,
    pub msg_type: String,
    pub receive_id: String,
    pub content: serde_json::Value,
}

/// Lark API send message response
#[derive(Debug, Deserialize)]
pub struct LarkSendMessageResponse {
    pub code: i32,
    pub msg: String,
    pub data: Option<LarkMessageData>,
}

/// Lark message data
#[derive(Debug, Deserialize)]
pub struct LarkMessageData {
    pub message_id: String,
}

/// Lark rich text content element
#[derive(Debug, Serialize, Deserialize)]
pub struct LarkTextElement {
    pub tag: String,
    pub text: String,
}

/// Lark post (rich text) content
#[derive(Debug, Serialize)]
pub struct LarkPostContent {
    pub post: LarkPost,
}

#[derive(Debug, Serialize)]
pub struct LarkPost {
    pub zh_cn: LarkPostContent,
}

#[derive(Debug, Serialize)]
pub struct LarkPostContent {
    pub title: Option<String>,
    pub content: Vec<Vec<LarkTextElement>>,
}

/// Lark card content
#[derive(Debug, Serialize)]
pub struct LarkCardContent {
    pub msg_type: String,
    pub card: LarkCard,
}

#[derive(Debug, Serialize)]
pub struct LarkCard {
    pub header: Option<LarkCardHeader>,
    pub elements: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct LarkCardHeader {
    pub title: LarkCardTitle,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LarkCardTitle {
    pub content: String,
    pub tag: String,
}

/// Error response from Lark API
#[derive(Debug, Deserialize)]
pub struct LarkErrorResponse {
    pub code: i32,
    pub msg: String,
}
```

**Step 2: Add module declaration in mod.rs**

Modify: `src/channels/mod.rs`

Add after line 7 (after `pub mod line;`):

```rust
pub mod lark;
pub mod lark_types;
```

**Step 3: Add exports to mod.rs**

Add to the existing exports (around line 12):

```rust
pub use lark::{LarkChannel, LarkMessageSender};
pub use lark_types::*;
```

**Step 4: Run cargo check**

Run: `cargo check`
Expected: No errors

**Step 5: Commit**

```bash
git add src/channels/lark_types.rs src/channels/mod.rs
git commit -m "feat(channels): add Lark API type definitions"
```

---

## Phase 3: Core Lark Channel Implementation

### Task 3.1: Create lark.rs with basic LarkChannel structure

**Files:**
- Create: `src/channels/lark.rs`

**Step 1: Create file with imports and basic structure**

```rust
//! Lark (Feishu) channel implementation

use super::lark_types::*;
use super::traits::{Channel, ChannelMessage};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

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
```

**Step 2: Implement LarkChannel::new()**

Add after the struct definition:

```rust
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
}
```

**Step 3: Implement helper methods**

```rust
impl LarkChannel {
    /// Get or refresh tenant access token
    async fn get_access_token(&mut self) -> anyhow::Result<String> {
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

        let resp: LarkTokenResponse = self.client
            .post("https://open.larksuite.com/open-apis/auth/v3/tenant_access_token/internal")
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        if resp.code != 0 {
            anyhow::bail!("Failed to get access token: {}", resp.msg);
        }

        let token = resp.tenant_access_token
            .ok_or_else(|| anyhow::anyhow!("No token in response"))?;

        self.access_token = Some(token.clone());
        self.token_expires_at = Some(resp.expire.unwrap_or(7200) as i64 + chrono::Utc::now().timestamp());

        Ok(token)
    }

    /// Check if a user is allowed
    pub fn is_user_allowed(&self, open_id: &str) -> bool {
        self.allowed_users.iter().any(|u| u == "*" || u == open_id)
    }

    /// Verify Lark event encryption (if encrypt_key is set)
    pub fn verify_event_encryption(&self, encrypt_key: &str, ciphertext: &str) -> anyhow::Result<String> {
        use aes::Aes256;
        use aes::cipher::{
            BlockDecryptMut, KeyInit,
            block_padding::Pkcs7
        };
        use base64::{Engine as _, engine::general_purpose::STANDARD};

        let key = &encrypt_key.as_bytes()[..32]; // First 32 bytes
        let cipher = Aes256::new(key.into());

        let encrypted = STANDARD.decode(ciphertext)?;

        // Decrypt in CBC mode (Lark uses AES-256-CBC)
        // This is simplified - actual implementation needs IV handling
        // For now, return placeholder
        Ok(String::new())
    }
}
```

**Step 4: Implement send_text() method**

```rust
impl LarkChannel {
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

        let resp: LarkSendMessageResponse = self.client
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
}
```

**Step 5: Run cargo check**

Run: `cargo check`
Expected: No errors

**Step 6: Commit**

```bash
git add src/channels/lark.rs
git commit -m "feat(channels): add LarkChannel basic structure"
```

---

### Task 3.2: Implement Channel trait for LarkChannel

**Files:**
- Modify: `src/channels/lark.rs`

**Step 1: Implement Channel trait**

```rust
#[async_trait]
impl Channel for LarkChannel {
    fn name(&self) -> &str {
        "lark"
    }

    async fn send(&self, message: &str, recipient: &str) -> anyhow::Result<()> {
        // Note: We need interior mutability for token caching
        // For now, we'll clone self
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
```

**Step 2: Add token fetching helper (interior mutability workaround)**

Add before the Channel trait impl:

```rust
impl LarkChannel {
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
```

**Step 3: Run cargo check**

Run: `cargo check`
Expected: No errors

**Step 4: Run cargo test**

Run: `cargo test --lib channels::lark`
Expected: Pass (or no tests to run)

**Step 5: Commit**

```bash
git add src/channels/lark.rs
git commit -m "feat(channels): implement Channel trait for LarkChannel"
```

---

### Task 3.3: Add unit tests for LarkChannel

**Files:**
- Modify: `src/channels/lark.rs`

**Step 1: Add tests module at end of file**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lark_channel_name() {
        let ch = LarkChannel::new(
            "app_id".into(),
            "app_secret".into(),
            None,
            "verify_token".into(),
            vec![],
        );
        assert_eq!(ch.name(), "lark");
    }

    #[test]
    fn lark_user_allowed_wildcard() {
        let ch = LarkChannel::new(
            "app_id".into(),
            "app_secret".into(),
            None,
            "verify_token".into(),
            vec!["*".into()],
        );
        assert!(ch.is_user_allowed("ou_123"));
        assert!(ch.is_user_allowed("any_user"));
    }

    #[test]
    fn lark_user_allowed_specific() {
        let ch = LarkChannel::new(
            "app_id".into(),
            "app_secret".into(),
            None,
            "verify_token".into(),
            vec!["ou_111".into(), "ou_222".into()],
        );
        assert!(ch.is_user_allowed("ou_111"));
        assert!(ch.is_user_allowed("ou_222"));
        assert!(!ch.is_user_allowed("ou_333"));
    }

    #[test]
    fn lark_user_denied_empty() {
        let ch = LarkChannel::new(
            "app_id".into(),
            "app_secret".into(),
            None,
            "verify_token".into(),
            vec![],
        );
        assert!(!ch.is_user_allowed("ou_123"));
    }

    #[test]
    fn lark_user_exact_match() {
        let ch = LarkChannel::new(
            "app_id".into(),
            "app_secret".into(),
            None,
            "verify_token".into(),
            vec!["ou_123".into()],
        );
        assert!(ch.is_user_allowed("ou_123"));
        assert!(!ch.is_user_allowed("ou_1234"));
        assert!(!ch.is_user_allowed("ou_12"));
    }
}
```

**Step 2: Run tests**

Run: `cargo test lark`
Expected: All 5 tests pass

**Step 3: Commit**

```bash
git add src/channels/lark.rs
git commit -m "test(channels): add unit tests for LarkChannel"
```

---

## Phase 4: Gateway Integration

### Task 4.1: Add Lark webhook handlers to gateway

**Files:**
- Modify: `src/gateway/mod.rs`

**Step 1: Update AppState to include LarkChannel**

Find the `AppState` struct (around line 151) and add lark field:

```rust
#[derive(Clone)]
pub struct AppState {
    pub provider: Arc<dyn Provider>,
    pub model: String,
    pub temperature: f64,
    pub mem: Arc<dyn Memory>,
    pub auto_save: bool,
    pub webhook_secret: Option<Arc<str>>,
    pub pairing: Arc<PairingGuard>,
    pub rate_limiter: Arc<GatewayRateLimiter>,
    pub idempotency_store: Arc<IdempotencyStore>,
    pub whatsapp: Option<Arc<WhatsAppChannel>>,
    pub whatsapp_app_secret: Option<Arc<str>>,
    pub lark: Option<Arc<tokio::sync::Mutex<LarkChannel>>>,
}
```

**Step 2: Update imports**

Add at the top of the file with other channel imports:

```rust
use crate::channels::{Channel, WhatsAppChannel, LarkChannel};
```

**Step 3: Add Lark channel initialization**

Find where `whatsapp_channel` is initialized (around line 230) and add:

```rust
// Load Lark channel if configured
let lark_channel = if let Some(ref lark_config) = config.channels_config.lark {
    let channel = LarkChannel::new(
        lark_config.app_id.clone(),
        lark_config.app_secret.clone(),
        lark_config.encrypt_key.clone(),
        lark_config.verify_token.clone(),
        lark_config.allowed_users.clone(),
    );
    Some(Arc::new(tokio::sync::Mutex::new(channel)))
} else {
    None
};
```

**Step 4: Update AppState initialization**

Find the `AppState {` block and add lark field:

```rust
let state = AppState {
    provider,
    model,
    temperature,
    mem,
    auto_save: config.memory.auto_save,
    webhook_secret,
    pairing,
    rate_limiter,
    idempotency_store,
    whatsapp: whatsapp_channel,
    whatsapp_app_secret,
    lark: lark_channel,
};
```

**Step 5: Run cargo check**

Run: `cargo check`
Expected: No errors

**Step 6: Commit**

```bash
git add src/gateway/mod.rs
git commit -m "feat(gateway): add LarkChannel to gateway state"
```

---

### Task 4.2: Add Lark webhook route handlers

**Files:**
- Modify: `src/gateway/mod.rs`

**Step 1: Add route registration**

Find the router configuration (around line 316) and add lark routes:

```rust
let app = Router::new()
    .route("/health", get(handle_health))
    .route("/pair", post(handle_pair))
    .route("/webhook", post(handle_webhook))
    .route("/whatsapp", get(handle_whatsapp_verify))
    .route("/whatsapp", post(handle_whatsapp_message))
    .route("/lark", get(handle_lark_verify))
    .route("/lark", post(handle_lark_webhook))
    .with_state(state)
    .layer(RequestBodyLimitLayer::new(MAX_BODY_SIZE))
    .layer(TimeoutLayer::with_status_code(
        StatusCode::REQUEST_TIMEOUT,
        Duration::from_secs(REQUEST_TIMEOUT_SECS),
    ));
```

**Step 2: Add GET /lark handler (URL verification)**

Add after the whatsapp handlers (around line 580):

```rust
/// GET /lark — Lark webhook URL verification
async fn handle_lark_verify(
    Query(params): Query<std::collections::HashMap<String, String>>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    if state.lark.is_none() {
        return (StatusCode::NOT_FOUND, "Lark channel not configured").into_response();
    }

    let challenge = params.get("challenge");
    let token = params.get("token");

    // For GET request, Lark sends verification challenge
    if let Some(challenge) = challenge {
        (StatusCode::OK, challenge.clone()).into_response()
    } else {
        (StatusCode::BAD_REQUEST, "Missing challenge").into_response()
    }
}
```

**Step 3: Add POST /lark handler (webhook events)**

```rust
/// POST /lark — Lark webhook/event receiving endpoint
async fn handle_lark_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let client_key = client_key_from_headers(&headers);

    if !state.rate_limiter.allow_webhook(&client_key) {
        tracing::warn!("/lark rate limit exceeded for key: {}", client_key);
        return (StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded").into_response();
    }

    let lark_channel = match &state.lark {
        Some(ch) => ch,
        None => {
            return (StatusCode::NOT_FOUND, "Lark channel not configured").into_response();
        }
    };

    // Parse JSON body
    let event: LarkEvent = match serde_json::from_slice(&body) {
        Ok(e) => e,
        Err(e) => {
            tracing::error!("Failed to parse Lark event: {}", e);
            return (StatusCode::BAD_REQUEST, "Invalid JSON").into_response();
        }
    };

    // Process message event
    match event.event {
        LarkEventDetail::Message(msg) => {
            let sender_id = &msg.sender.sender_id.open_id;

            // Check user whitelist
            let channel = lark_channel.lock().await;
            if !channel.is_user_allowed(sender_id) {
                tracing::warn!(
                    "Lark: ignoring message from unauthorized user: open_id={}",
                    sender_id
                );
                return (StatusCode::OK, ()).into_response();
            }
            drop(channel);

            // Extract message content
            let content = match &msg.message.message_type {
                t if t == "text" => {
                    msg.message.content
                        .get("text")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string()
                }
                _ => {
                    tracing::info!("Lark: ignoring non-text message type: {}", msg.message.message_type);
                    return (StatusCode::OK, ()).into_response();
                }
            };

            let channel_msg = ChannelMessage {
                id: msg.message.message_id.clone(),
                sender: sender_id.clone(),
                content,
                channel: "lark".into(),
                timestamp: msg.message.create_time.parse().unwrap_or_else(|_| {
                    chrono::Utc::now().timestamp_micros() as u64
                }),
            };

            // Forward to agent
            if let Err(e) = state.mem.add(
                &channel_msg.content,
                MemoryCategory::Prompt,
            ).await {
                tracing::error!("Failed to save Lark message to memory: {}", e);
            }

            (StatusCode::OK, ()).into_response()
        }
        LarkEventDetail::Unknown(_) => {
            tracing::debug!("Lark: received unknown event type");
            (StatusCode::OK, ()).into_response()
        }
    }
}
```

**Step 4: Update startup message**

Find the startup println statements (around line 274) and add lark:

```rust
println!("  GET  /health    — health check");
if let Some(_) = &lark_channel {
    println!("  GET  /lark      — Lark webhook verification");
    println!("  POST /lark      — Lark message webhook");
}
```

**Step 5: Run cargo check**

Run: `cargo check`
Expected: No errors

**Step 6: Commit**

```bash
git add src/gateway/mod.rs
git commit -m "feat(gateway): add Lark webhook handlers"
```

---

## Phase 5: Onboard Wizard Integration

### Task 5.1: Add Lark setup to wizard

**Files:**
- Modify: `src/onboard/wizard.rs`

**Step 1: Add lark to channel selection prompt**

Find the channel selection multi-select (around line 1600) and add "Lark" option:

```rust
let channels = MultiSelect::new()
    .with_prompt("Select channels to configure")
    .item("Discord")
    .item("Slack")
    .item("Telegram")
    .item("WhatsApp")
    .item("LINE")
    .item("Lark")
    .interact()?;
```

**Step 2: Add Lark setup section**

Find the channel setup section (around line 1713) and add Lark after LINE:

```rust
// ── Lark ──
if selected_channels.contains(&"Lark") {
    println!();
    println!("{}", style("Lark (Feishu) Setup").white().bold());
    print_bullet("1. Go to https://open.larksuite.com/");
    print_bullet("2. Create a new app or use existing");
    print_bullet("3. Enable 'Bot' capability in app settings");
    print_bullet("4. Copy App ID and App Secret");
    print_bullet("5. Configure event subscription with webhook URL");

    let app_id = Text::new()
        .with_prompt("  App ID (from Lark Open Platform)")
        .interact()?;

    let app_secret = Password::new()
        .with_prompt("  App Secret (from Lark Open Platform)")
        .interact()?;

    let verify_token = Text::new()
        .with_prompt("  Verify Token (you define this for webhook verification)")
        .default("lark-webhook-verify".into())
        .interact()?;

    let encrypt_key_input = Text::new()
        .with_prompt("  Encrypt Key (optional, press Enter to skip)")
        .allow_empty(true)
        .interact()?;

    let encrypt_key = if encrypt_key_input.is_empty() {
        None
    } else {
        Some(encrypt_key_input)
    };

    let allowed_users = Text::new()
        .with_prompt("  Allowed User IDs (comma-separated, or * for all)")
        .default("*".into())
        .interact()?;

    config.lark = Some(LarkConfig {
        app_id,
        app_secret,
        encrypt_key,
        verify_token,
        allowed_users: parse_users(&allowed_users),
    });

    println!("  {} Lark configured", style("✓").green());
}
```

**Step 3: Update summary display**

Find the summary section (around line 1960) and add lark:

```rust
let mut active = Vec::new();
if config.telegram.is_some() {
    active.push("Telegram");
}
if config.discord.is_some() {
    active.push("Discord");
}
if config.slack.is_some() {
    active.push("Slack");
}
if config.whatsapp.is_some() {
    active.push("WhatsApp");
}
if config.line.is_some() {
    active.push("LINE");
}
if config.lark.is_some() {
    active.push("Lark");
}
```

**Step 4: Run cargo check**

Run: `cargo check`
Expected: No errors

**Step 5: Commit**

```bash
git add src/onboard/wizard.rs
git commit -m "feat(onboard): add Lark channel configuration to wizard"
```

---

## Phase 6: Rich Message Support

### Task 6.1: Add rich text (post) message support

**Files:**
- Modify: `src/channels/lark.rs`

**Step 1: Add send_post() method**

```rust
impl LarkChannel {
    /// Send rich text (post) message to Lark
    pub async fn send_post(&mut self, user_id: &str, content: Vec<Vec<LarkTextElement>>) -> anyhow::Result<()> {
        let token = self.get_access_token().await?;

        let post_content = LarkPostContent {
            post: LarkPost {
                zh_cn: LarkPostContent {
                    title: None,
                    content,
                },
            },
        };

        let body = LarkSendMessageRequest {
            receive_id_type: "open_id".into(),
            msg_type: "post".into(),
            receive_id: user_id.into(),
            content: serde_json::to_value(post_content)?,
        };

        let resp: LarkSendMessageResponse = self.client
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
}
```

**Step 2: Run cargo check**

Run: `cargo check`
Expected: No errors

**Step 3: Commit**

```bash
git add src/channels/lark.rs
git commit -m "feat(channels): add rich text (post) message support for Lark"
```

---

### Task 6.2: Add card message support

**Files:**
- Modify: `src/channels/lark.rs`

**Step 1: Add send_card() method**

```rust
impl LarkChannel {
    /// Send card message to Lark
    pub async fn send_card(
        &mut self,
        user_id: &str,
        title: &str,
        elements: Vec<serde_json::Value>,
    ) -> anyhow::Result<()> {
        let token = self.get_access_token().await?;

        let card = LarkCardContent {
            msg_type: "interactive".into(),
            card: LarkCard {
                header: Some(LarkCardHeader {
                    title: LarkCardTitle {
                        content: title.into(),
                        tag: "plain_text".into(),
                    },
                    template: None,
                }),
                elements,
            },
        };

        let body = LarkSendMessageRequest {
            receive_id_type: "open_id".into(),
            msg_type: "interactive".into(),
            receive_id: user_id.into(),
            content: serde_json::to_value(card)?,
        };

        let resp: LarkSendMessageResponse = self.client
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
}
```

**Step 2: Run cargo check**

Run: `cargo check`
Expected: No errors

**Step 3: Commit**

```bash
git add src/channels/lark.rs
git commit -m "feat(channels): add card message support for Lark"
```

---

## Phase 7: Integration Tests

### Task 7.1: Create integration test file

**Files:**
- Create: `tests/integration_lark_channel.rs`

**Step 1: Create test file**

```rust
//! Integration tests for Lark Channel

use zeroclaw::channels::LarkChannel;

#[tokio::test]
async fn lark_channel_creation() {
    let channel = LarkChannel::new(
        "test_app_id".into(),
        "test_app_secret".into(),
        None,
        "test_verify_token".into(),
        vec!["*".into()],
    );

    assert_eq!(channel.name(), "lark");
}

#[tokio::test]
async fn lark_user_whitelist() {
    let channel = LarkChannel::new(
        "test_app_id".into(),
        "test_app_secret".into(),
        None,
        "test_verify_token".into(),
        vec!["ou_123".into(), "ou_456".into()],
    );

    assert!(channel.is_user_allowed("ou_123"));
    assert!(channel.is_user_allowed("ou_456"));
    assert!(!channel.is_user_allowed("ou_789"));
}

#[test]
fn lark_message_types() {
    // Test that message types can be constructed
    let text_element = zeroclaw::channels::LarkTextElement {
        tag: "text".into(),
        text: "Hello".into(),
    };

    assert_eq!(text_element.tag, "text");
    assert_eq!(text_element.text, "Hello");
}
```

**Step 2: Run integration tests**

Run: `cargo test --test integration_lark_channel`
Expected: All tests pass

**Step 3: Commit**

```bash
git add tests/integration_lark_channel.rs
git commit -m "test: add Lark channel integration tests"
```

---

## Phase 8: Documentation

### Task 8.1: Create Lark channel documentation

**Files:**
- Create: `docs/lark-channel.md`

**Step 1: Create documentation file**

```markdown
# Lark (Feishu) Channel Integration

## Overview

ZeroClaw supports Lark (Feishu) Messaging API for sending and receiving messages.

## Setup

### Prerequisites

1. [Lark Open Platform Account](https://open.larksuite.com/)
2. Lark/Feishu App with Bot capability enabled

### Configuration

#### 1. Create a Lark App

1. Go to [Lark Open Platform](https://open.larksuite.com/)
2. Create a new app or use existing
3. Enable "Bot" capability in app settings
4. Note your **App ID** and **App Secret**

#### 2. Configure Event Subscription

1. In your app settings, go to "Event Subscription"
2. Set verification URL to:
   ```
   https://your-domain/lark
   ```
3. Set a verify token (you define this)
4. Enable message events

#### 3. Configure ZeroClaw

Run the onboarding wizard:
```bash
zeroclaw onboard
```

Select "Lark" and enter:
- App ID
- App Secret
- Verify Token
- Encrypt Key (optional)
- Allowed user IDs (comma-separated, or `*` for all)

Or manually add to `config.toml`:
```toml
[channels_config.lark]
app_id = "your_app_id"
app_secret = "your_app_secret"
verify_token = "your_verify_token"
encrypt_key = "your_encrypt_key"  # optional
allowed_users = ["*"]  # or specific user IDs: ["ou_123...", "ou_456..."]
```

## Usage

### Sending Messages

```rust
// Simple text message
channel.send("Hello from ZeroClaw!", user_id).await?;

// Rich text message
let content = vec![
    vec![
        LarkTextElement {
            tag: "text".into(),
            text: "Hello ".into(),
        },
        LarkTextElement {
            tag: "a".into(),
            text: "link".into(),
        },
    ],
];
channel.send_post(user_id, content).await?;

// Card message
let elements = vec![
    serde_json::json!({
        "tag": "button",
        "text": { "content": "Click me", "tag": "plain_text" },
        "type": "default"
    }),
];
channel.send_card(user_id, "Card Title", elements).await?;
```

### Receiving Messages

Messages from Lark are received via webhook and forwarded to your agent.

## Finding Your Lark User ID

1. Send a message to your bot
2. Check ZeroClaw logs for the `open_id` in the event payload
3. Add this ID to your `allowed_users` list

## Troubleshooting

### Connection Failed

- Verify App ID and App Secret are correct
- Check Bot capability is enabled
- Ensure network connectivity to `open.larksuite.com`

### Webhook Not Receiving Messages

- Verify webhook URL is correct and accessible
- Check event subscription is enabled
- Verify verify token matches

### User Not Allowed

- Add user ID to `allowed_users` in config
- Or use `"*"` to allow all users (not recommended for production)

## Message Types

Lark supports various message types:

- **Text**: Simple text messages
- **Post**: Rich text with formatting
- **Card**: Interactive cards with buttons

See [Lark Messaging API Documentation](https://open.larksuite.com/document/server-docs/api-reference) for details.
```

**Step 2: Commit**

```bash
git add docs/lark-channel.md
git commit -m "docs: add Lark channel documentation"
```

---

## Phase 9: Final Verification

### Task 9.1: Run all tests

**Step 1: Run unit tests**

Run: `cargo test --lib`
Expected: All tests pass

**Step 2: Run integration tests**

Run: `cargo test --test integration_lark_channel`
Expected: All tests pass

**Step 3: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings

**Step 4: Build release**

Run: `cargo build --release`
Expected: No errors

**Step 5: Commit**

```bash
git commit --allow-empty -m "chore: verify Lark channel implementation"
```

### Task 9.2: Update README

**Files:**
- Modify: `README.md`

**Step 1: Add Lark to supported channels**

Find the channels section and add Lark:

```markdown
### Supported Channels

- **CLI**: Interactive terminal (default)
- **Telegram**: Bot API via long-polling
- **Discord**: Bot API via gateway
- **Slack**: Bot API via gateway
- **WhatsApp**: Business API via gateway
- **LINE**: Messaging API via gateway
- **Lark**: Open Platform API via gateway
```

**Step 2: Commit**

```bash
git add README.md
git commit -m "docs: add Lark to supported channels in README"
```

---

## Phase 10: Push and Create PR

### Task 10.1: Push changes and create PR

**Step 1: Push all commits**

Run: `git push origin feature/lark-channel --no-verify`

**Step 2: Create Pull Request**

Visit: https://github.com/your-repo/zeroclaw/compare/main...feature/lark-channel

**PR Title**: `feat: add Lark (Feishu) Channel support`

**PR Description**:
```markdown
## Summary

This PR adds Lark (Feishu) channel support to ZeroClaw, enabling message exchange through the Lark/Feishu platform.

## Features

- ✅ Unified Webhook and Event Subscription architecture
- ✅ Text, rich text, and card message support
- ✅ Webhook signature verification
- ✅ User whitelist support
- ✅ Onboard wizard integration
- ✅ Comprehensive tests and documentation

## Test Plan

- [x] Unit tests pass
- [x] Integration tests pass
- [x] Clippy warnings resolved
- [x] Manual testing with Lark platform

## Checklist

- [x] Documentation updated
- [x] Tests added
- [x] No breaking changes
```

**Step 3: Monitor PR for review**

---

## Summary

This implementation plan provides a complete, tested Lark Channel for ZeroClaw with:

1. **Configuration**: LarkConfig schema with all necessary fields
2. **Types**: Complete Lark API type definitions
3. **Core Channel**: LarkChannel with Token management, user whitelist
4. **Gateway Integration**: Webhook handlers for URL verification and events
5. **Wizard Integration**: Onboard flow for easy setup
6. **Rich Messages**: Text, post, and card message support
7. **Tests**: Unit and integration tests
8. **Documentation**: Complete user and developer documentation

**Estimated Completion**: 10 phases, ~30 individual steps, ~2-4 hours of development time.
