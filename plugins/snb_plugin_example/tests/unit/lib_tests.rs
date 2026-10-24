use super::*;
use snb_core::plugin::Version;
use std::ffi::CStr;

#[test]
fn test_plugin_ffi() {
    let ptr = create_plugin();
    let cell = unsafe { snb_core::plugin::PluginCell::new(ptr, destroy_plugin, Box::new(())) };

    assert_eq!(cell.name(), "MyPlugin");
    assert_eq!(
        cell.version(),
        Version {
            major: 0,
            minor: 1,
            patch: 0,
        }
    );

    let abi = unsafe { CStr::from_ptr(plugin_abi()).to_str().unwrap() };
    assert_eq!(abi, snb_core::plugin::snb_plugin_abi().to_string());
}

#[test]
fn detects_html_rich_text() {
    assert_eq!(detect_rich_text_format("hello <b>world</b>"), Some(TextFormat::Html));
    assert_eq!(detect_rich_text_format("<code>1 + 1</code>"), Some(TextFormat::Html));
    assert_eq!(detect_rich_text_format("<blockquote>a quote</blockquote>"), Some(TextFormat::Html));
    assert_eq!(detect_rich_text_format("<tg-spoiler>hidden</tg-spoiler>"), Some(TextFormat::Html));
    assert_eq!(detect_rich_text_format("<a href=\"https://t.me\">link</a>"), Some(TextFormat::Html));
}

#[test]
fn detects_markdown_rich_text() {
    assert_eq!(detect_rich_text_format("hello *world*"), Some(TextFormat::MarkdownV2));
    assert_eq!(detect_rich_text_format("`code block`"), Some(TextFormat::MarkdownV2));
    assert_eq!(detect_rich_text_format("```rust\nfn main() {}\n```"), Some(TextFormat::MarkdownV2));
    assert_eq!(detect_rich_text_format("||spoiler||"), Some(TextFormat::MarkdownV2));
    assert_eq!(detect_rich_text_format("> a quote line"), Some(TextFormat::MarkdownV2));
    assert_eq!(detect_rich_text_format("[Telegram](https://telegram.org)"), Some(TextFormat::MarkdownV2));
}

#[test]
fn plain_text_returns_none() {
    assert_eq!(detect_rich_text_format("just plain text"), None);
    assert_eq!(detect_rich_text_format("numbers 123 and symbols !?"), None);
}
