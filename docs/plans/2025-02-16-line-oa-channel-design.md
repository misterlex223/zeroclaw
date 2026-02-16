# LINE OA Channel Design

**Date:** 2025-02-16
**Author:** Claude
**Status:** Design Approved

## Overview

Add LINE Official Account (OA) as a new channel to ZeroClaw, enabling users to interact with the AI assistant via LINE messaging platform.

## Architecture

```
┌─────────────────┐     webhook      ┌──────────────────┐
│  LINE Platform  │ ──────────────────▶│ ZeroClaw Gateway │
│                 │◀────────────────── │                  │
└─────────────────┘     reply         └──────────────────┘
                                                │
                                                ▼
                                       ┌────────────────┐
                                       │  LINE Channel  │
                                       │  (validate &   │
                                       │   parse)       │
                                       └────────────────┘
                                                │
                                                ▼
                                       ┌────────────────┐
                                       │  Message Bus   │
                                       │ (mpsc channel) │
                                       └────────────────┘
                                                │
                                                ▼
                                       ┌────────────────┐
                                       │   LLM Provider │
                                       └────────────────┘
```

## Requirements

### Functional Requirements

1. **FR1:** Receive text messages from LINE OA via webhook
2. **FR2:** Send text replies to LINE users
3. **FR3:** Support LINE User ID allowlist for access control
4. **FR4:** Verify webhook signatures using `X-Line-Signature`
5. **FR5:** Support Flex Messages for rich formatted responses
6. **FR6:** Support Quick Reply buttons
7. **FR7:** Support image/file attachments
8. **FR8:** Health check via LINE Bot Info API

### Non-Functional Requirements

1. **NFR1:** Webhook response within 3 seconds
2. **NFR2:** Automatic retry on network failures
3. **NFR3:** Secure credential storage (encrypt tokens)
4. **NFR4:** Graceful degradation on LINE API failures

## Configuration

### Config Structure

```toml
[channels_config.line]
# Channel Access Token (long-lived)
channel_access_token = "LINE_CHANNEL_ACCESS_TOKEN"
# Channel Secret (for webhook signature verification)
channel_secret = "LINE_CHANNEL_SECRET"
# Allowed LINE User IDs (use "*" for all users)
allowed_users = ["U1234567890", "U9876543210"]
```

### Environment Variables

```bash
ZEROCLAW_LINE_CHANNEL_ACCESS_TOKEN=...
ZEROCLAW_LINE_CHANNEL_SECRET=...
```

## Implementation

### 1. Configuration Schema

**File:** `src/config/schema.rs`

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

### 2. LINE Channel

**File:** `src/channels/line.rs`

```rust
pub struct LineChannel {
    channel_access_token: String,
    channel_secret: String,
    allowed_users: Vec<String>,
    client: reqwest::Client,
}

impl LineChannel {
    pub fn new(channel_access_token: String,
               channel_secret: String,
               allowed_users: Vec<String>) -> Self { ... }

    /// Verify LINE webhook signature
    pub fn verify_webhook_signature(&self,
                                    body: &[u8],
                                    signature: &str) -> bool { ... }

    /// Send text message
    pub async fn send_text(&self,
                           to: &str,
                           text: &str) -> anyhow::Result<()> { ... }

    /// Send flex message
    pub async fn send_flex(&self,
                           to: &str,
                           flex_contents: &serde_json::Value) -> anyhow::Result<()> { ... }

    /// Send with quick reply
    pub async fn send_with_quick_reply(&self,
                                       to: &str,
                                       text: &str,
                                       quick_items: Vec<QuickReplyItem>) -> anyhow::Result<()> { ... }
}

#[async_trait]
impl Channel for LineChannel {
    fn name(&self) -> &str { "line" }

    async fn send(&self, message: &str, recipient: &str) -> anyhow::Result<()> {
        self.send_text(recipient, message).await
    }

    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        // Webhook-based: Gateway will push messages via a separate channel
        // This method waits indefinitely
        std::future::pending().await
    }

    async fn health_check(&self) -> bool {
        // Call LINE Bot Info API
        self.client.get("https://api.line.me/v2/bot/info")
            .bearer_auth(&self.channel_access_token)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}
```

### 3. Gateway Integration

**File:** `src/gateway/mod.rs`

Add LINE webhook endpoint:

```rust
// POST /webhook/line
async fn handle_line_webhook(
    State(state): State<Arc<GatewayState>>,
    signature: Header<XLineSignature>,
    body: Bytes,
) -> Result<StatusCode, Error> {
    // Verify signature
    if !state.line_channel.verify_webhook_signature(&body, signature.as_str()) {
        return Ok(StatusCode::UNAUTHORIZED);
    }

    // Parse webhook events
    let webhook: LineWebhook = serde_json::from_slice(&body)?;

    // Process events
    for event in webhook.events {
        match event.type {
            WebhookEventType::Message => {
                if let Some(msg) = event.message {
                    let channel_msg = ChannelMessage {
                        id: Uuid::new_v4().to_string(),
                        sender: event.source.user_id,
                        content: msg.text,
                        channel: "line".to_string(),
                        timestamp: event.timestamp,
                    };
                    let _ = state.message_tx.send(channel_msg).await;
                }
            }
            // Handle other event types...
        }
    }

    Ok(StatusCode::OK)
}
```

### 4. Module Exports

**File:** `src/channels/mod.rs`

```rust
pub mod line;
pub use line::LineChannel;
```

**File:** `src/config/schema.rs`

```rust
pub use schema::{
    // ...
    LineConfig,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelsConfig {
    pub cli: bool,
    pub telegram: Option<TelegramConfig>,
    pub discord: Option<DiscordConfig>,
    pub slack: Option<SlackConfig>,
    pub line: Option<LineConfig>,  // NEW
    // ...
}
```

## Message Types

### Text Message
```json
{
  "replyToken": "reply_token",
  "messages": [
    {
      "type": "text",
      "text": "Hello, world!"
    }
  ]
}
```

### Flex Message
```json
{
  "replyToken": "reply_token",
  "messages": [
    {
      "type": "flex",
      "altText": "This is a flex message",
      "contents": { ... }
    }
  ]
}
```

### Quick Reply
```json
{
  "replyToken": "reply_token",
  "messages": [
    {
      "type": "text",
      "text": "Pick one!",
      "quickReply": {
        "items": [
          {
            "type": "action",
            "action": {
              "type": "message",
              "label": "Yes",
              "text": "yes"
            }
          }
        ]
      }
    }
  ]
}
```

## Security

1. **Webhook Signature Verification**
   - Use HMAC-SHA256 with channel_secret
   - Reject requests with invalid signatures

2. **Access Control**
   - Allowlist based on LINE User ID
   - Wildcard "*" for open access (development only)

3. **Token Security**
   - Encrypt `channel_access_token` in config
   - Use environment variables for deployment

## Testing

### Unit Tests
- Signature verification logic
- User allowlist matching
- Message payload serialization

### Integration Tests
- Webhook endpoint with mock LINE payload
- API send/receive with test channel

### Health Check Tests
- Bot Info API response validation

## Deployment Notes

1. **Public URL Required**
   - Use Cloudflare Tunnel (already supported)
   - Or configure ngrok/custom tunnel

2. **LINE Developers Console**
   - Create Messaging API channel
   - Set webhook URL: `https://your-domain/webhook/line`
   - Enable "Use webhook" checkbox
   - Disable "Auto-reply messages" for bot experience

3. **Tunnel Configuration**
```toml
[tunnel]
provider = "cloudflare"
[tunnel.cloudflare]
token = "your-tunnel-token"
```

## Files to Create/Modify

### New Files
- `src/channels/line.rs` - LINE channel implementation

### Modified Files
- `src/channels/mod.rs` - Export LINE channel
- `src/config/schema.rs` - Add LineConfig and ChannelsConfig field
- `src/gateway/mod.rs` - Add LINE webhook endpoint
- `src/channels/mod.rs` - Include in doctor_channels and start_channels

## References

- [LINE Messaging API Documentation](https://developers.line.biz/en/reference/messaging-api/)
- [LINE Webhook Documentation](https://developers.line.biz/en/reference/messaging-api/#webhook-event)
- [LINE Signature Validation](https://developers.line.biz/en/reference/messaging-api/#signature-validation)
