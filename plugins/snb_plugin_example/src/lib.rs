//! Example plugin demonstrating Shinobu's plugin capabilities.
//!
//! Registers sample commands (`/echo`, `/ping`), hooks, a message handler,
//! session-based multi-turn echo, and a demo async adapter — all declared with
//! the `#[command]` / `#[hook]` / `#[message_handler]` / `#[adapter]` macros and
//! auto-registered via [`register_all`](snb_core::context::register_all).

use std::sync::Arc;
use std::time::Duration;

use snb_core::command::CommandContext;
use snb_core::context::BotContext;
use snb_core::event::{ContentItem, Event, TextFormat};
use snb_core::hook::HookType;
use snb_core::session::{SessionKey, SessionMessage, SessionState};
use snb_macros::{adapter, command, hook, message_handler, plugin};

/// Build a reply event addressed back to `src`'s sender with optional text format.
fn reply_formatted(src: &Event, text: impl Into<String>, format: Option<TextFormat>) -> Event {
    let text = text.into();
    let mut resp = match format {
        Some(fmt) => Event::formatted_message("MyPlugin", text, fmt),
        None => Event::message("MyPlugin", text),
    };
    if let Some(m) = &src.message {
        let out = resp.message.as_mut().unwrap();
        out.chat = m.chat.clone();
        out.reply_to = m.id.clone();
    }
    if let Some(plugin) = &src.reply_plugin {
        resp.target_plugin = Some(plugin.clone());
    }
    resp
}

/// Build a plain text reply event addressed back to `src`'s sender.
fn reply(src: &Event, text: impl Into<String>) -> Event {
    reply_formatted(src, text, None)
}

fn is_html_markup(text: &str) -> bool {
    const HTML_TAGS: &[&str] = &[
        "<b>", "</b>", "<strong>", "</strong>",
        "<i>", "</i>", "<em>", "</em>",
        "<u>", "</u>", "<ins>", "</ins>",
        "<s>", "</s>", "<strike>", "</strike>", "<del>", "</del>",
        "<code>", "</code>",
        "<pre>", "</pre>",
        "<blockquote>", "</blockquote>",
        "<tg-spoiler>", "</tg-spoiler>",
        "<tg-emoji", "</tg-emoji>",
        "<a href=", "</a>",
    ];
    HTML_TAGS.iter().any(|tag| text.contains(tag))
}

fn is_markdown_markup(text: &str) -> bool {
    if text.contains("```") || (text.contains('`') && text.chars().filter(|&c| c == '`').count() >= 2) {
        return true;
    }
    if text.contains("||") && text.matches("||").count() >= 2 {
        return true;
    }
    if text.lines().any(|l| l.trim_start().starts_with('>') || l.trim_start().starts_with("**>")) {
        return true;
    }
    if let (Some(bracket), Some(paren)) = (text.find('['), text.find("](")) {
        if paren > bracket && text[paren..].contains(')') {
            return true;
        }
    }
    for delimiter in ['*', '_', '~'] {
        let count = text.chars().filter(|&c| c == delimiter).count();
        if count >= 2 {
            return true;
        }
    }
    false
}

fn escape_html_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

fn escape_attr_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

fn replace_delimited(text: &str, delim: &str, open: &str, close: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(delim) {
        out.push_str(&rest[..start]);
        let after_start = &rest[start + delim.len()..];
        if let Some(end) = after_start.find(delim) {
            let inner = &after_start[..end];
            if !inner.is_empty() && !inner.starts_with(' ') && !inner.ends_with(' ') {
                out.push_str(open);
                out.push_str(inner);
                out.push_str(close);
                rest = &after_start[end + delim.len()..];
            } else {
                out.push_str(delim);
                rest = after_start;
            }
        } else {
            out.push_str(delim);
            rest = after_start;
        }
    }
    out.push_str(rest);
    out
}

fn replace_links(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(bracket_open) = rest.find('[') {
        out.push_str(&rest[..bracket_open]);
        let after_bracket = &rest[bracket_open + 1..];
        if let Some(bracket_close) = after_bracket.find("](") {
            let link_text = &after_bracket[..bracket_close];
            let after_paren = &after_bracket[bracket_close + 2..];
            if let Some(paren_close) = after_paren.find(')') {
                let url = &after_paren[..paren_close];
                if !url.is_empty()
                    && (url.starts_with("http://")
                        || url.starts_with("https://")
                        || url.starts_with("tg://"))
                {
                    out.push_str(&format!(
                        "<a href=\"{}\">{}</a>",
                        escape_attr_str(url),
                        link_text
                    ));
                    rest = &after_paren[paren_close + 1..];
                    continue;
                }
            }
        }
        out.push('[');
        rest = after_bracket;
    }
    out.push_str(rest);
    out
}

fn replace_blockquotes(text: &str) -> String {
    let mut lines = Vec::new();
    let mut in_quote = false;
    let mut is_expandable = false;
    let mut quote_lines = Vec::new();

    let flush_quote = |lines: &mut Vec<String>, quote_lines: &mut Vec<String>, expandable: bool| {
        if quote_lines.is_empty() {
            return;
        }
        let open_tag = if expandable { "<blockquote expandable>" } else { "<blockquote>" };
        lines.push(format!("{open_tag}{}</blockquote>", quote_lines.join("\n")));
        quote_lines.clear();
    };

    for line in text.lines() {
        let trimmed = line.trim_start();
        let (stripped, expandable) = if let Some(s) = trimmed.strip_prefix("**&gt; ").or_else(|| trimmed.strip_prefix("**> ")) {
            (Some(s), true)
        } else if let Some(s) = trimmed.strip_prefix("&gt; ").or_else(|| trimmed.strip_prefix("> ")) {
            (Some(s), false)
        } else if let Some(s) = trimmed.strip_prefix("&gt;").or_else(|| trimmed.strip_prefix(">")) {
            (Some(s), false)
        } else if let Some(s) = trimmed.strip_prefix("＞ ").or_else(|| trimmed.strip_prefix("＞")).or_else(|| trimmed.strip_prefix("》 ")) {
            (Some(s), false)
        } else {
            (None, false)
        };

        if let Some(s) = stripped {
            if in_quote && is_expandable != expandable {
                flush_quote(&mut lines, &mut quote_lines, is_expandable);
            }
            in_quote = true;
            is_expandable = expandable;
            quote_lines.push(s.to_string());
            continue;
        }

        // Check if there is an inline quote marker mid-line: " &gt; ", " > ", " **&gt; ", " **> "
        let (inline_pos, gt_len, inline_expandable) = if let Some(pos) = line.find(" **&gt; ").or_else(|| line.find(" **> ")) {
            let len = if line[pos..].starts_with(" **&gt; ") { 8 } else { 5 };
            (Some(pos), len, true)
        } else if let Some(pos) = line.find(" &gt; ").or_else(|| line.find(" > ")) {
            let len = if line[pos..].starts_with(" &gt; ") { 6 } else { 3 };
            (Some(pos), len, false)
        } else if let Some(pos) = line.find(" ＞ ").or_else(|| line.find(" ＞")).or_else(|| line.find(" 》 ")) {
            let len = if line[pos..].starts_with(" ＞ ") || line[pos..].starts_with(" 》 ") { 4 } else { 3 };
            (Some(pos), len, false)
        } else {
            (None, 0, false)
        };

        if let Some(pos) = inline_pos {
            flush_quote(&mut lines, &mut quote_lines, is_expandable);
            in_quote = false;

            let before = &line[..pos];
            let quote_part = &line[pos + gt_len..];
            let open_tag = if inline_expandable { "<blockquote expandable>" } else { "<blockquote>" };
            lines.push(format!("{before}\n{open_tag}{quote_part}</blockquote>"));
            continue;
        }

        flush_quote(&mut lines, &mut quote_lines, is_expandable);
        in_quote = false;
        lines.push(line.to_string());
    }

    flush_quote(&mut lines, &mut quote_lines, is_expandable);
    lines.join("\n")
}

fn is_valid_telegram_html_tag(tag: &str) -> bool {
    let lower = tag.to_ascii_lowercase();
    const VALID_EXACT: &[&str] = &[
        "<b>", "</b>", "<strong>", "</strong>",
        "<i>", "</i>", "<em>", "</em>",
        "<u>", "</u>", "<ins>", "</ins>",
        "<s>", "</s>", "<strike>", "</strike>", "<del>", "</del>",
        "<code>", "</code>",
        "<pre>", "</pre>",
        "<blockquote>", "</blockquote>", "<blockquote expandable>",
        "<tg-spoiler>", "</tg-spoiler>",
        "</tg-emoji>", "</a>",
    ];
    if VALID_EXACT.contains(&lower.as_str()) {
        return true;
    }
    if (lower.starts_with("<a href=") && lower.ends_with('>'))
        || (lower.starts_with("<tg-emoji") && lower.ends_with('>'))
        || (lower.starts_with("<pre") && lower.ends_with('>'))
        || (lower.starts_with("<code") && lower.ends_with('>'))
        || (lower.starts_with("<blockquote") && lower.ends_with('>'))
    {
        return true;
    }
    false
}

fn process_markdown_delimiters(text: &str) -> String {
    let mut out = text.to_string();
    out = replace_links(&out);
    out = replace_delimited(&out, "||", "<tg-spoiler>", "</tg-spoiler>");
    out = replace_delimited(&out, "**", "<b>", "</b>");
    out = replace_delimited(&out, "__", "<u>", "</u>");
    out = replace_delimited(&out, "~~", "<s>", "</s>");
    out = replace_delimited(&out, "*", "<b>", "</b>");
    out = replace_delimited(&out, "_", "<i>", "</i>");
    out = replace_delimited(&out, "~", "<s>", "</s>");
    out
}

fn convert_markdown_segments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let parts: Vec<&str> = input.split('`').collect();

    for (i, part) in parts.iter().enumerate() {
        if i % 2 == 1 {
            out.push_str(&format!("<code>{}</code>", escape_html_str(part)));
        } else {
            out.push_str(&process_markdown_delimiters(part));
        }
    }
    out
}

/// Detect and convert rich text format (HTML or Markdown) to HTML.
pub(crate) fn convert_rich_text(input: &str) -> (String, Option<TextFormat>) {
    let has_html = is_html_markup(input);
    let has_md = is_markdown_markup(input);

    if !has_html && !has_md {
        return (input.to_string(), None);
    }

    let mut tokens = Vec::new();
    let mut i = 0;
    let mut text_start = 0;
    let bytes = input.as_bytes();

    while i < bytes.len() {
        if bytes[i] == b'<' {
            if let Some(close_pos) = input[i..].find('>') {
                let candidate = &input[i..=i + close_pos];
                if is_valid_telegram_html_tag(candidate) {
                    if i > text_start {
                        tokens.push((&input[text_start..i], false));
                    }
                    tokens.push((candidate, true));
                    i += close_pos + 1;
                    text_start = i;
                    continue;
                }
            }
        }
        i += 1;
    }
    if text_start < input.len() {
        tokens.push((&input[text_start..], false));
    }

    let mut assembled = String::with_capacity(input.len() + 32);
    let mut in_code = false;

    for (val, is_tag) in tokens {
        if is_tag {
            let lower = val.to_ascii_lowercase();
            if lower.starts_with("<pre") || lower.starts_with("<code") {
                in_code = true;
            } else if lower.starts_with("</pre") || lower.starts_with("</code") {
                in_code = false;
            }
            assembled.push_str(val);
        } else if in_code {
            assembled.push_str(val);
        } else {
            assembled.push_str(&convert_markdown_segments(val));
        }
    }

    let final_text = replace_blockquotes(&assembled);
    (final_text, Some(TextFormat::Html))
}

// -- Commands ----------------------------------------------------------------

#[command(name = "echo", aliases = ["say"], description = "Echo back your message")]
fn echo(ctx: &CommandContext) -> anyhow::Result<()> {
    let bot = snb_core::context::bot();
    let msg = ctx.event.message.as_ref();

    // No args → enter session "echo" mode, wait for next message.
    if ctx.args.is_empty() {
        if let (Some(chat_id), Some(user_id)) = (
            msg.map(|m| m.chat_id()),
            msg.and_then(|m| m.sender_id()),
        ) {
            let key = SessionKey::private(chat_id, user_id);
            let sm = bot.get_session_manager();
            sm.append_message(&key, SessionMessage::system("echo"));
            sm.set_state(&key, SessionState::WaitingForInput);
        }
        bot.emit_event(reply(ctx.event, "Send me a message to echo."));
        return Ok(());
    }

    // Has args → convert rich text (markdown/HTML) and echo.
    let (reply_text, format) = convert_rich_text(ctx.args);
    let format = format.or_else(|| {
        msg.and_then(|m| {
            m.content.iter().find_map(|c| match c {
                ContentItem::Text { format, .. } => *format,
                _ => None,
            })
        })
    });

    log::info!(
        "[command] /echo args={:?} format={:?} source={} sender={:?} chat={:?}",
        ctx.args,
        format,
        ctx.event.source,
        msg.and_then(|m| m.sender_id()),
        msg.map(|m| m.chat_id()),
    );
    bot.emit_event(reply_formatted(ctx.event, reply_text, format));
    Ok(())
}

#[command(name = "ping", description = "Reply with pong")]
fn ping(ctx: &CommandContext) -> anyhow::Result<()> {
    log::info!("[command] /ping source={}", ctx.event.source);
    snb_core::context::bot().emit_event(reply(ctx.event, "pong!"));
    Ok(())
}

// -- Message Handlers --------------------------------------------------------

#[message_handler(name = "echo_handler")]
fn echo_handler(event: &Event) -> anyhow::Result<()> {
    let Some(msg) = &event.message else {
        return Ok(());
    };
    let text = msg.text();

    // Check if this user is in "echo" session mode.
    if let Some(user_id) = msg.sender_id() {
        let key = SessionKey::private(msg.chat_id(), user_id);
        let sm = snb_core::context::bot().get_session_manager();
        if sm.get_state(&key) == SessionState::WaitingForInput {
            let recent = sm.get_all_messages(&key);
            if recent
                .last()
                .is_some_and(|m| m.role == "system" && m.content == "echo")
            {
                // Echo the user's message preserving rich text format and attachments.
                let mut resp = Event::message("MyPlugin", "");
                let out = resp.message.as_mut().unwrap();
                out.chat = msg.chat.clone();
                out.reply_to = msg.id.clone();
                out.content = msg.content.clone();
                if let Some(plugin) = &event.reply_plugin {
                    resp.target_plugin = Some(plugin.clone());
                }
                snb_core::context::bot().emit_event(resp);
                sm.clear_session(&key);
                return Ok(());
            }
        }
    }

    log::info!(
        "[message] text={:?} sender={:?} chat={:?} at={:?} is_admin={} source={}",
        text,
        msg.sender,
        msg.chat,
        msg.at,
        msg.is_admin,
        event.source,
    );
    Ok(())
}

// -- Hooks -------------------------------------------------------------------

#[hook(name = "log_hook", kind = HookType::All)]
fn log_hook(event: &mut Event) -> anyhow::Result<()> {
    log::debug!("event: {:?} | source: {}", event.event_type, event.source);
    if let Some(msg) = event.message.as_mut()
        && msg.text() == "hook"
    {
        msg.content = vec![snb_core::event::ContentItem::text("hooked")];
    }
    Ok(())
}

/// Demonstrates [`HookType::BeforeNamedCommand`]: rewrites the args of
/// `/echo` only, before the command runs.
#[hook(name = "echo_arg_rewrite", kind = HookType::BeforeNamedCommand("echo".to_string()))]
fn echo_arg_rewrite(event: &mut Event) -> anyhow::Result<()> {
    if let Some(cmd) = event.command.as_mut()
        && cmd.args == "rewrite"
    {
        cmd.args = "rewritten by hook".to_string();
    }
    Ok(())
}

// -- Demo async adapter ------------------------------------------------------

#[adapter]
async fn demo_tick(bot: Arc<dyn BotContext>) {
    for i in 1..=3 {
        tokio::time::sleep(Duration::from_millis(1000)).await;
        bot.emit_event(Event::message("demo-tick", format!("tick {i}")));
    }
}

// -- Plugin ------------------------------------------------------------------

#[plugin(name = "MyPlugin", version = "0.1.0", kind = Plugin)]
struct MyPlugin;

#[cfg(test)]
#[path = "../tests/unit/lib_tests.rs"]
mod lib_tests;
