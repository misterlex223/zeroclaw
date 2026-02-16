use zeroclaw::channels::Channel;
use zeroclaw::channels::line::{LineChannel, QuickReplyItem};

#[test]
fn test_line_channel_creation() {
    let channel = LineChannel::new(
        "test_token".to_string(),
        "test_secret".to_string(),
        vec!["*".to_string()],
    );
    assert_eq!(channel.name(), "line");
}

#[test]
fn test_line_quick_reply_structure() {
    let item = QuickReplyItem {
        label: "Option A".to_string(),
        text: "a".to_string(),
    };
    assert_eq!(item.label, "Option A");
    assert_eq!(item.text, "a");
}

#[test]
fn test_line_user_allowlist() {
    let channel = LineChannel::new(
        "token".to_string(),
        "secret".to_string(),
        vec!["U123".to_string(), "U456".to_string()],
    );
    assert!(channel.is_user_allowed("U123"));
    assert!(channel.is_user_allowed("U456"));
    assert!(!channel.is_user_allowed("U789"));
}

#[test]
fn test_line_wildcard_user() {
    let channel = LineChannel::new(
        "token".to_string(),
        "secret".to_string(),
        vec!["*".to_string()],
    );
    assert!(channel.is_user_allowed("U123"));
    assert!(channel.is_user_allowed("any_user"));
}

#[test]
fn test_line_empty_allowlist() {
    let channel = LineChannel::new(
        "token".to_string(),
        "secret".to_string(),
        vec![],
    );
    assert!(!channel.is_user_allowed("U123"));
}
