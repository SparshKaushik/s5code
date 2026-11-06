//! A tiny blocking client for `opencode serve`. Every request runs on its own short-lived thread
//! and lands in the shared inbox, which wakes the window; the view never waits on the network.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rune::Waker;
use serde_json::{Value, json};

#[derive(Clone, Debug)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub updated_ms: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    Text(String),
    Reasoning(String),
    Tool { name: String, title: String, done: bool },
}

#[derive(Clone, Debug)]
pub struct Message {
    pub id: String,
    pub user: bool,
    pub parts: Vec<Part>,
    pub done: bool,
    pub error: Option<String>,
}

pub enum Event {
    Connected(bool),
    Sessions { project: usize, sessions: Vec<Session> },
    Created { thread: u64, session: String },
    Messages { thread: u64, messages: Vec<Message>, busy: bool },
    Failed { thread: u64, error: String },
}

#[derive(Clone)]
pub struct Client {
    base: String,
    inbox: Arc<Mutex<Vec<Event>>>,
    waker: Option<Waker>,
}

impl Client {
    pub fn new(base: impl Into<String>) -> Client {
        Client { base: base.into(), inbox: Arc::default(), waker: None }
    }

    pub fn set_waker(&mut self, waker: Waker) {
        self.waker = Some(waker);
    }

    pub fn drain(&self) -> Vec<Event> {
        std::mem::take(&mut *self.inbox.lock().unwrap())
    }

    fn post(&self, e: Event) {
        self.inbox.lock().unwrap().push(e);
        if let Some(w) = &self.waker {
            w();
        }
    }

    fn url(&self, path: &str, dir: &str) -> String {
        let sep = if path.contains('?') { '&' } else { '?' };
        format!("{}{path}{sep}directory={}", self.base, encode(dir))
    }

    pub fn ping(&self) {
        let c = self.clone();
        std::thread::spawn(move || {
            let ok = ureq::get(&format!("{}/config/providers", c.base)).timeout(Duration::from_secs(3)).call().is_ok();
            c.post(Event::Connected(ok));
        });
    }

    pub fn list_sessions(&self, project: usize, dir: String) {
        let c = self.clone();
        std::thread::spawn(move || {
            let Ok(v) = ureq::get(&c.url("/session", &dir)).call().and_then(|r| Ok(r.into_json::<Value>()?)) else { return };
            let mut sessions: Vec<Session> = v
                .as_array()
                .into_iter()
                .flatten()
                .filter(|s| s["parentID"].is_null() && s["directory"].as_str() == Some(dir.as_str()))
                .map(|s| Session {
                    id: s["id"].as_str().unwrap_or_default().into(),
                    title: s["title"].as_str().unwrap_or("Untitled").into(),
                    updated_ms: s["time"]["updated"].as_u64().unwrap_or(0),
                })
                .collect();
            sessions.sort_by(|a, b| b.updated_ms.cmp(&a.updated_ms));
            c.post(Event::Sessions { project, sessions });
        });
    }

    pub fn load(&self, thread: u64, session: String, dir: String) {
        let c = self.clone();
        std::thread::spawn(move || {
            if let Some(messages) = c.messages(&session, &dir) {
                c.post(Event::Messages { thread, messages, busy: false });
            }
        });
    }

    /// Creates the session when the thread has none, sends the prompt, then follows the reply
    /// until the assistant's message completes.
    pub fn prompt(&self, thread: u64, session: Option<String>, dir: String, model: (String, String), text: String) {
        let c = self.clone();
        std::thread::spawn(move || {
            let session = match session {
                Some(s) => s,
                None => match ureq::post(&c.url("/session", &dir)).send_json(json!({})).and_then(|r| Ok(r.into_json::<Value>()?)) {
                    Ok(v) => {
                        let id = v["id"].as_str().unwrap_or_default().to_string();
                        c.post(Event::Created { thread, session: id.clone() });
                        id
                    }
                    Err(e) => return c.post(Event::Failed { thread, error: e.to_string() }),
                },
            };
            let body = json!({
                "model": { "providerID": model.0, "modelID": model.1 },
                "parts": [{ "type": "text", "text": text }],
            });
            if let Err(e) = ureq::post(&c.url(&format!("/session/{session}/prompt_async"), &dir)).send_json(body) {
                return c.post(Event::Failed { thread, error: e.to_string() });
            }
            let start = Instant::now();
            let mut last: Option<Vec<Message>> = None;
            while start.elapsed() < Duration::from_secs(300) {
                std::thread::sleep(Duration::from_millis(250));
                let Some(messages) = c.messages(&session, &dir) else { continue };
                let replied = messages.last().is_some_and(|m| !m.user && m.done);
                let changed = last.as_ref().is_none_or(|l| !same(l, &messages));
                if changed || replied {
                    c.post(Event::Messages { thread, messages: messages.clone(), busy: !replied });
                }
                if replied {
                    return;
                }
                last = Some(messages);
            }
        });
    }

    fn messages(&self, session: &str, dir: &str) -> Option<Vec<Message>> {
        let v: Value = ureq::get(&self.url(&format!("/session/{session}/message"), dir)).call().ok()?.into_json().ok()?;
        Some(v.as_array()?.iter().map(parse_message).collect())
    }
}

fn same(a: &[Message], b: &[Message]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.id == y.id && x.done == y.done && x.parts == y.parts)
}

fn parse_message(m: &Value) -> Message {
    let info = &m["info"];
    let parts = m["parts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| match p["type"].as_str()? {
            "text" => Some(Part::Text(p["text"].as_str()?.to_string())).filter(|_| p["synthetic"] != true),
            "reasoning" => Some(Part::Reasoning(p["text"].as_str().unwrap_or_default().to_string())),
            "tool" => {
                let state = &p["state"];
                let name = p["tool"].as_str().unwrap_or("tool").to_string();
                let title = state["title"]
                    .as_str()
                    .map(str::to_string)
                    .or_else(|| state["input"]["filePath"].as_str().map(str::to_string))
                    .or_else(|| state["input"]["command"].as_str().map(str::to_string))
                    .unwrap_or_default();
                let done = matches!(state["status"].as_str(), Some("completed" | "error"));
                Some(Part::Tool { name, title, done })
            }
            _ => None,
        })
        .collect();
    Message {
        id: info["id"].as_str().unwrap_or_default().into(),
        user: info["role"] == "user",
        parts,
        done: info["role"] == "user" || !info["time"]["completed"].is_null(),
        error: info["error"]["data"]["message"].as_str().or(info["error"]["name"].as_str()).map(str::to_string),
    }
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}
