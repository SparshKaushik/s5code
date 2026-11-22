//! Markdown list editing in the plain-text prompt (composer-list-continuation.ts): Enter continues
//! a `-`, `*`, `+`, `- [ ]` or `1.` item and ends an empty one, Tab nests an item and Shift+Tab
//! lifts it, renumbering the ordered items that follow. Offsets are bytes; `pills` are the
//! prompt's chips, whose text never starts an item and is never rewritten.

use std::ops::Range;

/// The changes to make, in order and never overlapping (each `(start, end, replacement)` in the
/// original text), and where the caret lands once they are made.
#[derive(Debug, PartialEq)]
pub struct Edit {
    pub changes: Vec<(usize, usize, String)>,
    pub caret: usize,
}

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Ordered { number: String, delimiter: char },
    Task,
    Bullet(char),
}

#[derive(Clone, Debug, PartialEq)]
struct Marker {
    kind: Kind,
    indent: String,
    space: String,
}

struct Parsed {
    marker: Marker,
    end: usize,
    /// The column a nested item must reach to belong to this one.
    content_column: usize,
}

/// Markdown tabs advance to the next four-column stop.
fn indent_width(line: &str, from: usize) -> usize {
    let mut column = from;
    for c in line.chars() {
        match c {
            ' ' => column += 1,
            '\t' => column += 4 - column % 4,
            _ => break,
        }
    }
    column - from
}

/// CommonMark content follows 1-4 columns of padding; wider padding is code.
fn content_column(indent: &str, marker_width: usize, padding: &str) -> usize {
    let end = indent_width(indent, 0) + marker_width;
    let space = indent_width(padding, end);
    end + if (1..=4).contains(&space) { space } else { 1 }
}

/// The padding after a marker (`(?:[ \t]+)|\s*$`): spaces and tabs, or whitespace to the end.
fn padding(rest: &str) -> Option<&str> {
    let n = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    if n > 0 {
        Some(&rest[..n])
    } else {
        rest.trim().is_empty().then_some(rest)
    }
}

fn parse(line: &str) -> Option<Parsed> {
    let indent = &line[..line.len() - line.trim_start_matches([' ', '\t']).len()];
    let rest = &line[indent.len()..];
    let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits > 0
        && let Some(delimiter) = rest[digits..].chars().next().filter(|c| matches!(c, '.' | ')'))
        && let Some(space) = padding(&rest[digits + 1..])
    {
        let number = rest[..digits].to_owned();
        return Some(Parsed {
            end: indent.len() + digits + 1 + space.len(),
            content_column: content_column(indent, digits + 1, space),
            marker: Marker { kind: Kind::Ordered { number, delimiter }, indent: indent.to_owned(), space: space.to_owned() },
        });
    }
    // The checkbox is item content, so a task's content starts after "- ".
    let mut chars = rest.chars();
    if chars.next() == Some('-')
        && let Some(gap) = chars.next().filter(|c| c.is_whitespace())
        && chars.next() == Some('[')
        && chars.next().is_some_and(|c| matches!(c, ' ' | 'x' | 'X'))
        && chars.next() == Some(']')
    {
        let head = 1 + gap.len_utf8() + 3;
        if let Some(space) = padding(&rest[head..]) {
            return Some(Parsed {
                end: indent.len() + head + space.len(),
                content_column: content_column(indent, 1, &gap.to_string()),
                marker: Marker { kind: Kind::Task, indent: indent.to_owned(), space: space.to_owned() },
            });
        }
    }
    let bullet = rest.chars().next().filter(|c| matches!(c, '-' | '*' | '+'))?;
    let space = padding(&rest[1..])?;
    Some(Parsed {
        end: indent.len() + 1 + space.len(),
        content_column: content_column(indent, 1, space),
        marker: Marker { kind: Kind::Bullet(bullet), indent: indent.to_owned(), space: space.to_owned() },
    })
}

/// Number.isSafeInteger.
const MAX_SAFE: u64 = (1 << 53) - 1;

fn safe(number: &str) -> Option<u64> {
    number.parse::<u64>().ok().filter(|n| *n <= MAX_SAFE)
}

/// nextOrderedMarkerText's number: one more, keeping zero padding.
fn next_number(number: &str) -> String {
    match safe(number) {
        Some(n) => format!("{:0width$}", n + 1, width = number.len()),
        None => number.to_owned(),
    }
}

fn next_marker(m: &Marker) -> String {
    match &m.kind {
        Kind::Ordered { number, delimiter } => format!("{}{}{delimiter} ", m.indent, next_number(number)),
        Kind::Task => format!("{}- [ ] ", m.indent),
        Kind::Bullet(b) => format!("{}{b} ", m.indent),
    }
}

fn inside_pill(pills: &[Range<usize>], at: usize) -> bool {
    pills.iter().any(|p| at > p.start && at < p.end)
}

fn current_line(value: &str, at: usize) -> (usize, usize) {
    let start = value[..at].rfind('\n').map_or(0, |i| i + 1);
    let end = value[at..].find('\n').map_or(value.len(), |i| at + i);
    (start, end)
}

/// The list items directly above the line at `line_start`, nearest first, until a line that is
/// not one. A newline inside a chip does not start a line.
fn markers_above(value: &str, mut line_start: usize, pills: &[Range<usize>]) -> Vec<Parsed> {
    let mut out = Vec::new();
    let mut next = pills.len();
    while line_start > 0 {
        let line_end = line_start - 1;
        line_start = value[..line_end].rfind('\n').map_or(0, |i| i + 1);
        while next > 0 {
            let pill = &pills[next - 1];
            if pill.start >= line_start {
                next -= 1;
            } else if pill.end > line_start {
                line_start = value[..pill.start].rfind('\n').map_or(0, |i| i + 1);
            } else {
                break;
            }
        }
        let Some(parsed) = parse(&value[line_start..line_end]) else { break };
        out.push(parsed);
    }
    out
}

fn parent(value: &str, line_start: usize, m: &Marker, pills: &[Range<usize>]) -> Option<Marker> {
    let width = indent_width(&m.indent, 0);
    markers_above(value, line_start, pills).into_iter().find(|a| indent_width(&a.marker.indent, 0) < width).map(|a| a.marker)
}

/// An ordered item's new number once it moves under `previous`: the number after it, else 1.
/// Returns the length of the number it replaces and the new one.
fn nested_number(m: &Marker, previous: Option<&Marker>) -> (usize, String) {
    let Kind::Ordered { number, delimiter } = &m.kind else { return (0, String::new()) };
    let text = match previous.map(|p| &p.kind) {
        Some(Kind::Ordered { number: n, delimiter: d }) if d == delimiter => next_number(n),
        _ => "1".to_owned(),
    };
    (number.len(), text)
}

/// renumberFollowingItems: the ordered siblings after the line ending at `after`, numbered on
/// from `m`'s next number.
fn renumber(value: &str, after: usize, m: &Marker, pills: &[Range<usize>]) -> Vec<(usize, usize, String)> {
    let Kind::Ordered { number, delimiter } = &m.kind else { return Vec::new() };
    let base = indent_width(&m.indent, 0);
    let Some(mut expected) = safe(number).map(|n| n + 2) else { return Vec::new() };
    let mut out = Vec::new();
    let mut position = after;
    let mut next = 0;
    while position < value.len() && expected <= MAX_SAFE {
        let line_start = position + 1;
        let line_end = value[line_start..].find('\n').map_or(value.len(), |i| line_start + i);
        position = line_end;
        while pills.get(next).is_some_and(|p| p.end <= line_start) {
            next += 1;
        }
        if pills.get(next).is_some_and(|p| p.start < line_start) {
            continue;
        }
        let line = &value[line_start..line_end];
        let width = indent_width(line, 0);
        if line.trim().is_empty() || width < base {
            break;
        }
        if width > base {
            continue;
        }
        let Some(Parsed { marker: Marker { kind: Kind::Ordered { number: current, delimiter: d }, indent, .. }, .. }) = parse(line) else { break };
        if d != *delimiter {
            break;
        }
        let start = line_start + indent.len();
        let replacement = if current.starts_with('0') { format!("{expected:0width$}", width = current.len()) } else { expected.to_string() };
        if replacement != current {
            out.push((start, start + current.len(), replacement));
        }
        expected += 1;
    }
    out
}

/// listContinuationForEnter.
pub fn enter(value: &str, start: usize, end: usize, pills: &[Range<usize>]) -> Option<Edit> {
    let cursor = start;
    if start != end || cursor > value.len() {
        return None;
    }
    let (line_start, line_end) = current_line(value, cursor);
    let parsed = parse(&value[line_start..line_end])?;
    if parsed.marker.space.is_empty() {
        return None;
    }
    let marker_end = line_start + parsed.end;
    if cursor < marker_end || inside_pill(pills, cursor) {
        return None;
    }
    if value[marker_end..line_end].trim().is_empty() {
        let up = parent(value, line_start, &parsed.marker, pills);
        let replacement = up.as_ref().map(next_marker).unwrap_or_default();
        let caret = line_start + replacement.len();
        let mut changes = vec![(line_start, cursor.max(marker_end), replacement)];
        if let Some(up) = &up {
            changes.extend(renumber(value, line_end, up, pills));
        }
        return Some(Edit { changes, caret });
    }
    let insertion = format!("\n{}", next_marker(&parsed.marker));
    let caret = cursor + insertion.len();
    let mut changes = vec![(cursor, cursor, insertion)];
    changes.extend(renumber(value, line_end, &parsed.marker, pills));
    Some(Edit { changes, caret })
}

/// listIndentForTab: nests the item under the one above at its content column, or two spaces in.
pub fn indent(value: &str, start: usize, end: usize, pills: &[Range<usize>]) -> Option<Edit> {
    if start != end || start > value.len() {
        return None;
    }
    let (line_start, line_end) = current_line(value, start);
    let m = parse(&value[line_start..line_end])?.marker;
    if inside_pill(pills, start) {
        return None;
    }
    let width = indent_width(&m.indent, 0);
    let mut previous: Option<Marker> = None;
    for above in markers_above(value, line_start, pills) {
        let above_width = indent_width(&above.marker.indent, 0);
        if above_width < width {
            break;
        }
        if above_width > width {
            if previous.as_ref().is_none_or(|p| above_width < indent_width(&p.indent, 0)) {
                previous = Some(above.marker);
            }
            continue;
        }
        let indent = previous.as_ref().map_or_else(|| format!("{}{}", above.marker.indent, " ".repeat(above.content_column - above_width)), |p| p.indent.clone());
        let (replaced, number) = nested_number(&m, previous.as_ref());
        let replaced_end = line_start + m.indent.len() + replaced;
        let caret = if start < replaced_end { line_start + indent.len() } else { start - replaced_end + line_start + indent.len() + number.len() };
        return Some(Edit { changes: vec![(line_start, replaced_end, format!("{indent}{number}"))], caret });
    }
    Some(Edit { changes: vec![(line_start, line_start, "  ".to_owned())], caret: start + 2 })
}

/// listOutdentForTab: lifts the item to its parent's indent, or undoes one step of Tab's
/// two-space fallback.
pub fn outdent(value: &str, start: usize, end: usize, pills: &[Range<usize>]) -> Option<Edit> {
    if start != end || start > value.len() {
        return None;
    }
    let (line_start, line_end) = current_line(value, start);
    let m = parse(&value[line_start..line_end])?.marker;
    if m.indent.is_empty() || inside_pill(pills, start) {
        return None;
    }
    let up = parent(value, line_start, &m, pills);
    let indent = match &up {
        Some(p) => p.indent.clone(),
        None => {
            let lifted = m.indent.strip_prefix('\t').or_else(|| m.indent.strip_prefix("  ")).or_else(|| m.indent.strip_prefix(' '));
            lifted.unwrap_or(&m.indent).to_owned()
        }
    };
    let continues = match (up.as_ref().map(|p| &p.kind), &m.kind) {
        (Some(Kind::Ordered { delimiter: a, .. }), Kind::Ordered { delimiter: b, .. }) => a == b,
        _ => false,
    };
    let (replaced, number) = if continues { nested_number(&m, up.as_ref()) } else { (0, String::new()) };
    let replaced_end = line_start + m.indent.len() + replaced;
    let caret = if start < replaced_end { line_start + indent.len() } else { start - replaced_end + line_start + indent.len() + number.len() };
    let mut changes = vec![(line_start, replaced_end, format!("{indent}{number}"))];
    if let Some(up) = up.filter(|_| continues) {
        changes.extend(renumber(value, line_end, &up, pills));
    }
    Some(Edit { changes, caret })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The text after `f` at the caret marked `|`, with the caret marked again.
    fn run(f: fn(&str, usize, usize, &[Range<usize>]) -> Option<Edit>, marked: &str) -> Option<String> {
        let at = marked.find('|').unwrap();
        let value = marked.replacen('|', "", 1);
        let edit = f(&value, at, at, &[])?;
        let mut out = value.clone();
        for (start, end, with) in edit.changes.iter().rev() {
            out.replace_range(*start..*end, with);
        }
        out.insert(edit.caret, '|');
        Some(out)
    }

    #[test]
    fn enter_continues_bullets_tasks_and_numbers_and_ends_an_empty_item() {
        assert_eq!(run(enter, "- one|").as_deref(), Some("- one\n- |"));
        assert_eq!(run(enter, "  * one|").as_deref(), Some("  * one\n  * |"));
        assert_eq!(run(enter, "- [x] done|").as_deref(), Some("- [x] done\n- [ ] |"));
        assert_eq!(run(enter, "9. nine|").as_deref(), Some("9. nine\n10. |"));
        assert_eq!(run(enter, "09) nine|").as_deref(), Some("09) nine\n10) |"));
        assert_eq!(run(enter, "- |").as_deref(), Some("|"), "an empty item ends the list");
        assert_eq!(run(enter, "- a\n  - |").as_deref(), Some("- a\n- |"), "an empty nested item steps out");
        assert_eq!(run(enter, "1. a|\n2. b\n3. c").as_deref(), Some("1. a\n2. |\n3. b\n4. c"), "the items after renumber");
        assert_eq!(run(enter, "plain text|"), None);
        assert_eq!(run(enter, "-|"), None, "a bare marker is not an item yet");
        assert_eq!(run(enter, "-| one"), None, "the caret inside the marker");
        assert_eq!(run(enter, "-x|"), None);
    }

    #[test]
    fn tab_nests_under_the_item_above_and_shift_tab_lifts_it() {
        assert_eq!(run(indent, "- a\n- b|").as_deref(), Some("- a\n  - b|"));
        assert_eq!(run(indent, "1. a\n2. b|").as_deref(), Some("1. a\n   1. b|"), "a nested ordered item restarts at 1");
        assert_eq!(run(indent, "1. a\n   1. x\n2. b|").as_deref(), Some("1. a\n   1. x\n   2. b|"), "and continues its new siblings");
        assert_eq!(run(indent, "- only|").as_deref(), Some("  - only|"), "no item above: two spaces");
        assert_eq!(run(indent, "text|"), None);
        assert_eq!(run(outdent, "- a\n  - b|").as_deref(), Some("- a\n- b|"));
        assert_eq!(run(outdent, "1. a\n   1. b|\n2. c").as_deref(), Some("1. a\n2. b|\n3. c"), "lifted into the parent's numbering");
        assert_eq!(run(outdent, "  - lone|").as_deref(), Some("- lone|"));
        assert_eq!(run(outdent, "- top|"), None, "nothing to lift");
    }

    #[test]
    fn chips_neither_start_items_nor_take_edits() {
        assert!(enter("- see Term\n1 here", 4, 4, &[2..12]).is_none(), "the caret inside a chip");
        // A chip whose label spans a newline: its second line belongs to the item above.
        let value = "1. see Te\nrm\n2. b";
        let edit = indent(value, value.len(), value.len(), &[7..12]).unwrap();
        assert_eq!(edit.changes, [(13, 14, "   1".to_owned())]);
    }
}
