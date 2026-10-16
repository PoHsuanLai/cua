//! Links that carry a secret in their address.
//!
//! An injected text often asks the assistant to open or send to a link whose address holds a
//! password, token or key, or whose authority holds `user:password@`. The check looks at links
//! written out in screen text. It names the host, never the query: the secret is not copied into
//! an excerpt.

use super::excerpt::Excerpt;

/// Schemes that carry a request to a server.
const SCHEMES: &[&str] = &["http", "https", "ftp", "ftps", "ws", "wss"];

/// Query parameter names that carry a credential. Lower case.
const SECRET_PARAMS: &[&str] = &[
    "token",
    "access_token",
    "id_token",
    "refresh_token",
    "auth_token",
    "api_key",
    "apikey",
    "api-key",
    "secret",
    "client_secret",
    "password",
    "passwd",
    "pwd",
    "pass",
    "auth",
    "authorization",
    "bearer",
    "session",
    "sessionid",
    "session_id",
    "sid",
    "otp",
    "cookie",
    "credentials",
];

/// Characters that end a link written in running text.
const ENDS: &str = "\"'<>)]}";

fn param_carries_secret(param: &str) -> bool {
    let mut parts = param.splitn(2, '=');
    let name = parts.next().unwrap_or_default().to_lowercase();
    let value = parts.next().unwrap_or_default();
    !value.is_empty() && SECRET_PARAMS.contains(&name.as_str())
}

/// The scheme that ends at byte `at` of `text` (the `:` of `://`), if it is one of [`SCHEMES`].
fn scheme_before(text: &str, at: usize) -> Option<&str> {
    let start = text[..at]
        .char_indices()
        .rev()
        .find(|(_, c)| !c.is_ascii_alphabetic())
        .map_or(0, |(i, c)| i + c.len_utf8());
    let scheme = &text[start..at];
    SCHEMES
        .iter()
        .any(|s| s.eq_ignore_ascii_case(scheme))
        .then_some(scheme)
}

/// The first link in `shown` that carries a credential, as an excerpt naming its host.
pub(super) fn credential_link(shown: &str) -> Option<Excerpt> {
    shown.match_indices("://").find_map(|(at, _)| {
        let scheme = scheme_before(shown, at)?;
        let rest = &shown[at + 3..];
        let end = rest
            .find(|c: char| c.is_whitespace() || ENDS.contains(c))
            .unwrap_or(rest.len());
        let link = &rest[..end];
        let authority_end = link.find(['/', '?', '#']).unwrap_or(link.len());
        let authority = &link[..authority_end];
        let (userinfo, host) = match authority.rsplit_once('@') {
            Some((userinfo, host)) => (userinfo, host),
            None => ("", authority),
        };
        let query = link[authority_end..]
            .split_once('?')
            .map_or("", |(_, query)| query.split('#').next().unwrap_or_default());
        let secret_query = query.split('&').any(param_carries_secret);
        (userinfo.contains(':') || secret_query)
            .then(|| Excerpt::start_of(&format!("{}://{host}/\u{2026}", scheme.to_lowercase())))
    })
}
