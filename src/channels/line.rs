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

// =============================================================================
// Message Builders
// =============================================================================

/// Quick reply action types for LINE messages
#[derive(Debug, Clone)]
pub enum QuickReplyAction {
    /// Message action - sends a text message when tapped
    Message { label: String, text: String },
    /// Postback action - sends data via postback event
    Postback { label: String, data: String, text: Option<String> },
    /// URI action - opens a URL
    Uri { label: String, uri: String, alt_uri: Option<String> },
    /// Date picker action - sends date value
    DatePicker { label: String, data: String, initial: Option<String>, max: Option<String>, min: Option<String> },
    /// Time picker action - sends time value
    TimePicker { label: String, data: String, initial: Option<String> },
    /// Datetime picker action - sends datetime value
    DateTimePicker { label: String, data: String, initial: Option<String>, max: Option<String>, min: Option<String> },
}

impl QuickReplyAction {
    fn to_json(&self) -> serde_json::Value {
        match self {
            QuickReplyAction::Message { label, text } => serde_json::json!({
                "type": "action",
                "action": {
                    "type": "message",
                    "label": label,
                    "text": text
                }
            }),
            QuickReplyAction::Postback { label, data, text } => {
                let mut action = serde_json::json!({
                    "type": "postback",
                    "label": label,
                    "data": data
                });
                if let Some(text) = text {
                    action["text"] = serde_json::json!(text);
                }
                serde_json::json!({
                    "type": "action",
                    "action": action
                })
            }
            QuickReplyAction::Uri { label, uri, alt_uri } => {
                let mut action = serde_json::json!({
                    "type": "uri",
                    "label": label,
                    "uri": uri
                });
                if let Some(alt_uri) = alt_uri {
                    action["altUri"] = serde_json::json!({ "desktop": alt_uri });
                }
                serde_json::json!({
                    "type": "action",
                    "action": action
                })
            }
            QuickReplyAction::DatePicker { label, data, initial, max, min } => {
                let mut action = serde_json::json!({
                    "type": "datepicker",
                    "label": label,
                    "data": data
                });
                if let Some(initial) = initial {
                    action["initial"] = serde_json::json!(initial);
                }
                if let Some(max) = max {
                    action["max"] = serde_json::json!(max);
                }
                if let Some(min) = min {
                    action["min"] = serde_json::json!(min);
                }
                serde_json::json!({
                    "type": "action",
                    "action": action
                })
            }
            QuickReplyAction::TimePicker { label, data, initial } => {
                let mut action = serde_json::json!({
                    "type": "timepicker",
                    "label": label,
                    "data": data
                });
                if let Some(initial) = initial {
                    action["initial"] = serde_json::json!(initial);
                }
                serde_json::json!({
                    "type": "action",
                    "action": action
                })
            }
            QuickReplyAction::DateTimePicker { label, data, initial, max, min } => {
                let mut action = serde_json::json!({
                    "type": "datetimepicker",
                    "label": label,
                    "data": data
                });
                if let Some(initial) = initial {
                    action["initial"] = serde_json::json!(initial);
                }
                if let Some(max) = max {
                    action["max"] = serde_json::json!(max);
                }
                if let Some(min) = min {
                    action["min"] = serde_json::json!(min);
                }
                serde_json::json!({
                    "type": "action",
                    "action": action
                })
            }
        }
    }
}

/// Action for template message buttons
#[derive(Debug, Clone)]
pub enum TemplateAction {
    Message { label: String, text: String },
    Postback { label: String, data: String, text: Option<String> },
    Uri { label: String, uri: String, alt_uri: Option<String> },
    DatetimePicker { label: String, data: String, mode: String, initial: Option<String>, max: Option<String>, min: Option<String> },
}

impl TemplateAction {
    fn to_json(&self) -> serde_json::Value {
        match self {
            TemplateAction::Message { label, text } => serde_json::json!({
                "type": "message",
                "label": label,
                "text": text
            }),
            TemplateAction::Postback { label, data, text } => {
                let mut action = serde_json::json!({
                    "type": "postback",
                    "label": label,
                    "data": data
                });
                if let Some(text) = text {
                    action["text"] = serde_json::json!(text);
                }
                action
            }
            TemplateAction::Uri { label, uri, alt_uri } => {
                let mut action = serde_json::json!({
                    "type": "uri",
                    "label": label,
                    "uri": uri
                });
                if let Some(alt_uri) = alt_uri {
                    action["altUri"] = serde_json::json!({ "desktop": alt_uri });
                }
                action
            }
            TemplateAction::DatetimePicker { label, data, mode, initial, max, min } => {
                let mut action = serde_json::json!({
                    "type": "datetimepicker",
                    "label": label,
                    "data": data,
                    "mode": mode
                });
                if let Some(initial) = initial {
                    action["initial"] = serde_json::json!(initial);
                }
                if let Some(max) = max {
                    action["max"] = serde_json::json!(max);
                }
                if let Some(min) = min {
                    action["min"] = serde_json::json!(min);
                }
                action
            }
        }
    }
}

/// Template message column for carousel
#[derive(Debug, Clone)]
pub struct TemplateColumn {
    pub title: String,
    pub text: String,
    pub thumbnail_image_url: Option<String>,
    pub image_background_color: Option<String>,
    pub image_aspect_ratio: Option<String>,
    pub image_size: Option<String>,
    pub image_content_mode: Option<String>,
    pub actions: Vec<TemplateAction>,
}

/// Quick reply item for LINE messages (legacy - use QuickReplyAction instead)
#[deprecated(note = "Use QuickReplyAction instead for more action types")]
pub struct QuickReplyItem {
    pub label: String,
    pub text: String,
}

impl LineChannel {
    // ─────────────────────────────────────────────────────────────────────────────
    // Rich Message Types
    // ─────────────────────────────────────────────────────────────────────────────

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

    /// Send message with quick reply buttons (new version with all action types)
    pub async fn send_with_quick_reply_actions(&self,
                                               to: &str,
                                               text: &str,
                                               actions: Vec<QuickReplyAction>) -> anyhow::Result<()> {
        let quick_reply_items: Vec<serde_json::Value> = actions
            .into_iter()
            .map(|action| action.to_json())
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

    /// Send message with quick reply buttons (legacy version)
    #[deprecated(note = "Use send_with_quick_reply_actions instead")]
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

    // ─────────────────────────────────────────────────────────────────────────────
    // Template Messages
    // ─────────────────────────────────────────────────────────────────────────────

    /// Send buttons template message
    pub async fn send_buttons_template(&self,
                                       to: &str,
                                       alt_text: &str,
                                       title: &str,
                                       text: &str,
                                       thumbnail_image_url: Option<&str>,
                                       actions: Vec<TemplateAction>) -> anyhow::Result<()> {
        let mut template = serde_json::json!({
            "type": "buttons",
            "title": title,
            "text": text,
            "actions": actions.into_iter().map(|action| action.to_json()).collect::<Vec<_>>()
        });
        if let Some(url) = thumbnail_image_url {
            template["thumbnailImageUrl"] = serde_json::json!(url);
        }
        let messages = serde_json::json!([{
            "type": "template",
            "altText": alt_text,
            "template": template
        }]);
        self.send_push(to, messages).await
    }

    /// Send confirm template message (simple yes/no dialog)
    pub async fn send_confirm_template(&self,
                                       to: &str,
                                       alt_text: &str,
                                       text: &str,
                                       ok_action: TemplateAction,
                                       cancel_action: TemplateAction) -> anyhow::Result<()> {
        let messages = serde_json::json!([{
            "type": "template",
            "altText": alt_text,
            "template": {
                "type": "confirm",
                "text": text,
                "actions": [ok_action.to_json(), cancel_action.to_json()]
            }
        }]);
        self.send_push(to, messages).await
    }

    /// Send carousel template message (scrollable columns)
    pub async fn send_carousel_template(&self,
                                        to: &str,
                                        alt_text: &str,
                                        columns: Vec<TemplateColumn>,
                                        image_aspect_ratio: Option<&str>) -> anyhow::Result<()> {
        let columns_json: Vec<serde_json::Value> = columns
            .into_iter()
            .map(|col| {
                let mut json = serde_json::json!({
                    "title": col.title,
                    "text": col.text,
                    "actions": col.actions.into_iter().map(|action| action.to_json()).collect::<Vec<_>>()
                });
                if let Some(url) = col.thumbnail_image_url {
                    json["thumbnailImageUrl"] = serde_json::json!(url);
                }
                if let Some(color) = col.image_background_color {
                    json["imageBackgroundColor"] = serde_json::json!(color);
                }
                if let Some(ratio) = col.image_aspect_ratio {
                    json["imageAspectRatio"] = serde_json::json!(ratio);
                } else if let Some(ratio) = image_aspect_ratio {
                    json["imageAspectRatio"] = serde_json::json!(ratio);
                }
                if let Some(size) = col.image_size {
                    json["imageSize"] = serde_json::json!(size);
                }
                if let Some(mode) = col.image_content_mode {
                    json["imageContentMode"] = serde_json::json!(mode);
                }
                json
            })
            .collect();

        let mut template = serde_json::json!({
            "type": "carousel",
            "columns": columns_json
        });
        if let Some(ratio) = image_aspect_ratio {
            template["imageAspectRatio"] = serde_json::json!(ratio);
        }

        let messages = serde_json::json!([{
            "type": "template",
            "altText": alt_text,
            "template": template
        }]);
        self.send_push(to, messages).await
    }

    /// Send image carousel template message (multiple images)
    pub async fn send_image_carousel_template(&self,
                                              to: &str,
                                              alt_text: &str,
                                              columns: Vec<TemplateColumn>) -> anyhow::Result<()> {
        let columns_json: Vec<serde_json::Value> = columns
            .into_iter()
            .map(|col| {
                let mut json = serde_json::json!({
                    "imageUrl": col.thumbnail_image_url.unwrap_or_default(),
                    "action": col.actions.get(0).map(TemplateAction::to_json).unwrap_or(serde_json::json!({
                        "type": "message",
                        "label": col.title,
                        "text": col.text
                    }))
                });
                if let Some(label) = (!col.title.is_empty()).then(|| col.title.clone()) {
                    json["label"] = serde_json::json!(label);
                }
                json
            })
            .collect();

        let messages = serde_json::json!([{
            "type": "template",
            "altText": alt_text,
            "template": {
                "type": "image_carousel",
                "columns": columns_json
            }
        }]);
        self.send_push(to, messages).await
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // Media Messages
    // ─────────────────────────────────────────────────────────────────────────────

    /// Send image message with URL
    pub async fn send_image(&self,
                            to: &str,
                            original_content_url: &str,
                            preview_image_url: &str) -> anyhow::Result<()> {
        let messages = serde_json::json!([{
            "type": "image",
            "originalContentUrl": original_content_url,
            "previewImageUrl": preview_image_url
        }]);
        self.send_push(to, messages).await
    }

    /// Send video message with URL
    pub async fn send_video(&self,
                            to: &str,
                            original_content_url: &str,
                            preview_image_url: &str) -> anyhow::Result<()> {
        let messages = serde_json::json!([{
            "type": "video",
            "originalContentUrl": original_content_url,
            "previewImageUrl": preview_image_url
        }]);
        self.send_push(to, messages).await
    }

    /// Send audio message with URL
    pub async fn send_audio(&self,
                            to: &str,
                            original_content_url: &str,
                            duration: u64) -> anyhow::Result<()> {
        let messages = serde_json::json!([{
            "type": "audio",
            "originalContentUrl": original_content_url,
            "duration": duration
        }]);
        self.send_push(to, messages).await
    }

    /// Upload and send image (returns the content URL)
    pub async fn upload_image(&self,
                              to: &str,
                              image_data: Vec<u8>,
                              content_type: &str) -> anyhow::Result<String> {
        let url = format!("https://api.line.me/v2/bot/message/{to}/upload");

        let resp = self.client
            .post(&url)
            .bearer_auth(&self.channel_access_token)
            .header("Content-Type", content_type)
            .body(image_data)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_body = resp.text().await.unwrap_or_default();
            anyhow::bail!("LINE image upload failed ({status}): {error_body}");
        }

        let json: serde_json::Value = resp.json().await?;
        json.get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("No content ID in upload response"))
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // Location & Sticker Messages
    // ─────────────────────────────────────────────────────────────────────────────

    /// Send location message
    pub async fn send_location(&self,
                               to: &str,
                               title: &str,
                               address: &str,
                               latitude: f64,
                               longitude: f64) -> anyhow::Result<()> {
        let messages = serde_json::json!([{
            "type": "location",
            "title": title,
            "address": address,
            "latitude": latitude,
            "longitude": longitude
        }]);
        self.send_push(to, messages).await
    }

    /// Send sticker message
    pub async fn send_sticker(&self,
                              to: &str,
                              package_id: &str,
                              sticker_id: &str) -> anyhow::Result<()> {
        let messages = serde_json::json!([{
            "type": "sticker",
            "packageId": package_id,
            "stickerId": sticker_id
        }]);
        self.send_push(to, messages).await
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // Reply Variants
    // ─────────────────────────────────────────────────────────────────────────────

    /// Reply with quick reply actions
    pub async fn reply_with_quick_reply_actions(&self,
                                                 reply_token: &str,
                                                 text: &str,
                                                 actions: Vec<QuickReplyAction>) -> anyhow::Result<()> {
        let quick_reply_items: Vec<serde_json::Value> = actions
            .into_iter()
            .map(|action| action.to_json())
            .collect();

        let messages = serde_json::json!([{
            "type": "text",
            "text": text,
            "quickReply": {
                "items": quick_reply_items
            }
        }]);
        self.send_reply(reply_token, messages).await
    }

    /// Reply with buttons template
    pub async fn reply_buttons_template(&self,
                                        reply_token: &str,
                                        alt_text: &str,
                                        title: &str,
                                        text: &str,
                                        thumbnail_image_url: Option<&str>,
                                        actions: Vec<TemplateAction>) -> anyhow::Result<()> {
        let mut template = serde_json::json!({
            "type": "buttons",
            "title": title,
            "text": text,
            "actions": actions.into_iter().map(|action| action.to_json()).collect::<Vec<_>>()
        });
        if let Some(url) = thumbnail_image_url {
            template["thumbnailImageUrl"] = serde_json::json!(url);
        }
        let messages = serde_json::json!([{
            "type": "template",
            "altText": alt_text,
            "template": template
        }]);
        self.send_reply(reply_token, messages).await
    }

    /// Reply with image
    pub async fn reply_image(&self,
                             reply_token: &str,
                             original_content_url: &str,
                             preview_image_url: &str) -> anyhow::Result<()> {
        let messages = serde_json::json!([{
            "type": "image",
            "originalContentUrl": original_content_url,
            "previewImageUrl": preview_image_url
        }]);
        self.send_reply(reply_token, messages).await
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
