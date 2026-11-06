use crate::{
    App,
    data::s,
    icons,
    theme::{self, T},
};
use rune::{El, Pixels, col, el, icon, row, text};
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Default)]
pub struct Assets {
    pending: HashMap<String, u64>,
    ready: HashMap<String, (String, Pixels)>,
    failed: std::collections::HashSet<String>,
}

pub fn poll(app: &mut App) -> bool {
    let crate::Route::Thread(thread) = app.frame_route() else {
        return false;
    };
    let attachments: Vec<Value> = app
        .state()
        .thread_views
        .get(thread)
        .into_iter()
        .flat_map(|v| super::rows::derive(&v.projection, app.now_ms))
        .filter_map(|r| {
            if let super::rows::Row::User(v) = r {
                Some(v)
            } else {
                None
            }
        })
        .flat_map(|v| super::rows::array(v, "attachments").iter().cloned())
        .collect();
    let Some(net) = app.src.net() else {
        return false;
    };
    let assets = &mut app.ui.chat.timeline.assets;
    let mut changed = false;
    for a in attachments {
        let id = s(&a, "id");
        if id.is_empty() || assets.ready.contains_key(id) || assets.failed.contains(id) {
            continue;
        }
        if !assets.pending.contains_key(id) && assets.pending.len() >= 6 {
            continue;
        }
        let request = *assets.pending.entry(id.to_owned()).or_insert_with(|| net.request("assets.createUrl", json!({"resource":{"_tag":"attachment","attachmentId":id,"fileName":s(&a,"name"),"mimeType":s(&a,"mimeType"),"disposition":"inline"}})));
        if let Some(reply) = net.take_reply(request) {
            assets.pending.remove(id);
            changed = true;
            let Ok(reply) = reply else {
                assets.failed.insert(id.to_owned());
                continue;
            };
            let Some((base, _)) = crate::host::server() else {
                continue;
            };
            let url = format!("{}{}", base.trim_end_matches('/'), s(&reply, "relativeUrl"));
            let pixels = Pixels::new();
            if s(&a, "mimeType").starts_with("image/") {
                let (url, pixels) = (url.clone(), pixels.clone());
                std::thread::spawn(move || {
                    let Ok(response) = ureq::get(&url).call() else {
                        return;
                    };
                    let mut bytes = Vec::new();
                    use std::io::Read;
                    if response
                        .into_reader()
                        .take(20 * 1024 * 1024)
                        .read_to_end(&mut bytes)
                        .is_err()
                    {
                        return;
                    }
                    if let Ok(image) = image::load_from_memory(&bytes) {
                        let image = image.thumbnail(720, 540).to_rgba8();
                        pixels.set(image.width(), image.height(), image.into_raw());
                    }
                });
            }
            assets.ready.insert(id.to_owned(), (url, pixels));
        }
    }
    changed
}

pub fn view(app: &App, attachment: &Value, t: &T) -> El {
    let id = s(attachment, "id");
    let name = s(attachment, "name");
    let asset = app.ui.chat.timeline.assets.ready.get(id);
    let mut content = if s(attachment, "mimeType").starts_with("image/") {
        let mut frame = el()
            .size(240.0, 180.0)
            .radius(16.0)
            .clip_rounded(16.0)
            .fill(t.c.message_surface);
        if let Some((_, pixels)) = asset {
            frame = frame.pixels_fit(pixels, rune::SurfaceFit::Fit);
        }
        frame
    } else {
        row()
            .w(256.0)
            .gap(12.0)
            .padding(8.0)
            .radius(16.0)
            .fill(t.c.message_surface)
            .child(
                icon(&icons::FILE, 20.0)
                    .padding(8.0)
                    .radius(8.0)
                    .fill(t.c.background),
            )
            .child(
                col()
                    .grow(1.0)
                    .child(
                        text(name)
                            .font(theme::SM_MEDIUM)
                            .color(t.c.message_foreground),
                    )
                    .child(
                        text(format!(
                            "{} · {} bytes",
                            name.rsplit('.').next().unwrap_or("File").to_uppercase(),
                            attachment["sizeBytes"].as_u64().unwrap_or(0)
                        ))
                        .font(theme::TEXT_XS)
                        .color(t.c.muted_foreground),
                    ),
            )
            .child(icon(&icons::EXTERNAL_LINK, 12.0).color(t.c.muted_foreground))
    };
    if let Some((url, _)) = asset {
        let url = url.clone();
        content = content
            .focusable(true)
            .role("button")
            .label(format!("Open {name}"))
            .on_press_tx(move |_: &mut App, tx| {
                let _ = tx.open_url(&url);
            });
    }
    content
}
