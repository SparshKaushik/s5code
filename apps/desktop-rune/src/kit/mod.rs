//! Ports of apps/web/src/components/ui: each primitive keeps its web `variant`/`size` names, and
//! owns its look the way the web components do (screens pick a variant; they don't restyle).

use crate::App;

/// A gallery of every primitive, in each variant and state.
pub const SCENES: &[(&str, fn(&mut App))] = &[];
