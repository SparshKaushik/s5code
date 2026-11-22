//! Electron's `CaptureShortcutConfig`, `captureConfigEdit` and `captureConfigKdl`: on Hyprland and
//! Niri the capture shortcut is a binding in the compositor's own config. The user reviews the
//! exact change before it is written, a backup is kept, and the file must not change in between.

use std::path::{Path, PathBuf};

pub const NIRI_PATH: &str = "/com/t3tools/SnapShot";
pub const NIRI_INTERFACE: &str = "com.t3tools.SnapShot";
/// The GlobalShortcuts id Hyprland's `global` dispatcher names.
pub const HYPRLAND_ACTION: &str = "capture-window";
const DEFAULT_KEYS: &str = "Ctrl+Shift+2";
const MAX_BYTES: u64 = 1024 * 1024;
const MAX_FILES: usize = 64;
const KEYS_ERROR: &str = "Choose a letter, number, or function key with Ctrl, Alt, or Super.";
const KDL_ERROR: &str = "Couldn't read this Niri config. Use Advanced to configure it manually.";
const CHANGED: &str = "Your config changed since this preview. Nothing was saved. Review changes again before saving.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Desktop {
    Niri,
    Hyprland,
}

impl Desktop {
    pub fn name(self) -> &'static str {
        match self {
            Desktop::Niri => "Niri",
            Desktop::Hyprland => "Hyprland",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Niri,
    Hyprland,
    HyprlandLua,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Install,
    Remove,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keys {
    pub label: String,
    pub modifiers: String,
    pub key: String,
    pub mask: u32,
}

/// `captureConfigKeys` over `parseKeybindingShortcut`, with Super read as Meta.
pub fn keys(input: &str) -> Result<Keys, String> {
    let lowered = input.to_lowercase().replace("super", "meta");
    let mut tokens: Vec<&str> = lowered.split('+').map(str::trim).collect();
    let mut trailing = false;
    while tokens.last() == Some(&"") {
        tokens.pop();
        trailing = true;
    }
    if trailing {
        tokens.push("+");
    }
    if tokens.is_empty() || tokens.iter().any(|t| t.is_empty()) {
        return Err(KEYS_ERROR.into());
    }
    let (mut ctrl, mut alt, mut shift, mut meta, mut modifier) = (false, false, false, false, false);
    let mut key: Option<&str> = None;
    for token in tokens {
        match token {
            "cmd" | "meta" => meta = true,
            "ctrl" | "control" => ctrl = true,
            "shift" => shift = true,
            "alt" | "option" => alt = true,
            "mod" => modifier = true,
            other if key.is_none() => key = Some(other),
            _ => return Err(KEYS_ERROR.into()),
        }
    }
    let key = key.ok_or(KEYS_ERROR)?;
    let function = key.strip_prefix('f').filter(|n| !n.starts_with('0')).and_then(|n| n.parse::<u8>().ok()).filter(|n| (1..=24).contains(n));
    let plain = key.len() == 1 && key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    if modifier || !(plain || function.is_some()) || !(ctrl || alt || meta) {
        return Err(KEYS_ERROR.into());
    }
    let names: Vec<&str> = [(ctrl, "Ctrl"), (alt, "Alt"), (shift, "Shift"), (meta, "Super")].into_iter().filter(|(on, _)| *on).map(|(_, n)| n).collect();
    let key = key.to_uppercase();
    Ok(Keys {
        label: names.iter().copied().chain([key.as_str()]).collect::<Vec<_>>().join("+"),
        modifiers: names.iter().map(|n| n.to_uppercase()).collect::<Vec<_>>().join(" "),
        mask: u32::from(shift) | u32::from(ctrl) << 2 | u32::from(alt) << 3 | u32::from(meta) << 6,
        key,
    })
}

fn same_keys(left: &str, right: &str) -> bool {
    matches!((keys(left), keys(right)), (Ok(a), Ok(b)) if a.label == b.label)
}

/// `niriCaptureBinding`: Niri runs `gdbus`, which calls the endpoint this app serves.
pub fn niri_binding(app_id: &str) -> String {
    format!(
        "{DEFAULT_KEYS} repeat=false {{ spawn \"gdbus\" \"call\" \"--session\" \"--dest\" \"{app_id}.SnapShot\" \"--object-path\" \"{NIRI_PATH}\" \"--method\" \"{NIRI_INTERFACE}.Capture\"; }}"
    )
}

/// `captureConfigBinding`.
pub fn binding(format: Format, app_id: &str, chord: &str) -> Result<String, String> {
    if app_id.is_empty() || !app_id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')) {
        return Err("Invalid capture application ID.".into());
    }
    let chord = keys(chord)?;
    let action = format!("{app_id}:{HYPRLAND_ACTION}");
    Ok(match format {
        Format::Niri => niri_binding(app_id).replace(DEFAULT_KEYS, &chord.label),
        Format::HyprlandLua => format!("hl.bind(\"{}\", hl.dsp.global(\"{action}\"))", chord.label.to_uppercase().replace('+', " + ")),
        Format::Hyprland => format!("bind = {}, {}, global, {action}", chord.modifiers, chord.key),
    })
}

/// The binding to show for manual setup on this desktop.
pub fn manual_binding(desktop: Desktop, app_id: &str, lua: bool) -> String {
    match desktop {
        Desktop::Niri => niri_binding(app_id),
        Desktop::Hyprland => binding(if lua { Format::HyprlandLua } else { Format::Hyprland }, app_id, DEFAULT_KEYS).unwrap_or_default(),
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Token {
    value: String,
    start: usize,
    end: usize,
    quoted: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct Node {
    name: String,
    header: Vec<Token>,
    children: Vec<Node>,
    start: usize,
    end: usize,
    close: Option<usize>,
}

fn kdl_space(c: char) -> bool {
    matches!(c, '\t' | ' ' | '\r' | '\u{FEFF}' | '\u{A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}')
}

/// `readKdlNodes`: enough KDL to find `binds` and `include`, refusing anything it can't read.
fn kdl(source: &str) -> Result<Vec<Node>, String> {
    let invalid = || KDL_ERROR.to_owned();
    let bytes = source.as_bytes();
    let len = source.len();
    let mut tokens = Vec::new();
    let mut offset = 0;
    while offset < len {
        let start = offset;
        let rest = &source[offset..];
        let c = rest.chars().next().unwrap_or(' ');
        if kdl_space(c) {
            offset += c.len_utf8();
            continue;
        }
        if rest.starts_with("//") {
            offset = rest.find('\n').map_or(len, |i| offset + i);
            continue;
        }
        if rest.starts_with("/*") {
            offset += 2;
            let mut depth = 1;
            while depth > 0 && offset < len {
                if source[offset..].starts_with("/*") {
                    depth += 1;
                    offset += 2;
                } else if source[offset..].starts_with("*/") {
                    depth -= 1;
                    offset += 2;
                } else {
                    offset += source[offset..].chars().next().map_or(1, char::len_utf8);
                }
            }
            if depth > 0 {
                return Err(invalid());
            }
            continue;
        }
        if c == '\\' {
            let after = &rest[1..];
            let trimmed = after.trim_start_matches(|c: char| c != '\u{FEFF}' && kdl_space(c));
            if trimmed.starts_with('\n') {
                offset += 1 + (after.len() - trimmed.len()) + 1;
                continue;
            }
        }
        let hashes = rest.strip_prefix('r').map(|r| r.len() - r.trim_start_matches('#').len());
        if let Some(hashes) = hashes.filter(|&h| h <= 32 && rest[1 + h..].starts_with('"')) {
            let terminator = format!("\"{}", "#".repeat(hashes));
            let body = offset + 2 + hashes;
            let end = source[body..].find(&terminator).map(|i| body + i).ok_or_else(invalid)?;
            offset = end + terminator.len();
            tokens.push(Token { value: source[body..end].to_owned(), start, end: offset, quoted: true });
            continue;
        }
        if c == '"' {
            offset += 1;
            while offset < len && bytes[offset] != b'"' {
                if bytes[offset] == b'\\' {
                    offset += 1;
                }
                offset += 1;
            }
            if offset >= len {
                return Err(invalid());
            }
            offset += 1;
            // KDL's unicode escapes differ from JSON; names needing those stay on the manual path.
            let value: String = serde_json::from_str(&source[start..offset]).map_err(|_| invalid())?;
            tokens.push(Token { value, start, end: offset, quoted: true });
            continue;
        }
        if rest.starts_with("/-") {
            offset += 2;
        } else if "{};\n=()".contains(c) {
            offset += 1;
        } else {
            while offset < len {
                let here = &source[offset..];
                let ch = here.chars().next().unwrap_or(' ');
                if ch.is_whitespace() || "{};=\"()".contains(ch) || ["//", "/*", "/-"].iter().any(|p| here.starts_with(p)) {
                    break;
                }
                offset += ch.len_utf8();
            }
        }
        if offset == start {
            return Err(invalid());
        }
        tokens.push(Token { value: source[start..offset].to_owned(), start, end: offset, quoted: false });
    }
    let mut cursor = 0;
    nodes(&tokens, &mut cursor, false)
}

fn is(token: Option<&Token>, value: &str) -> bool {
    token.is_some_and(|t| !t.quoted && t.value == value)
}

fn nodes(tokens: &[Token], cursor: &mut usize, nested: bool) -> Result<Vec<Node>, String> {
    let invalid = || KDL_ERROR.to_owned();
    let mut result = Vec::new();
    while *cursor < tokens.len() {
        if is(tokens.get(*cursor), "\n") || is(tokens.get(*cursor), ";") {
            *cursor += 1;
            continue;
        }
        if is(tokens.get(*cursor), "}") {
            return if nested { Ok(result) } else { Err(invalid()) };
        }
        let disabled = is(tokens.get(*cursor), "/-");
        if disabled {
            *cursor += 1;
        }
        let name = tokens.get(*cursor).ok_or_else(invalid)?.clone();
        *cursor += 1;
        if ["{", "}", "=", ";", "\n", "("].contains(&name.value.as_str()) {
            return Err(invalid());
        }
        let mut node = Node { name: name.value.clone(), start: name.start, end: name.end, header: vec![name], children: Vec::new(), close: None };
        while *cursor < tokens.len() && !is(tokens.get(*cursor), "\n") && !is(tokens.get(*cursor), ";") && !is(tokens.get(*cursor), "}") {
            let token = tokens[*cursor].clone();
            *cursor += 1;
            if is(Some(&token), "{") {
                node.children = nodes(tokens, cursor, true)?;
                let close = tokens.get(*cursor).filter(|t| !t.quoted && t.value == "}").ok_or_else(invalid)?;
                *cursor += 1;
                node.close = Some(close.start);
                node.end = close.end;
                break;
            }
            node.end = token.end;
            node.header.push(token);
        }
        if !disabled {
            result.push(node);
        }
    }
    if nested { Err(invalid()) } else { Ok(result) }
}

fn niri_binds(source: &str) -> Result<Option<Node>, String> {
    let blocks: Vec<Node> = kdl(source)?.into_iter().filter(|n| n.name == "binds").collect();
    if blocks.len() > 1 || blocks.iter().any(|n| n.close.is_none()) {
        return Err("This Niri config has an unexpected binds section. Check it in Advanced.".into());
    }
    Ok(blocks.into_iter().next())
}

fn header_values(node: &Node) -> Vec<&str> {
    node.header.iter().map(|t| t.value.as_str()).collect()
}

fn niri_capture_node(node: &Node, app_id: &str) -> bool {
    let Ok(Some(expected)) = kdl(&niri_binding(app_id)).map(|n| n.into_iter().next().and_then(|n| n.children.into_iter().next())) else { return false };
    matches!(node.children.as_slice(), [action] if action.children.is_empty() && header_values(action) == header_values(&expected))
}

struct Bind {
    start: usize,
    end: usize,
    keys: String,
    action: String,
}

/// The `bind… = mods, key, dispatcher, arg` line, with its trailing `#` comment.
fn legacy_bind(line: &str) -> Option<(String, String)> {
    let rest = line.trim_start().strip_prefix("bind")?;
    let rest = rest.trim_start_matches(|c: char| c.is_ascii_lowercase()).trim_start().strip_prefix('=')?;
    let mut parts = rest.splitn(4, ',');
    let (mods, key, dispatcher, arg) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    let arg = arg.split('#').next().unwrap_or("").trim();
    let mods = mods.split_whitespace().collect::<Vec<_>>().join("+");
    let action = if dispatcher.trim() == "global" { arg.to_owned() } else { String::new() };
    Some((format!("{mods}+{}", key.trim()), action))
}

fn quoted(s: &str) -> Option<(&str, &str)> {
    let quote = s.chars().next().filter(|c| matches!(c, '"' | '\''))?;
    let body = &s[1..];
    let end = body.find(quote)?;
    Some((&body[..end], &body[end + 1..]))
}

/// `hl.bind("KEYS", hl.dsp.global("ACTION"))`, with an optional `;` and `--` comment.
fn lua_bind(line: &str) -> Option<(String, String)> {
    let rest = line.trim_start().strip_prefix("hl.bind(")?.trim_start();
    let (keys, rest) = quoted(rest)?;
    let rest = rest.trim_start().strip_prefix(',')?.trim_start().strip_prefix("hl.dsp.global(")?.trim_start();
    let (action, rest) = quoted(rest)?;
    let rest = rest.trim_start().strip_prefix(')')?.trim_start().strip_prefix(')')?.trim_start();
    let rest = rest.strip_prefix(';').unwrap_or(rest).trim_start();
    (rest.is_empty() || rest.starts_with("--")).then(|| (keys.to_owned(), action.to_owned()))
}

fn hyprland_binds(source: &str, lua: bool) -> Vec<Bind> {
    let mut binds = Vec::new();
    let mut start = 0;
    for line in source.split_inclusive('\n') {
        let parsed = if lua { lua_bind(line.trim_end()) } else { legacy_bind(line.trim_end()) };
        if let Some((keys, action)) = parsed {
            binds.push(Bind { start, end: start + line.len(), keys, action });
        }
        start += line.len();
    }
    binds
}

/// `niriConfigIncludes`: each include's path and whether it is optional.
pub fn niri_includes(source: &str) -> Result<Vec<(String, bool)>, String> {
    kdl(source)?
        .iter()
        .filter(|n| n.name == "include")
        .map(|node| {
            let h = &node.header;
            let path = (1..h.len()).find(|&i| h[i].quoted && h[i - 1].value != "=").map(|i| h[i].value.clone());
            let path = path.ok_or("Couldn't resolve a Niri include. Choose the config in Advanced.")?;
            let optional = (0..h.len()).any(|i| h[i].value == "optional" && h.get(i + 1).is_some_and(|t| t.value == "=") && h.get(i + 2).is_some_and(|t| t.value == "true"));
            Ok((path, optional))
        })
        .collect()
}

/// `niriConfigConflict`: another binding already uses these keys.
pub fn niri_conflict(source: &str, app_id: &str, chord: &str) -> Result<bool, String> {
    Ok(niri_binds(source)?.is_some_and(|b| b.children.iter().any(|n| same_keys(&n.name, chord) && !niri_capture_node(n, app_id))))
}

/// `removeNodes`: whole lines go when nothing else shares them.
fn remove_ranges(source: &str, ranges: &[(usize, usize)]) -> String {
    let mut sorted = ranges.to_vec();
    sorted.sort_by(|a, b| b.0.cmp(&a.0));
    let mut result = source.to_owned();
    for (start, end) in sorted {
        let line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
        let line_end = source[end..].find('\n').map_or(source.len(), |i| end + i);
        let whole = source[line_start..start].trim().is_empty() && source[end..line_end].trim().is_empty();
        let (from, to) = if whole { (line_start, (line_end + 1).min(source.len())) } else { (start, end) };
        result.replace_range(from..to, "");
    }
    result
}

fn lua_needs_manual_edit(source: &str) -> bool {
    let early_return = source.lines().any(|l| l.trim_start().strip_prefix("return").is_some_and(|r| !r.starts_with(|c: char| c.is_alphanumeric() || c == '_')));
    let long_bracket = source.match_indices('[').any(|(i, _)| source[i + 1..].trim_start_matches('=').starts_with('['));
    early_return || long_bracket
}

/// `editCaptureConfig`: the config with exactly one capture binding (or none), and its keys.
pub fn edit(source: &str, format: Format, app_id: &str, operation: Operation, requested: Option<&str>) -> Result<(String, String), String> {
    if format == Format::HyprlandLua && lua_needs_manual_edit(source) {
        return Err("This Lua config needs a manual edit. Choose your bindings file or use manual setup in Advanced.".into());
    }
    let niri = format == Format::Niri;
    let action = format!("{app_id}:{HYPRLAND_ACTION}");
    let existing: Vec<(usize, usize, String)> = if niri {
        niri_binds(source)?.map(|b| b.children).unwrap_or_default().into_iter().filter(|n| niri_capture_node(n, app_id)).map(|n| (n.start, n.end, n.name)).collect()
    } else {
        hyprland_binds(source, format == Format::HyprlandLua).into_iter().filter(|b| b.action == action).map(|b| (b.start, b.end, b.keys)).collect()
    };
    let chord = keys(requested.or(existing.first().map(|e| e.2.as_str())).unwrap_or(DEFAULT_KEYS))?.label;
    if operation == Operation::Install && existing.len() == 1 && same_keys(&existing[0].2, &chord) {
        return Ok((source.to_owned(), chord));
    }
    let mut after = remove_ranges(source, &existing.iter().map(|e| (e.0, e.1)).collect::<Vec<_>>());
    if operation == Operation::Remove {
        return Ok((after, chord));
    }
    let conflict = if niri { niri_conflict(&after, app_id, &chord)? } else { hyprland_binds(&after, false).iter().any(|b| same_keys(&b.keys, &chord)) };
    if conflict {
        return Err(format!("{chord} is already used in this config. Choose another shortcut."));
    }
    let newline = if source.contains("\r\n") { "\r\n" } else { "\n" };
    let line = binding(format, app_id, &chord)?;
    let separator = if !after.is_empty() && !after.ends_with('\n') { newline } else { "" };
    if niri {
        match niri_binds(&after)?.and_then(|b| b.close) {
            Some(close) => {
                let line_start = after[..close].rfind('\n').map_or(0, |i| i + 1);
                let indent = after[line_start..close].to_owned();
                after = if indent.trim().is_empty() {
                    format!("{}{indent}    {line}{newline}{}", &after[..line_start], &after[line_start..])
                } else {
                    format!("{}{newline}    {line}{newline}{}", &after[..close], &after[close..])
                };
            }
            None => after = format!("{after}{separator}{newline}binds {{{newline}    {line}{newline}}}{newline}"),
        }
        kdl(&after)?;
    } else {
        // Never inside a Lua block or string, or after an early return: appended at the end.
        after = format!("{after}{separator}{newline}{line}{newline}");
    }
    Ok((after, chord))
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map_or_else(|| home().join(".config"), PathBuf::from)
}

/// `niriCaptureConfigPath` and `hyprlandCaptureShortcut`'s config: a Lua setup and a separate
/// bindings file are preferred where they exist. Also whether it's Lua.
pub fn default_path(desktop: Desktop) -> (PathBuf, bool) {
    match desktop {
        Desktop::Niri => (std::env::var_os("NIRI_CONFIG").filter(|v| !v.is_empty()).map_or_else(|| config_home().join("niri").join("config.kdl"), PathBuf::from), false),
        Desktop::Hyprland => {
            let dir = config_home().join("hypr");
            let lua = dir.join("hyprland.lua").exists();
            let bindings = if lua { "bindings.lua" } else { "bindings.conf" };
            let name = if dir.join(bindings).exists() { bindings } else if lua { "hyprland.lua" } else { "hyprland.conf" };
            (dir.join(name), lua)
        }
    }
}

/// A file as read for the preview, compared again before anything is written.
#[derive(Clone, Debug)]
pub struct Snapshot {
    path: PathBuf,
    resolved: PathBuf,
    text: String,
    bytes: Vec<u8>,
    identity: (u64, u32, u32),
}

fn read_snapshot(path: &Path) -> std::io::Result<Snapshot> {
    let resolved = std::fs::canonicalize(path)?;
    let meta = std::fs::metadata(&resolved)?;
    let too_large = || std::io::Error::other("Choose a config file smaller than 1 MB.");
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return Err(too_large());
    }
    let bytes = std::fs::read(&resolved)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(too_large());
    }
    let text = String::from_utf8(bytes.clone()).map_err(|_| std::io::Error::other("Choose a text config file."))?;
    if text.contains('\0') {
        return Err(std::io::Error::other("Choose a text config file."));
    }
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        (meta.ino(), meta.mode(), meta.uid())
    };
    #[cfg(not(unix))]
    let identity = (0, 0, 0);
    Ok(Snapshot { path: path.to_owned(), resolved, text, bytes, identity })
}

/// The proposal the user reviews; only this exact change is ever applied.
#[derive(Clone, Debug)]
pub struct Preview {
    pub desktop: Desktop,
    pub path: PathBuf,
    pub resolved: PathBuf,
    pub before: String,
    pub after: String,
    pub shortcut: String,
    pub operation: Operation,
    files: Vec<Snapshot>,
    missing: Vec<PathBuf>,
    check_keys: bool,
    app_id: String,
}

impl Preview {
    pub fn changed(&self) -> bool {
        self.before != self.after
    }
}

/// A reviewed install with no files to check again, for scenes.
pub fn scene_preview(desktop: Desktop, path: &str, before: &str, after: &str, shortcut: &str) -> Preview {
    let path = PathBuf::from(path);
    Preview { desktop, resolved: path.clone(), path, before: before.into(), after: after.into(), shortcut: shortcut.into(), operation: Operation::Install, files: Vec::new(), missing: Vec::new(), check_keys: false, app_id: String::new() }
}

fn run(program: &str, args: &[&str]) -> Result<String, ()> {
    let output = std::process::Command::new(program).args(args).stdin(std::process::Stdio::null()).output().map_err(drop)?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned()).ok_or(())
}

/// `checkHyprlandKeys`: the live binds, less our own.
fn check_hyprland_keys(app_id: &str, chord: &str) -> Result<(), String> {
    let keys = keys(chord)?;
    let failed = || "Couldn't check Hyprland's current shortcuts. Try again from your Hyprland session.".to_owned();
    let binds: serde_json::Value = run("hyprctl", &["-j", "binds"]).ok().and_then(|out| serde_json::from_str(&out).ok()).ok_or_else(failed)?;
    let ours = format!("{app_id}:{HYPRLAND_ACTION}");
    let taken = binds.as_array().ok_or_else(failed)?.iter().any(|b| {
        b["modmask"].as_u64() == Some(u64::from(keys.mask)) && b["key"].as_str().is_some_and(|k| k.to_uppercase() == keys.key) && !(b["dispatcher"] == "global" && b["arg"] == ours.as_str())
    });
    if taken { Err(format!("{} is already used by Hyprland. Choose another shortcut.", keys.label)) } else { Ok(()) }
}

/// The uid that owns this process, as the owner of `/proc/self`.
fn own_uid() -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata("/proc/self").ok().map(|m| m.uid())
    }
    #[cfg(not(unix))]
    None
}

/// `CaptureShortcutConfig.preview`.
pub fn preview(desktop: Desktop, path: &Path, app_id: &str, operation: Operation, requested: Option<&str>) -> Result<Preview, String> {
    let root = read_snapshot(path).map_err(|_| "Couldn't read your settings file. Choose a different file in Advanced.".to_owned())?;
    if own_uid().is_some_and(|uid| uid != root.identity.2) || root.resolved.to_string_lossy().contains("/omarchy/default/") {
        return Err("Choose your own config, not system or Omarchy defaults.".into());
    }
    let name = path.to_string_lossy();
    let format = match desktop {
        Desktop::Niri => Format::Niri,
        Desktop::Hyprland if name.ends_with(".lua") => Format::HyprlandLua,
        Desktop::Hyprland => Format::Hyprland,
    };
    let extension = match format {
        Format::Niri => ".kdl",
        Format::HyprlandLua => ".lua",
        Format::Hyprland => ".conf",
    };
    if !name.ends_with(extension) {
        return Err("Choose a .kdl Niri config or a .conf/.lua Hyprland config.".into());
    }
    let (after, shortcut) = edit(&root.text, format, app_id, operation, requested)?;
    // Lua bindings show as opaque callbacks, including our own unchanged shortcut.
    let check_keys = operation == Operation::Install && (format == Format::Hyprland || (format == Format::HyprlandLua && after != root.text));
    let mut files = vec![root.clone()];
    let mut missing = Vec::new();
    if desktop == Desktop::Niri {
        visit(&root, 0, app_id, operation, &shortcut, &mut files, &mut missing)?;
    } else if check_keys {
        check_hyprland_keys(app_id, &shortcut)?;
    }
    Ok(Preview { desktop, path: root.path.clone(), resolved: root.resolved.clone(), before: root.text, after, shortcut, operation, files, missing, check_keys, app_id: app_id.to_owned() })
}

/// Every file Niri includes, so a conflict anywhere is found and a change anywhere is noticed.
fn visit(file: &Snapshot, depth: usize, app_id: &str, operation: Operation, chord: &str, files: &mut Vec<Snapshot>, missing: &mut Vec<PathBuf>) -> Result<(), String> {
    if depth > 10 || files.len() > MAX_FILES || files.iter().map(|f| f.bytes.len() as u64).sum::<u64>() > MAX_BYTES {
        return Err("This config has too many included files. Use manual setup in Advanced.".into());
    }
    for (include, optional) in niri_includes(&file.text)? {
        if include.contains('$') || include.contains(['*', '?', '[', ']']) {
            return Err("This config uses a dynamic include. Use manual setup in Advanced.".into());
        }
        let path = match include.strip_prefix("~/") {
            Some(rest) => home().join(rest),
            None => file.path.parent().unwrap_or(Path::new("/")).join(&include),
        };
        let child = match read_snapshot(&path) {
            Ok(child) => child,
            Err(e) if optional && e.kind() == std::io::ErrorKind::NotFound => {
                missing.push(path);
                continue;
            }
            Err(_) => return Err("Couldn't read an included Niri config. Check its location in Advanced.".into()),
        };
        if files.iter().any(|f| f.resolved == child.resolved) {
            continue;
        }
        files.push(child.clone());
        if operation == Operation::Install && niri_conflict(&child.text, app_id, chord)? {
            return Err(format!("{chord} is already used in {}. Choose another shortcut.", child.path.display()));
        }
        visit(&child, depth + 1, app_id, operation, chord, files, missing)?;
    }
    Ok(())
}

fn unchanged(preview: &Preview) -> Result<(), String> {
    if preview.missing.iter().any(|p| std::fs::symlink_metadata(p).is_ok()) {
        return Err("Your config changed since this preview. Review changes again before saving.".into());
    }
    for original in &preview.files {
        let current = read_snapshot(&original.path).map_err(|_| CHANGED.to_owned())?;
        if current.resolved != original.resolved || current.identity.0 != original.identity.0 || current.identity.1 != original.identity.1 || current.bytes != original.bytes {
            return Err(CHANGED.into());
        }
    }
    Ok(())
}

fn unique() -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    format!("{:x}{:x}", std::process::id(), nanos)
}

fn write_new(path: &Path, bytes: &[u8], mode: u32) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, mode);
    let _ = mode;
    options.open(path)?.write_all(bytes)
}

/// What saving did: the backup it kept, and a warning when Hyprland didn't reload cleanly.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Applied {
    pub backup: Option<PathBuf>,
    pub warning: Option<String>,
}

/// `CaptureShortcutConfig.apply`: stage beside the file, validate Niri's, check nothing moved,
/// back up the original, then swap it in.
pub fn apply(preview: &Preview) -> Result<Applied, String> {
    unchanged(preview)?;
    if preview.check_keys {
        check_hyprland_keys(&preview.app_id, &preview.shortcut)?;
    }
    if !preview.changed() {
        return Ok(Applied::default());
    }
    let mode = preview.files[0].identity.1 & 0o777;
    let dir = preview.resolved.parent().ok_or(CHANGED)?;
    let temporary = dir.join(format!(".t3-capture-{}.tmp", unique()));
    let failed = |_| "Couldn't save your shortcut. Review the changes and try again.".to_owned();
    write_new(&temporary, preview.after.as_bytes(), mode).map_err(failed)?;
    let result = (|| {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(mode)).map_err(failed)?;
        }
        if preview.desktop == Desktop::Niri {
            validate_niri(preview, &temporary)?;
        }
        unchanged(preview)?;
        let backup = PathBuf::from(format!("{}.t3-capture-backup-{}", preview.resolved.display(), unique()));
        write_new(&backup, &preview.files[0].bytes, 0o600).map_err(failed)?;
        std::fs::rename(&temporary, &preview.resolved).map_err(failed)?;
        let warning = (preview.desktop == Desktop::Hyprland && !reload_hyprland())
            .then(|| "Config saved, but Hyprland couldn't reload it cleanly. Check hyprctl configerrors, then run hyprctl reload.".to_owned());
        Ok(Applied { backup: Some(backup), warning })
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

/// Niri resolves includes against the chosen path, not a dotfile link's target, so a staged
/// copy is validated from the chosen file's folder.
fn validate_niri(preview: &Preview, temporary: &Path) -> Result<(), String> {
    let refused = || "Niri couldn't validate the proposed config. Nothing was changed. Check your config in Advanced.".to_owned();
    let beside = preview.path.parent().unwrap_or(Path::new("/")).join(temporary.file_name().unwrap_or_default());
    let validate = |path: &Path| run("niri", &["validate", "--config", &path.to_string_lossy()]).map(drop).map_err(|()| refused());
    if beside == temporary {
        return validate(temporary);
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(temporary, &beside).map_err(|_| refused())?;
        let result = validate(&beside);
        let _ = std::fs::remove_file(&beside);
        result
    }
    #[cfg(not(unix))]
    validate(temporary)
}

fn reload_hyprland() -> bool {
    run("hyprctl", &["reload"]).is_ok() && run("hyprctl", &["configerrors"]).is_ok_and(|errors| errors.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP: &str = "com.t3tools.T3Code";

    #[test]
    fn keys_read_like_electrons_capture_config_keys() {
        let k = keys("ctrl+shift+2").unwrap();
        assert_eq!((k.label.as_str(), k.modifiers.as_str(), k.key.as_str(), k.mask), ("Ctrl+Shift+2", "CTRL SHIFT", "2", 5));
        assert_eq!(keys("Super+Alt+F12").unwrap().label, "Alt+Super+F12");
        assert_eq!(keys("SUPER+a").unwrap().mask, 64);
        for bad in ["Shift+2", "Ctrl+Space", "Mod+2", "Ctrl+F25", "Ctrl+F01", "Ctrl+a+b", ""] {
            assert_eq!(keys(bad).unwrap_err(), KEYS_ERROR, "{bad}");
        }
        assert_eq!(binding(Format::Hyprland, APP, "ctrl+alt+k").unwrap(), "bind = CTRL ALT, K, global, com.t3tools.T3Code:capture-window");
        assert_eq!(binding(Format::HyprlandLua, APP, "ctrl+shift+2").unwrap(), r#"hl.bind("CTRL + SHIFT + 2", hl.dsp.global("com.t3tools.T3Code:capture-window"))"#);
        assert!(binding(Format::Niri, APP, "alt+3").unwrap().starts_with("Alt+3 repeat=false { spawn \"gdbus\""));
        assert!(binding(Format::Hyprland, "bad id", "ctrl+2").is_err());
    }

    #[test]
    fn hyprland_configs_gain_and_lose_one_binding() {
        let source = "$mod = SUPER\nbind = SUPER, Q, exec, kitty # terminal\n";
        let (after, chord) = edit(source, Format::Hyprland, APP, Operation::Install, None).unwrap();
        assert_eq!(chord, "Ctrl+Shift+2");
        assert_eq!(after, format!("{source}\nbind = CTRL SHIFT, 2, global, {APP}:capture-window\n"));
        assert_eq!(edit(&after, Format::Hyprland, APP, Operation::Install, None).unwrap().0, after, "already set up");
        let (moved, chord) = edit(&after, Format::Hyprland, APP, Operation::Install, Some("Ctrl+Alt+K")).unwrap();
        assert_eq!(chord, "Ctrl+Alt+K");
        assert!(moved.contains("bind = CTRL ALT, K, global") && !moved.contains("CTRL SHIFT, 2"), "{moved}");
        assert_eq!(edit(&after, Format::Hyprland, APP, Operation::Remove, None).unwrap().0, format!("{source}\n"));
        assert_eq!(edit(source, Format::Hyprland, APP, Operation::Install, Some("Super+Q")).unwrap_err(), "Super+Q is already used in this config. Choose another shortcut.");
        let lua = "hl.bind(\"SUPER + Q\", hl.exec(\"kitty\"))\n";
        let (after, _) = edit(lua, Format::HyprlandLua, APP, Operation::Install, None).unwrap();
        assert!(after.ends_with("hl.bind(\"CTRL + SHIFT + 2\", hl.dsp.global(\"com.t3tools.T3Code:capture-window\"))\n"));
        assert_eq!(edit(&after, Format::HyprlandLua, APP, Operation::Install, None).unwrap().0, after);
        assert!(edit("return {}\n", Format::HyprlandLua, APP, Operation::Install, None).is_err(), "an early return needs a manual edit");
        assert!(edit("local s = [==[x]==]\n", Format::HyprlandLua, APP, Operation::Install, None).is_err());
    }

    #[test]
    fn niri_configs_bind_inside_their_binds_block() {
        let source = "input {\n    keyboard { xkb { layout \"us\"; } }\n}\n\nbinds {\n    Mod+T { spawn \"alacritty\"; }\n}\n";
        let (after, _) = edit(source, Format::Niri, APP, Operation::Install, None).unwrap();
        assert!(after.contains(&format!("    Mod+T {{ spawn \"alacritty\"; }}\n    {}\n}}\n", niri_binding(APP))), "{after}");
        assert_eq!(edit(&after, Format::Niri, APP, Operation::Install, None).unwrap().0, after);
        assert_eq!(edit(&after, Format::Niri, APP, Operation::Remove, None).unwrap().0, source);
        let (fresh, _) = edit("input {}", Format::Niri, APP, Operation::Install, Some("Alt+3")).unwrap();
        assert!(fresh.starts_with("input {}\n\nbinds {\n    Alt+3 repeat=false"), "{fresh}");
        assert!(niri_conflict("binds {\n    Ctrl+Shift+2 { screenshot; }\n}", APP, "Ctrl+Shift+2").unwrap());
        assert!(edit("binds {\n    Ctrl+Shift+2 { screenshot; }\n}", Format::Niri, APP, Operation::Install, None).is_err());
        assert!(edit("binds {}\nbinds {}\n", Format::Niri, APP, Operation::Install, None).is_err(), "two binds blocks");
        assert!(kdl("binds { \"unterminated }").is_err());
        assert_eq!(kdl("/* a /* nested */ c */ node r#\"raw \"x\"\"# \"q\\\"s\"").unwrap()[0].header.iter().map(|t| t.value.as_str()).collect::<Vec<_>>(), ["node", "raw \"x\"", "q\"s"]);
        assert_eq!(
            niri_includes("include \"a.kdl\"\ninclude optional=true \"~/b.kdl\"\n/-include \"c.kdl\"").unwrap(),
            [("a.kdl".to_owned(), false), ("~/b.kdl".to_owned(), true)]
        );
    }

    #[test]
    fn applying_keeps_a_backup_and_refuses_a_changed_file() {
        let dir = std::env::temp_dir().join(format!("s5-capture-config-{}", unique()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bindings.lua");
        std::fs::write(&path, "hl.bind(\"SUPER + Q\", hl.exec(\"kitty\"))\n").unwrap();
        let mut preview = preview(Desktop::Hyprland, &path, APP, Operation::Remove, None).unwrap();
        assert!(!preview.changed());
        assert_eq!(apply(&preview).unwrap(), Applied::default(), "nothing to remove writes nothing");
        preview.after = "-- changed\n".into();
        std::fs::write(&path, "hl.bind(\"SUPER + W\", hl.exec(\"kitty\"))\n").unwrap();
        assert_eq!(apply(&preview).unwrap_err(), CHANGED);
        let fresh = super::preview(Desktop::Hyprland, &path, APP, Operation::Remove, None).unwrap();
        let mut staged = fresh.clone();
        staged.after = "-- removed\n".into();
        let applied = apply(&staged).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "-- removed\n");
        assert_eq!(std::fs::read_to_string(applied.backup.unwrap()).unwrap(), fresh.before);
        assert_eq!(super::preview(Desktop::Niri, &path, APP, Operation::Install, None).unwrap_err(), "Choose a .kdl Niri config or a .conf/.lua Hyprland config.");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
