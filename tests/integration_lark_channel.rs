//! Integration tests for Lark Channel
use zeroclaw::channels::LarkChannel;
#[tokio::test]
async fn lark_channel_creation() {
    let channel = LarkChannel::new("test".into(), "secret".into(), None, "token".into(), vec!["*".into()]);
    assert_eq!(channel.name(), "lark");
}
#[tokio::test]
async fn lark_user_whitelist() {
    let channel = LarkChannel::new("test".into(), "secret".into(), None, "token".into(), vec!["ou_123".into()]);
    assert!(channel.is_user_allowed("ou_123"));
}
