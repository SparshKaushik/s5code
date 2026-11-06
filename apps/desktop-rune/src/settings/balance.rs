use crate::theme;
use rune::text::TextSystem;
use std::{cell::RefCell, collections::HashMap};

thread_local! {
    static MEASURER: RefCell<(TextSystem, HashMap<(String,u32),String>)> = RefCell::new((TextSystem::default(),HashMap::new()));
}

// CSS text-balance minimizes line width while preserving the greedy layout's line count.
pub fn description(copy: &str, width: f32) -> String {
    MEASURER.with_borrow_mut(|(fonts, cache)| {
        let key = (copy.to_owned(), width.to_bits());
        if let Some(copy) = cache.get(&key) {
            return copy.clone();
        }
        let words: Vec<_> = copy.split_whitespace().collect();
        let font = theme::TEXT_XS.lh(18.0);
        let mut wrap = |limit: f32| {
            let mut lines = vec![String::new()];
            for word in &words {
                let line = lines.last_mut().unwrap();
                let next = if line.is_empty() {
                    (*word).to_owned()
                } else {
                    format!("{line} {word}")
                };
                if !line.is_empty() && fonts.measure(&next, font, 1.0).0 > limit {
                    lines.push((*word).to_owned());
                } else {
                    *line = next;
                }
            }
            lines
        };
        let count = wrap(width).len();
        let mut lo = 0.0;
        let mut hi = width;
        if count > 1 && count <= 6 {
            while hi - lo > 0.5 {
                let mid = (lo + hi) / 2.0;
                if wrap(mid).len() <= count {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
        }
        let result = wrap(hi).join("\n");
        if cache.len() > 256 {
            cache.clear();
        }
        cache.insert(key, result.clone());
        result
    })
}
