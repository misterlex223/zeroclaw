//! LINE webhook event types and parsing
//!
//! This module provides types for parsing LINE webhook events,
//! including message, postback, follow/unfollow, and member events.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// LINE webhook request body
#[derive(Debug, Deserialize)]
pub struct LineWebhook {
    pub destination: String,
    pub events: Vec<WebhookEvent>,
}

/// A webhook event from LINE
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
    #[serde(default)]
    pub beacon: Option<WebhookBeacon>,
    #[serde(default)]
    pub joined: Option<WebhookMember>,
    #[serde(default)]
    pub left: Option<WebhookMember>,
    #[serde(default)]
    pub link: Option<WebhookAccountLink>,
    /// Webhook event ID (for deduplication)
    #[serde(default)]
    pub webhook_event_id: Option<String>,
}

/// Event type from LINE webhook
#[derive(Debug, Deserialize, PartialEq, Clone, Copy)]
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

impl WebhookEventType {
    /// Returns true if this event type should trigger a bot response
    pub fn should_respond(&self) -> bool {
        matches!(
            self,
            Self::Message | Self::Postback | Self::Follow | Self::MemberJoined
        )
    }

    /// Returns true if this event type has a reply token
    pub fn has_reply_token(&self) -> bool {
        matches!(
            self,
            Self::Message | Self::Postback | Self::Follow | Self::MemberJoined | Self::Beacon
        )
    }
}

/// Source of the webhook event
#[derive(Debug, Deserialize, Clone)]
pub struct WebhookSource {
    #[serde(rename = "type")]
    pub source_type: String,
    #[serde(rename = "userId")]
    pub user_id: String,
    #[serde(rename = "groupId")]
    #[serde(default)]
    pub group_id: Option<String>,
    #[serde(rename = "roomId")]
    #[serde(default)]
    pub room_id: Option<String>,
}

impl WebhookSource {
    /// Returns the source identifier (user_id, group_id, or room_id)
    pub fn source_id(&self) -> &str {
        if let Some(gid) = &self.group_id {
            gid
        } else if let Some(rid) = &self.room_id {
            rid
        } else {
            &self.user_id
        }
    }

    /// Returns true if this is a group chat event
    pub fn is_group(&self) -> bool {
        self.group_id.is_some()
    }

    /// Returns true if this is a room chat event
    pub fn is_room(&self) -> bool {
        self.room_id.is_some()
    }

    /// Returns true if this is a direct message (1-on-1)
    pub fn is_direct(&self) -> bool {
        self.group_id.is_none() && self.room_id.is_none()
    }
}

/// Message data from webhook
#[derive(Debug, Deserialize)]
pub struct WebhookMessage {
    #[serde(rename = "type")]
    pub message_type: String,
    pub id: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub file_id: Option<String>,
    #[serde(default)]
    pub content_provider: Option<ContentProvider>,
}

/// Content provider for media messages
#[derive(Debug, Deserialize)]
pub struct ContentProvider {
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub original_content_url: Option<String>,
    #[serde(default)]
    pub preview_image_url: Option<String>,
}

/// Postback data from webhook
#[derive(Debug, Deserialize, Clone)]
pub struct WebhookPostback {
    pub data: String,
    #[serde(default)]
    pub params: Option<PostbackParams>,
}

impl WebhookPostback {
    /// Parses the postback data as key=value pairs
    pub fn parse_data(&self) -> HashMap<String, String> {
        self.data
            .split('&')
            .filter_map(|pair| {
                let mut parts = pair.splitn(2, '=');
                Some((parts.next()?.to_string(), parts.next()?.to_string()))
            })
            .collect()
    }

    /// Gets a specific parameter from the postback data
    pub fn get_param(&self, key: &str) -> Option<String> {
        self.parse_data().get(key).cloned()
    }
}

/// Postback parameters from date/time pickers
#[derive(Debug, Deserialize, Clone)]
pub struct PostbackParams {
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub time: Option<String>,
    #[serde(default)]
    pub datetime: Option<String>,
}

impl PostbackParams {
    /// Returns the date/time value depending on picker type
    pub fn value(&self) -> Option<&str> {
        self.date
            .as_deref()
            .or(self.time.as_deref())
            .or(self.datetime.as_deref())
    }
}

/// Beacon event data
#[derive(Debug, Deserialize, Clone)]
pub struct WebhookBeacon {
    pub hwid: String,
    pub r#type: BeaconType,
    #[serde(default)]
    pub device_message: Option<String>,
}

/// Beacon event type
#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum BeaconType {
    Enter,
    Leave,
    Ban,
    #[serde(other)]
    Unknown,
}

/// Member event data (joined/left)
#[derive(Debug, Deserialize, Clone)]
pub struct WebhookMember {
    pub members: Vec<Member>,
}

/// Member information
#[derive(Debug, Deserialize, Clone)]
pub struct Member {
    #[serde(rename = "userId")]
    pub user_id: String,
}

/// Account link event data
#[derive(Debug, Deserialize, Clone)]
pub struct WebhookAccountLink {
    pub result: AccountLinkResult,
    pub nonce: String,
    #[serde(default)]
    pub user_id: Option<String>,
}

/// Account link result
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum AccountLinkResult {
    Ok,
    Failed,
    #[serde(other)]
    Unknown,
}

/// Parsed event with extracted content
#[derive(Debug, Clone)]
pub enum ParsedEvent {
    Message {
        source: WebhookSource,
        reply_token: String,
        text: String,
        message_id: String,
    },
    Postback {
        source: WebhookSource,
        reply_token: String,
        data: String,
        params: Option<PostbackParams>,
    },
    Follow {
        source: WebhookSource,
        reply_token: String,
    },
    Unfollow {
        source: WebhookSource,
    },
    MemberJoined {
        source: WebhookSource,
        reply_token: String,
        members: Vec<Member>,
    },
    MemberLeft {
        source: WebhookSource,
        members: Vec<Member>,
    },
    Beacon {
        source: WebhookSource,
        reply_token: Option<String>,
        hwid: String,
        beacon_type: BeaconType,
    },
    Unknown {
        source: WebhookSource,
        event_type: String,
    },
}

impl WebhookEvent {
    /// Parse the event into a more convenient enum
    pub fn parse(&self) -> ParsedEvent {
        match self.event_type {
            WebhookEventType::Message => {
                if let Some(ref msg) = self.message {
                    ParsedEvent::Message {
                        source: self.source.clone(),
                        reply_token: self.reply_token.clone().unwrap_or_default(),
                        text: msg.text.clone().unwrap_or_default(),
                        message_id: msg.id.clone(),
                    }
                } else {
                    ParsedEvent::Unknown {
                        source: self.source.clone(),
                        event_type: "message_no_data".to_string(),
                    }
                }
            }
            WebhookEventType::Postback => {
                if let Some(ref pb) = self.postback {
                    ParsedEvent::Postback {
                        source: self.source.clone(),
                        reply_token: self.reply_token.clone().unwrap_or_default(),
                        data: pb.data.clone(),
                        params: pb.params.clone(),
                    }
                } else {
                    ParsedEvent::Unknown {
                        source: self.source.clone(),
                        event_type: "postback_no_data".to_string(),
                    }
                }
            }
            WebhookEventType::Follow => ParsedEvent::Follow {
                source: self.source.clone(),
                reply_token: self.reply_token.clone().unwrap_or_default(),
            },
            WebhookEventType::Unfollow => ParsedEvent::Unfollow {
                source: self.source.clone(),
            },
            WebhookEventType::MemberJoined => {
                if let Some(ref joined) = self.joined {
                    ParsedEvent::MemberJoined {
                        source: self.source.clone(),
                        reply_token: self.reply_token.clone().unwrap_or_default(),
                        members: joined.members.clone(),
                    }
                } else {
                    ParsedEvent::Unknown {
                        source: self.source.clone(),
                        event_type: "member_joined_no_data".to_string(),
                    }
                }
            }
            WebhookEventType::MemberLeft => {
                if let Some(ref left) = self.left {
                    ParsedEvent::MemberLeft {
                        source: self.source.clone(),
                        members: left.members.clone(),
                    }
                } else {
                    ParsedEvent::Unknown {
                        source: self.source.clone(),
                        event_type: "member_left_no_data".to_string(),
                    }
                }
            }
            WebhookEventType::Beacon => {
                if let Some(ref beacon) = self.beacon {
                    ParsedEvent::Beacon {
                        source: self.source.clone(),
                        reply_token: self.reply_token.clone(),
                        hwid: beacon.hwid.clone(),
                        beacon_type: beacon.r#type,
                    }
                } else {
                    ParsedEvent::Unknown {
                        source: self.source.clone(),
                        event_type: "beacon_no_data".to_string(),
                    }
                }
            }
            _ => ParsedEvent::Unknown {
                source: self.source.clone(),
                event_type: format!("{:?}", self.event_type),
            },
        }
    }
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
        assert_eq!(
            webhook.events[0].message.as_ref().unwrap().text,
            Some("Hello".to_string())
        );
    }

    #[test]
    fn line_message_serialization() {
        let msg = LineMessage::text("test");
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        assert!(json.contains("\"text\":\"test\""));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Postback Event Tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn webhook_parse_postback_event() {
        let json = r#"{
            "destination": "U123",
            "events": [{
                "type": "postback",
                "mode": "active",
                "timestamp": 1234567890,
                "source": {
                    "type": "user",
                    "userId": "Uabc123"
                },
                "postback": {
                    "data": "action=buy&item=123"
                },
                "replyToken": "reply_token"
            }]
        }"#;

        let webhook: LineWebhook = serde_json::from_str(json).unwrap();
        assert_eq!(webhook.events[0].event_type, WebhookEventType::Postback);
        assert_eq!(
            webhook.events[0].postback.as_ref().unwrap().data,
            "action=buy&item=123"
        );
    }

    #[test]
    fn webhook_postback_parse_data() {
        let postback = WebhookPostback {
            data: "action=buy&item=123&qty=2".into(),
            params: None,
        };
        let parsed = postback.parse_data();
        assert_eq!(parsed.get("action"), Some(&"buy".to_string()));
        assert_eq!(parsed.get("item"), Some(&"123".to_string()));
        assert_eq!(parsed.get("qty"), Some(&"2".to_string()));
    }

    #[test]
    fn webhook_postback_get_param() {
        let postback = WebhookPostback {
            data: "key=value&foo=bar".into(),
            params: None,
        };
        assert_eq!(postback.get_param("key"), Some("value".to_string()));
        assert_eq!(postback.get_param("foo"), Some("bar".to_string()));
        assert_eq!(postback.get_param("missing"), None);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Follow/Unfollow Event Tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn webhook_parse_follow_event() {
        let json = r#"{
            "destination": "U123",
            "events": [{
                "type": "follow",
                "mode": "active",
                "timestamp": 1234567890,
                "source": {
                    "type": "user",
                    "userId": "Uabc123"
                },
                "replyToken": "reply_token"
            }]
        }"#;

        let webhook: LineWebhook = serde_json::from_str(json).unwrap();
        assert_eq!(webhook.events[0].event_type, WebhookEventType::Follow);
    }

    #[test]
    fn webhook_parse_unfollow_event() {
        let json = r#"{
            "destination": "U123",
            "events": [{
                "type": "unfollow",
                "mode": "active",
                "timestamp": 1234567890,
                "source": {
                    "type": "user",
                    "userId": "Uabc123"
                }
            }]
        }"#;

        let webhook: LineWebhook = serde_json::from_str(json).unwrap();
        assert_eq!(webhook.events[0].event_type, WebhookEventType::Unfollow);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // WebhookSource Tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn webhook_source_user() {
        let source = WebhookSource {
            source_type: "user".into(),
            user_id: "U123".into(),
            group_id: None,
            room_id: None,
        };
        assert_eq!(source.source_id(), "U123");
        assert!(source.is_direct());
        assert!(!source.is_group());
        assert!(!source.is_room());
    }

    #[test]
    fn webhook_source_group() {
        let source = WebhookSource {
            source_type: "group".into(),
            user_id: "U123".into(),
            group_id: Some("G456".into()),
            room_id: None,
        };
        assert_eq!(source.source_id(), "G456");
        assert!(!source.is_direct());
        assert!(source.is_group());
        assert!(!source.is_room());
    }

    #[test]
    fn webhook_source_room() {
        let source = WebhookSource {
            source_type: "room".into(),
            user_id: "U123".into(),
            group_id: None,
            room_id: Some("R789".into()),
        };
        assert_eq!(source.source_id(), "R789");
        assert!(!source.is_direct());
        assert!(!source.is_group());
        assert!(source.is_room());
    }

    // ─────────────────────────────────────────────────────────────────────────
    // ParsedEvent Tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn webhook_parse_message_to_parsed_event() {
        let json = r#"{
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
                "text": "Hello World"
            },
            "replyToken": "reply_token"
        }"#;

        let event: WebhookEvent = serde_json::from_str(json).unwrap();
        let parsed = event.parse();

        match parsed {
            ParsedEvent::Message { source, text, .. } => {
                assert_eq!(source.user_id, "Uabc123");
                assert_eq!(text, "Hello World");
            }
            _ => panic!("Expected Message event"),
        }
    }

    #[test]
    fn webhook_parse_postback_to_parsed_event() {
        let json = r#"{
            "type": "postback",
            "mode": "active",
            "timestamp": 1234567890,
            "source": {
                "type": "user",
                "userId": "Uabc123"
            },
            "postback": {
                "data": "data=test"
            },
            "replyToken": "reply_token"
        }"#;

        let event: WebhookEvent = serde_json::from_str(json).unwrap();
        let parsed = event.parse();

        match parsed {
            ParsedEvent::Postback { data, .. } => {
                assert_eq!(data, "data=test");
            }
            _ => panic!("Expected Postback event"),
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Event Type Helper Tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn webhook_event_type_should_respond() {
        assert!(WebhookEventType::Message.should_respond());
        assert!(WebhookEventType::Postback.should_respond());
        assert!(WebhookEventType::Follow.should_respond());
        assert!(WebhookEventType::MemberJoined.should_respond());
        assert!(!WebhookEventType::Unfollow.should_respond());
        assert!(!WebhookEventType::Beacon.should_respond());
    }

    #[test]
    fn webhook_event_type_has_reply_token() {
        assert!(WebhookEventType::Message.has_reply_token());
        assert!(WebhookEventType::Postback.has_reply_token());
        assert!(WebhookEventType::Follow.has_reply_token());
        assert!(!WebhookEventType::Unfollow.has_reply_token());
    }
}
