//! Web's file-type icons (`PierreEntryIcon` over `resolvePierreIconForEntry`) for every surface
//! that names a file. The data comes from `tools/gen-file-icons.mjs`.

mod data;

use rune::{Color, El, Icon};

/// `resolvePierreIconForEntry(path, "file")`: the icon and its colour token.
fn resolve(path: &str) -> (usize, Option<&'static str>) {
    let lower = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    let (index, token) = resolve_name(&lower);
    // Web gives the image icon its token when an extension rule picked it without one.
    match token {
        None if data::ICONS[index].name == "file-tree-builtin-image" => (index, Some("image")),
        token => (index, token),
    }
}

fn resolve_name(lower: &str) -> (usize, Option<&'static str>) {
    if let Some(i) = find(data::T3_NAMES, lower) {
        return (i, None);
    }
    // `getExtensionCandidates`: everything after each dot, longest first.
    let candidates: Vec<&str> = lower.match_indices('.').map(|(i, _)| &lower[i + 1..]).collect();
    if let Some(i) = candidates.iter().find_map(|c| find(data::T3_EXTENSIONS, c)) {
        return (i, None);
    }
    let token = find(data::NAME_TOKENS, lower)
        .or_else(|| candidates.iter().find_map(|c| find(data::EXTENSION_OVERRIDES, c).or_else(|| find(data::EXTENSION_TOKENS, c))))
        .unwrap_or("default");
    (find(data::TOKEN_ICONS, token).unwrap_or(0), Some(token))
}

fn find<T: Copy>(table: &[(&str, T)], key: &str) -> Option<T> {
    table.binary_search_by(|(k, _)| (*k).cmp(key)).ok().map(|i| table[i].1)
}

/// The icon for a file at `path`, tinted as web tints it in this appearance.
pub fn glyph(path: &str, dark: bool) -> (&'static Icon, Color) {
    let (index, token) = resolve(path);
    let (light, night) = data::FIXED[index]
        .or_else(|| find(data::TOKEN_COLORS, token.unwrap_or("default")))
        .or_else(|| find(data::TOKEN_COLORS, "default"))
        .unwrap_or(("#84848a", "#adadb1"));
    let color = rune::color::hex(if dark { night } else { light }).unwrap_or(Color::WHITE);
    (&data::ICONS[index], color)
}

/// `PierreEntryIcon` for a file: its type's icon at `size`, in its colour.
pub fn view(path: &str, size: f32, dark: bool) -> El {
    let (glyph, color) = glyph(path, dark);
    rune::icon(glyph, size).size(size, size).color(color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rule_resolves_as_web_resolves_it() {
        for (path, icon, token) in data::WEB_CASES {
            let (index, got) = resolve(path);
            assert_eq!((data::ICONS[index].name, got), (*icon, *token), "FAILED: {path} resolves differently from web");
        }
    }

    #[test]
    fn tables_are_sorted_for_lookup() {
        fn sorted<T>(table: &[(&str, T)]) -> bool {
            table.windows(2).all(|w| w[0].0 < w[1].0)
        }
        assert!(sorted(data::T3_NAMES) && sorted(data::T3_EXTENSIONS) && sorted(data::NAME_TOKENS));
        assert!(sorted(data::EXTENSION_OVERRIDES) && sorted(data::EXTENSION_TOKENS) && sorted(data::TOKEN_ICONS) && sorted(data::TOKEN_COLORS));
    }
}
