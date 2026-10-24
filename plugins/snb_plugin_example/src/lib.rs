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
use snb_core::event::Event;
use snb_core::hook::HookType;
use snb_core::session::{SessionKey, SessionMessage, SessionState};
use snb_macros::{adapter, command, hook, message_handler, plugin};

/// Build a reply event addressed back to `src`'s sender, mirroring its chat/reply_to.
fn reply(src: &Event, text: impl Into<String>) -> Event {
    let mut resp = Event::message("MyPlugin", text);
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

    // Has args → echo directly as-is.
    log::info!(
        "[command] /echo args={:?} source={} sender={:?} chat={:?}",
        ctx.args,
        ctx.event.source,
        msg.and_then(|m| m.sender_id()),
        msg.map(|m| m.chat_id()),
    );
    bot.emit_event(reply(ctx.event, ctx.args));
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
                // Echo the user's message as-is and exit session mode.
                snb_core::context::bot().emit_event(reply(event, &text));
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
