//! URLs and file paths in terminal text (`terminal-links.ts`), found by hand rather than by
//! regex: `https?://…` runs, and paths that start with `~/`, `./`, `../`, `/` or a drive, or
//! word segments joined by slashes, with an optional `:line:col`. Positions are char indices.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Url,
    Path,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Match {
    pub kind: Kind,
    pub text: String,
    pub start: usize,
    pub end: usize,
}

pub fn is_url(text: &str) -> bool {
    let lower = text.get(..8).unwrap_or(text).to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// `[^\s"'`<>]`: what a URL or a prefixed path runs over.
fn link_char(c: char) -> bool {
    !c.is_whitespace() && !matches!(c, '"' | '\'' | '`' | '<' | '>')
}

/// `[\p{L}\p{M}\p{N}._-]`: one segment of a bare relative path.
fn segment_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '.' | '_' | '-')
}

fn starts_with(chars: &[char], at: usize, prefix: &str) -> bool {
    let mut i = at;
    for p in prefix.chars() {
        if chars.get(i).is_none_or(|c| !c.eq_ignore_ascii_case(&p)) {
            return false;
        }
        i += 1;
    }
    true
}

/// Drops trailing punctuation (and a path's trailing colon), then closers without an opener.
fn trim(text: &str, kind: Kind) -> &str {
    let punct: &[char] = match kind {
        Kind::Url => &['.', ',', ';', '!', '?'],
        Kind::Path => &['.', ',', ';', ':', '!', '?'],
    };
    let mut out = text.trim_end_matches(punct);
    for (open, close) in [('(', ')'), ('[', ']'), ('{', '}')] {
        while out.ends_with(close) && out.matches(close).count() > out.matches(open).count() {
            out = &out[..out.len() - 1];
        }
    }
    out
}

/// The end of a prefixed path (`~/`, `./`, `../`, `/`, `C:\`) starting at `at`, if one does.
fn prefixed_path(chars: &[char], at: usize) -> Option<usize> {
    let c = chars[at];
    let sep = |i: usize| chars.get(i).is_some_and(|c| matches!(c, '/' | '\\'));
    let prefix = if c == '/' {
        1
    } else if c == '~' && sep(at + 1) {
        2
    } else if c == '.' && sep(at + 1) {
        2
    } else if c == '.' && chars.get(at + 1) == Some(&'.') && sep(at + 2) {
        3
    } else if c.is_ascii_alphabetic() && chars.get(at + 1) == Some(&':') && sep(at + 2) {
        3
    } else {
        return None;
    };
    let mut end = at + prefix;
    while end < chars.len() && link_char(chars[end]) {
        end += 1;
    }
    (end > at + prefix).then_some(end)
}

/// The end of `seg(/seg)+(:digits){0,2}` starting at `at`, if one does.
fn bare_path(chars: &[char], at: usize) -> Option<usize> {
    let segment = |mut i: usize| {
        let start = i;
        while i < chars.len() && segment_char(chars[i]) {
            i += 1;
        }
        (i > start).then_some(i)
    };
    let mut end = segment(at)?;
    let mut segments = 1;
    while end < chars.len() && matches!(chars[end], '/' | '\\') {
        match segment(end + 1) {
            Some(next) => {
                end = next;
                segments += 1;
            }
            None => break,
        }
    }
    if segments < 2 {
        return None;
    }
    for _ in 0..2 {
        let digits = chars[(end + 1).min(chars.len())..].iter().take_while(|c| c.is_ascii_digit()).count();
        if chars.get(end) == Some(&':') && digits > 0 {
            end += 1 + digits;
        } else {
            break;
        }
    }
    Some(end)
}

/// Every link in a line, in order.
pub fn extract(line: &str) -> Vec<Match> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if !link_char(c) {
            i += 1;
            continue;
        }
        let (kind, raw_end) = if starts_with(&chars, i, "http://") || starts_with(&chars, i, "https://") {
            let mut end = i;
            while end < chars.len() && link_char(chars[end]) {
                end += 1;
            }
            (Kind::Url, end)
        } else if let Some(end) = scheme_url(&chars, i) {
            // `redis://host/0` and `file:///a` are not paths: skip the whole run.
            i = end;
            continue;
        } else if let Some(end) = prefixed_path(&chars, i) {
            (Kind::Path, end)
        } else if (i == 0 || !segment_char(chars[i - 1])) && let Some(end) = bare_path(&chars, i) {
            (Kind::Path, end)
        } else {
            i += 1;
            continue;
        };
        let raw: String = chars[i..raw_end].iter().collect();
        let text = trim(&raw, kind);
        if !text.is_empty() && !(kind == Kind::Path && text.contains("://")) {
            let len = text.chars().count();
            out.push(Match { kind, text: text.to_string(), start: i, end: i + len });
        }
        i = raw_end;
    }
    out
}

/// The end of a `scheme://…` run (other than http and https) starting at `at`.
fn scheme_url(chars: &[char], at: usize) -> Option<usize> {
    let start = if matches!(chars[at], '+' | '-') { at + 1 } else { at };
    let scheme = chars[start..].iter().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')).count();
    if scheme < 2 || !chars.get(start)?.is_ascii_alphabetic() || !starts_with(chars, start + scheme, "://") {
        return None;
    }
    let mut end = start + scheme + 3;
    while end < chars.len() && link_char(chars[end]) {
        end += 1;
    }
    Some(end)
}

/// `splitFilePathPosition`: the path without a trailing `:line[:col]`.
pub fn strip_position(path: &str) -> &str {
    let mut out = path;
    for _ in 0..2 {
        match out.rsplit_once(':') {
            Some((head, digits)) if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) => out = head,
            _ => break,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(line: &str) -> Vec<(Kind, String)> {
        extract(line).into_iter().map(|m| (m.kind, m.text)).collect()
    }

    #[test]
    fn urls_trim_punctuation_and_unbalanced_closers() {
        assert_eq!(texts("see https://example.com/a)."), [(Kind::Url, "https://example.com/a".into())]);
        assert_eq!(texts("(https://x.dev/a_(b))"), [(Kind::Url, "https://x.dev/a_(b)".into())]);
        assert_eq!(texts("open HTTP://Host:3000/?q=1, then"), [(Kind::Url, "HTTP://Host:3000/?q=1".into())]);
    }

    #[test]
    fn paths_with_positions_prefixes_and_boundaries() {
        assert_eq!(texts("src/main.rs:12:3: error"), [(Kind::Path, "src/main.rs:12:3".into())]);
        assert_eq!(texts("at ./foo/bar.ts and ../up"), [(Kind::Path, "./foo/bar.ts".into()), (Kind::Path, "../up".into())]);
        assert_eq!(texts("(/usr/bin/env)"), [(Kind::Path, "/usr/bin/env".into())]);
        assert_eq!(texts("~/notes.md"), [(Kind::Path, "~/notes.md".into())]);
        assert_eq!(texts("\"apps/web/src\" done"), [(Kind::Path, "apps/web/src".into())]);
        assert!(texts("plain words only, no.links").is_empty());
        assert!(texts("redis://host/0 file:///a.mjs").is_empty());
    }

    #[test]
    fn positions_are_char_indices() {
        let m = &extract("é → src/a.rs")[0];
        assert_eq!((m.start, m.end), (4, 12));
        assert_eq!(strip_position("src/a.rs:12:3"), "src/a.rs");
        assert_eq!(strip_position("src/a.rs"), "src/a.rs");
    }
}
