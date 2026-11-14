//! Web's `data-diff-color-scheme`: blue additions and orange deletions replace red and green.

use crate::theme::T;
use crate::tokens::tw;
use rune::Color;

/// The addition and deletion colours for `scheme`, or `None` to keep the theme's own.
fn colors(scheme: &str, dark: bool) -> Option<(Color, Color)> {
    (scheme == "blue-orange").then(|| if dark { (tw::BLUE_400, tw::ORANGE_400) } else { (tw::BLUE_600, tw::ORANGE_600) })
}

pub fn apply(scheme: &str, t: &mut T) {
    if let Some((add, del)) = colors(scheme, t.dark) {
        t.c.diff_addition = add;
        t.c.diff_addition_foreground = add;
        t.c.diff_deletion = del;
        t.c.diff_deletion_foreground = del;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blue_orange_follows_light_dark_and_red_green_keeps_the_theme() {
        assert_eq!(colors("blue-orange", true), Some((tw::BLUE_400, tw::ORANGE_400)));
        assert_eq!(colors("blue-orange", false), Some((tw::BLUE_600, tw::ORANGE_600)));
        assert_eq!(colors("red-green", true), None);
        assert_eq!(colors("", false), None);
    }
}
