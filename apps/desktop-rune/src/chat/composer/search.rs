//! Model picker search across every provider (modelPickerSearch.ts over searchRanking.ts): each
//! query token takes its best match over the model's fields, earlier fields ranking higher.

pub struct Searchable<'a> {
    pub name: &'a str,
    pub short_name: &'a str,
    pub sub_provider: &'a str,
    pub driver: &'a str,
    pub provider_name: &'a str,
    pub favorite: bool,
}

const FAVORITE_BOOST: i64 = 24;
const EXACT_NAME: i64 = -1000;
const BOUNDARY_MARKERS: [char; 4] = [' ', '-', '_', '/'];

fn normalize(s: &str) -> String {
    s.trim().to_lowercase()
}

fn collapsed(s: &str) -> String {
    normalize(s).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// scoreSubsequenceMatch with `ignorePosition`: gaps between matched characters, then the span.
fn subsequence(value: &str, query: &[char]) -> Option<i64> {
    let (mut matched, mut first, mut previous, mut gaps) = (0usize, None, None::<usize>, 0i64);
    for (i, c) in value.chars().enumerate() {
        if c != query[matched] {
            continue;
        }
        let start = *first.get_or_insert(i);
        if let Some(p) = previous {
            gaps += (i - p - 1) as i64;
        }
        previous = Some(i);
        matched += 1;
        if matched == query.len() {
            return Some(gaps * 3 + (i - start + 1 - query.len()) as i64);
        }
    }
    None
}

/// scoreQueryMatch with `ignorePosition`: exact, prefix, after a word boundary, anywhere, then
/// (for three or more characters) in order with gaps.
fn field_score(value: &str, token: &str, base: i64) -> Option<i64> {
    if value.is_empty() {
        return None;
    }
    if value == token {
        return Some(base);
    }
    if value.starts_with(token) {
        return Some(base + 2);
    }
    if BOUNDARY_MARKERS.iter().any(|m| value.contains(&format!("{m}{token}"))) {
        return Some(base + 4);
    }
    if value.contains(token) {
        return Some(base + 6);
    }
    let chars: Vec<char> = token.chars().collect();
    if chars.len() >= 3 {
        return subsequence(value, &chars).map(|s| base + 100 + s);
    }
    None
}

/// Lower is better; None when some token matches nothing.
pub fn score(m: &Searchable, query: &str) -> Option<i64> {
    let tokens: Vec<&str> = query.split_whitespace().collect();
    if tokens.is_empty() {
        return Some(0);
    }
    let query = tokens.join(" ").to_lowercase();
    if query == collapsed(m.name) || query == collapsed(m.short_name) {
        return Some(EXACT_NAME);
    }
    let parts = [m.name, m.short_name, m.sub_provider, m.driver, m.provider_name];
    let mut fields = vec![normalize(m.name)];
    fields.extend([m.short_name, m.sub_provider].iter().filter(|v| !v.is_empty()).map(|v| normalize(v)));
    fields.push(normalize(m.driver));
    fields.push(normalize(m.provider_name));
    fields.push(normalize(&parts.iter().filter(|v| !v.is_empty()).copied().collect::<Vec<_>>().join(" ")));
    let mut total = 0;
    for token in query.split(' ') {
        total += fields.iter().enumerate().filter_map(|(i, f)| field_score(f, token, i as i64 * 10)).min()?;
    }
    Some(if m.favorite { total - FAVORITE_BOOST } else { total })
}

/// Indices of the matching models, best first; ties go to favorites, then the original order.
pub fn rank(models: &[Searchable], query: &str) -> Vec<usize> {
    let mut ranked: Vec<(i64, bool, usize)> = models.iter().enumerate().filter_map(|(i, m)| Some((score(m, query)?, !m.favorite, i))).collect();
    ranked.sort();
    ranked.into_iter().map(|(_, _, i)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model<'a>(driver: &'a str, provider_name: &'a str, name: &'a str, sub_provider: &'a str) -> Searchable<'a> {
        Searchable { name, short_name: "", sub_provider, driver, provider_name, favorite: false }
    }

    fn catalog() -> Vec<Searchable<'static>> {
        let mut out = Vec::new();
        let family = |driver: &'static str, provider: &'static str, names: &[&'static str], out: &mut Vec<Searchable<'static>>| {
            out.extend(names.iter().map(|&n| model(driver, provider, n, "")));
        };
        family("claudeAgent", "Claude", &[
            "Claude Opus 5.5", "Claude Sonnet 5.5", "Claude Fable 5.1", "Claude Fable 5", "Claude Opus 5", "Claude Opus 4.8",
            "Claude Opus 4.7", "Claude Opus 4.6", "Claude Opus 4.5", "Claude Sonnet 5", "Claude Sonnet 4.6", "Claude Haiku 5.5",
            "Claude Haiku 4.5",
        ], &mut out);
        family("codex", "Codex", &["GPT-6.1 Sol", "GPT-6 Astra", "GPT-6 Luna", "GPT-6 Sol", "GPT-5.6 Sol", "GPT-5.6 Terra", "GPT-5.6 Luna", "GPT-5.5"], &mut out);
        family("grok", "Grok", &["Grok 4.7", "Grok 4.6", "Grok 4.5"], &mut out);
        family("antigravity", "Antigravity", &[
            "Gemini 3.8 Flash (High)", "Gemini 3.8 Flash (Medium)", "Gemini 3.8 Flash (Low)", "Gemini 3.7 Flash (High)",
            "Gemini 3.7 Flash (Medium)", "Gemini 3.7 Flash (Low)", "Gemini 3.6 Flash (High)", "Gemini 3.6 Flash (Medium)",
            "Gemini 3.6 Flash (Low)", "Gemini 3.1 Pro (High)", "Gemini 3.1 Pro (Low)",
        ], &mut out);
        out.extend(["Kimi K3", "Kimi K2.5", "Kimi K2"].map(|n| model("opencode", "OpenCode", n, "Moonshot")));
        out
    }

    fn search<'a>(models: &'a [Searchable], query: &str) -> Vec<&'a str> {
        rank(models, query).into_iter().map(|i| models[i].name).collect()
    }

    #[test]
    fn exact_tokens_beat_fuzzy_ones_and_a_favorite_never_beats_a_better_match() {
        let copilot = model("opencode", "opencode", "Claude Opus 4.7", "GitHub Copilot");
        assert!(score(&copilot, "copilot opus").unwrap() < score(&copilot, "coplt op").unwrap());
        let favorite = Searchable { favorite: true, ..model("claudeAgent", "Claude", "Claude Opus 4.7", "") };
        let exact = model("cursor", "Cursor", "Opus 4.7", "");
        assert!(score(&exact, "opus 4.7").unwrap() < score(&favorite, "opus 4.7").unwrap());
    }

    #[test]
    fn ranks_the_catalog_like_the_web_picker() {
        let models = catalog();
        assert_eq!(search(&models, "opus 4"), ["Claude Opus 4.8", "Claude Opus 4.7", "Claude Opus 4.6", "Claude Opus 4.5"]);
        assert_eq!(search(&models, "opus 5"), ["Claude Opus 5.5", "Claude Opus 5", "Claude Opus 4.5"]);
        assert_eq!(search(&models, "opus 5."), ["Claude Opus 5.5"]);
        assert_eq!(search(&models, "gpt-6 sol")[..2], ["GPT-6 Sol", "GPT-6.1 Sol"]);
        assert_eq!(search(&models, "claude opus 5")[..2], ["Claude Opus 5", "Claude Opus 5.5"]);
        assert_eq!(search(&models, "Grok 4.6")[0], "Grok 4.6");
        assert_eq!(search(&models, "moonshot"), ["Kimi K3", "Kimi K2.5", "Kimi K2"]);
        assert!(search(&models, "zzz").is_empty());
    }

    #[test]
    fn a_favorite_leads_its_family() {
        let mut models = catalog();
        models.iter_mut().find(|m| m.name == "Claude Opus 4.5").unwrap().favorite = true;
        assert_eq!(search(&models, "opus")[..3], ["Claude Opus 4.5", "Claude Opus 5.5", "Claude Opus 5"]);
    }
}
