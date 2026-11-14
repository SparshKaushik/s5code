//! Web `searchSettings` over `SETTINGS_SEARCH_ITEMS` (search.json, extracted from settingsSearch.ts):
//! every query token must hit the title, the page label or a search term; title hits rank first.
use serde_json::Value;
use std::sync::OnceLock;

pub struct Item {
    pub title: String,
    pub to: String,
    pub terms: Vec<String>,
    /// The setting a result scrolls to when it lives inside another row (web `targetId`).
    target: Option<String>,
    secondary: bool,
}

/// Every setting this client has, for the command palette's own matcher.
pub fn items() -> &'static [Item] {
    static ITEMS: OnceLock<Vec<Item>> = OnceLock::new();
    ITEMS.get_or_init(|| {
        let raw: Vec<Value> = serde_json::from_str(include_str!("search.json")).unwrap();
        raw.iter()
            // Pages this client has, on this platform.
            .filter(|v| super::has_page(v["to"].as_str().unwrap_or_default()))
            .filter(|v| v["macOnly"] != true || cfg!(target_os = "macos"))
            // Connections sections this client doesn't draw.
            .filter(|v| !matches!(v["id"].as_str(), Some("load-balancing" | "pull-request-routing")))
            .map(|v| Item {
                title: v["title"].as_str().unwrap_or("").to_owned(),
                to: v["to"].as_str().unwrap_or("").to_owned(),
                terms: v["terms"].as_array().into_iter().flatten().filter_map(Value::as_str).map(normalize).collect(),
                target: v["target"].as_str().map(str::to_owned),
                secondary: v["secondary"] == true,
            })
            .collect()
    })
}

/// The title a result for `title` on `page` scrolls to.
pub fn reveal<'a>(page: &str, title: &'a str) -> &'a str {
    items()
        .iter()
        .find(|item| item.to == page && item.title == title)
        .and_then(|item| item.target.as_deref())
        .unwrap_or(title)
}

/// `normalizeSearchText`: lowercase with runs of whitespace collapsed.
pub fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

pub fn search(query: &str) -> Vec<&'static Item> {
    rank(items(), query)
}

fn rank<'a>(items: &'a [Item], query: &str) -> Vec<&'a Item> {
    let query = normalize(query);
    if query.is_empty() {
        return vec![];
    }
    let tokens: Vec<&str> = query.split(' ').collect();
    let mut hits: Vec<(usize, usize, &Item)> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let title = normalize(&item.title);
            let mut fields = vec![title.clone(), normalize(super::label(&item.to))];
            fields.extend(item.terms.iter().cloned());
            if !tokens.iter().all(|token| fields.iter().any(|field| field.contains(token))) {
                return None;
            }
            let rank = if title == query {
                5
            } else if title.starts_with(&query) {
                4
            } else if title.contains(&query) {
                3
            } else if tokens.iter().all(|token| title.contains(token)) {
                2
            } else if fields.iter().any(|field| field.contains(&query)) {
                1
            } else {
                0
            };
            Some((rank, index, item))
        })
        .collect();
    hits.sort_by(|a, b| a.2.secondary.cmp(&b.2.secondary).then(b.0.cmp(&a.0)).then(a.1.cmp(&b.1)));
    hits.into_iter().map(|(_, _, item)| item).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_outrank_search_terms_and_every_token_must_match() {
        let item = |title: &str, to: &str, terms: &[&str], secondary: bool| Item {
            title: title.into(),
            to: to.into(),
            terms: terms.iter().map(|t| normalize(t)).collect(),
            target: None,
            secondary,
        };
        let fixture = [
            item("Thread notifications", "general", &["alerts sound"], false),
            item("In-app notifications", "general", &["toast"], false),
            item("Notifications", "general", &[], true),
            item("Font smoothing", "appearance", &["notifications text"], false),
        ];
        let titles = |q: &str| rank(&fixture, q).iter().map(|i| i.title.as_str()).collect::<Vec<_>>();
        assert_eq!(titles("notifications"), ["Thread notifications", "In-app notifications", "Font smoothing", "Notifications"]);
        assert_eq!(titles("  ALERTS   thread "), ["Thread notifications"]);
        assert_eq!(titles("appearance smoothing"), ["Font smoothing"]);
        assert!(titles("missing").is_empty());
        assert!(titles("   ").is_empty());
        // The real index loads and only lists pages this client has.
        assert!(search("contrast").iter().any(|i| i.to == "appearance"));
        assert!(super::items().iter().all(|i| super::super::has_page(&i.to)));
        assert!(search("open source licenses").iter().any(|i| i.to == "about"));
        assert!(search("load balancing").is_empty(), "no result for a section this client doesn't draw");
    }
}
