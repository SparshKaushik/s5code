use crate::{
    App, data, icons, kit,
    theme::{self, T},
};
use rune::{Color, El, Key, KeyEvent, ScrollSpec, Tx, col, el, row, text};
use serde_json::{Value, json};

#[derive(Clone, Copy, Default)]
struct Cell {
    ch: char,
    fg: Option<Color>,
    bg: Option<Color>,
}
#[derive(Default)]
pub struct Screen {
    lines: Vec<Vec<Cell>>,
    x: usize,
    y: usize,
    fg: Option<Color>,
    bg: Option<Color>,
    escape: String,
    mode: u8,
    saved: (usize, usize),
    cols: usize,
    rows: usize,
}
impl Screen {
    fn size(&mut self, cols: usize, rows: usize) {
        self.cols = cols.max(1);
        self.rows = rows.max(1);
    }
    fn write(&mut self, c: char) {
        if self.cols > 0 && self.x >= self.cols {
            self.x = 0;
            self.y += 1;
        }
        while self.lines.len() <= self.y {
            self.lines.push(vec![]);
        }
        let line = &mut self.lines[self.y];
        while line.len() <= self.x {
            line.push(Cell {
                ch: ' ',
                ..Default::default()
            });
        }
        line[self.x] = Cell {
            ch: c,
            fg: self.fg,
            bg: self.bg,
        };
        self.x += 1;
    }
    pub fn feed(&mut self, data: &str) {
        for c in data.chars() {
            match self.mode {
                1 => {
                    self.mode = match c {
                        '[' => 2,
                        ']' => 3,
                        '7' => {
                            self.saved = (self.x, self.y);
                            0
                        }
                        '8' => {
                            (self.x, self.y) = self.saved;
                            0
                        }
                        _ => 0,
                    };
                    self.escape.clear();
                }
                2 => {
                    if ('@'..='~').contains(&c) {
                        let params = std::mem::take(&mut self.escape);
                        self.csi(c, &params);
                        self.mode = 0;
                    } else if self.escape.len() < 256 {
                        self.escape.push(c);
                    } else {
                        self.mode = 0;
                    }
                }
                3 => {
                    if c == '\x07' {
                        self.mode = 0;
                    } else if c == '\x1b' {
                        self.mode = 4;
                    }
                }
                4 => {
                    self.mode = if c == '\\' { 0 } else { 3 };
                }
                _ => match c {
                    '\x1b' => self.mode = 1,
                    '\r' => self.x = 0,
                    '\n' => self.y += 1,
                    '\x08' => self.x = self.x.saturating_sub(1),
                    '\t' => {
                        let n = 8 - self.x % 8;
                        for _ in 0..n {
                            self.write(' ');
                        }
                    }
                    c if !c.is_control() => self.write(c),
                    _ => {}
                },
            }
            // Bound scrollback even when a process emits only newlines.
            if self.y > 4000 {
                let n = self.y - 4000;
                self.lines.drain(..n.min(self.lines.len()));
                self.y -= n;
                self.saved.1 = self.saved.1.saturating_sub(n);
            }
        }
    }
    fn csi(&mut self, kind: char, raw: &str) {
        let p: Vec<usize> = raw.split(';').map(|v| v.parse().unwrap_or(0)).collect();
        let n = p.first().copied().unwrap_or(0);
        let amount = n.max(1);
        match kind {
            'A' => self.y = self.y.saturating_sub(amount),
            'B' => self.y = (self.y + amount).min(4000),
            'C' => self.x = (self.x + amount).min(self.cols.max(1) - 1),
            'D' => self.x = self.x.saturating_sub(amount),
            'G' => self.x = (amount - 1).min(self.cols.max(1) - 1),
            'H' | 'f' => {
                let top = self.lines.len().saturating_sub(self.rows);
                self.y = top + amount.saturating_sub(1).min(self.rows.max(1) - 1);
                self.x = p.get(1).copied().unwrap_or(1).max(1) - 1;
            }
            'J' => {
                if n == 2 || n == 3 {
                    self.lines.clear();
                    self.x = 0;
                    self.y = 0;
                } else if n == 0 {
                    self.lines.truncate(self.y + 1);
                    if let Some(l) = self.lines.get_mut(self.y) {
                        l.truncate(self.x);
                    }
                }
            }
            'K' => {
                if let Some(l) = self.lines.get_mut(self.y) {
                    match n {
                        0 => l.truncate(self.x),
                        1 => {
                            let end = (self.x + 1).min(l.len());
                            for cell in &mut l[..end] {
                                *cell = Cell {
                                    ch: ' ',
                                    ..Default::default()
                                };
                            }
                        }
                        2 => l.clear(),
                        _ => {}
                    }
                }
            }
            's' => self.saved = (self.x, self.y),
            'u' => (self.x, self.y) = self.saved,
            'm' => {
                let mut i = 0;
                while i < p.len() {
                    match p[i] {
                        0 => {
                            self.fg = None;
                            self.bg = None
                        }
                        30..=37 => self.fg = Some(ansi(p[i] - 30)),
                        40..=47 => self.bg = Some(ansi(p[i] - 40)),
                        90..=97 => self.fg = Some(ansi(p[i] - 90 + 8)),
                        100..=107 => self.bg = Some(ansi(p[i] - 100 + 8)),
                        39 => self.fg = None,
                        49 => self.bg = None,
                        38 | 48 => {
                            let foreground = p[i] == 38;
                            let color = if p.get(i + 1) == Some(&5) && i + 2 < p.len() {
                                i += 2;
                                Some(ansi(p[i]))
                            } else if p.get(i + 1) == Some(&2) && i + 4 < p.len() {
                                let c = rgb(
                                    p[i + 2].min(255) as u8,
                                    p[i + 3].min(255) as u8,
                                    p[i + 4].min(255) as u8,
                                );
                                i += 4;
                                Some(c)
                            } else {
                                None
                            };
                            if foreground {
                                self.fg = color
                            } else {
                                self.bg = color
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
            }
            _ => {}
        }
    }
}
fn ansi(n: usize) -> Color {
    const BASE: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 49, 49),
        (13, 188, 121),
        (229, 229, 16),
        (36, 114, 200),
        (188, 63, 188),
        (17, 168, 205),
        (229, 229, 229),
        (102, 102, 102),
        (241, 76, 76),
        (35, 209, 139),
        (245, 245, 67),
        (59, 142, 234),
        (214, 112, 214),
        (41, 184, 219),
        (255, 255, 255),
    ];
    let (r, g, b) = if n < 16 {
        BASE[n]
    } else if n < 232 {
        let n = n - 16;
        let c = |v: usize| if v == 0 { 0 } else { (55 + 40 * v) as u8 };
        (c(n / 36), c(n / 6 % 6), c(n % 6))
    } else {
        let v = (8 + 10 * (n.min(255) - 232)) as u8;
        (v, v, v)
    };
    rgb(r, g, b)
}
#[derive(Default)]
pub struct Ui {
    screen: Screen,
    payload: Option<Value>,
    thread: String,
    sequence: Option<u64>,
    status: String,
    grid: (usize, usize),
    pending: Vec<u64>,
}
pub fn attach(app: &mut App) {
    let Some((thread, cwd)) = super::context(app) else {
        return;
    };
    if app.ui.panel.terminal.thread == thread {
        return;
    }
    if let Some(payload) = app.ui.panel.terminal.payload.take() {
        if let Some(n) = app.src.net() {
            n.unwatch_resource("terminal.attach", payload);
        }
    }
    let pending = std::mem::take(&mut app.ui.panel.terminal.pending);
    app.ui.panel.terminal = Ui::default();
    app.ui.panel.terminal.pending = pending;
    let payload = json!({"threadId":thread,"terminalId":"term-1","cwd":cwd,"cols":80,"rows":40,"resume":true});
    app.ui.panel.terminal.thread = thread;
    app.ui.panel.terminal.grid = (80, 40);
    app.ui.panel.terminal.screen.size(80, 40);
    if let Some(n) = app.src.net() {
        n.watch_resource("terminal.attach", payload.clone());
    }
    app.ui.panel.terminal.payload = Some(payload);
}
pub fn poll(app: &mut App) -> bool {
    let pending = std::mem::take(&mut app.ui.panel.terminal.pending);
    let mut reply_changed = false;
    for id in pending {
        match app.src.net().and_then(|net| net.take_reply(id)) {
            None => app.ui.panel.terminal.pending.push(id),
            Some(Err(error)) => {
                app.ui.panel.terminal.status = error.to_string();
                reply_changed = true;
            }
            Some(Ok(_)) => {}
        }
    }

    let Some(payload) = app.ui.panel.terminal.payload.clone() else {
        return reply_changed;
    };
    let events = app
        .src
        .net()
        .map(|n| n.take_resource_events("terminal.attach", payload))
        .unwrap_or_default();
    let changed = !events.is_empty() || reply_changed;
    for event in events {
        apply(&mut app.ui.panel.terminal, &event);
    }
    changed
}
fn apply(ui: &mut Ui, event: &Value) {
    if !event["error"].is_null() {
        ui.status = event["error"].to_string();
        return;
    }
    let sequence = event["sequence"].as_u64();
    let kind = data::s(event, "type");
    if kind != "snapshot" && sequence.is_some_and(|s| ui.sequence.is_some_and(|old| s <= old)) {
        return;
    }
    match kind {
        "snapshot" | "restarted" => {
            let snapshot = &event["snapshot"];
            ui.screen = Screen::default();
            ui.screen.size(
                snapshot["cols"].as_u64().unwrap_or(80) as usize,
                snapshot["rows"].as_u64().unwrap_or(40) as usize,
            );
            ui.screen.feed(data::s(snapshot, "history"));
            ui.status = data::s(snapshot, "status").into();
            ui.sequence = snapshot["sequence"].as_u64();
        }
        "activity" => {
            let cols = event["cols"]
                .as_u64()
                .map(|v| v as usize)
                .unwrap_or(ui.screen.cols);
            let rows = event["rows"]
                .as_u64()
                .map(|v| v as usize)
                .unwrap_or(ui.screen.rows);
            ui.screen.size(cols, rows);
        }
        "output" => ui.screen.feed(data::s(event, "data")),
        "cleared" => {
            ui.screen.lines.clear();
            ui.screen.x = 0;
            ui.screen.y = 0;
        }
        "exited" | "closed" => ui.status = kind.into(),
        "error" => ui.status = data::s(event, "message").into(),
        _ => {}
    }
    if sequence.is_some() {
        ui.sequence = sequence;
    }
}
fn send(app: &mut App, data: String) {
    let thread = app.ui.panel.terminal.thread.clone();
    if let Some(n) = app.src.net() {
        let id = n.request(
            "terminal.write",
            json!({"threadId":thread,"terminalId":"term-1","data":data}),
        );
        app.ui.panel.terminal.pending.push(id);
    }
}
fn key(app: &mut App, e: &KeyEvent, _: &mut Tx) -> bool {
    if super::shortcut(app, e, "terminal.toggle") {
        super::toggle_terminal(app);
        return true;
    }
    if super::shortcut(app, e, "rightPanel.toggle") {
        app.ui.shell.panel_open = !app.ui.shell.panel_open;
        return true;
    }

    let data = match &e.key {
        Key::Text(v) => {
            if e.mods.ctrl {
                v.chars()
                    .next()
                    .filter(|c| c.is_ascii_alphabetic())
                    .map(|c| ((c.to_ascii_lowercase() as u8) - b'a' + 1) as char)
                    .map(|c| c.to_string())
                    .unwrap_or_default()
            } else {
                v.clone()
            }
        }
        Key::Command(c) if e.mods.ctrl && !e.mods.shift && c.is_ascii_alphabetic() => {
            send(
                app,
                ((c.to_ascii_lowercase() as u8 - b'a' + 1) as char).to_string(),
            );
            return true;
        }
        Key::Enter => "\r".into(),
        Key::Backspace => "\x7f".into(),
        Key::Tab => "\t".into(),
        Key::Escape => "\x1b".into(),
        Key::Up => "\x1b[A".into(),
        Key::Down => "\x1b[B".into(),
        Key::Right => "\x1b[C".into(),
        Key::Left => "\x1b[D".into(),
        _ => return false,
    };
    if e.mods.cmd {
        return false;
    }
    send(app, data);
    true
}
pub fn view(app: &App, t: &T) -> El {
    let ui = &app.ui.panel.terminal;
    let size = app.client_settings.values["fontSizeTerminal"]
        .as_f64()
        .unwrap_or(12.) as f32;
    let font = theme::mono(rune::TextStyle::new(size, 400.).lh(size * 1.2));
    let mut lines = col().w_full();
    for (i, line) in ui.screen.lines.iter().enumerate() {
        let mut r = row().h(size * 1.2).key(format!("terminal-line-{i}"));
        // Coalesce cells into paint runs so a long command doesn't create a node per character.
        let mut start = 0;
        while start < line.len() {
            let cell = line[start];
            let mut end = start + 1;
            while end < line.len() && line[end].fg == cell.fg && line[end].bg == cell.bg {
                end += 1;
            }
            let value: String = line[start..end].iter().map(|c| c.ch).collect();
            let mut run = text(value)
                .font(font)
                .color(cell.fg.unwrap_or(t.c.contrast_foreground));
            if let Some(bg) = cell.bg {
                run = run.fill(bg);
            }
            r = r.child(run);
            start = end;
        }
        lines = lines.child(r);
    }
    lines = lines.min_h((ui.screen.y + 1) as f32 * size * 1.2).child(
        el().absolute(
            ui.screen.x as f32 * size * 0.6,
            ui.screen.y as f32 * size * 1.2,
        )
        .size(size * 0.6, size * 1.2)
        .border(1., t.c.contrast_foreground),
    );
    if ui.screen.lines.is_empty() {
        lines = lines.child(text(" ").font(font));
    }
    col().grow(1.).min_h(0.).child(row().h(32.).px(8.).center_y().child(text(format!("Terminal  {}",ui.status)).font(theme::TEXT_XS).color(t.c.muted_foreground).grow(1.))
        .child(kit::Button::new("Clear terminal").icon(&icons::TRASH_2).variant("ghost-muted").size("icon-xs").on_press(|s:&mut App|{let thread=s.ui.panel.terminal.thread.clone();if let Some(n)=s.src.net(){let id=n.request("terminal.clear",json!({"threadId":thread,"terminalId":"term-1"}));s.ui.panel.terminal.pending.push(id);}s.ui.panel.terminal.screen.lines.clear();s.ui.panel.terminal.screen.x=0;s.ui.panel.terminal.screen.y=0;}).build(t)))
        .child(el().grow(1.).min_h(0.).w_full().key("panel-terminal-grid").padding(8.).scroll(ScrollSpec {stick_bottom: true}).focusable(true).role("textbox").label("Terminal input").on_key(key).on_paste(|s:&mut App,v,_|send(s,v.into())).on_layout(move |s:&mut App,r|{
            let grid=(((r[2]-16.)/(size*0.6)).floor().clamp(1.,1000.) as usize,((r[3]-16.)/(size*1.2)).floor().clamp(1.,500.) as usize);
            if s.ui.panel.terminal.grid!=grid {s.ui.panel.terminal.grid=grid;s.ui.panel.terminal.screen.size(grid.0,grid.1);let thread=s.ui.panel.terminal.thread.clone();if let Some(n)=s.src.net(){let id=n.request("terminal.resize",json!({"threadId":thread,"terminalId":"term-1","cols":grid.0,"rows":grid.1}));s.ui.panel.terminal.pending.push(id);}}
        }).child(lines))
}
pub fn fixture(app: &mut App) {
    let v: Value =
        serde_json::from_str(include_str!("../../fixtures/panel/terminal.json")).unwrap();
    apply(&mut app.ui.panel.terminal, &v);
}
#[cfg(test)]
mod tests {
    use super::*;
    fn plain(s: &Screen) -> String {
        s.lines
            .iter()
            .map(|l| l.iter().map(|c| c.ch).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
    #[test]
    fn fragmented_shell_output_overwrites_and_erases() {
        let mut s = Screen::default();
        s.size(80, 24);
        s.feed("old progress\r\x1b[");
        s.feed("2K\x1b[32mready\x1b[0m\r\nλ $ ");
        assert_eq!(plain(&s), "ready\nλ $ ");
        assert_eq!(s.lines[0][0].fg, Some(ansi(2)));
        assert_eq!(s.lines[1][0].fg, None);
        s.feed("\x1b]0;private title\x07echo");
        assert_eq!(plain(&s), "ready\nλ $ echo");
    }
    #[test]
    fn snapshot_then_ordered_events_and_resume() {
        let mut ui = Ui::default();
        apply(
            &mut ui,
            &json!({"type":"snapshot","snapshot":{"history":"$ ","cols":80,"rows":24,"sequence":10}}),
        );
        for seq in [11, 11, 9, 12] {
            apply(&mut ui, &json!({"type":"output","sequence":seq,"data":"x"}));
        }
        assert_eq!(plain(&ui.screen), "$ xx");
        apply(
            &mut ui,
            &json!({"type":"activity","sequence":13,"cols":100,"rows":30}),
        );
        assert_eq!((ui.screen.cols, ui.screen.rows), (100, 30));
        apply(
            &mut ui,
            &json!({"type":"snapshot","snapshot":{"history":"resumed","sequence":20}}),
        );
        assert_eq!(plain(&ui.screen), "resumed");
    }
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color {
        r: rune::color::srgb_decode(r as f32 / 255.),
        g: rune::color::srgb_decode(g as f32 / 255.),
        b: rune::color::srgb_decode(b as f32 / 255.),
        a: 1.,
    }
}

pub fn close(app: &mut App) {
    let ui = &mut app.ui.panel.terminal;
    if let Some(net) = app.src.net() {
        let id = net.request(
            "terminal.close",
            json!({"threadId":ui.thread,"terminalId":"term-1"}),
        );
        ui.pending.push(id);
        if let Some(payload) = ui.payload.take() {
            net.unwatch_resource("terminal.attach", payload);
        }
    }
    let pending = std::mem::take(&mut ui.pending);
    *ui = Ui::default();
    ui.pending = pending;
}
