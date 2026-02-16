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
