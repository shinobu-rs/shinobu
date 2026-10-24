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
        ("hello <i>world</i>!".to_string(), Some(TextFormat::Html))
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
        convert_rich_text("> line 1\n> line 2"),
        ("<blockquote>line 1\nline 2</blockquote>".to_string(), Some(TextFormat::Html))
    );
    assert_eq!(
        convert_rich_text("### 标题测试"),
        ("<h3>标题测试</h3>".to_string(), Some(TextFormat::Html))
    );
    assert_eq!(
        convert_rich_text("| 标题1 | 标题2 |\n|---|---|\n| A | B |"),
        (
            "<table><tr><th>标题1</th><th>标题2</th></tr><tr><td>A</td><td>B</td></tr></table>".to_string(),
            Some(TextFormat::Html)
        )
    );
}

#[test]
fn converts_full_miuix_readme() {
    let readme = r#"## Miuix

A UI library for Compose Multiplatform.

> This library is experimental. APIs may change without notice.

[![Kotlin](https://img.shields.io/badge/kotlin-2.4.21-7F52FF)](https://kotlinlang.org/)

### Supported Platforms

![Android](https://img.shields.io/badge/Android-3DDC84?logo=android&logoColor=white)

### Modules

|Module|Description|
|---|---|
|`miuix-ui`|Core UI component library|
|`miuix-preference`|Preference components library, depends on `miuix-ui`|

### Getting Started

```diff
kotlin {
    sourceSets {
        commonMain.dependencies {
            implementation("top.yukonga.miuix.kmp:miuix-ui:<version>")
        }
    }
}
```

### Screenshots

<table>
  <tr>
    <td><a href="https://example.com/001.webp"><img src="https://example.com/001.webp" width="300" alt="001"/></a></td>
  </tr>
</table>
"#;
    let (html, fmt) = convert_rich_text(readme);
    assert_eq!(fmt, Some(TextFormat::Html));
    assert!(html.contains("<h2>Miuix</h2>"));
    assert!(html.contains("<blockquote>"));
    assert!(html.contains("<h3>Supported Platforms</h3>"));
    assert!(html.contains("<table><tr><th>Module</th><th>Description</th></tr>"));
    assert!(html.contains("<pre><code class=\"language-diff\">"));
    assert!(html.contains("implementation"));
    assert!(html.contains("https://example.com/001.webp"));
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
