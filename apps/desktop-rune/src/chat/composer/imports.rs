//! useComposerContextImport's attachment import: a pasted image or file chip brings the copied
//! attachment into the draft it was pasted into, signed and downloaded from this environment.
//! A picked element's screenshot keeps its `preview-annotation-{id}.png` name, so it goes with
//! its chip.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::App;
use crate::data::s;

type Slot = Arc<Mutex<Option<Result<PathBuf, String>>>>;

enum Step {
    Signing(u64),
    Downloading(Slot),
}

struct Import {
    /// The draft the paste landed in.
    draft: String,
    name: String,
    /// The name the bytes are saved under.
    file: String,
    step: Step,
}

#[derive(Default)]
pub struct Imports(Vec<Import>);

impl Imports {
    pub fn start(&mut self, net: &mut s5_net::Net, draft: String, record: &Value, file: String) {
        let request = net.request("assets.createUrl", json!({"resource": {"_tag": "attachment", "attachmentId": s(record, "attachmentId")}}));
        self.0.push(Import { draft, name: s(record, "name").to_owned(), file, step: Step::Signing(request) });
    }
}

fn fail(app: &mut App, name: &str, reason: &str) {
    let description = format!("{reason} Attach the file again.");
    crate::shell::palette::toast_error_brief(app, &format!("Couldn't bring {name} into this message"), Some(&description));
}

/// Signs, downloads, and adds each finished import to its draft.
pub fn poll(app: &mut App) -> bool {
    if app.ui.chat.composer.imports.0.is_empty() {
        return false;
    }
    let waker = app.ui.panel.waker.clone();
    let mut done: Vec<(String, String, Result<PathBuf, String>)> = Vec::new();
    let Some(net) = app.src.net() else { return false };
    let base = net.base_url().trim_end_matches('/').to_owned();
    let imports = &mut app.ui.chat.composer.imports.0;
    let mut i = 0;
    while i < imports.len() {
        let import = &mut imports[i];
        match &import.step {
            Step::Signing(request) => match net.take_reply(*request) {
                Some(Ok(reply)) => {
                    let (url, file, slot) = (format!("{base}{}", s(&reply, "relativeUrl")), import.file.clone(), Slot::default());
                    let (into, waker) = (slot.clone(), waker.clone());
                    std::thread::spawn(move || {
                        let dir = std::env::temp_dir().join("s5-pasted-attachments").join(s5_net::uuid());
                        let path = dir.join(&file);
                        let saved = std::fs::create_dir_all(&dir).map_err(|e| e.to_string()).and_then(|()| crate::chat::timeline::media::download(&url, &path));
                        *into.lock().unwrap() = Some(saved.map(|()| path));
                        if let Some(wake) = waker {
                            wake();
                        }
                    });
                    import.step = Step::Downloading(slot);
                }
                Some(Err(_)) => {
                    let import = imports.remove(i);
                    done.push((import.draft, import.name, Err("The original attachment is no longer available.".into())));
                    continue;
                }
                None => {}
            },
            Step::Downloading(slot) => {
                let finished = slot.lock().unwrap().take();
                if let Some(result) = finished {
                    let import = imports.remove(i);
                    done.push((import.draft, import.name, result.map_err(|_| "Downloading it from the source failed.".to_owned())));
                    continue;
                }
            }
        }
        i += 1;
    }
    let changed = !done.is_empty();
    for (draft, name, result) in done {
        match result {
            // A draft sent or dropped meanwhile takes nothing.
            Ok(path) => {
                if let Some(d) = app.ui.chat.composer.drafts.get_mut(&draft)
                    && let Some(error) = super::attachments::intake(&mut d.attachments, [path.display().to_string()])
                {
                    fail(app, &name, &error);
                }
            }
            Err(reason) => fail(app, &name, &reason),
        }
    }
    changed
}
