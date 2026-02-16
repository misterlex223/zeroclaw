# Lark (Feishu) Channel Integration

## Overview
ZeroClaw supports Lark Messaging API via webhook/events.

## Setup

### Prerequisites
1. [Lark Open Platform Account](https://open.larksuite.com/)
2. Lark App with Bot capability

### Configuration

#### 1. Create Lark App
1. Go to [Lark Open Platform](https://open.larksuite.com/)
2. Create app, enable Bot capability
3. Copy App ID and App Secret

#### 2. Configure Webhook
Webhook URL: `https://your-domain/lark`

#### 3. Configure ZeroClaw
```bash
zeroclaw onboard
```

Or manually to `config.toml`:
```toml
[channels_config.lark]
app_id = "your_app_id"
app_secret = "your_app_secret"
verify_token = "your_verify_token"
encrypt_key = "your_encrypt_key"  # optional
allowed_users = ["*"]
```

## Usage
```rust
channel.send("Hello", user_id).await?;
channel.send_post(user_id, content).await?;
channel.send_card(user_id, "Title", elements).await?;
```
