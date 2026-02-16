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
    pub zh_cn: LarkPostContentZhCn,
}

#[derive(Debug, Serialize)]
pub struct LarkPostContentZhCn {
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
