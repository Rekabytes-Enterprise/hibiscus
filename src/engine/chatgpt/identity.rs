//! Verify the ID token against the official discovery document and signing keys.
use super::{json, now, ISSUER};
use crate::Result;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ring::signature::{RsaPublicKeyComponents, RSA_PKCS1_2048_8192_SHA256};
use serde::Deserialize;

#[derive(Deserialize)]
struct Discovery {
    issuer: String,
    jwks_uri: String,
}

#[derive(Deserialize)]
struct KeySet {
    keys: Vec<Key>,
}

#[derive(Deserialize)]
struct Key {
    kid: Option<String>,
    kty: String,
    n: Option<String>,
    e: Option<String>,
}

#[derive(Deserialize)]
struct Header {
    alg: String,
    kid: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize)]
struct Claims {
    iss: String,
    sub: String,
    aud: Audience,
    exp: u64,
    iat: u64,
    nbf: Option<u64>,
    nonce: Option<String>,
    azp: Option<String>,
}

pub(super) async fn verify(
    http: &reqwest::Client,
    discovery_url: &str,
    token: &str,
    client: &str,
    nonce: Option<&str>,
) -> Result<String> {
    let discovery: Discovery = json(
        http.get(discovery_url)
            .send()
            .await
            .map_err(|_| "Could not reach ChatGPT identity discovery.")?,
    )
    .await?;
    let url =
        reqwest::Url::parse(&discovery.jwks_uri).map_err(|_| "Invalid ChatGPT signing-key URL.")?;
    let official = url.scheme() == "https"
        && url.host_str() == Some("auth.openai.com")
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none();
    #[cfg(test)]
    let official = official
        || reqwest::Url::parse(discovery_url).is_ok_and(|discovery| {
            discovery.host_str() == Some("127.0.0.1") && discovery.origin() == url.origin()
        });
    if discovery.issuer != ISSUER || !official {
        return Err("Invalid ChatGPT identity discovery document.".into());
    }
    let keys = json(
        http.get(url)
            .send()
            .await
            .map_err(|_| "Could not retrieve ChatGPT signing keys.")?,
    )
    .await?;
    verify_claims(token, client, nonce, keys)
}

fn verify_claims(token: &str, client: &str, nonce: Option<&str>, keys: KeySet) -> Result<String> {
    if token.len() > 65536 {
        return Err("ChatGPT identity token exceeds the supported size.".into());
    }
    let (signed, signature) = token
        .rsplit_once('.')
        .ok_or("Invalid ChatGPT identity token.")?;
    let (header, payload) = signed
        .split_once('.')
        .ok_or("Invalid ChatGPT identity token.")?;
    let decode = |part: &str| {
        URL_SAFE_NO_PAD
            .decode(part)
            .map_err(|_| "Invalid encoding in ChatGPT identity token.")
    };
    let header: Header = serde_json::from_slice(&decode(header)?)
        .map_err(|_| "Invalid ChatGPT identity token header.")?;
    if header.alg != "RS256" || header.kid.is_empty() {
        return Err("Unsupported ChatGPT identity signature.".into());
    }
    let mut matching = keys
        .keys
        .iter()
        .filter(|key| key.kid.as_deref() == Some(&header.kid));
    let key = matching
        .next()
        .filter(|key| key.kty == "RSA" && matching.next().is_none())
        .ok_or("ChatGPT identity signing key is unavailable.")?;
    let key = RsaPublicKeyComponents {
        n: decode(key.n.as_deref().ok_or("Missing ChatGPT RSA modulus.")?)?,
        e: decode(key.e.as_deref().ok_or("Missing ChatGPT RSA exponent.")?)?,
    };
    key.verify(
        &RSA_PKCS1_2048_8192_SHA256,
        signed.as_bytes(),
        &decode(signature)?,
    )
    .map_err(|_| "ChatGPT identity token signature did not validate.")?;
    let claims: Claims = serde_json::from_slice(&decode(payload)?)
        .map_err(|_| "Invalid ChatGPT identity claims.")?;
    let audience = match claims.aud {
        Audience::One(audience) => audience == client,
        Audience::Many(audience) => {
            audience.iter().any(|id| id == client)
                && (audience.len() == 1 || claims.azp.as_deref() == Some(client))
        }
    };
    let now = now();
    if claims.iss != ISSUER
        || !audience
        || claims.sub.is_empty()
        || claims.exp <= now
        || claims.iat > now.saturating_add(5)
        || claims.nbf.is_some_and(|n| n > now.saturating_add(5))
        || claims.azp.as_deref().is_some_and(|id| id != client)
        || nonce.is_some_and(|nonce| claims.nonce.as_deref() != Some(nonce))
    {
        return Err("ChatGPT identity claims did not match the sign-in.".into());
    }
    Ok(claims.sub)
}
