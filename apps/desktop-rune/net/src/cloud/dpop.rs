//! DPoP proofs (RFC 9449) as `packages/shared/src/dpop.ts` verifies them: ES256 over P-256,
//! the public JWK in the header, `htu` without query or fragment, and `ath` when a token rides
//! along. One key per device; its thumbprint is what relay and environment tokens bind to.

use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};
use serde_json::{Value, json};

use super::b64url;
#[cfg(test)]
use super::b64url_decode;

pub struct Key {
    pair: EcdsaKeyPair,
    pkcs8: Vec<u8>,
    x: String,
    y: String,
}

impl Key {
    pub fn generate() -> Result<Key, String> {
        let document = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new()).map_err(|_| "Could not generate the DPoP proof key.".to_owned())?;
        Self::from_pkcs8(document.as_ref())
    }

    pub fn from_pkcs8(bytes: &[u8]) -> Result<Key, String> {
        let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, bytes, &SystemRandom::new()).map_err(|_| "The DPoP proof key is unreadable.".to_owned())?;
        let public = pair.public_key().as_ref();
        if public.len() != 65 || public[0] != 4 {
            return Err("The DPoP proof key is not an uncompressed P-256 key.".into());
        }
        let (x, y) = (b64url(&public[1..33]), b64url(&public[33..]));
        Ok(Key { pair, pkcs8: bytes.to_vec(), x, y })
    }

    /// The private key, for storing on this device.
    pub fn pkcs8(&self) -> &[u8] {
        &self.pkcs8
    }

    pub fn jwk(&self) -> Value {
        json!({"kty": "EC", "crv": "P-256", "x": self.x, "y": self.y})
    }

    pub fn thumbprint(&self) -> String {
        thumbprint(&self.x, &self.y)
    }

    pub fn proof(&self, method: &str, url: &str, access_token: Option<&str>) -> Result<String, String> {
        let iat = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default();
        self.proof_at(method, url, access_token, iat, &crate::uuid())
    }

    pub fn proof_at(&self, method: &str, url: &str, access_token: Option<&str>, iat: u64, jti: &str) -> Result<String, String> {
        let header = json!({"typ": "dpop+jwt", "alg": "ES256", "jwk": self.jwk()});
        let mut claims = json!({"htm": method, "htu": htu(url)?, "jti": jti, "iat": iat});
        if let Some(token) = access_token {
            claims["ath"] = json!(access_token_hash(token));
        }
        let input = format!("{}.{}", b64url(header.to_string().as_bytes()), b64url(claims.to_string().as_bytes()));
        let signature = self.pair.sign(&SystemRandom::new(), input.as_bytes()).map_err(|_| "Could not sign the DPoP proof.".to_owned())?;
        Ok(format!("{input}.{}", b64url(signature.as_ref())))
    }
}

/// RFC 7638 over the required members in lexical order (`computeDpopJwkThumbprint`).
pub fn thumbprint(x: &str, y: &str) -> String {
    let members = format!(r#"{{"crv":"P-256","kty":"EC","x":"{x}","y":"{y}"}}"#);
    b64url(ring::digest::digest(&ring::digest::SHA256, members.as_bytes()).as_ref())
}

pub fn access_token_hash(token: &str) -> String {
    b64url(ring::digest::digest(&ring::digest::SHA256, token.as_bytes()).as_ref())
}

/// `normalizeDpopHtu`: the URL without its query and fragment.
pub fn htu(url: &str) -> Result<String, String> {
    let mut url = url::Url::parse(url).map_err(|e| format!("Invalid DPoP target URL: {e}"))?;
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}

/// What `verifyDpopProof` checks, for tests against proofs the TypeScript side signed.
#[cfg(test)]
pub(crate) fn verify(proof: &str, method: &str, url: &str, access_token: Option<&str>) -> Result<String, String> {
    use ring::signature::{ECDSA_P256_SHA256_FIXED, UnparsedPublicKey};
    let parts: Vec<&str> = proof.split('.').collect();
    let [header, claims, signature] = parts[..] else { return Err("malformed".into()) };
    let decode = |part: &str| -> Result<Value, String> { serde_json::from_slice(&b64url_decode(part).ok_or("base64")?).map_err(|e| e.to_string()) };
    let (h, c) = (decode(header)?, decode(claims)?);
    if h["typ"] != "dpop+jwt" || h["alg"] != "ES256" || h["jwk"]["kty"] != "EC" || h["jwk"]["crv"] != "P-256" || !h["jwk"]["d"].is_null() {
        return Err("header".into());
    }
    let (x, y) = (h["jwk"]["x"].as_str().unwrap_or_default(), h["jwk"]["y"].as_str().unwrap_or_default());
    let mut public = vec![4u8];
    public.extend(b64url_decode(x).ok_or("x")?);
    public.extend(b64url_decode(y).ok_or("y")?);
    UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, &public)
        .verify(format!("{header}.{claims}").as_bytes(), &b64url_decode(signature).ok_or("signature")?)
        .map_err(|_| "invalid_signature".to_owned())?;
    if c["htm"] != method {
        return Err("method_mismatch".into());
    }
    if c["htu"] != htu(url)?.as_str() {
        return Err("url_mismatch".into());
    }
    if c["jti"].as_str().is_none_or(str::is_empty) || !c["iat"].is_u64() {
        return Err("claims".into());
    }
    if c["ath"].as_str() != access_token.map(access_token_hash).as_deref() {
        return Err("access_token_hash_mismatch".into());
    }
    Ok(thumbprint(x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Signed and accepted by `verifyDpopProof` in `packages/shared/src/dpop.ts`.
    const TS_PROOF: &str = "eyJ0eXAiOiJkcG9wK2p3dCIsImFsZyI6IkVTMjU2IiwiandrIjp7Imt0eSI6IkVDIiwiY3J2IjoiUC0yNTYiLCJ4IjoiandnNm9SaVk4NnpaV1M0T2dONkF1dVVSaExHUGRMUG5wUTBKMUg0djF1RSIsInkiOiJhSlRNMkk5R0FEeUZjQlZNbmI0aC1kbEFTUlk5MWx0b2IySDU4Q2hUXzNNIn19.eyJodG0iOiJQT1NUIiwiaHR1IjoiaHR0cHM6Ly9yZWxheS5zNWNvZGUuZHBkbnMub3JnL3YxL2Vudmlyb25tZW50cy9lbnYtMS9jb25uZWN0IiwianRpIjoiNmYxYzJhNTItMGYzZC00YjBlLTlkMGEtM2YxZTJiN2M5ZDExIiwiaWF0IjoxNzk1MDAwMDAwLCJhdGgiOiItbXRaQXZaSEt0ZDVYUnNnX1NUejFmeWZiVUQtUlNpZU13MUwyWU0yZlhFIn0.ueNBLiY3VjraLhCYxmL088kZJWfr_b0U_yHa9EHpKCufxFD7XC5ZDkWwYmo9Kq5mPWrtJhj8OoO9DPZsqjRexA";
    const CONNECT: &str = "https://relay.s5code.dpdns.org/v1/environments/env-1/connect";

    #[test]
    fn hashes_match_the_typescript_implementation() {
        assert_eq!(thumbprint("f83OJ3D2xF1Bg8vub9tLe1gHMzV76e8Tus9uPHvRVEU", "x_FEzRu9m36HLN_tue659LNpXW6pCyStikYjKIWI5a0"), "oKIywvGUpTVTyxMQ3bwIIeQUudfr_CkLMjCE19ECD-U");
        assert_eq!(access_token_hash("Kz~8mXK1EalYznwH-LC-1fBAo.4Ljp~zsPE_NeO.gxU"), "fUHyO2r2Z3DZ53EsNrWBb0xWXoaNy59IiKCAqksmQEo");
        assert_eq!(htu("https://relay.example.com/v1/environments/env-1/connect?x=1#frag").unwrap(), "https://relay.example.com/v1/environments/env-1/connect");
    }

    #[test]
    fn a_typescript_proof_verifies_here() {
        assert_eq!(verify(TS_PROOF, "POST", CONNECT, Some("relay-access-token-123")).as_deref(), Ok("8R3LfWqX_qJaYzybL7pOIyrOIJ0keblK9DvUHUqdKEg"));
        assert_eq!(verify(TS_PROOF, "GET", CONNECT, Some("relay-access-token-123")), Err("method_mismatch".into()));
        assert_eq!(verify(TS_PROOF, "POST", CONNECT, Some("another")), Err("access_token_hash_mismatch".into()));
    }

    #[test]
    fn proofs_sign_what_the_server_checks_and_survive_a_reload() {
        let key = Key::generate().unwrap();
        let proof = key.proof_at("POST", &format!("{CONNECT}?ignored=1"), Some("relay-access-token-123"), 1_795_000_000, "jti-1").unwrap();
        assert_eq!(verify(&proof, "POST", CONNECT, Some("relay-access-token-123")), Ok(key.thumbprint()));
        let bare = key.proof("POST", "https://env.example.com/oauth/token", None).unwrap();
        assert_eq!(verify(&bare, "POST", "https://env.example.com/oauth/token", None), Ok(key.thumbprint()), "no ath without a token");
        let header: Value = serde_json::from_slice(&b64url_decode(proof.split('.').next().unwrap()).unwrap()).unwrap();
        assert!(header["jwk"]["d"].is_null(), "the private key never leaves");
        let reloaded = Key::from_pkcs8(key.pkcs8()).unwrap();
        assert_eq!(reloaded.thumbprint(), key.thumbprint());
        assert!(Key::from_pkcs8(b"nonsense").is_err());
    }
}
