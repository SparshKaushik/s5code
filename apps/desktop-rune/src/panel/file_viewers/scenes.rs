use std::io::Write;
use std::time::{Duration, Instant};

use base64::Engine;
use rune::Pixels;

use super::{Archive, Content, Database, Frame, LargeText, Mode, Viewer, font_url, html_url, media_url, notebook};
use crate::panel::{self, Surface, browser, files, html_frame, preview};
use crate::App;

pub(in crate::panel) const SCENES: &[(&str, fn(&mut App))] = &[
    ("panel-file-large-text", large_text),
    ("panel-file-large-table", large_table),
    ("panel-file-image-zoom", image_zoom),
    ("panel-file-html", html),
    ("panel-file-pdf", pdf),
    ("panel-file-video", video),
    ("panel-file-audio", audio),
    ("panel-file-font", font),
    ("panel-file-archive", archive),
    ("panel-file-sqlite", sqlite),
    ("panel-file-notebook", notebook),
    ("panel-diff-hide-tests", hide_tests),
];

fn scene(app: &mut App, name: &str, rendered: bool) {
    panel::scene(app, Some(Surface::Files));
    files::preview_fixture(app, name, "", rendered);
    if let crate::Route::Thread(id) = app.frame_route() {
        let id = id.clone();
        app.ui.panel.maximized.insert(id);
    }
}

fn show(app: &mut App, mode: Mode, content: Result<Content, String>) {
    app.ui.panel.files.viewer = Some(Viewer::ready(mode, content));
}

fn large_text(app: &mut App) {
    scene(app, "logs/build.log", false);
    let fixture: String = (1..=30000).map(|n| format!("2026-11-25 09:48:33 [INFO] Completed build step {n:05}\n")).collect();
    app.ui.panel.files.line = Some(6);
    show(app, Mode::Large(None), LargeText::new(fixture.into_bytes(), None).map(Content::Large));
}

fn large_table(app: &mut App) {
    scene(app, "data/builds.csv", true);
    let fixture = format!("machine,job,elapsed,result\n{}", "Studio,Rune Linux,278,Passed\nBuild box,Server bundle,104,Passed\n".repeat(20000));
    show(app, Mode::Large(Some(',')), LargeText::new(fixture.into_bytes(), Some(',')).map(Content::Large));
}

fn image_zoom(app: &mut App) {
    scene(app, "reports/architecture.png", false);
    files::image_fixture(app);
}

fn shot(app: &mut App, mode: Mode, url: String, ready: bool) {
    html_frame::scene_waker(app);
    let result = (|| {
        let live = preview::live(app, url, 768, 686, true, None).ok_or("Chrome is unavailable.")?;
        let deadline = Instant::now() + Duration::from_secs(12);
        let mut painted = None;
        let mut finished = !ready;
        while Instant::now() < deadline {
            finished |= live.messages().iter().any(|event| event == "file-preview-ready");
            if finished && live.pixels.size().0 > 0 {
                let first = painted.get_or_insert_with(Instant::now);
                if first.elapsed() >= Duration::from_millis(300) { break; }
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        let mut frame = Frame::new(mode, String::new());
        if live.pixels.size().0 == 0 || !finished {
            return Err("Chrome could not render this scene.".into());
        }
        frame.still = Some(live.pixels);
        Ok(Content::Browser(frame))
    })();
    browser::shutdown();
    show(app, mode, result);
}

fn html(app: &mut App) {
    scene(app, "reports/metrics.html", true);
    shot(app, Mode::Html, html_url(&html_frame::data_url(html_frame::SCENE_PAGE), "Metrics"), false);
}

fn fixture_pdf() -> Vec<u8> {
    let first = b"BT /F1 28 Tf 72 690 Td (Build report) Tj 0 -46 Td /F1 16 Tf (All pipeline checks passed.) Tj ET";
    let second = b"BT /F1 28 Tf 72 690 Td (Build results) Tj 0 -46 Td /F1 16 Tf (Linux test suite: passed.) Tj ET";
    let stream = |body: &[u8]| format!("<< /Length {} >>\nstream\n{}\nendstream", body.len(), String::from_utf8_lossy(body));
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Count 2 /Kids [3 0 R 4 0 R] >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 6 0 R >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 7 0 R >>".to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        stream(first),
        stream(second),
    ];
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        let _ = writeln!(pdf, "{} 0 obj\n{object}\nendobj", i + 1);
    }
    let xref = pdf.len();
    let _ = writeln!(pdf, "xref\n0 {}\n0000000000 65535 f ", offsets.len() + 1);
    for offset in &offsets {
        let _ = writeln!(pdf, "{offset:010} 00000 n ");
    }
    let _ = writeln!(pdf, "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF", offsets.len() + 1);
    pdf
}

fn pdf(app: &mut App) {
    scene(app, "reports/build.pdf", true);
    let html = browser::file_pdf_viewer("build.pdf", Some(&fixture_pdf()));
    shot(app, Mode::Pdf, html_frame::data_url(&html), true);
}

fn audio(app: &mut App) {
    scene(app, "notices/completion.mp3", true);
    let encoded = base64::engine::general_purpose::STANDARD.encode(include_bytes!("../../../assets/notification-completion.mp3"));
    let url = media_url(Mode::Audio, &format!("data:audio/mpeg;base64,{encoded}"), "completion.mp3");
    shot(app, Mode::Audio, url, false);
}

fn video(app: &mut App) {
    scene(app, "captures/build.webm", true);
    let html = r##"<!doctype html><style>:root{color-scheme:dark;font:12px system-ui}body{margin:0;display:grid;place-items:center;height:100dvh}video{max-width:95%;max-height:95%}</style>
<video controls muted autoplay playsinline></video><script>
const canvas=document.createElement("canvas");canvas.width=640;canvas.height=360;
const ctx=canvas.getContext("2d");ctx.fillStyle="#17243a";ctx.fillRect(0,0,640,360);
ctx.fillStyle="#60a5fa";ctx.fillRect(30,30,580,6);ctx.fillStyle="#fff";ctx.font="28px system-ui";
ctx.fillText("Linux test run",44,106);ctx.font="18px monospace";ctx.fillText("✓ All workspace tests passed",44,190);
const video=document.querySelector("video");
video.addEventListener("playing",()=>window.__s5Frame?.("file-preview-ready"));
video.srcObject=canvas.captureStream(1);
setTimeout(()=>ctx.fillRect(29,30,1,6),100);
</script>"##;
    shot(app, Mode::Video, html_frame::data_url(html), true);
}

fn font(app: &mut App) {
    scene(app, "fonts/Inter-Regular.ttf", true);
    let data = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
    shot(app, Mode::Font, font_url(data, "Inter-Regular.ttf"), false);
}

fn tar() -> Vec<u8> {
    let mut tar = Vec::new();
    for (path, data) in [("docs/build.txt", "Linux CI completed."), ("docs/review.txt", "Ready to review."), ("README.md", "Build artifacts")] {
        let mut header = [0u8; 512];
        header[..path.len()].copy_from_slice(path.as_bytes());
        header[124..136].copy_from_slice(format!("{:011o}\0", data.len()).as_bytes());
        header[148..156].fill(b' ');
        header[156] = b'0';
        let checksum: usize = header.iter().map(|b| *b as usize).sum();
        header[148..156].copy_from_slice(format!("{checksum:06o}\0 ").as_bytes());
        tar.extend(header);
        tar.extend(data.as_bytes());
        tar.resize(tar.len().next_multiple_of(512), 0);
    }
    tar.resize(tar.len() + 1024, 0);
    tar
}

fn archive(app: &mut App) {
    scene(app, "artifacts/checks.tar", true);
    show(app, Mode::Archive, Archive::open("checks.tar", tar()).map(Content::Archive));
}

fn sqlite(app: &mut App) {
    scene(app, "data/builds.sqlite", true);
    show(app, Mode::Sqlite, Database::fixture().map(Content::Sqlite));
}

fn notebook(app: &mut App) {
    scene(app, "data/test-report.ipynb", true);
    let json = br#"{"nbformat":4,"metadata":{"language_info":{"name":"python"}},"cells":[{"cell_type":"code","source":"display(build_summary)","outputs":[{"data":{"image/png":"AQID"}},{"data":{"text/html":"<table><tr><th>Run</th><th>Status</th></tr><tr><td>Linux</td><td>Passed</td></tr></table>"}}]}]}"#;
    let content = notebook::NotebookView::new(json).map(|mut notebook| {
        let pixels = Pixels::new();
        let mut bytes = Vec::new();
        for y in 0..96 {
            for x in 0..320 {
                let bar = (32..80).contains(&x) && y > 26 || (108..156).contains(&x) && y > 18 || (184..232).contains(&x) && y > 52;
                bytes.extend(if bar { [36, 162, 129, 255] } else { [28, 34, 44, 255] });
            }
        }
        pixels.set(320, 96, bytes);
        html_frame::scene_waker(app);
        let html = notebook::html_url("<table><tr><th>Run</th><th>Status</th></tr><tr><td>Linux</td><td>Passed</td></tr></table>");
        let shot = preview::snapshot(app, html, 624, true);
        let deadline = Instant::now() + Duration::from_secs(12);
        while shot.as_ref().is_some_and(|shot| shot.result().is_none()) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(30));
        }
        browser::shutdown();
        notebook.fixture(pixels, shot);
        Content::Notebook(notebook)
    });
    show(app, Mode::Notebook, content);
}

fn hide_tests(app: &mut App) {
    panel::scene(app, Some(Surface::Diff));
    app.client_settings.values.insert(crate::settings::client::HIDE_TEST_FILES.into(), true.into());
    panel::diff::fixture(app);
}
