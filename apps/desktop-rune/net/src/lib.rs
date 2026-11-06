//! The S5 Code server, as the Rune client sees it: one WebSocket speaking Effect-RPC JSON, the
//! shell/config/thread streams folded into plain state, and the commands the UI sends.
//! (Interface stub; the implementation lands separately.)

use std::collections::HashMap;

use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub enum Conn {
    Connecting,
    Open,
    Closed(String),
}

#[derive(Clone, Debug, Default)]
pub struct ThreadView {
    pub projection: Value,
    pub synchronized: bool,
    pub sequence: u64,
}

#[derive(Clone, Debug)]
pub struct State {
    pub conn: Conn,
    pub config: Option<Value>,
    pub projects: Vec<Value>,
    pub threads: Vec<Value>,
    pub archived: Vec<Value>,
    pub shell_ready: bool,
    pub thread_views: HashMap<String, ThreadView>,
    pub vcs: HashMap<String, Value>,
}

pub type Reply = Result<Value, Value>;

pub struct Net {}

impl Net {
    pub fn connect(_base_url: &str, _token: &str, _wake: impl Fn() + Send + Sync + 'static) -> Net {
        todo!()
    }
    pub fn drain(&mut self) -> bool {
        todo!()
    }
    pub fn state(&self) -> &State {
        todo!()
    }
    pub fn watch_thread(&mut self, _thread_id: &str) {
        todo!()
    }
    pub fn unwatch_thread(&mut self, _thread_id: &str) {
        todo!()
    }
    pub fn watch_vcs(&mut self, _cwd: &str) {
        todo!()
    }
    pub fn request(&mut self, _tag: &str, _payload: Value) -> u64 {
        todo!()
    }
    pub fn take_reply(&mut self, _id: u64) -> Option<Reply> {
        todo!()
    }
    pub fn dispatch(&mut self, _command: Value) -> u64 {
        todo!()
    }
}

pub fn uuid() -> String {
    todo!()
}
