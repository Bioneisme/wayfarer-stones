//! The client half: the screen of this window, while the server's last list is open.

use crate::{rules, server::line};
use mod_api::{
    ClientHost, Flow, IPayloadContext, IPayloadHandler, Identifier, PayloadBytes, PayloadHandleError, PayloadId,
    ScreenRenderPostEvent, ScreenView,
};
use std::sync::{Mutex, MutexGuard};

static UI: Mutex<Option<rules::Ui>> = Mutex::new(None);

fn ui() -> MutexGuard<'static, Option<rules::Ui>> {
    UI.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Every mod's own screen is `mod`: this one is ours when the open mod screen is this mod's.
fn ours(host: &dyn ClientHost, screen: &ScreenView) -> bool {
    screen.id == "mod" && host.mod_screen() == Some(rules::ID)
}

pub(crate) fn flow(consumed: bool) -> Flow {
    if consumed { Flow::Cancel } else { Flow::Continue }
}

/// The server's list: opens the screen, or refreshes the one that is open.
pub(crate) struct FromServer;

impl IPayloadHandler for FromServer {
    fn handle(&self, payload: PayloadBytes, context: &mut dyn IPayloadContext) -> Result<(), PayloadHandleError> {
        let Some(message) = rules::decode_list(&payload.0) else {
            return Err(PayloadHandleError::Failed("an unreadable list from the server".into()));
        };
        let host = context.client_host().map_err(|e| PayloadHandleError::Failed(format!("{e:?}")))?;
        if message.open {
            if host.mod_screen_open(rules::ID) {
                let ready_at = now_ms() + message.cooldown_ms;
                let mut open = rules::Ui::new(message);
                open.ready_at = ready_at;
                *ui() = Some(open);
            } else {
                log::warn!("[wayfarer_stones] the screen can't open over the one that is open");
            }
        } else if host.mod_screen() == Some(rules::ID)
            && let Some(open) = ui().as_mut()
        {
            open.ready_at = now_ms() + message.cooldown_ms;
            open.refresh(message);
        }
        Ok(())
    }
}

/// Each client tick: what the screen's read-only hooks asked for.
pub(crate) fn tick(host: &mut dyn ClientHost) {
    let actions = match ui().as_mut() {
        Some(open) if !open.outbox.is_empty() => std::mem::take(&mut open.outbox),
        _ => return,
    };
    for action in actions {
        match action {
            rules::UiAction::Send(act) => {
                let data = rules::encode_act(&act);
                let sent = host.send_payload_to_server(&PayloadId(rules::CHANNEL_ACT.into()), PayloadBytes(data));
                if let Err(e) = sent
                    && let Some(open) = ui().as_mut()
                {
                    let e = format!("{e:?}");
                    open.notice = Some(rules::msg("wayfarer_stones.screen.send_failed", &[&e]));
                }
            }
            rules::UiAction::Close => host.mod_screen_close(rules::ID),
        }
    }
}

/// Draws the screen over the world.
pub(crate) fn render(host: &dyn ClientHost, event: &mut ScreenRenderPostEvent<'_>) {
    if !ours(host, event.screen) {
        return;
    }
    let caret = now_ms() / 500 % 2 == 0;
    let draws = {
        let mut guard = ui();
        let Some(open) = guard.as_mut() else {
            return;
        };
        let draw: &dyn mod_api::DrawContext = &*event.draw;
        open.window = draw.size();
        open.cooldown = rules::seconds_left(open.ready_at, now_ms());
        open.render(event.screen.mouse, caret, &|text| draw.text_width(text), &|m| {
            host.text_string(&line(m)).unwrap_or_else(|_| m.key.clone())
        })
    };
    let draw = &mut *event.draw;
    for d in draws {
        match d {
            rules::Draw::Fill(rect, argb) => draw.fill(rect, argb),
            rules::Draw::Sprite(name, src, dst) => {
                draw.sprite(name, src, dst, None);
            }
            // The rules' text colours are opaque RGB.
            rules::Draw::Text(text, at, rgb) => draw.text(&text, at, 0xFF00_0000 | rgb, false),
            rules::Draw::Item(item, at, size) => {
                if let Some(key) = Identifier::parse(item).ok().and_then(|id| host.item_key(&id)) {
                    draw.item(key, at, size);
                }
            }
        }
    }
}

pub(crate) fn mouse_pressed(host: &dyn ClientHost, screen: &ScreenView, button: i32) -> bool {
    let consumed = ours(host, screen);
    if consumed && let Some(open) = ui().as_mut() {
        open.click(screen.mouse, button);
    }
    consumed
}

pub(crate) fn mouse_scrolled(host: &dyn ClientHost, screen: &ScreenView, scroll: [f64; 2]) -> bool {
    let consumed = ours(host, screen);
    if consumed && let Some(open) = ui().as_mut() {
        open.wheel(scroll[1]);
    }
    consumed
}

pub(crate) fn key_pressed(host: &dyn ClientHost, screen: &ScreenView, key: &str) -> bool {
    ours(host, screen) && ui().as_mut().is_some_and(|open| open.key(key))
}

pub(crate) fn char_typed(host: &dyn ClientHost, screen: &ScreenView, c: char) -> bool {
    let consumed = ours(host, screen);
    if consumed && let Some(open) = ui().as_mut() {
        open.typed(c);
    }
    consumed
}
