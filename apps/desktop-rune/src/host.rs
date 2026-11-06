//! Which server this window talks to. `S5_SERVER_URL` + `S5_TOKEN` (a bearer access token) point
//! it at a running S5 Code server; headless renders and a bare launch fall back to fixtures.
//! Spawning the bundled server the way the Electron shell does lands here later.

pub fn server() -> Option<(String, String)> {
    if rune::tooling::is_headless() {
        return None;
    }
    let url = std::env::var("S5_SERVER_URL").ok()?;
    let token = std::env::var("S5_TOKEN").ok().or_else(|| {
        let path = std::env::var("S5_TOKEN_FILE").ok()?;
        std::fs::read_to_string(path).ok().map(|t| t.trim().to_string())
    })?;
    Some((url, token))
}

pub fn shutdown() {}
