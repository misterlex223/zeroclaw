# Lark (Feishu) Channel Design

## Overview

This document outlines the design for implementing Lark (Feishu) Channel support in ZeroClaw, enabling message exchange through the Lark/Feishu platform.

## Design Goals

1. **Unified Architecture**: Single `LarkChannel` structure integrating both Webhook and Event Subscription
2. **Full Message Support**: Text, rich text, cards, and interactive elements
3. **Complete Security**: Signature verification, user whitelist, request source validation
4. **Onboard Integration**: Seamless setup through the existing wizard

## Architecture

### Component Structure

```
src/
├── channels/
│   ├── mod.rs              # Exports LarkChannel
│   ├── lark.rs             # Main channel implementation
│   └── lark_types.rs       # Lark API type definitions
├── config/
│   └── schema.rs           # LarkConfig addition
├── gateway/
│   └── mod.rs              # /lark webhook route
└── onboard/
    └── wizard.rs           # Lark setup flow
```

### Core Structures

**LarkChannel**:
```rust
pub struct LarkChannel {
    // App Credentials
    app_id: String,
    app_secret: String,
    encrypt_key: Option<String>,
    verify_token: String,

    // Security
    allowed_users: Vec<String>,

    // Client
    client: reqwest::Client,
}
```

**LarkConfig**:
```rust
pub struct LarkConfig {
    pub app_id: String,
    pub app_secret: String,
    pub encrypt_key: Option<String>,
    pub verify_token: String,
    #[serde(default)]
    pub allowed_users: Vec<String>,
}
```

### Gateway Routes

- `GET /lark` - Lark URL verification endpoint
- `POST /lark` - Lark webhook/event receiving endpoint

## Data Flow

### Message Receiving Flow

```
Lark Platform (Webhook/Event)
        │
        │ HTTP POST
        ▼
Gateway (/lark)
        │
        ▼
Signature Verification
        │
        ▼
Event Router (Webhook vs Subscribe)
        │
        ▼
Message Parser
        │
        ▼
Channel Message Queue
```

### Message Sending Flow

```
Agent Core
    │
    ▼
LarkChannel
    │
    │ API Request
    ▼
POST /open-apis/message/v4/send
    │
    ▼
Lark API (HTTPS)
```

### Message Type Mapping

| Lark Type | LarkChannel Method |
|-----------|-------------------|
| Text | `send_text()` |
| Post (Rich Text) | `send_post()` |
| Card | `send_card()` |
| Interactive | `send_interactive()` |

## Error Handling

### Webhook Verification Failures
- Signature verification failed → 401 Unauthorized
- URL verification failed → 403 Forbidden

### API Request Failures
- Rate limit → Exponential backoff retry
- Authentication failed → Log error, return Err
- Network error → Return anyhow::Error

### Message Parsing Failures
- Invalid JSON → Log original payload, return 400
- Missing required fields → Log warning, skip message

## Health Check

```rust
async fn health_check(&self) -> bool {
    self.client
        .get("https://open.larksuite.com/open-apis/bot/v3/info")
        .header("Authorization", format!("Bearer {}", self.get_access_token()?))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}
```

## Testing Strategy

### Unit Tests (`src/channels/lark.rs`)
- Signature verification tests
- User whitelist tests
- Message formatting tests

### Integration Tests (`tests/integration_lark_channel.rs`)
- Webhook end-to-end tests
- API sending tests
- Error scenario tests

## Onboard Wizard Integration

### Setup Flow

1. Guide user to obtain credentials
2. Collect configuration:
   - App ID
   - App Secret
   - Verify Token (user-defined)
   - Encrypt Key (optional)
   - Allowed User IDs
3. Save to `config.toml`
4. Display webhook URL configuration instructions

### Configuration Display

```
Lark (Feishu)  ✓ configured
```

## Security Considerations

1. **Webhook Signature**: Verify Lark's signature for all incoming requests
2. **User Whitelist**: Restrict bot interaction to approved users only
3. **Request Validation**: Ensure requests originate from Lark's official servers
4. **Token Management**: Securely store app credentials

## API Reference

### Lark Open Platform APIs

- **Bot Info**: `GET /open-apis/bot/v3/info`
- **Send Message**: `POST /open-apis/message/v4/send`
- **Get User Info**: `GET /open-apis/contact/v3/users/:user_id`

## Implementation Notes

1. **Access Token**: Auto-fetch tenant access token using `app_id` and `app_secret`
2. **Token Refresh**: Handle token expiration and refresh automatically
3. **Event Types**: Support message events, bot menu events, and card action events
4. **Rate Limiting**: Respect Lark's API rate limits (configurable)

## Dependencies

- `reqwest`: HTTP client for Lark API calls
- `serde`: JSON serialization/deserialization
- `async-trait`: Async trait implementation for Channel

## Timeline

1. Phase 1: Core channel implementation with text messages
2. Phase 2: Rich text and card message support
3. Phase 3: Interactive elements and advanced features
4. Phase 4: Testing and documentation
