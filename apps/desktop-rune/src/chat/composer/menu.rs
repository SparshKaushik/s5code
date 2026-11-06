//! The composer's trigger drawer (ComposerCommandMenu): `@` files and folders from
//! `projects.searchEntries`, `/` commands, `$` skills. It rises out of the top of the composer,
//! tucked 17 points under it.

use rune::{Cursor, Cx, El, Tx, col, el, icon, row, text};
use serde_json::{Value, json};

use super::{ctx, draft, draft_mut, provider, selection};
use crate::data;
use crate::icons;
use crate::theme::{self, T, radius};
use crate::App;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Path,
    Slash,
    Skill,
}

/// The trigger token under the caret.
pub struct Open {
    pub kind: Kind,
    pub token: String,
    pub start: usize,
    pub end: usize,
}

impl Open {
    fn query(&self) -> &str {
        &self.token[1..]
    }
}

#[derive(Default)]
pub struct Mention {
    query: Option<String>,
    request: Option<u64>,
    pub loading: bool,
    entries: Vec<(String, bool)>,
}

pub struct Item {
    pub id: String,
    pub label: String,
    pub description: String,
    pub folder: Option<bool>,
}

const BUILTIN_SLASH: [(&str, &str); 3] =
    [("model", "Switch response model for this thread"), ("side", "Ask a side question in a new side chat"), ("btw", "Same as /side")];

pub fn open_menu(s: &App) -> Option<Open> {
    let d = draft(s)?;
    let text = &d.edit.text;
    let caret = d.edit.focus.min(text.len());
    if !text.is_char_boundary(caret) {
        return None;
    }
    let start = text[..caret].rfind(char::is_whitespace).map_or(0, |i| i + text[i..].chars().next().map_or(1, char::len_utf8));
    let token = &text[start..caret];
    let kind = match token.chars().next()? {
        '@' => Kind::Path,
        '/' if start == 0 => Kind::Slash,
        '$' => Kind::Skill,
        _ => return None,
    };
    if s.ui.chat.composer.menu_dismissed.as_deref() == Some(token) {
        return None;
    }
    Some(Open { kind, token: token.to_string(), start, end: caret })
}

fn provider_list<'a>(s: &'a App, key: &str) -> Vec<&'a Value> {
    selection(s).and_then(|(i, _)| provider(s, &i)).and_then(|p| p[key].as_array()).map(|v| v.iter().collect()).unwrap_or_default()
}

pub fn items(s: &App, m: &Open) -> Vec<Item> {
    let q = m.query().to_lowercase();
    let item = |id: String, label: String, description: String| Item { id, label, description, folder: None };
    match m.kind {
        Kind::Slash => {
            let builtins = BUILTIN_SLASH.iter().map(|(n, d)| item(format!("/{n}"), format!("/{n}"), d.to_string()));
            let provided = provider_list(s, "slashCommands")
                .into_iter()
                .map(|c| item(format!("/{}", data::s(c, "name")), format!("/{}", data::s(c, "name")), data::s(c, "description").to_string()));
            builtins.chain(provided).filter(|i| i.label[1..].to_lowercase().starts_with(&q)).collect()
        }
        Kind::Skill => provider_list(s, "skills")
            .into_iter()
            .map(|c| item(format!("${}", data::s(c, "name")), data::s(c, "name").to_string(), data::s(c, "description").to_string()))
            .filter(|i| i.label.to_lowercase().contains(&q))
            .collect(),
        Kind::Path => {
            let mention = &s.ui.chat.composer.mention;
            if mention.query.as_deref() != Some(m.query()) {
                return Vec::new();
            }
            mention
                .entries
                .iter()
                .map(|(path, folder)| {
                    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
                    Item { id: path.clone(), label: name.to_string(), description: dir.to_string(), folder: Some(*folder) }
                })
                .collect()
        }
    }
}

/// The working directory file searches run in: the thread's worktree, else the project root.
fn search_root(s: &App) -> Option<String> {
    let c = ctx(s);
    let worktree = c.thread.map(|t| data::s(t, "worktreePath")).filter(|p| !p.is_empty());
    worktree.or_else(|| c.project.map(|p| data::s(p, "workspaceRoot"))).filter(|p| !p.is_empty()).map(str::to_string)
}

/// Starts a file search when the `@` query changed.
pub fn refresh(s: &mut App) {
    let Some(m) = open_menu(s) else { return };
    if m.kind != Kind::Path || s.ui.chat.composer.mention.query.as_deref() == Some(m.query()) {
        return;
    }
    let query = m.query().to_string();
    let root = search_root(s);
    let mention = &mut s.ui.chat.composer.mention;
    mention.query = Some(query.clone());
    mention.entries.clear();
    mention.request = None;
    mention.loading = false;
    let (Some(root), Some(net)) = (root, s.src.net()) else { return };
    let request = net.request("projects.searchEntries", json!({"cwd": root, "query": query, "limit": 20}));
    let mention = &mut s.ui.chat.composer.mention;
    mention.request = Some(request);
    mention.loading = true;
}

pub fn poll(mention: &mut Mention, net: &mut s5_net::Net) -> bool {
    let Some(request) = mention.request else { return false };
    let Some(reply) = net.take_reply(request) else { return false };
    mention.request = None;
    mention.loading = false;
    mention.entries = reply
        .ok()
        .and_then(|v| v["entries"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .map(|e| (data::s(e, "path").to_string(), data::s(e, "kind") == "directory"))
        .collect();
    true
}

pub fn choose(s: &mut App, m: &Open, index: usize, _tx: &mut Tx) {
    let Some(item) = items(s, m).into_iter().nth(index) else { return };
    s.ui.chat.composer.menu_active = 0;
    if item.id == "/model" {
        let d = draft_mut(s);
        d.edit.set_selection(m.start, m.end);
        d.edit.insert("");
        super::picker::toggle(s);
        return;
    }
    let d = draft_mut(s);
    d.edit.set_selection(m.start, m.end);
    match m.kind {
        Kind::Path => {
            d.edit.replace_with_pill(m.start..m.end, item.id.clone(), &format!("@{}", item.label));
            d.edit.insert(" ");
        }
        _ => d.edit.insert(&format!("{} ", item.id)),
    }
}

fn surface(t: &T) -> El {
    col()
        .key("composer-menu")
        .corners([radius::XL2, radius::XL2, 0.0, 0.0])
        .fill(if t.dark { t.c.surface_raised } else { t.c.card })
        .border(1.0, if t.dark { rune::Color::WHITE.alpha(0.05) } else { rune::Color::BLACK.alpha(0.08) })
}

pub fn view(s: &App, _cx: &mut Cx<App>, t: &T) -> Option<El> {
    if s.ui.chat.composer.picker.open {
        return None;
    }
    let m = open_menu(s)?;
    let list = items(s, &m);
    if list.is_empty() {
        let message = match m.kind {
            Kind::Path if s.ui.chat.composer.mention.loading => "Searching workspace files...",
            Kind::Path => "No matching files or folders.",
            Kind::Skill => "No skills found. Try / to browse provider commands.",
            Kind::Slash => "No matching command.",
        };
        return Some(surface(t).px(20.0).pt(14.0).pb(28.0 + 17.0).child(text(message).font(theme::TEXT_XS).color(t.c.secondary_label)));
    }
    let active = s.ui.chat.composer.menu_active.min(list.len() - 1);
    let mut rows = col().w_full();
    for (i, item) in list.into_iter().enumerate() {
        let mut r = row()
            .key(format!("composer-menu-{}", item.id))
            .h(28.0)
            .px(8.0)
            .gap(8.0)
            .center_y()
            .radius(theme::layout::CONTROL_RADIUS)
            .cursor(Cursor::Pointer)
            .on_hover(move |s: &mut App| s.ui.chat.composer.menu_active = i)
            .on_press_tx(move |s: &mut App, tx| {
                if let Some(m) = open_menu(s) {
                    choose(s, &m, i, tx);
                }
            });
        if i == active {
            r = r.fill(t.c.accent);
        }
        if let Some(folder) = item.folder {
            r = r.child(icon(if folder { &icons::FOLDER } else { &icons::FILE }, 16.0).color(t.c.secondary_label));
        }
        r = r
            .child(text(item.label).font(theme::XS_MEDIUM).color(t.c.foreground).truncate().shrink())
            .child(text(item.description).font(theme::TEXT_XS).color(t.c.secondary_label).truncate().grow(1.0).shrink().min_w(0.0));
        rows = rows.child(r);
    }
    let rows = col().w_full().max_h(280.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(rows);
    Some(surface(t).px(8.0).pt(8.0).pb(8.0 + 17.0).child(rows).child(el()))
}
