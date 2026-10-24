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

fn preprocess_spoilers(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("||") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        if let Some(end) = after.find("||") {
            let spoiler = &after[..end];
            if !spoiler.is_empty() {
                out.push_str("<tg-spoiler>");
                out.push_str(spoiler);
                out.push_str("</tg-spoiler>");
                rest = &after[end + 2..];
                continue;
            }
        }
        out.push_str("||");
        rest = after;
    }
    out.push_str(rest);
    out
}

fn render_table_html(rows: &[(bool, Vec<String>)], out: &mut String) {
    if rows.is_empty() {
        return;
    }
    out.push_str("<table>");
    for (is_head, cells) in rows {
        out.push_str("<tr>");
        let tag = if *is_head { "th" } else { "td" };
        for cell in cells {
            out.push_str(&format!("<{tag}>{cell}</{tag}>"));
        }
        out.push_str("</tr>");
    }
    out.push_str("</table>");
}

pub(crate) fn convert_rich_text(input: &str) -> (String, Option<TextFormat>) {
    use pulldown_cmark::{Event as MdEvent, Options, Parser, Tag, TagEnd};

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let preprocessed = preprocess_spoilers(input);
    let parser = Parser::new_ext(&preprocessed, options);
    let mut out = String::with_capacity(input.len() + 64);
    let mut table_cells: Vec<String> = Vec::new();
    let mut table_rows: Vec<(bool, Vec<String>)> = Vec::new();
    let mut in_cell = false;
    let mut current_cell = String::new();
    let mut in_link = false;
    let mut has_formatting = false;

    for event in parser {
        match event {
            MdEvent::Start(tag) => match tag {
                Tag::Paragraph => {},
                Tag::Heading { level, .. } => {
                    has_formatting = true;
                    out.push_str(&format!("<h{}>", level as u8));
                }
                Tag::BlockQuote(_) => {
                    has_formatting = true;
                    out.push_str("<blockquote>");
                }
                Tag::CodeBlock(kind) => {
                    has_formatting = true;
                    match kind {
                        pulldown_cmark::CodeBlockKind::Fenced(lang) if !lang.is_empty() => {
                            let clean_lang = lang.split_whitespace().next().unwrap_or("");
                            out.push_str(&format!("<pre><code class=\"language-{}\">", escape_attr_str(clean_lang)));
                        }
                        _ => out.push_str("<pre><code>"),
                    }
                }
                Tag::List(Some(start)) => {
                    has_formatting = true;
                    out.push_str(&format!("<ol start=\"{start}\">"));
                }
                Tag::List(None) => {
                    has_formatting = true;
                    out.push_str("<ul>");
                }
                Tag::Item => {
                    has_formatting = true;
                    out.push_str("<li>");
                }
                Tag::Strong => {
                    has_formatting = true;
                    out.push_str("<b>");
                }
                Tag::Emphasis => {
                    has_formatting = true;
                    out.push_str("<i>");
                }
                Tag::Strikethrough => {
                    has_formatting = true;
                    out.push_str("<s>");
                }
                Tag::Link { dest_url, .. } => {
                    has_formatting = true;
                    in_link = true;
                    out.push_str(&format!("<a href=\"{}\">", escape_attr_str(&dest_url)));
                }
                Tag::Image { dest_url, .. } => {
                    if !in_link {
                        has_formatting = true;
                        out.push_str(&format!("<a href=\"{}\">", escape_attr_str(&dest_url)));
                    }
                }
                Tag::Table(_) => {
                    has_formatting = true;
                    table_rows.clear();
                }
                Tag::TableHead => {
                    table_cells.clear();
                }
                Tag::TableRow => {
                    table_cells.clear();
                }
                Tag::TableCell => {
                    in_cell = true;
                    current_cell.clear();
                }
                _ => {}
            },
            MdEvent::End(tag_end) => match tag_end {
                TagEnd::Paragraph => {
                    out.push_str("\n\n");
                }
                TagEnd::Heading(level) => {
                    out.push_str(&format!("</h{}>\n\n", level as u8));
                }
                TagEnd::BlockQuote(_) => {
                    while out.ends_with('\n') {
                        out.pop();
                    }
                    out.push_str("</blockquote>\n\n");
                }
                TagEnd::CodeBlock => {
                    out.push_str("</code></pre>\n\n");
                }
                TagEnd::List(true) => {
                    out.push_str("</ol>\n\n");
                }
                TagEnd::List(false) => {
                    out.push_str("</ul>\n\n");
                }
                TagEnd::Item => {
                    out.push_str("</li>\n");
                }
                TagEnd::Strong => out.push_str("</b>"),
                TagEnd::Emphasis => out.push_str("</i>"),
                TagEnd::Strikethrough => out.push_str("</s>"),
                TagEnd::Link => {
                    in_link = false;
                    out.push_str("</a>");
                }
                TagEnd::Image => {
                    if !in_link {
                        out.push_str("</a>");
                    }
                }
                TagEnd::TableCell => {
                    in_cell = false;
                    table_cells.push(current_cell.clone());
                }
                TagEnd::TableHead => {
                    table_rows.push((true, table_cells.clone()));
                }
                TagEnd::TableRow => {
                    table_rows.push((false, table_cells.clone()));
                }
                TagEnd::Table => {
                    render_table_html(&table_rows, &mut out);
                    out.push_str("\n\n");
                }
                _ => {}
            },
            MdEvent::Text(text) => {
                let escaped = escape_html_str(&text);
                if in_cell {
                    current_cell.push_str(&escaped);
                } else {
                    out.push_str(&escaped);
                }
            }
            MdEvent::Code(code) => {
                has_formatting = true;
                let escaped = format!("<code>{}</code>", escape_html_str(&code));
                if in_cell {
                    current_cell.push_str(&escaped);
                } else {
                    out.push_str(&escaped);
                }
            }
            MdEvent::Html(raw_html) | MdEvent::InlineHtml(raw_html) => {
                has_formatting = true;
                if in_cell {
                    current_cell.push_str(&raw_html);
                } else {
                    out.push_str(&raw_html);
                }
            }
            MdEvent::SoftBreak => out.push('\n'),
            MdEvent::HardBreak => out.push_str("<br/>\n"),
            MdEvent::Rule => {
                has_formatting = true;
                out.push_str("<hr/>\n\n");
            }
            MdEvent::TaskListMarker(checked) => {
                has_formatting = true;
                if checked {
                    out.push_str("☑ ");
                } else {
                    out.push_str("☐ ");
                }
            }
            _ => {}
        }
    }

    let final_text = sanitize_html_for_telegram(out.trim());
    if has_formatting {
        (final_text, Some(TextFormat::Html))
    } else {
        (input.to_string(), None)
    }
}

fn sanitize_html_for_telegram(html: &str) -> String {
    if !html.contains("<table") {
        return html.to_string();
    }

    // Replace <img ... alt="ALT"/> inside tables with 🖼️ ALT so table cells contain only inline text links
    let mut result = String::with_capacity(html.len());
    let mut rest = html;

    while let Some(start) = rest.find("<img") {
        result.push_str(&rest[..start]);
        let after_img = &rest[start..];
        if let Some(end) = after_img.find('>') {
            let img_tag = &after_img[..=end];
            let alt = extract_attribute(img_tag, "alt");
            let label = match alt {
                Some(a) if !a.is_empty() => format!("🖼️ {a}"),
                _ => "🖼️ 查看图片".to_string(),
            };
            result.push_str(&label);
            rest = &after_img[end + 1..];
        } else {
            result.push_str("<img");
            rest = &after_img[4..];
        }
    }
    result.push_str(rest);
    result
}

fn extract_attribute(tag: &str, attr: &str) -> Option<String> {
    let key = format!("{attr}=");
    let pos = tag.find(&key)?;
    let after_key = &tag[pos + key.len()..];
    let quote = after_key.chars().next()?;
    if quote == '"' || quote == '\'' {
        let val_start = 1;
        let val_end = after_key[val_start..].find(quote)?;
        Some(after_key[val_start..val_start + val_end].to_string())
    } else {
        None
    }
}

fn has_rich_features(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("<table")
        || lower.contains("<h1")
        || lower.contains("<h2")
        || lower.contains("<h3")
        || lower.contains("<h4")
        || lower.contains("<h5")
        || lower.contains("<h6")
        || lower.contains("<details")
        || lower.contains("<tg-math")
        || lower.contains("<tg-button")
        || text.lines().any(|l| {
            let t = l.trim_start();
            t.starts_with("# ")
                || t.starts_with("## ")
                || t.starts_with("### ")
                || t.starts_with("#### ")
                || t.starts_with("##### ")
                || t.starts_with("###### ")
        })
        || (text.contains('|') && (text.contains("|-") || text.contains("-|") || text.contains("|:")))
}

fn split_markdown_into_chunks(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::with_capacity(max_len);

    // Split at paragraph/block boundaries (double newline)
    let blocks = text.split("\n\n");

    for block in blocks {
        let block_len = block.len();
        if current.len() + block_len + 2 > max_len && !current.is_empty() {
            chunks.push(current.trim().to_string());
            current = String::with_capacity(max_len);
        }

        if block_len > max_len {
            if !current.is_empty() {
                chunks.push(current.trim().to_string());
                current = String::with_capacity(max_len);
            }
            for line in block.lines() {
                if current.len() + line.len() + 1 > max_len && !current.is_empty() {
                    chunks.push(current.trim().to_string());
                    current = String::with_capacity(max_len);
                }
                if !current.is_empty() {
                    current.push('\n');
                }
                current.push_str(line);
            }
            continue;
        }

        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(block);
    }

    if !current.trim().is_empty() {
        chunks.push(current.trim().to_string());
    }

    if chunks.is_empty() {
        vec![text.to_string()]
    } else {
        chunks
    }
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
    let max_len = if has_rich_features(ctx.args) {
        30000 // Telegram sendRichMessage limit is 32768
    } else {
        4000  // Telegram standard sendMessage limit is 4096
    };

    let md_chunks = split_markdown_into_chunks(ctx.args, max_len);
    for chunk in md_chunks {
        let (reply_text, format) = convert_rich_text(&chunk);
        let format = format.or_else(|| {
            msg.and_then(|m| {
                m.content.iter().find_map(|c| match c {
                    ContentItem::Text { format, .. } => *format,
                    _ => None,
                })
            })
        });

        log::info!(
            "[command] /echo chunk_len={} format={:?} source={} sender={:?} chat={:?}",
            reply_text.len(),
            format,
            ctx.event.source,
            msg.and_then(|m| m.sender_id()),
            msg.map(|m| m.chat_id()),
        );
        bot.emit_event(reply_formatted(ctx.event, reply_text, format));
    }
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
