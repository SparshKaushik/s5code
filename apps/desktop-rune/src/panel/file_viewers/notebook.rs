use std::collections::HashMap;

use rune::{El, Pixels, col, el};

use super::{Content, Job, escape, image_pixels, note};
use crate::panel::browser::{Cmd, Shot};
use crate::panel::{file_preview as fp, html_frame, preview};
use crate::theme::T;
use crate::App;
use fp::{Notebook, NotebookOutput};

pub(super) struct NotebookView {
    notebook: Notebook,
    visible: usize,
    output: HashMap<(usize, usize), Rich>,
}

enum Rich {
    Image(Job<Pixels>),
    ImageReady(Result<Pixels, String>),
    Html(Shot),
}

impl NotebookView {
    pub(super) fn new(bytes: &[u8]) -> Result<Self, String> {
        let source = std::str::from_utf8(bytes).map_err(|_| "This notebook is not valid UTF-8.")?;
        Ok(Self { notebook: fp::notebook(source)?, visible: 20, output: HashMap::new() })
    }

    pub(in crate::panel::file_viewers) fn fixture(&mut self, pixels: Pixels, shot: Option<Shot>) {
        self.output.insert((0, 0), Rich::ImageReady(Ok(pixels)));
        if let Some(shot) = shot {
            self.output.insert((0, 1), Rich::Html(shot));
        }
    }

    pub(super) fn poll(&mut self) -> bool {
        let mut changed = false;
        for output in self.output.values_mut() {
            if let Rich::Image(job) = output
                && let Some(result) = job.take()
            {
                *output = Rich::ImageReady(result);
                changed = true;
            }
        }
        changed
    }

    pub(super) fn show_more(&mut self) {
        self.visible += 20;
    }

    pub(super) fn view(&self, app: &App, t: &T) -> El {
        fp::notebook_view(app, super::shown_name(app), &self.notebook, self.visible, t,
            |cell, index, _| self.output_view(cell, index, t))
    }

    fn output_view(&self, cell: usize, index: usize, t: &T) -> El {
        let key = (cell, index);
        let (label, output) = match self.output.get(&key) {
            Some(Rich::ImageReady(Ok(pixels))) => {
                let (w, h) = pixels.size();
                let picture = el().w_full().max_w(w as f32).aspect_ratio(w as f32 / h.max(1) as f32)
                    .max_h(384.0).pixels_fit(pixels, rune::SurfaceFit::Fit);
                (format!("Cell {} output", cell + 1), col().w_full().padding(16.0).child(picture))
            }
            Some(Rich::ImageReady(Err(error))) => ("Image output".to_owned(), note(error.to_owned(), t).padding(16.0)),
            Some(Rich::Html(shot)) => {
                let out = match shot.result() {
                    Some(Some(height)) => el().w_full().h(height.min(384.0)).pixels_fit(&shot.pixels, rune::SurfaceFit::Fit),
                    Some(None) => note("Unable to display HTML output.", t).padding(16.0),
                    None => note("Loading…", t).padding(16.0),
                };
                (format!("Cell {} HTML output", cell + 1), out)
            }
            _ => ("Notebook output".to_owned(), note("Loading…", t).padding(16.0).min_h(80.0)),
        };
        let dark = t.dark;
        col().key(format!("file-notebook-{cell}-{index}")).w_full().label(label)
            .on_layout_tx(move |app: &mut App, rect, tx| {
                let visible = app.ui.desktop.window_size().is_none_or(|[_, height]| rect[1] + rect[3] > 0.0 && rect[1] < height);
                if !visible || !mount(app, key, rect[2].round() as u32, dark) { tx.live_only() }
            })
            .child(output)
    }
}

fn mount(app: &mut App, key: (usize, usize), width: u32, dark: bool) -> bool {
    if width == 0 { return false }
    let Some(Content::Notebook(nb)) = super::active(app) else { return false };
    if nb.output.contains_key(&key) { return false }
    let Some(output) = nb.notebook.cells.get(key.0).and_then(|cell| cell.outputs.get(key.1)) else { return false };
    let rich = match output {
        NotebookOutput::Image(bytes) => {
            let bytes = bytes.clone();
            Rich::Image(Job::spawn(app.ui.panel.waker.clone(), move || image_pixels(&bytes)))
        }
        NotebookOutput::Html(html) => {
            let shot = Shot::new(html_url(html), width, dark);
            if !preview::inline_frame_cmd(app, Cmd::Snapshot(shot.clone())) { return false }
            Rich::Html(shot)
        }
        NotebookOutput::Text(_) => return false,
    };
    if let Some(Content::Notebook(nb)) = super::active_mut(app) {
        nb.output.insert(key, rich);
    }
    true
}

pub(super) fn html_url(html: &str) -> String {
    let inner = format!(r#"<!doctype html><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src data:; style-src 'unsafe-inline'; form-action 'none'; base-uri 'none'"><style>html{{color-scheme:light dark}}body{{font:12px system-ui;color:CanvasText;background:Canvas;margin:12px}}img{{max-width:100%}}table{{border-collapse:collapse}}td,th{{border:1px solid GrayText;padding:4px 6px}}</style>{html}"#);
    html_frame::data_url(&format!(r#"<!doctype html><meta charset="utf-8"><style>body{{margin:0}}iframe{{display:block;width:100%;height:240px;border:0}}</style><iframe sandbox="" title="Saved HTML output" srcdoc="{}"></iframe>"#, escape(&inner)))
}
