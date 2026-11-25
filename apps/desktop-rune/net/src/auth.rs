//! How a connection authorizes its socket and its own `/api` requests: a bearer token, or
//! T3 Connect's DPoP-bound environment token (web `buildEnvironmentAuthHeaders`,
//! `executeEnvironmentRequest` and `renewDpopBeforeExpiry`).

use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use serde_json::{Value, json};

use crate::cloud::Failure;

#[derive(Clone)]
pub enum Auth {
    /// A token sent as `Authorization: Bearer`.
    Bearer(String),
    /// T3 Connect: a key-bound environment token and a proof per request.
    Dpop(Arc<dyn Dpop>),
}

/// A T3 Connect environment's credential (web `RemoteEnvironmentAuthorization`). Calls block, so
/// they run on socket and request threads.
pub trait Dpop: Send + Sync {
    /// A socket URL for the next attempt: a key-bound ticket and its upgrade proof.
    fn socket(&self) -> Result<String, Failure>;
    /// The environment's access token, renewed within a minute of expiry, or at once when
    /// `rejected` is the token the environment just refused.
    fn token(&self, rejected: Option<&str>) -> Result<String, Failure>;
    /// When the current token lapses, in Unix seconds, once there is one.
    fn expires_at(&self) -> Option<u64>;
    /// This device's proof for one request that carries `token`.
    fn proof(&self, method: &str, url: &str, token: &str) -> Result<String, Failure>;
}

impl Auth {
    /// The headers for one request, and the DPoP token among them.
    pub fn headers(&self, method: &str, url: &str, rejected: Option<&str>) -> Result<(Vec<(&'static str, String)>, Option<String>), String> {
        match self {
            Auth::Bearer(token) => Ok((vec![("Authorization", format!("Bearer {token}"))], None)),
            Auth::Dpop(dpop) => {
                let token = dpop.token(rejected).map_err(|f| f.message)?;
                let proof = dpop.proof(method, url, &token).map_err(|f| f.message)?;
                Ok((vec![("Authorization", format!("DPoP {token}")), ("DPoP", proof)], Some(token)))
            }
        }
    }

    /// Sets this credential on a request of the caller's own, such as an event stream.
    pub fn apply(&self, request: ureq::Request) -> Result<ureq::Request, String> {
        let (headers, _) = self.headers(request.method(), request.url(), None)?;
        Ok(headers.into_iter().fold(request, |r, (name, value)| r.set(name, &value)))
    }
}

pub(crate) enum Failed {
    Status(u16, ureq::Response),
    Other(String),
}

impl Failed {
    /// The body as text, else `HTTP <status>`.
    pub(crate) fn text(self) -> String {
        match self {
            Failed::Status(code, response) => response.into_string().ok().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| format!("HTTP {code}")),
            Failed::Other(message) => message,
        }
    }

    /// The body as JSON, else `{message}`: the shape an RPC failure has.
    pub(crate) fn json(self) -> Value {
        match self {
            Failed::Status(code, response) => response.into_json::<Value>().unwrap_or_else(|_| json!({"message": format!("HTTP {code}")})),
            Failed::Other(message) => json!({"message": message}),
        }
    }
}

/// `method` to `url` with this connection's credentials; `body` sends the request. A DPoP token
/// the environment refuses is renewed and the request sent once more. A bearer is not retried.
pub(crate) fn send(auth: &Auth, agent: &ureq::Agent, method: &str, url: &str, body: impl Fn(ureq::Request) -> Result<ureq::Response, ureq::Error>) -> Result<ureq::Response, Failed> {
    let mut rejected: Option<String> = None;
    loop {
        let (headers, token) = auth.headers(method, url, rejected.as_deref()).map_err(Failed::Other)?;
        let request = headers.into_iter().fold(agent.request(method, url), |r, (name, value)| r.set(name, &value));
        match body(request) {
            Ok(response) => return Ok(response),
            Err(ureq::Error::Status(401, _)) if token.is_some() && rejected.is_none() => rejected = token,
            Err(ureq::Error::Status(code, response)) => return Err(Failed::Status(code, response)),
            Err(error) => return Err(Failed::Other(error.to_string())),
        }
    }
}

/// Seconds until the next renewal of a token lapsing at `expires`, or `None` once it has: the
/// server has closed the socket by then. The floor spaces out retries after a failed renewal.
pub(crate) fn renewal_wait(now: u64, expires: u64) -> Option<u64> {
    (now < expires).then(|| expires.saturating_sub(now + 60).max(5))
}

/// While a socket is open, mints the environment's next token a minute before the current one
/// lapses, so the server moves the socket to it instead of closing it. Ends when the returned
/// guard drops.
pub(crate) fn renew_while_open(dpop: Arc<dyn Dpop>) -> mpsc::Sender<()> {
    let (guard, closed) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        while let Some(wait) = dpop.expires_at().and_then(|expires| renewal_wait(crate::cloud::relay::now(), expires)) {
            match closed.recv_timeout(Duration::from_secs(wait)) {
                // A relay blip retries until expiry, like web's.
                Err(RecvTimeoutError::Timeout) => {
                    let _ = dpop.token(None);
                }
                _ => return,
            }
        }
    });
    guard
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::dpop::{Key, verify};
    use std::io::{BufRead, BufReader, Write};
    use std::sync::Mutex;

    /// Tokens `tok-1`, `tok-2`, … and proofs from a real key; records what it was asked.
    struct Fake {
        key: Key,
        issued: Mutex<Vec<Option<String>>>,
    }

    impl Dpop for Fake {
        fn socket(&self) -> Result<String, Failure> {
            Err(Failure::new("unused"))
        }
        fn token(&self, rejected: Option<&str>) -> Result<String, Failure> {
            let mut issued = self.issued.lock().unwrap();
            issued.push(rejected.map(str::to_owned));
            Ok(format!("tok-{}", if rejected.is_some() { issued.len() } else { 1 }))
        }
        fn expires_at(&self) -> Option<u64> {
            None
        }
        fn proof(&self, method: &str, url: &str, token: &str) -> Result<String, Failure> {
            self.key.proof(method, url, Some(token)).map_err(Failure::new)
        }
    }

    fn fake() -> Arc<Fake> {
        Arc::new(Fake { key: Key::generate().unwrap(), issued: Mutex::default() })
    }

    #[test]
    fn dpop_headers_carry_the_token_and_a_proof_the_server_verifies() {
        let fake = fake();
        let auth = Auth::Dpop(fake.clone());
        let url = "https://env-1.tunnel.example.com/api/orchestration/threads/t-1/history?cursor=c&deferWork=1";
        let (headers, token) = auth.headers("GET", url, None).unwrap();
        assert_eq!(token.as_deref(), Some("tok-1"));
        assert_eq!(headers[0], ("Authorization", "DPoP tok-1".to_owned()));
        assert_eq!(headers[1].0, "DPoP");
        // `verifyDpopProof`'s checks, themselves checked against a proof the TypeScript side signed.
        assert_eq!(verify(&headers[1].1, "GET", "https://env-1.tunnel.example.com/api/orchestration/threads/t-1/history", Some("tok-1")), Ok(fake.key.thumbprint()), "the proof binds the method, the URL without its query, and the token");
        assert_eq!(verify(&headers[1].1, "POST", url, Some("tok-1")), Err("method_mismatch".into()));
        let (bearer, none) = Auth::Bearer("b-1".into()).headers("GET", url, None).unwrap();
        assert_eq!((bearer, none), (vec![("Authorization", "Bearer b-1".to_owned())], None));
    }

    /// Answers each connection with the next status, keeping each request's head.
    fn serve(statuses: &'static [u16]) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            statuses
                .iter()
                .map(|status| {
                    let (stream, _) = listener.accept().unwrap();
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut head = String::new();
                    loop {
                        let mut line = String::new();
                        reader.read_line(&mut line).unwrap();
                        if line == "\r\n" || line.is_empty() {
                            break;
                        }
                        head.push_str(&line);
                    }
                    let body = if *status == 200 { "{\"ok\":true}" } else { "{\"_tag\":\"EnvironmentAuthInvalidError\",\"reason\":\"invalid_credential\"}" };
                    write!(&stream, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                    head.to_ascii_lowercase()
                })
                .collect()
        });
        (base, server)
    }

    #[test]
    fn a_refused_dpop_token_is_renewed_and_the_request_sent_once_more() {
        let fake = fake();
        let auth = Auth::Dpop(fake.clone());
        let (base, server) = serve(&[401, 200]);
        let body = crate::api_get(&base, &auth, "/api/assets/a.png", "", 1 << 10).unwrap();
        assert_eq!(body, b"{\"ok\":true}");
        let heads = server.join().unwrap();
        assert!(heads[0].contains("authorization: dpop tok-1") && heads[0].contains("\r\ndpop: "));
        assert!(heads[1].contains("authorization: dpop tok-2"), "the retry carries a renewed token: {}", heads[1]);
        assert_eq!(*fake.issued.lock().unwrap(), [None, Some("tok-1".to_owned())], "the renewal names the token that was refused");

        let (base, server) = serve(&[401, 401]);
        assert!(crate::api_get(&base, &auth, "/api/assets/a.png", "", 1 << 10).unwrap_err().contains("invalid_credential"), "a second refusal is final");
        assert_eq!(server.join().unwrap().len(), 2);

        let (base, server) = serve(&[401]);
        assert!(crate::api_get(&base, &Auth::Bearer("b".into()), "/api/assets/a.png", "", 1 << 10).is_err());
        assert_eq!(server.join().unwrap().len(), 1, "a bearer is not retried");

        let (base, server) = serve(&[401, 401]);
        assert!(crate::api_socket(&base, &Auth::Dpop(self::fake()), "/api/ws", "", Duration::from_secs(1)).is_err());
        let heads = server.join().unwrap();
        assert!(heads[0].contains("authorization: dpop tok-1") && heads[0].contains("upgrade: websocket"));
        assert!(heads[1].contains("authorization: dpop tok-2"), "a refused socket upgrade retries with a renewed token: {}", heads[1]);
    }

    #[test]
    fn renewal_wakes_a_minute_before_expiry_and_stops_at_it() {
        assert_eq!(renewal_wait(1_000, 1_900), Some(840));
        assert_eq!(renewal_wait(1_000, 1_030), Some(5), "inside the skew: retry soon, not at once");
        assert_eq!(renewal_wait(1_000, 1_000), None, "the server has closed the socket by expiry");
    }
}
