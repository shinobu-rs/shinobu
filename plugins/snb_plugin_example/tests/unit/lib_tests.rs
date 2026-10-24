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
fn preserves_raw_html_rich_text() {
    assert_eq!(
        convert_rich_text("hello <b>world</b>"),
        ("hello <b>world</b>".to_string(), Some(TextFormat::Html))
    );
    assert_eq!(
        convert_rich_text("<code>1 + 1</code>"),
        ("<code>1 + 1</code>".to_string(), Some(TextFormat::Html))
    );
}

#[test]
fn converts_markdown_syntax_to_html() {
    assert_eq!(
        convert_rich_text("hello *world*!"),
        ("hello <b>world</b>!".to_string(), Some(TextFormat::Html))
    );
    assert_eq!(
        convert_rich_text("hello **world**!"),
        ("hello <b>world</b>!".to_string(), Some(TextFormat::Html))
    );
    assert_eq!(
        convert_rich_text("`code block`"),
        ("<code>code block</code>".to_string(), Some(TextFormat::Html))
    );
    assert_eq!(
        convert_rich_text("||spoiler||"),
        ("<tg-spoiler>spoiler</tg-spoiler>".to_string(), Some(TextFormat::Html))
    );
    assert_eq!(
        convert_rich_text("[Telegram](https://telegram.org)"),
        ("<a href=\"https://telegram.org\">Telegram</a>".to_string(), Some(TextFormat::Html))
    );
    assert_eq!(
        convert_rich_text("> quote line"),
        ("<blockquote>quote line</blockquote>".to_string(), Some(TextFormat::Html))
    );
    assert_eq!(
        convert_rich_text("*粗体* _斜体_ ~删除线~ ||剧透|| `等宽代码` > 引用文本"),
        (
            "<b>粗体</b> <i>斜体</i> <s>删除线</s> <tg-spoiler>剧透</tg-spoiler> <code>等宽代码</code>\n<blockquote>引用文本</blockquote>".to_string(),
            Some(TextFormat::Html)
        )
    );
    assert_eq!(
        convert_rich_text("> line 1\n> line 2"),
        ("<blockquote>line 1\nline 2</blockquote>".to_string(), Some(TextFormat::Html))
    );
    assert_eq!(
        convert_rich_text("**> expandable quote"),
        ("<blockquote expandable>expandable quote</blockquote>".to_string(), Some(TextFormat::Html))
    );
}

#[test]
fn plain_text_returns_none() {
    assert_eq!(
        convert_rich_text("just plain text"),
        ("just plain text".to_string(), None)
    );
    assert_eq!(
        convert_rich_text("numbers 123 and symbols !?"),
        ("numbers 123 and symbols !?".to_string(), None)
    );
}
