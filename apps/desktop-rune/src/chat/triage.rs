//! In-app `t3 triage` (useStartTriage): the client the request names, the failure toast, the No
//! project draft it opens with the playbook linked, and how the draft hero spots that draft.

use rune::{TextEdit, Tx};
use serde_json::{Value, json};

use crate::shell::palette::{self, ToastAction};
use crate::{App, Route};

/// TRIAGE_CLI_COMMAND.
pub const CLI_COMMAND: &str = "npx t3 triage";
/// TRIAGE_PROMPT_FILE_NAME: the playbook a triage draft links.
const PROMPT_FILE_NAME: &str = "triage-playbook.md";
const PROMPT_LINK: &str = "[triage-playbook.md](";

/// `currentClientMetadata` on desktop (clientPresentationMetadata with a desktop bridge).
pub fn client_metadata() -> Value {
    let os = match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        _ => "other",
    };
    let mut client = json!({"label": "T3 Code Desktop", "deviceType": "desktop", "os": os, "surface": "desktop"});
    let version = env!("CARGO_PKG_VERSION");
    if version != "0.0.0" {
        client["appVersion"] = json!(version);
    }
    client
}

/// The `server.prepareTriage` payload.
pub fn prepare_input() -> Value {
    json!({"client": client_metadata()})
}

/// useStartTriage's `fail`: a toast that stays, with "Copy command" as its only copy.
pub fn failed(s: &mut App, description: &str) {
    palette::toast_error(s, "Could not start triage", Some(&format!("{description} Run {CLI_COMMAND} in a terminal on that machine instead.")));
    let copy: ToastAction = (
        "Copy command".to_owned(),
        false,
        std::rc::Rc::new(|s: &mut App, tx: &mut Tx| {
            tx.copy(CLI_COMMAND);
            palette::toast_copied(s, tx, "Copy command");
        }),
    );
    palette::set_toast_actions(s, vec![copy]);
}

/// `handleNewThread(project, { prompt })`: the scratch project's draft holds the prompt, its
/// playbook link as a file chip, and opens.
pub fn open_draft(s: &mut App, project: String, prompt: &str) {
    let route = Route::Home { project_id: Some(project) };
    let key = crate::chat::composer::draft_key(s, &route);
    s.ui.chat.composer.drafts.entry(key.clone()).or_default().edit.clear();
    crate::chat::composer::restore_rewound(s, &key, prompt, &[], Vec::new(), true);
    // The prompt ends in a space so the description goes after it.
    if prompt.ends_with(' ')
        && let Some(d) = s.ui.chat.composer.drafts.get_mut(&key)
    {
        d.edit.insert(" ");
    }
    crate::shell::sidebar::go(s, route);
}

/// `isTriagePrompt`: the draft still links the playbook, as written or as its file chip.
pub fn is_triage_prompt(edit: &TextEdit) -> bool {
    edit.text.contains(PROMPT_LINK) || edit.pills().any(|(_, id)| id.rsplit(['/', '\\']).next() == Some(PROMPT_FILE_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data;

    const PROMPT: &str = "Read [triage-playbook.md](/home/dev/.t3/userdata/triage/2026-11-22/triage-playbook.md) and follow it. ";

    /// This machine on a recording connection whose server offers triage and a scratch workspace.
    fn triage_ready() -> (App, s5_net::Sent) {
        let mut app = App::new();
        let mut state = app.state().clone();
        let config = state.config.get_or_insert_with(|| json!({}));
        config["environment"]["capabilities"]["triage"] = json!(true);
        config["scratchWorkspaceRoot"] = json!("/home/dev/.t3/scratch");
        state.projects.push(json!({"id": "scratch", "title": "Scratch", "workspaceRoot": "/home/dev/.t3/scratch"}));
        let (mut net, sent) = s5_net::Net::in_memory_sent("http://local.test");
        *net.state_mut() = state;
        app.src = data::Source::Live(net);
        (app, sent)
    }

    fn request(sent: &s5_net::Sent, method: &str) -> (u64, Value) {
        sent.requests().into_iter().find(|r| r.1 == method).map(|r| (r.0, r.2)).unwrap_or_else(|| panic!("no {method} request"))
    }

    fn answer(app: &mut App, id: u64, reply: s5_net::Reply) {
        app.src.net().expect("live").put_reply(id, reply);
        palette::actions::poll(app);
    }

    #[test]
    fn the_palette_entry_prepares_triage_then_opens_the_scratch_draft() {
        let (mut app, sent) = triage_ready();
        assert!(palette::actions::triage_env(&app).is_some(), "the palette lists Triage an issue");
        palette::actions::triage(&mut app);
        let (prepare, input) = request(&sent, "server.prepareTriage");
        assert_eq!(input, prepare_input());
        assert_eq!(input["client"]["surface"], "desktop");
        assert_eq!(input["client"]["deviceType"], "desktop");
        palette::actions::triage(&mut app);
        assert!(sent.requests().is_empty(), "a second press waits for the first");

        answer(&mut app, prepare, Ok(json!({"prompt": PROMPT})));
        let (scratch, input) = request(&sent, "projects.ensureScratch");
        assert_eq!(input, json!({}));
        answer(&mut app, scratch, Ok(json!({"projectId": "scratch"})));

        assert_eq!(app.route, Route::Home { project_id: Some("scratch".into()) });
        let d = app.ui.chat.composer.drafts.get(&crate::chat::composer::home_key("scratch")).expect("the draft");
        let pills: Vec<&str> = d.edit.pills().map(|(_, id)| id).collect();
        assert_eq!(pills, ["/home/dev/.t3/userdata/triage/2026-11-22/triage-playbook.md"], "the playbook is a file chip");
        assert_eq!(crate::chat::composer::chips::outgoing(&d.edit, &d.contexts).0, PROMPT, "it sends as the server wrote it");
        assert!(is_triage_prompt(&d.edit));
    }

    #[test]
    fn triage_stays_closed_when_the_user_went_elsewhere_and_says_why_it_failed() {
        let (mut app, sent) = triage_ready();
        palette::actions::triage(&mut app);
        let (prepare, _) = request(&sent, "server.prepareTriage");
        answer(&mut app, prepare, Err(json!({"_tag": "ServerPrepareTriageError", "message": "Could not write the triage session files."})));
        assert_eq!(palette::toast_title(&app), Some("Could not start triage"));
        assert!(sent.requests().is_empty(), "no scratch project after a failure");

        palette::actions::triage(&mut app);
        let (prepare, _) = request(&sent, "server.prepareTriage");
        answer(&mut app, prepare, Ok(json!({"prompt": PROMPT})));
        let (scratch, _) = request(&sent, "projects.ensureScratch");
        app.route = Route::PullRequests;
        answer(&mut app, scratch, Ok(json!({"projectId": "scratch"})));
        assert_eq!(app.route, Route::PullRequests, "FAILED: the draft pulled the user back");
        assert!(!app.ui.chat.composer.drafts.contains_key(&crate::chat::composer::home_key("scratch")));
    }

    #[test]
    fn a_triage_draft_is_one_that_still_links_the_playbook() {
        assert!(is_triage_prompt(&TextEdit::new(PROMPT)));
        assert!(!is_triage_prompt(&TextEdit::new("Read the playbook and follow it.")));
        let mut edit = TextEdit::new("Read ");
        edit.insert_pill("/tmp/t/triage-playbook.md", "triage-playbook.md");
        assert!(is_triage_prompt(&edit));
        edit.clear();
        assert!(!is_triage_prompt(&edit), "a cleared draft is a plain one again");
    }
}
