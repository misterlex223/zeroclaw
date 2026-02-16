# LINE Channel Integration

## Overview

ZeroClaw supports LINE Messaging API for sending and receiving messages via LINE Official Accounts.

## Setup

### Prerequisites

1. [LINE Developers Account](https://developers.line.biz/)
2. LINE Official Account
3. Messaging API channel

### Configuration

#### 1. Create a LINE Provider and Channel

1. Go to [LINE Developers Console](https://developers.line.biz/console/)
2. Create a new provider (or use existing)
3. Create a new Messaging API channel
4. Note your **Channel Access Token** and **Channel Secret**

#### 2. Configure Webhook

1. In your channel settings, set webhook URL to:
   ```
   https://your-domain/webhook/line
   ```
2. Enable "Use webhook"
3. Verify webhook endpoint is accessible

#### 3. Configure ZeroClaw

Run the onboarding wizard:
```bash
zeroclaw onboard
```

Select "LINE" and enter:
- Channel Access Token
- Channel Secret
- Allowed user IDs (comma-separated, or `*` for all)

Or manually add to `config.toml`:
```toml
[channels_config.line]
channel_access_token = "your_channel_access_token"
channel_secret = "your_channel_secret"
allowed_users = ["*"]  # or specific user IDs: ["U123...", "U456..."]
```

## Usage

### Sending Messages

```rust
// Simple text message
channel.send("Hello from ZeroClaw!", user_id).await?;

// Rich message with quick reply
let items = vec![
    QuickReplyItem {
        label: "Yes".to_string(),
        text: "yes".to_string(),
    },
    QuickReplyItem {
        label: "No".to_string(),
        text: "no".to_string(),
    },
];
channel.send_with_quick_reply(user_id, "Do you want to continue?", items).await?;

// Flex message (rich templates)
let flex_contents = serde_json::json!({
    "type": "bubble",
    "body": {
        "type": "box",
        "contents": [
            {
                "type": "text",
                "text": "Hello"
            }
        ]
    }
});
channel.send_flex(user_id, "Alt text", &flex_contents).await?;
```

### Receiving Messages

Messages from LINE are received via webhook and forwarded to your agent through the gateway channel.

## Finding Your LINE User ID

1. Add your LINE Official Account as a friend
2. Send a message
3. Check ZeroClaw logs for the user ID in the webhook payload
4. Add this ID to your `allowed_users` list

## Troubleshooting

### Connection Failed

- Verify Channel Access Token is correct
- Check token has not expired
- Ensure network connectivity to `api.line.me`

### Webhook Not Receiving Messages

- Verify webhook URL is correct and accessible
- Check webhook is enabled in LINE console
- Verify webhook endpoint returns 200 OK

### Signature Verification Failed

- Verify Channel Secret matches
- Ensure webhook body is not modified before verification

### User Not Allowed

- Add user ID to `allowed_users` in config
- Or use `"*"` to allow all users (not recommended for production)

## Rich Messages

LINE supports various message types:

- **Text**: Simple text messages
- **Flex**: Rich custom layouts (cards, carousels)
- **Quick Reply**: Action buttons for user responses

See [LINE Messaging API Documentation](https://developers.line.biz/en/reference/messaging-api/) for details.

## Security Notes

- Keep Channel Access Token and Channel Secret secure
- Use environment variables for sensitive values
- Restrict `allowed_users` in production
- Verify webhook signatures for all incoming messages
