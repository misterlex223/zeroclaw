use super::traits::Channel;
use async_trait::async_trait;
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

/// LINE API rate limit information from response headers
#[derive(Debug, Clone)]
pub struct LineRateLimitInfo {
    /// API call limit per unit (e.g., 1000)
    pub limit: u64,
    /// Remaining calls allowed
    pub remaining: u64,
    /// Epoch time when limit resets
    pub reset_at: u64,
}

/// LINE API error information
#[derive(Debug, Clone)]
pub struct LineApiError {
    /// HTTP status code
    pub status: u16,
    /// LINE error code (e.g., "INVALID_TOKEN")
    pub code: Option<String>,
    /// Human-readable error message
    pub message: String,
    /// Whether this error is retryable
    pub retryable: bool,
    /// Suggested retry delay (if provided by API)
    pub retry_after: Option<Duration>,
}

impl std::fmt::Display for LineApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LINE API error ({}): ", self.status)?;
        if let Some(ref code) = self.code {
            write!(f, "[{}] ", code)?;
        }
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for LineApiError {}

/// Retry configuration for LINE API calls
#[derive(Debug, Clone)]
pub struct LineRetryConfig {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Initial retry delay
    pub initial_delay: Duration,
    /// Maximum retry delay
    pub max_delay: Duration,
    /// Exponential backoff multiplier
    pub backoff_multiplier: f64,
}

impl Default for LineRetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(10),
            backoff_multiplier: 2.0,
        }
    }
}

/// LINE channel — receives messages via webhook, sends via Messaging API
pub struct LineChannel {
    channel_access_token: String,
    channel_secret: String,
    allowed_users: Vec<String>,
    client: reqwest::Client,
    /// Retry configuration
    retry_config: LineRetryConfig,
    /// Rate limit tracking (remaining calls)
    rate_limit_remaining: AtomicU64,
    /// Rate limit reset time (epoch seconds)
    rate_limit_reset_at: AtomicU64,
}

impl LineChannel {
    pub fn new(
        channel_access_token: String,
        channel_secret: String,
        allowed_users: Vec<String>,
    ) -> Self {
        Self {
            channel_access_token,
            channel_secret,
            allowed_users,
            client: reqwest::Client::new(),
            retry_config: LineRetryConfig::default(),
            rate_limit_remaining: AtomicU64::new(u64::MAX),
            rate_limit_reset_at: AtomicU64::new(0),
        }
    }

    /// Create a new LineChannel with custom retry configuration
    pub fn with_retry_config(
        channel_access_token: String,
        channel_secret: String,
        allowed_users: Vec<String>,
        retry_config: LineRetryConfig,
    ) -> Self {
        Self {
            channel_access_token,
            channel_secret,
            allowed_users,
            client: reqwest::Client::new(),
            retry_config,
            rate_limit_remaining: AtomicU64::new(u64::MAX),
            rate_limit_reset_at: AtomicU64::new(0),
        }
    }

    /// Get the current retry configuration
    pub fn retry_config(&self) -> &LineRetryConfig {
        &self.retry_config
    }

    /// Update the retry configuration
    pub fn set_retry_config(&mut self, config: LineRetryConfig) {
        self.retry_config = config;
    }

    /// Get rate limit information (if known from recent API calls)
    pub fn rate_limit_info(&self) -> Option<LineRateLimitInfo> {
        let reset_at = self.rate_limit_reset_at.load(Ordering::Relaxed);
        let remaining = self.rate_limit_remaining.load(Ordering::Relaxed);
        if reset_at > 0 {
            Some(LineRateLimitInfo {
                limit: 1000, // LINE's typical limit
                remaining,
                reset_at,
            })
        } else {
            None
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
        decoded_sig.len() == expected.len() && decoded_sig.ct_eq(&expected).into()
    }

    /// Check if a LINE user ID is in the allowlist
    pub fn is_user_allowed(&self, user_id: &str) -> bool {
        self.allowed_users.iter().any(|u| u == "*" || u == user_id)
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // Error Handling & Retry Logic
    // ─────────────────────────────────────────────────────────────────────────────

    /// Update rate limit info from response headers
    fn update_rate_limit_from_headers(&self, headers: &reqwest::header::HeaderMap) {
        if let Some(remaining) = headers
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
        {
            self.rate_limit_remaining
                .store(remaining, Ordering::Relaxed);
        }
        if let Some(reset) = headers
            .get("x-ratelimit-reset")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
        {
            self.rate_limit_reset_at.store(reset, Ordering::Relaxed);
        }
    }

    /// Parse LINE API error response
    fn parse_api_error(&self, status: reqwest::StatusCode, body: &str) -> LineApiError {
        let status_code = status.as_u16();

        // Try to parse LINE's error format
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
            let message = json
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error")
                .to_string();
            let code = json
                .get("error")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            let retryable = match status_code {
                429 => true,       // Rate limit
                500..=599 => true, // Server errors
                408 => true,       // Request timeout
                _ => false,
            };

            // Note: retry-after header should be extracted from response headers
            // before calling this function. We don't have headers here.
            let retry_after = None;

            return LineApiError {
                status: status_code,
                code,
                message,
                retryable,
                retry_after,
            };
        }

        // Fallback for non-JSON responses
        let retryable = matches!(status_code, 429 | 408 | 500..=599);
        LineApiError {
            status: status_code,
            code: None,
            message: body.to_string(),
            retryable,
            retry_after: None,
        }
    }

    /// Check if we should retry based on the error
    fn should_retry(&self, error: &LineApiError, attempt: u32) -> bool {
        if attempt >= self.retry_config.max_retries {
            return false;
        }
        error.retryable
    }

    /// Calculate retry delay with exponential backoff
    fn calculate_retry_delay(&self, attempt: u32, error: &LineApiError) -> Duration {
        // Use API-provided retry-after if available
        if let Some(delay) = error.retry_after {
            return delay;
        }

        // Exponential backoff
        let base_delay = self.retry_config.initial_delay.as_millis() as f64;
        let exponential_delay =
            base_delay * self.retry_config.backoff_multiplier.powi(attempt as i32);
        let delay_ms = exponential_delay as u64;

        // Cap at max delay
        let max_delay_ms = self.retry_config.max_delay.as_millis() as u64;
        Duration::from_millis(delay_ms.min(max_delay_ms))
    }

    /// Execute an HTTP request with retry logic
    async fn send_http_with_retry(
        &self,
        url: &str,
        body: serde_json::Value,
    ) -> anyhow::Result<reqwest::Response> {
        let mut attempt = 0;

        loop {
            attempt += 1;

            // Check rate limit before making request
            let remaining = self.rate_limit_remaining.load(Ordering::Relaxed);
            let reset_at = self.rate_limit_reset_at.load(Ordering::Relaxed);
            if remaining < 10 && reset_at > 0 {
                let now = chrono::Utc::now().timestamp() as u64;
                if reset_at > now {
                    let wait_secs = reset_at - now;
                    tracing::warn!("LINE API rate limit approached, waiting {}s", wait_secs);
                    tokio::time::sleep(Duration::from_secs(wait_secs)).await;
                }
            }

            // Build and send request
            match self
                .client
                .post(url)
                .bearer_auth(&self.channel_access_token)
                .json(&body)
                .send()
                .await
            {
                Ok(resp) => {
                    let status = resp.status();

                    // Update rate limit info
                    self.update_rate_limit_from_headers(resp.headers());

                    if status.is_success() {
                        return Ok(resp);
                    }

                    // Handle error
                    let error_body = resp.text().await.unwrap_or_default();
                    let error = self.parse_api_error(status, &error_body);

                    if self.should_retry(&error, attempt) {
                        let delay = self.calculate_retry_delay(attempt, &error);
                        tracing::warn!(
                            "LINE API request failed (attempt {}/{}): {}, retrying in {:?}",
                            attempt,
                            self.retry_config.max_retries,
                            error,
                            delay
                        );
                        tokio::time::sleep(delay).await;
                        continue;
                    }

                    return Err(anyhow::anyhow!(error));
                }
                Err(e) => {
                    // Network or other error
                    if attempt < self.retry_config.max_retries {
                        let delay = self.calculate_retry_delay(
                            attempt,
                            &LineApiError {
                                status: 0,
                                code: None,
                                message: e.to_string(),
                                retryable: true,
                                retry_after: None,
                            },
                        );
                        tracing::warn!(
                            "LINE API network error (attempt {}/{}): {}, retrying in {:?}",
                            attempt,
                            self.retry_config.max_retries,
                            e,
                            delay
                        );
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    return Err(e.into());
                }
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // Core Send Methods (with retry)
    // ─────────────────────────────────────────────────────────────────────────────

    /// Send reply message to LINE (with retry)
    async fn send_reply(
        &self,
        reply_token: &str,
        messages: serde_json::Value,
    ) -> anyhow::Result<()> {
        let body = serde_json::json!({
            "replyToken": reply_token,
            "messages": messages
        });

        let resp = self
            .send_http_with_retry("https://api.line.me/v2/bot/message/reply", body)
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_body = resp.text().await.unwrap_or_default();
            let error = self.parse_api_error(status, &error_body);
            return Err(anyhow::anyhow!(error));
        }

        Ok(())
    }

    /// Send push message to LINE (with retry)
    async fn send_push(&self, to: &str, messages: serde_json::Value) -> anyhow::Result<()> {
        let body = serde_json::json!({
            "to": to,
            "messages": messages
        });

        let resp = self
            .send_http_with_retry("https://api.line.me/v2/bot/message/push", body)
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let error_body = resp.text().await.unwrap_or_default();
            let error = self.parse_api_error(status, &error_body);
            return Err(anyhow::anyhow!(error));
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
    Postback {
        label: String,
        data: String,
        text: Option<String>,
    },
    /// URI action - opens a URL
    Uri {
        label: String,
        uri: String,
        alt_uri: Option<String>,
    },
    /// Date picker action - sends date value
    DatePicker {
        label: String,
        data: String,
        initial: Option<String>,
        max: Option<String>,
        min: Option<String>,
    },
    /// Time picker action - sends time value
    TimePicker {
        label: String,
        data: String,
        initial: Option<String>,
    },
    /// Datetime picker action - sends datetime value
    DateTimePicker {
        label: String,
        data: String,
        initial: Option<String>,
        max: Option<String>,
        min: Option<String>,
    },
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
            QuickReplyAction::Uri {
                label,
                uri,
                alt_uri,
            } => {
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
            QuickReplyAction::DatePicker {
                label,
                data,
                initial,
                max,
                min,
            } => {
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
            QuickReplyAction::TimePicker {
                label,
                data,
                initial,
            } => {
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
            QuickReplyAction::DateTimePicker {
                label,
                data,
                initial,
                max,
                min,
            } => {
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
    Message {
        label: String,
        text: String,
    },
    Postback {
        label: String,
        data: String,
        text: Option<String>,
    },
    Uri {
        label: String,
        uri: String,
        alt_uri: Option<String>,
    },
    DatetimePicker {
        label: String,
        data: String,
        mode: String,
        initial: Option<String>,
        max: Option<String>,
        min: Option<String>,
    },
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
            TemplateAction::Uri {
                label,
                uri,
                alt_uri,
            } => {
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
            TemplateAction::DatetimePicker {
                label,
                data,
                mode,
                initial,
                max,
                min,
            } => {
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
    pub async fn send_flex(
        &self,
        to: &str,
        alt_text: &str,
        contents: &serde_json::Value,
    ) -> anyhow::Result<()> {
        let messages = serde_json::json!([{
            "type": "flex",
            "altText": alt_text,
            "contents": contents
        }]);
        self.send_push(to, messages).await
    }

    /// Send flex bubble message using the flex builder types
    pub async fn send_flex_bubble(
        &self,
        to: &str,
        alt_text: &str,
        bubble: &flex::FlexBubble,
    ) -> anyhow::Result<()> {
        self.send_flex(to, alt_text, &bubble.to_json()).await
    }

    /// Send flex carousel message using the flex builder types
    pub async fn send_flex_carousel(
        &self,
        to: &str,
        alt_text: &str,
        carousel: &flex::FlexCarousel,
    ) -> anyhow::Result<()> {
        self.send_flex(to, alt_text, &carousel.to_json()).await
    }

    /// Reply with flex bubble message
    pub async fn reply_flex_bubble(
        &self,
        reply_token: &str,
        alt_text: &str,
        bubble: &flex::FlexBubble,
    ) -> anyhow::Result<()> {
        let messages = serde_json::json!([{
            "type": "flex",
            "altText": alt_text,
            "contents": bubble.to_json()
        }]);
        self.send_reply(reply_token, messages).await
    }

    /// Send message with quick reply buttons (new version with all action types)
    pub async fn send_with_quick_reply_actions(
        &self,
        to: &str,
        text: &str,
        actions: Vec<QuickReplyAction>,
    ) -> anyhow::Result<()> {
        let quick_reply_items: Vec<serde_json::Value> =
            actions.into_iter().map(|action| action.to_json()).collect();

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
    pub async fn send_with_quick_reply(
        &self,
        to: &str,
        text: &str,
        items: Vec<QuickReplyItem>,
    ) -> anyhow::Result<()> {
        let quick_reply_items: Vec<serde_json::Value> = items
            .into_iter()
            .map(|item| {
                serde_json::json!({
                    "type": "action",
                    "action": {
                        "type": "message",
                        "label": item.label,
                        "text": item.text
                    }
                })
            })
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
    pub async fn send_buttons_template(
        &self,
        to: &str,
        alt_text: &str,
        title: &str,
        text: &str,
        thumbnail_image_url: Option<&str>,
        actions: Vec<TemplateAction>,
    ) -> anyhow::Result<()> {
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
    pub async fn send_confirm_template(
        &self,
        to: &str,
        alt_text: &str,
        text: &str,
        ok_action: TemplateAction,
        cancel_action: TemplateAction,
    ) -> anyhow::Result<()> {
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
    pub async fn send_carousel_template(
        &self,
        to: &str,
        alt_text: &str,
        columns: Vec<TemplateColumn>,
        image_aspect_ratio: Option<&str>,
    ) -> anyhow::Result<()> {
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
    pub async fn send_image_carousel_template(
        &self,
        to: &str,
        alt_text: &str,
        columns: Vec<TemplateColumn>,
    ) -> anyhow::Result<()> {
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
    pub async fn send_image(
        &self,
        to: &str,
        original_content_url: &str,
        preview_image_url: &str,
    ) -> anyhow::Result<()> {
        let messages = serde_json::json!([{
            "type": "image",
            "originalContentUrl": original_content_url,
            "previewImageUrl": preview_image_url
        }]);
        self.send_push(to, messages).await
    }

    /// Send video message with URL
    pub async fn send_video(
        &self,
        to: &str,
        original_content_url: &str,
        preview_image_url: &str,
    ) -> anyhow::Result<()> {
        let messages = serde_json::json!([{
            "type": "video",
            "originalContentUrl": original_content_url,
            "previewImageUrl": preview_image_url
        }]);
        self.send_push(to, messages).await
    }

    /// Send audio message with URL
    pub async fn send_audio(
        &self,
        to: &str,
        original_content_url: &str,
        duration: u64,
    ) -> anyhow::Result<()> {
        let messages = serde_json::json!([{
            "type": "audio",
            "originalContentUrl": original_content_url,
            "duration": duration
        }]);
        self.send_push(to, messages).await
    }

    /// Upload and send image (returns the content URL)
    pub async fn upload_image(
        &self,
        to: &str,
        image_data: Vec<u8>,
        content_type: &str,
    ) -> anyhow::Result<String> {
        let url = format!("https://api.line.me/v2/bot/message/{to}/upload");

        let resp = self
            .client
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

    /// Upload image with progress tracking callback
    pub async fn upload_image_with_progress<F>(
        &self,
        to: &str,
        image_data: Vec<u8>,
        content_type: &str,
        mut progress_callback: F,
    ) -> anyhow::Result<String>
    where
        F: FnMut(u64, u64), // (bytes_uploaded, total_bytes)
    {
        let url = format!("https://api.line.me/v2/bot/message/{to}/upload");
        let total_bytes = image_data.len() as u64;

        // For chunked upload, we need to use the client with request body
        // Note: reqwest doesn't support upload progress natively,
        // so this is a simplified version that calls back at start/end
        progress_callback(0, total_bytes);

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.channel_access_token)
            .header("Content-Type", content_type)
            .body(image_data)
            .send()
            .await?;

        progress_callback(total_bytes, total_bytes);

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

    /// Upload image with retry on failure
    pub async fn upload_image_with_retry(
        &self,
        to: &str,
        image_data: Vec<u8>,
        content_type: &str,
    ) -> anyhow::Result<String> {
        let mut attempt = 0;
        let max_attempts = self.retry_config.max_retries;

        loop {
            attempt += 1;

            match self
                .upload_image(to, image_data.clone(), content_type)
                .await
            {
                Ok(id) => return Ok(id),
                Err(e) => {
                    if attempt >= max_attempts {
                        return Err(e);
                    }

                    let delay = self.calculate_retry_delay(
                        attempt,
                        &LineApiError {
                            status: 500,
                            code: None,
                            message: e.to_string(),
                            retryable: true,
                            retry_after: None,
                        },
                    );

                    tracing::warn!(
                        "LINE image upload failed (attempt {}/{}), retrying in {:?}",
                        attempt,
                        max_attempts,
                        delay
                    );
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────────
    // Location & Sticker Messages
    // ─────────────────────────────────────────────────────────────────────────────

    /// Send location message
    pub async fn send_location(
        &self,
        to: &str,
        title: &str,
        address: &str,
        latitude: f64,
        longitude: f64,
    ) -> anyhow::Result<()> {
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
    pub async fn send_sticker(
        &self,
        to: &str,
        package_id: &str,
        sticker_id: &str,
    ) -> anyhow::Result<()> {
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
    pub async fn reply_with_quick_reply_actions(
        &self,
        reply_token: &str,
        text: &str,
        actions: Vec<QuickReplyAction>,
    ) -> anyhow::Result<()> {
        let quick_reply_items: Vec<serde_json::Value> =
            actions.into_iter().map(|action| action.to_json()).collect();

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
    pub async fn reply_buttons_template(
        &self,
        reply_token: &str,
        alt_text: &str,
        title: &str,
        text: &str,
        thumbnail_image_url: Option<&str>,
        actions: Vec<TemplateAction>,
    ) -> anyhow::Result<()> {
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
    pub async fn reply_image(
        &self,
        reply_token: &str,
        original_content_url: &str,
        preview_image_url: &str,
    ) -> anyhow::Result<()> {
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
    use base64::{engine::general_purpose, Engine as _};
    general_purpose::URL_SAFE_NO_PAD.decode(input)
}

#[async_trait]
impl Channel for LineChannel {
    fn name(&self) -> &str {
        "line"
    }

    async fn send(&self, message: &super::traits::SendMessage) -> anyhow::Result<()> {
        // recipient is LINE User ID for push messages
        let messages = serde_json::json!([
            {
                "type": "text",
                "text": message.content
            }
        ]);
        self.send_push(&message.recipient, messages).await
    }

    async fn listen(
        &self,
        _tx: tokio::sync::mpsc::Sender<super::traits::ChannelMessage>,
    ) -> anyhow::Result<()> {
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

// =============================================================================
// Flex Message Builders
// =============================================================================

/// Flex Message component types and builders
///
/// Reference: https://developers.line.biz/en/docs/messaging-api/messages/#flex-messages
pub mod flex {
    use super::*;

    // ─────────────────────────────────────────────────────────────────────────
    // Component Types
    // ─────────────────────────────────────────────────────────────────────────

    /// Base trait for Flex components
    pub trait FlexComponent {
        fn component_type(&self) -> &'static str;
        fn to_json(&self) -> serde_json::Value;
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Box Component
    // ─────────────────────────────────────────────────────────────────────────

    /// Box container for components
    #[derive(Debug, Clone)]
    pub struct FlexBox {
        pub layout: FlexBoxLayout,
        pub contents: Vec<FlexComponentData>,
        pub flex: Option<i32>,
        pub spacing: Option<String>,
        pub margin: Option<String>,
        pub padding_all: Option<String>,
        pub padding_top: Option<String>,
        pub padding_bottom: Option<String>,
        pub padding_start: Option<String>,
        pub padding_end: Option<String>,
        pub background_color: Option<String>,
        pub background_width: Option<String>,
        pub corner_radius: Option<String>,
        pub width: Option<String>,
        pub height: Option<String>,
        pub align_items: Option<String>,
        pub justify_content: Option<String>,
        pub position: Option<String>,
        pub offset_top: Option<String>,
        pub offset_bottom: Option<String>,
        pub offset_start: Option<String>,
        pub offset_end: Option<String>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum FlexBoxLayout {
        Horizontal,
        Vertical,
        Baseline,
    }

    impl FlexBox {
        pub fn new(layout: FlexBoxLayout) -> Self {
            Self {
                layout,
                contents: Vec::new(),
                flex: None,
                spacing: None,
                margin: None,
                padding_all: None,
                padding_top: None,
                padding_bottom: None,
                padding_start: None,
                padding_end: None,
                background_color: None,
                background_width: None,
                corner_radius: None,
                width: None,
                height: None,
                align_items: None,
                justify_content: None,
                position: None,
                offset_top: None,
                offset_bottom: None,
                offset_start: None,
                offset_end: None,
            }
        }

        pub fn horizontal() -> Self {
            Self::new(FlexBoxLayout::Horizontal)
        }

        pub fn vertical() -> Self {
            Self::new(FlexBoxLayout::Vertical)
        }

        pub fn add_component(mut self, component: FlexComponentData) -> Self {
            self.contents.push(component);
            self
        }

        pub fn contents(mut self, contents: Vec<FlexComponentData>) -> Self {
            self.contents = contents;
            self
        }

        pub fn flex(mut self, flex: i32) -> Self {
            self.flex = Some(flex);
            self
        }

        pub fn spacing(mut self, spacing: &str) -> Self {
            self.spacing = Some(spacing.to_string());
            self
        }

        pub fn margin(mut self, margin: &str) -> Self {
            self.margin = Some(margin.to_string());
            self
        }

        pub fn padding(mut self, padding: &str) -> Self {
            self.padding_all = Some(padding.to_string());
            self
        }

        pub fn background(mut self, color: &str) -> Self {
            self.background_color = Some(color.to_string());
            self
        }

        pub fn corner_radius(mut self, radius: &str) -> Self {
            self.corner_radius = Some(radius.to_string());
            self
        }

        pub fn to_json(&self) -> serde_json::Value {
            FlexComponentData::Box(self.clone()).to_json()
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Text Component
    // ─────────────────────────────────────────────────────────────────────────

    /// Text component
    #[derive(Debug, Clone)]
    pub struct FlexText {
        pub text: String,
        pub size: Option<String>,
        pub align: Option<String>,
        pub gravity: Option<String>,
        pub wrap: Option<bool>,
        pub max_lines: Option<u32>,
        pub weight: Option<String>,
        pub color: Option<String>,
        pub margin: Option<String>,
        pub position: Option<String>,
        pub offset_top: Option<String>,
        pub offset_bottom: Option<String>,
        pub offset_start: Option<String>,
        pub offset_end: Option<String>,
        pub line_spacing: Option<String>,
        pub decoration: Option<String>,
        pub style: Option<FlexTextStyle>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum FlexTextStyle {
        Normal,
        Bold,
        Italic,
    }

    impl FlexText {
        pub fn new(text: impl Into<String>) -> Self {
            Self {
                text: text.into(),
                size: None,
                align: None,
                gravity: None,
                wrap: None,
                max_lines: None,
                weight: None,
                color: None,
                margin: None,
                position: None,
                offset_top: None,
                offset_bottom: None,
                offset_start: None,
                offset_end: None,
                line_spacing: None,
                decoration: None,
                style: None,
            }
        }

        pub fn size(mut self, size: &str) -> Self {
            self.size = Some(size.to_string());
            self
        }

        pub fn align(mut self, align: &str) -> Self {
            self.align = Some(align.to_string());
            self
        }

        pub fn color(mut self, color: &str) -> Self {
            self.color = Some(color.to_string());
            self
        }

        pub fn bold(mut self) -> Self {
            self.weight = Some("bold".to_string());
            self
        }

        pub fn wrap(mut self) -> Self {
            self.wrap = Some(true);
            self
        }

        pub fn max_lines(mut self, lines: u32) -> Self {
            self.max_lines = Some(lines);
            self
        }

        pub fn margin(mut self, margin: &str) -> Self {
            self.margin = Some(margin.to_string());
            self
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Image Component
    // ─────────────────────────────────────────────────────────────────────────

    /// Image component
    #[derive(Debug, Clone)]
    pub struct FlexImage {
        pub url: String,
        pub flex: Option<i32>,
        pub margin: Option<String>,
        pub align_items: Option<String>,
        pub gravity: Option<String>,
        pub aspect_ratio: Option<String>,
        pub aspect_mode: Option<String>,
        pub size: Option<String>,
        pub position: Option<String>,
        pub offset_top: Option<String>,
        pub offset_bottom: Option<String>,
        pub offset_start: Option<String>,
        pub offset_end: Option<String>,
    }

    impl FlexImage {
        pub fn new(url: impl Into<String>) -> Self {
            Self {
                url: url.into(),
                flex: None,
                margin: None,
                align_items: None,
                gravity: None,
                aspect_ratio: None,
                aspect_mode: None,
                size: None,
                position: None,
                offset_top: None,
                offset_bottom: None,
                offset_start: None,
                offset_end: None,
            }
        }

        pub fn flex(mut self, flex: i32) -> Self {
            self.flex = Some(flex);
            self
        }

        pub fn margin(mut self, margin: &str) -> Self {
            self.margin = Some(margin.to_string());
            self
        }

        pub fn aspect_ratio(mut self, ratio: &str) -> Self {
            self.aspect_ratio = Some(ratio.to_string());
            self
        }

        pub fn size(mut self, size: &str) -> Self {
            self.size = Some(size.to_string());
            self
        }

        pub fn to_json(&self) -> serde_json::Value {
            FlexComponentData::Image(self.clone()).to_json()
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Button Component
    // ─────────────────────────────────────────────────────────────────────────

    /// Button component
    #[derive(Debug, Clone)]
    pub struct FlexButton {
        pub action: TemplateAction,
        pub style: Option<FlexButtonStyle>,
        pub flex: Option<i32>,
        pub margin: Option<String>,
        pub height: Option<String>,
        pub position: Option<String>,
        pub offset_top: Option<String>,
        pub offset_bottom: Option<String>,
        pub offset_start: Option<String>,
        pub offset_end: Option<String>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum FlexButtonStyle {
        Link,
        Primary,
        Secondary,
    }

    impl FlexButton {
        pub fn new(action: TemplateAction) -> Self {
            Self {
                action,
                style: None,
                flex: None,
                margin: None,
                height: None,
                position: None,
                offset_top: None,
                offset_bottom: None,
                offset_start: None,
                offset_end: None,
            }
        }

        pub fn style(mut self, style: FlexButtonStyle) -> Self {
            self.style = Some(style);
            self
        }

        pub fn margin(mut self, margin: &str) -> Self {
            self.margin = Some(margin.to_string());
            self
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Icon Component
    // ─────────────────────────────────────────────────────────────────────────

    /// Icon component
    #[derive(Debug, Clone)]
    pub struct FlexIcon {
        pub url: String,
        pub margin: Option<String>,
        pub size: Option<String>,
        pub aspect_ratio: Option<String>,
        pub position: Option<String>,
    }

    impl FlexIcon {
        pub fn new(url: impl Into<String>) -> Self {
            Self {
                url: url.into(),
                margin: None,
                size: None,
                aspect_ratio: None,
                position: None,
            }
        }

        pub fn size(mut self, size: &str) -> Self {
            self.size = Some(size.to_string());
            self
        }

        pub fn margin(mut self, margin: &str) -> Self {
            self.margin = Some(margin.to_string());
            self
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Separator & Spacer
    // ─────────────────────────────────────────────────────────────────────────

    /// Separator component
    #[derive(Debug, Clone)]
    pub struct FlexSeparator {
        pub margin: Option<String>,
        pub color: Option<String>,
    }

    impl FlexSeparator {
        pub fn new() -> Self {
            Self {
                margin: None,
                color: None,
            }
        }

        pub fn margin(mut self, margin: &str) -> Self {
            self.margin = Some(margin.to_string());
            self
        }

        pub fn color(mut self, color: &str) -> Self {
            self.color = Some(color.to_string());
            self
        }
    }

    /// Spacer component
    #[derive(Debug, Clone)]
    pub struct FlexSpacer {
        pub size: String,
    }

    impl FlexSpacer {
        pub fn new(size: &str) -> Self {
            Self {
                size: size.to_string(),
            }
        }
    }

    /// Filler component (takes up available space)
    #[derive(Debug, Clone)]
    pub struct FlexFiller;

    // ─────────────────────────────────────────────────────────────────────────
    // Component Data (enum for all component types)
    // ─────────────────────────────────────────────────────────────────────────

    /// Enum holding any Flex component type
    #[derive(Debug, Clone)]
    pub enum FlexComponentData {
        Box(FlexBox),
        Text(FlexText),
        Image(FlexImage),
        Button(FlexButton),
        Icon(FlexIcon),
        Separator(FlexSeparator),
        Spacer(FlexSpacer),
        Filler(FlexFiller),
    }

    impl FlexComponentData {
        pub fn to_json(&self) -> serde_json::Value {
            match self {
                FlexComponentData::Box(b) => {
                    let mut json = serde_json::json!({
                        "type": "box",
                        "layout": match b.layout {
                            FlexBoxLayout::Horizontal => "horizontal",
                            FlexBoxLayout::Vertical => "vertical",
                            FlexBoxLayout::Baseline => "baseline",
                        },
                        "contents": b.contents.iter().map(|c| c.to_json()).collect::<Vec<_>>()
                    });
                    if let Some(f) = b.flex {
                        json["flex"] = serde_json::json!(f);
                    }
                    if let Some(s) = &b.spacing {
                        json["spacing"] = serde_json::json!(s);
                    }
                    if let Some(m) = &b.margin {
                        json["margin"] = serde_json::json!(m);
                    }
                    if let Some(p) = &b.padding_all {
                        json["paddingAll"] = serde_json::json!(p);
                    }
                    if let Some(c) = &b.background_color {
                        json["backgroundColor"] = serde_json::json!(c);
                    }
                    if let Some(r) = &b.corner_radius {
                        json["cornerRadius"] = serde_json::json!(r);
                    }
                    if let Some(w) = &b.width {
                        json["width"] = serde_json::json!(w);
                    }
                    if let Some(h) = &b.height {
                        json["height"] = serde_json::json!(h);
                    }
                    json
                }
                FlexComponentData::Text(t) => {
                    let mut json = serde_json::json!({
                        "type": "text",
                        "text": t.text
                    });
                    if let Some(s) = &t.size {
                        json["size"] = serde_json::json!(s);
                    }
                    if let Some(a) = &t.align {
                        json["align"] = serde_json::json!(a);
                    }
                    if let Some(c) = &t.color {
                        json["color"] = serde_json::json!(c);
                    }
                    if let Some(w) = &t.weight {
                        json["weight"] = serde_json::json!(w);
                    }
                    if let Some(m) = &t.margin {
                        json["margin"] = serde_json::json!(m);
                    }
                    if let Some(w) = t.wrap {
                        json["wrap"] = serde_json::json!(w);
                    }
                    if let Some(m) = t.max_lines {
                        json["maxLines"] = serde_json::json!(m);
                    }
                    json
                }
                FlexComponentData::Image(i) => {
                    let mut json = serde_json::json!({
                        "type": "image",
                        "url": i.url
                    });
                    if let Some(f) = i.flex {
                        json["flex"] = serde_json::json!(f);
                    }
                    if let Some(m) = &i.margin {
                        json["margin"] = serde_json::json!(m);
                    }
                    if let Some(r) = &i.aspect_ratio {
                        json["aspectRatio"] = serde_json::json!(r);
                    }
                    if let Some(s) = &i.size {
                        json["size"] = serde_json::json!(s);
                    }
                    json
                }
                FlexComponentData::Button(b) => {
                    let mut json = serde_json::json!({
                        "type": "button",
                        "action": b.action.to_json()
                    });
                    if let Some(s) = b.style {
                        json["style"] = serde_json::json!(match s {
                            FlexButtonStyle::Link => "link",
                            FlexButtonStyle::Primary => "primary",
                            FlexButtonStyle::Secondary => "secondary",
                        });
                    }
                    if let Some(m) = &b.margin {
                        json["margin"] = serde_json::json!(m);
                    }
                    json
                }
                FlexComponentData::Icon(i) => {
                    let mut json = serde_json::json!({
                        "type": "icon",
                        "url": i.url
                    });
                    if let Some(s) = &i.size {
                        json["size"] = serde_json::json!(s);
                    }
                    if let Some(m) = &i.margin {
                        json["margin"] = serde_json::json!(m);
                    }
                    json
                }
                FlexComponentData::Separator(s) => {
                    let mut json = serde_json::json!({"type": "separator"});
                    if let Some(m) = &s.margin {
                        json["margin"] = serde_json::json!(m);
                    }
                    if let Some(c) = &s.color {
                        json["color"] = serde_json::json!(c);
                    }
                    json
                }
                FlexComponentData::Spacer(s) => serde_json::json!({
                    "type": "spacer",
                    "size": s.size
                }),
                FlexComponentData::Filler(_) => serde_json::json!({"type": "filler"}),
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Container Types
    // ─────────────────────────────────────────────────────────────────────────

    /// Bubble container for Flex messages
    #[derive(Debug, Clone)]
    pub struct FlexBubble {
        pub header: Option<FlexBox>,
        pub hero: Option<FlexImage>,
        pub body: FlexBox,
        pub footer: Option<FlexBox>,
        pub styles: Option<FlexBubbleStyles>,
        pub direction: Option<FlexDirection>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum FlexDirection {
        LeftToRight,
        RightToLeft,
    }

    #[derive(Debug, Clone)]
    pub struct FlexBubbleStyles {
        pub header: Option<FlexBlockStyle>,
        pub hero: Option<FlexBlockStyle>,
        pub body: Option<FlexBlockStyle>,
        pub footer: Option<FlexBlockStyle>,
    }

    #[derive(Debug, Clone)]
    pub struct FlexBlockStyle {
        pub background_color: Option<String>,
        pub separator: Option<bool>,
        pub separator_color: Option<String>,
    }

    impl FlexBubble {
        pub fn new(body: FlexBox) -> Self {
            Self {
                header: None,
                hero: None,
                body,
                footer: None,
                styles: None,
                direction: None,
            }
        }

        pub fn header(mut self, header: FlexBox) -> Self {
            self.header = Some(header);
            self
        }

        pub fn hero(mut self, hero: FlexImage) -> Self {
            self.hero = Some(hero);
            self
        }

        pub fn footer(mut self, footer: FlexBox) -> Self {
            self.footer = Some(footer);
            self
        }

        pub fn direction(mut self, direction: FlexDirection) -> Self {
            self.direction = Some(direction);
            self
        }

        pub fn to_json(&self) -> serde_json::Value {
            let mut json = serde_json::json!({
                "type": "bubble",
                "body": self.body.to_json()
            });

            if let Some(header) = &self.header {
                json["header"] = header.to_json();
            }
            if let Some(hero) = &self.hero {
                json["hero"] = hero.to_json();
            }
            if let Some(footer) = &self.footer {
                json["footer"] = footer.to_json();
            }
            if let Some(direction) = self.direction {
                json["direction"] = serde_json::json!(match direction {
                    FlexDirection::LeftToRight => "ltr",
                    FlexDirection::RightToLeft => "rtl",
                });
            }

            json
        }
    }

    /// Carousel container for Flex messages
    #[derive(Debug, Clone)]
    pub struct FlexCarousel {
        pub contents: Vec<FlexBubble>,
    }

    impl FlexCarousel {
        pub fn new() -> Self {
            Self {
                contents: Vec::new(),
            }
        }

        pub fn add_bubble(mut self, bubble: FlexBubble) -> Self {
            self.contents.push(bubble);
            self
        }

        pub fn contents(mut self, contents: Vec<FlexBubble>) -> Self {
            self.contents = contents;
            self
        }

        pub fn to_json(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "carousel",
                "contents": self.contents.iter().map(|b| b.to_json()).collect::<Vec<_>>()
            })
        }
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

    // ─────────────────────────────────────────────────────────────────────────
    // Retry Configuration Tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn line_retry_config_default() {
        let config = LineRetryConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.initial_delay, Duration::from_millis(500));
        assert_eq!(config.max_delay, Duration::from_secs(10));
        assert_eq!(config.backoff_multiplier, 2.0);
    }

    #[test]
    fn line_channel_with_custom_retry() {
        let retry_config = LineRetryConfig {
            max_retries: 5,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 3.0,
        };
        let ch = LineChannel::with_retry_config(
            "token".into(),
            "secret".into(),
            vec![],
            retry_config.clone(),
        );
        assert_eq!(ch.retry_config().max_retries, 5);
        assert_eq!(ch.retry_config().initial_delay, Duration::from_millis(100));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Quick Reply Action Tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn line_quick_reply_action_message() {
        let action = QuickReplyAction::Message {
            label: "OK".into(),
            text: "ok".into(),
        };
        let json = action.to_json();
        assert_eq!(json["type"], "action");
        assert_eq!(json["action"]["type"], "message");
        assert_eq!(json["action"]["label"], "OK");
        assert_eq!(json["action"]["text"], "ok");
    }

    #[test]
    fn line_quick_reply_action_postback() {
        let action = QuickReplyAction::Postback {
            label: "Select".into(),
            data: "value=123".into(),
            text: Some("Selected".into()),
        };
        let json = action.to_json();
        assert_eq!(json["action"]["type"], "postback");
        assert_eq!(json["action"]["data"], "value=123");
        assert_eq!(json["action"]["text"], "Selected");
    }

    #[test]
    fn line_quick_reply_action_uri() {
        let action = QuickReplyAction::Uri {
            label: "Open".into(),
            uri: "https://example.com".into(),
            alt_uri: None,
        };
        let json = action.to_json();
        assert_eq!(json["action"]["type"], "uri");
        assert_eq!(json["action"]["uri"], "https://example.com");
    }

    #[test]
    fn line_quick_reply_action_date_picker() {
        let action = QuickReplyAction::DatePicker {
            label: "Pick Date".into(),
            data: "date".into(),
            initial: Some("2023-01-01".into()),
            max: Some("2023-12-31".into()),
            min: Some("2023-01-01".into()),
        };
        let json = action.to_json();
        assert_eq!(json["action"]["type"], "datepicker");
        assert_eq!(json["action"]["initial"], "2023-01-01");
        assert_eq!(json["action"]["max"], "2023-12-31");
        assert_eq!(json["action"]["min"], "2023-01-01");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Template Action Tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn line_template_action_message() {
        let action = TemplateAction::Message {
            label: "Click".into(),
            text: "clicked".into(),
        };
        let json = action.to_json();
        assert_eq!(json["type"], "message");
        assert_eq!(json["label"], "Click");
        assert_eq!(json["text"], "clicked");
    }

    #[test]
    fn line_template_action_postback() {
        let action = TemplateAction::Postback {
            label: "Submit".into(),
            data: "data=value".into(),
            text: Some("Submitted".into()),
        };
        let json = action.to_json();
        assert_eq!(json["type"], "postback");
        assert_eq!(json["data"], "data=value");
        assert_eq!(json["text"], "Submitted");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // LineApiError Tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn line_api_error_display() {
        let error = LineApiError {
            status: 429,
            code: Some("RATE_LIMIT".into()),
            message: "Too many requests".into(),
            retryable: true,
            retry_after: Some(Duration::from_secs(60)),
        };
        let display = format!("{}", error);
        assert!(display.contains("429"));
        assert!(display.contains("RATE_LIMIT"));
        assert!(display.contains("Too many requests"));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Rate Limit Info Tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn line_rate_limit_info() {
        let info = LineRateLimitInfo {
            limit: 1000,
            remaining: 500,
            reset_at: 1234567890,
        };
        assert_eq!(info.limit, 1000);
        assert_eq!(info.remaining, 500);
        assert_eq!(info.reset_at, 1234567890);
    }
}
