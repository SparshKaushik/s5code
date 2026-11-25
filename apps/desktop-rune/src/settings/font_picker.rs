//! Electron's installed-font menu (`FontFamilyPicker`). Rune's text stack (parley/fontique without
//! its `system` backends) doesn't list system fonts, so the families come from the `name` tables of
//! the font files in the platform font folders, read once on a background thread.
use crate::theme::{AlphaMul, T};
use crate::{App, kit};
use rune::{El, row, text};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

/// Web's `DEFAULT_FONT_VALUE`: the entry that clears the setting back to the theme's family.
pub const DEFAULT: &str = "__default__";

#[derive(Clone, Debug, PartialEq)]
pub struct Family {
    pub name: String,
    pub mono: bool,
}

#[derive(Default)]
struct Scan {
    started: bool,
    families: Option<Arc<Vec<Family>>>,
}

fn scan() -> &'static Mutex<Scan> {
    static SCAN: OnceLock<Mutex<Scan>> = OnceLock::new();
    SCAN.get_or_init(Mutex::default)
}

/// The installed families once the scan has finished, starting it on first use. An empty scan
/// stays `None`, so the field keeps its text input like web's denied enumeration.
pub fn families(s: &App) -> Option<Arc<Vec<Family>>> {
    let mut state = scan().lock().unwrap_or_else(|e| e.into_inner());
    if !state.started {
        state.started = true;
        let waker = s.ui.panel.waker.clone();
        std::thread::spawn(move || {
            let found = installed();
            scan().lock().unwrap_or_else(|e| e.into_inner()).families = (!found.is_empty()).then(|| Arc::new(found));
            if let Some(waker) = waker {
                waker();
            }
        });
    }
    state.families.clone()
}

/// Scenes show the menu without depending on the capture host's fonts.
pub fn inject(families: Vec<Family>) {
    let mut state = scan().lock().unwrap_or_else(|e| e.into_inner());
    state.started = true;
    state.families = Some(Arc::new(families));
}

fn installed() -> Vec<Family> {
    let mut faces = Vec::new();
    let mut stack: Vec<(std::path::PathBuf, u8)> = super::fonts::font_dirs().into_iter().map(|d| (d, 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if depth < 4 {
                    stack.push((path, depth + 1));
                }
            } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc" | "otc")) {
                faces.extend(read_faces(&path));
            }
        }
    }
    collect(faces)
}

/// Web's `queryInstalledFontFamilies`: one entry per family, dot-prefixed system UI faces dropped,
/// sorted alphabetically. A family is fixed-width when any of its faces is.
pub fn collect(faces: impl IntoIterator<Item = Family>) -> Vec<Family> {
    let mut out: Vec<Family> = Vec::new();
    for face in faces {
        let name = face.name.trim();
        if name.is_empty() || name.starts_with('.') {
            continue;
        }
        match out.iter_mut().find(|f| f.name == name) {
            Some(known) => known.mono |= face.mono,
            None => out.push(Family { name: name.to_owned(), mono: face.mono }),
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.name.cmp(&b.name)));
    out
}

/// The picker's rows for a query, as web builds `items`: the default entry only while the query is
/// empty, then the families whose name contains it, ignoring case, in list order.
pub fn filter(options: &[kit::ComboboxOption], query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    options
        .iter()
        .enumerate()
        .filter(|(_, o)| if o.id == DEFAULT { query.is_empty() } else { o.label.to_lowercase().contains(&query) })
        .map(|(i, _)| i)
        .collect()
}

fn read_faces(path: &Path) -> Vec<Family> {
    let Ok(mut file) = std::fs::File::open(path) else { return Vec::new() };
    faces(&mut file).unwrap_or_default()
}

fn faces<R: Read + Seek>(file: &mut R) -> Option<Vec<Family>> {
    let head = at(file, 0, 12)?;
    let offsets = if &head[..4] == b"ttcf" {
        let count = u32_at(&head, 8)?.min(256);
        let table = at(file, 12, count as usize * 4)?;
        (0..count as usize).filter_map(|i| u32_at(&table, i * 4)).map(u64::from).collect()
    } else {
        vec![0]
    };
    Some(offsets.into_iter().filter_map(|offset| face(file, offset)).collect())
}

/// One face's family (typographic family, else family) and whether it is fixed-width: `post`
/// `isFixedPitch`, or `OS/2` PANOSE's Latin-text "monospaced" proportion.
fn face<R: Read + Seek>(file: &mut R, offset: u64) -> Option<Family> {
    let header = at(file, offset, 12)?;
    let tables = u16_at(&header, 4)?.min(512) as usize;
    let records = at(file, offset + 12, tables * 16)?;
    let table = |tag: &[u8; 4]| {
        records.chunks_exact(16).find(|r| &r[..4] == tag).and_then(|r| Some((u32_at(r, 8)? as u64, u32_at(r, 12)? as usize)))
    };
    let (name_at, name_len) = table(b"name")?;
    let name = family_name(&at(file, name_at, name_len.min(1 << 20))?)?;
    let fixed_pitch = table(b"post").and_then(|(at_, _)| at(file, at_ + 12, 4)).and_then(|b| u32_at(&b, 0)).is_some_and(|v| v != 0);
    let panose_mono = table(b"OS/2").and_then(|(at_, len)| (len >= 36).then(|| at(file, at_ + 32, 4)).flatten()).is_some_and(|p| p[0] == 2 && p[3] == 9);
    Some(Family { name, mono: fixed_pitch || panose_mono })
}

fn family_name(table: &[u8]) -> Option<String> {
    let count = u16_at(table, 2)? as usize;
    let storage = u16_at(table, 4)? as usize;
    let mut best: Option<(u8, String)> = None;
    for i in 0..count {
        let r = 6 + i * 12;
        let (platform, encoding, language, id) = (u16_at(table, r)?, u16_at(table, r + 2)?, u16_at(table, r + 4)?, u16_at(table, r + 6)?);
        let (len, start) = (u16_at(table, r + 8)? as usize, u16_at(table, r + 10)? as usize);
        let rank = match (id, platform, language) {
            (16, 3, 0x409) => 0,
            (16, 3 | 0, _) => 1,
            (16, 1, 0) => 2,
            (1, 3, 0x409) => 3,
            (1, 3 | 0, _) => 4,
            (1, 1, 0) => 5,
            _ => continue,
        };
        if best.as_ref().is_some_and(|(b, _)| *b <= rank) {
            continue;
        }
        let Some(bytes) = table.get(storage + start..storage + start + len) else { continue };
        let name = if platform == 1 && encoding == 0 {
            bytes.iter().map(|&b| if b.is_ascii() { b as char } else { '\u{FFFD}' }).collect()
        } else {
            let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
            String::from_utf16_lossy(&units)
        };
        if !name.trim().is_empty() {
            best = Some((rank, name.trim().to_owned()));
        }
    }
    best.map(|(_, name)| name)
}

fn at<R: Read + Seek>(file: &mut R, offset: u64, len: usize) -> Option<Vec<u8>> {
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut buf = vec![0; len];
    file.read_exact(&mut buf).ok()?;
    Some(buf)
}

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// The fields that must stay fixed-width (`requireMonospace`).
pub fn requires_mono(key: &str) -> bool {
    matches!(key, "fontFamilyCode" | "fontFamilyTerminal")
}

/// The family trigger, or with `layer` the open menu anchored to it. `default_family` names the
/// family an empty setting falls back to.
pub fn picker(s: &App, t: &T, key: &str, title: &str, default_family: &str, families: &[Family], layer: &mut Vec<El>) -> El {
    let selected = super::value(s, key).as_str().unwrap_or("").trim().to_owned();
    let mut options = vec![kit::ComboboxOption::new(DEFAULT, default_family)];
    options.extend(families.iter().map(|f| kit::ComboboxOption::new(f.name.clone(), f.name.clone())));
    let state = s.ui.settings.font_pickers.get(key).cloned().unwrap_or_default();
    let k = kit::theme(t);
    let trigger = kit::Select::new::<&str>(format!("font-picker-{key}-value"), &[])
        .current(Some(if selected.is_empty() { default_family } else { &selected }))
        .label(format!("{title} family"))
        .width(176.0)
        .size("sm")
        .trigger(&k);
    let (owned, access_key) = (key.to_owned(), key.to_owned());
    let mono: Vec<String> = families.iter().filter(|f| f.mono).map(|f| f.name.clone()).collect();
    let tag = t.c.muted_foreground.alpha_mul(0.6);
    kit::Combobox::new(format!("font-picker-{key}"), &state, &options, move |s: &mut App| s.ui.settings.font_pickers.entry(access_key.clone()).or_default())
        .label(format!("{title} family"))
        .current(Some(if selected.is_empty() { DEFAULT } else { &selected }))
        .search_placeholder("Search fonts…")
        .empty_message("No fonts found.")
        .width(288.0)
        .max_height(288.0)
        .placement(rune_kit::Placement::BottomEnd)
        .filter(filter)
        .trigger(trigger)
        .trailing(move |o| (o.id == DEFAULT).then(|| row().child(text("default").font(crate::theme::TEXT_3XS).color(tag))))
        .on_choose(move |s: &mut App, id, _| {
            if let Some(state) = s.ui.settings.font_pickers.get_mut(&owned) {
                state.close();
            }
            if id != DEFAULT && requires_mono(&owned) && !mono.iter().any(|m| m == id) {
                crate::shell::palette::toast_error(s, &format!("\"{id}\" isn't monospace"), Some("Code and terminal need a fixed-width font, so the current font was kept."));
                return;
            }
            super::update(s, &owned, serde_json::json!(if id == DEFAULT { "" } else { id }));
        })
        .build(&k, layer)
}

/// The open menu, from the overlay pass: the trigger in the row anchors it.
pub fn overlays(s: &App, t: &T, layer: &mut Vec<El>) {
    let Some((key, _)) = s.ui.settings.font_pickers.iter().find(|(_, state)| state.open) else { return };
    let Some(families) = families(s) else { return };
    let Some(spec) = super::pages::specs()
        .as_object()
        .into_iter()
        .flat_map(|p| p.values())
        .flat_map(|p| p.as_array().into_iter().flatten())
        .flat_map(|sec| sec["rows"].as_array().into_iter().flatten())
        .find(|r| r["key"].as_str() == Some(key) && r["kind"] == "font" && super::controls::visible(s, r))
    else {
        return;
    };
    let title = spec["title"].as_str().unwrap_or("");
    let default_family = super::controls::default_family(s, spec);
    let _trigger = picker(s, t, key, title, &default_family, &families, layer);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fam(name: &str, mono: bool) -> Family {
        Family { name: name.into(), mono }
    }

    #[test]
    fn collect_dedups_drops_dot_faces_and_sorts_like_web() {
        let got = collect([fam("Menlo", false), fam(".SF NS", false), fam("arial", false), fam("Menlo", true), fam("  ", false), fam("Zapfino", false), fam("Courier", true)]);
        assert_eq!(got, vec![fam("arial", false), fam("Courier", true), fam("Menlo", true), fam("Zapfino", false)]);
    }

    #[test]
    fn filter_shows_default_only_without_a_query_and_matches_substrings() {
        let options: Vec<_> = [(DEFAULT, "SF Mono"), ("Fira Code", "Fira Code"), ("JetBrains Mono", "JetBrains Mono"), ("Monaco", "Monaco")]
            .into_iter()
            .map(|(id, label)| kit::ComboboxOption::new(id, label))
            .collect();
        assert_eq!(filter(&options, ""), vec![0, 1, 2, 3]);
        assert_eq!(filter(&options, "  "), vec![0, 1, 2, 3]);
        // The default entry's label is the fallback family; web never searches it.
        assert_eq!(filter(&options, "mono"), vec![2]);
        assert_eq!(filter(&options, "MON"), vec![2, 3]);
        // A substring, not web's subsequence fuzzy match elsewhere.
        assert_eq!(filter(&options, "fc"), Vec::<usize>::new());
    }

    fn name_table(records: &[(u16, u16, u16, u16, &[u8])]) -> Vec<u8> {
        let mut head = vec![0, 0];
        head.extend((records.len() as u16).to_be_bytes());
        head.extend((6 + records.len() as u16 * 12).to_be_bytes());
        let mut storage = Vec::new();
        for (platform, encoding, language, id, bytes) in records {
            for v in [*platform, *encoding, *language, *id, bytes.len() as u16, storage.len() as u16] {
                head.extend(v.to_be_bytes());
            }
            storage.extend_from_slice(bytes);
        }
        head.extend(storage);
        head
    }

    fn utf16(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_be_bytes).collect()
    }

    fn font(tables: &[(&[u8; 4], Vec<u8>)], base: u32) -> Vec<u8> {
        let mut out = vec![0, 1, 0, 0];
        out.extend((tables.len() as u16).to_be_bytes());
        out.extend([0; 6]);
        let mut data = Vec::new();
        let start = base + 12 + tables.len() as u32 * 16;
        for (tag, bytes) in tables {
            out.extend_from_slice(*tag);
            out.extend([0; 4]);
            out.extend((start + data.len() as u32).to_be_bytes());
            out.extend((bytes.len() as u32).to_be_bytes());
            data.extend(bytes);
        }
        out.extend(data);
        out
    }

    fn post(fixed: bool) -> Vec<u8> {
        let mut b = vec![0; 32];
        b[12..16].copy_from_slice(&(fixed as u32).to_be_bytes());
        b
    }

    #[test]
    fn reads_the_typographic_family_and_fixed_pitch() {
        let english = utf16("Iosevka Term");
        let typographic = utf16("Iosevka");
        let name = name_table(&[(1, 0, 0, 1, b"Mac Name"), (3, 1, 0x409, 1, &english), (3, 1, 0x409, 16, &typographic)]);
        let bytes = font(&[(b"name", name), (b"post", post(true))], 0);
        assert_eq!(faces(&mut std::io::Cursor::new(bytes)), Some(vec![fam("Iosevka", true)]));
    }

    #[test]
    fn reads_every_face_of_a_collection_and_panose_mono() {
        let mut os2 = vec![0; 40];
        os2[32] = 2;
        os2[35] = 9;
        let sans = font(&[(b"name", name_table(&[(3, 1, 0x409, 1, &utf16("Plex Sans"))])), (b"post", post(false))], 20);
        let mono = font(&[(b"name", name_table(&[(1, 0, 0, 1, b"Plex Mono")])), (b"OS/2", os2)], 20 + sans.len() as u32);
        let mut bytes = b"ttcf".to_vec();
        bytes.extend([0, 1, 0, 0]);
        bytes.extend(2u32.to_be_bytes());
        bytes.extend(20u32.to_be_bytes());
        bytes.extend((20 + sans.len() as u32).to_be_bytes());
        bytes.extend(sans);
        bytes.extend(mono);
        assert_eq!(faces(&mut std::io::Cursor::new(bytes)), Some(vec![fam("Plex Sans", false), fam("Plex Mono", true)]));
    }
}
