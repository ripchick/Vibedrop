//! Spotify metadata without the desktop app.
//!
//! Two ways to read a track or an album:
//! * **Spotify Web API** (Client Credentials flow) when a Client ID and Secret are configured.
//!   This is the stable, documented route and it also returns the album of a single track.
//! * **The public embed page** (`open.spotify.com/embed/...`) otherwise. It contains a
//!   `__NEXT_DATA__` JSON blob with the title, artist, cover, duration and the full track list,
//!   so the Music section works without any account or key. No user login is involved either way.
//!
//! Only `open.spotify.com` links are accepted. Nothing from the page is trusted as HTML: the JSON
//! is parsed with serde and every field is range-checked by the caller.

use base64::Engine as _;
use reqwest::blocking::Client;
use serde_json::Value;
use std::time::Duration;
use url::Url;

const API: &str = "https://api.spotify.com/v1";
const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";
const EMBED: &str = "https://open.spotify.com/embed";
const BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                          (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";
const HTTP_TIMEOUT: Duration = Duration::from_secs(25);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpotifyKind {
    Track,
    Album,
}

impl SpotifyKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SpotifyKind::Track => "track",
            SpotifyKind::Album => "album",
        }
    }
}

/// A parsed `open.spotify.com` link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpotifyLink {
    pub kind: SpotifyKind,
    pub id: String,
}

impl SpotifyLink {
    pub fn canonical_url(&self) -> String {
        format!("https://open.spotify.com/{}/{}", self.kind.as_str(), self.id)
    }
}

/// One track as Spotify describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct FetchedTrack {
    pub spotify_id: Option<String>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: f64,
}

/// An album, or a single track wrapped in a one-track entity.
#[derive(Debug, Clone, PartialEq)]
pub struct FetchedEntity {
    pub kind: SpotifyKind,
    pub title: String,
    pub artist: String,
    pub cover_url: Option<String>,
    pub tracks: Vec<FetchedTrack>,
    /// Which route produced the data, for the UI ("Spotify Web API" / "Spotify public page").
    pub source: &'static str,
}

/// Client Credentials pair. The secret is only ever sent to `accounts.spotify.com`.
#[derive(Debug, Clone)]
pub struct Credentials {
    pub client_id: String,
    pub client_secret: String,
}

/// Credentials from the settings, falling back to `SPOTIFY_CLIENT_ID` / `SPOTIFY_CLIENT_SECRET`.
/// Both parts must be present; half a pair is treated as "not configured".
pub fn resolve_credentials(client_id: &str, client_secret: &str) -> Option<Credentials> {
    let id = if client_id.trim().is_empty() {
        std::env::var("SPOTIFY_CLIENT_ID").unwrap_or_default()
    } else {
        client_id.trim().to_string()
    };
    let secret = if client_secret.trim().is_empty() {
        std::env::var("SPOTIFY_CLIENT_SECRET").unwrap_or_default()
    } else {
        client_secret.trim().to_string()
    };
    if id.is_empty() || secret.is_empty() {
        None
    } else {
        Some(Credentials {
            client_id: id,
            client_secret: secret,
        })
    }
}

/// Parse a Spotify track/album link: a full URL, a URL without a scheme, or a `spotify:` URI.
pub fn parse_link(input: &str) -> Result<SpotifyLink, String> {
    let text = input.trim();
    if text.is_empty() {
        return Err("Paste a Spotify track or album link first.".to_string());
    }
    if let Some(rest) = text.strip_prefix("spotify:") {
        let parts: Vec<&str> = rest.split(':').collect();
        if parts.len() >= 2 {
            let kind = match parts[0] {
                "track" => SpotifyKind::Track,
                "album" => SpotifyKind::Album,
                other => {
                    return Err(format!(
                        "\"{other}\" links are not supported. Use a track or an album link."
                    ))
                }
            };
            return checked(kind, parts[1]);
        }
        return Err("This Spotify link does not contain a track or album id.".to_string());
    }

    let url = if text.contains("://") {
        Url::parse(text).map_err(|_| "This is not a valid link.".to_string())?
    } else {
        Url::parse(&format!("https://{text}")).map_err(|_| "This is not a valid link.".to_string())?
    };
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Only http(s) Spotify links are supported.".to_string());
    }
    let host = url.host_str().unwrap_or("").to_lowercase();
    if host != "open.spotify.com" && host != "play.spotify.com" {
        return Err("This is not an open.spotify.com link.".to_string());
    }
    let segments: Vec<&str> = url
        .path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    // Accepted: /track/<id>, /album/<id>, /embed/track/<id>, /intl-ru/track/<id>.
    let start = segments
        .iter()
        .position(|s| *s == "track" || *s == "album")
        .ok_or_else(|| "Use a link that contains /track/ or /album/.".to_string())?;
    let kind = match segments[start] {
        "track" => SpotifyKind::Track,
        _ => SpotifyKind::Album,
    };
    let id = *segments
        .get(start + 1)
        .ok_or_else(|| "This Spotify link does not contain a track or album id.".to_string())?;
    checked(kind, id)
}

fn checked(kind: SpotifyKind, id: &str) -> Result<SpotifyLink, String> {
    let id = id.trim();
    let ok = (8..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric());
    if !ok {
        return Err("This Spotify link contains an invalid id.".to_string());
    }
    Ok(SpotifyLink {
        kind,
        id: id.to_string(),
    })
}

/// Fetch metadata for a link. With credentials the Web API is tried first; any API problem falls
/// back to the public embed page, so a missing/expired key never blocks the feature.
pub fn fetch(link: &SpotifyLink, credentials: Option<&Credentials>) -> Result<FetchedEntity, String> {
    if let Some(credentials) = credentials {
        match fetch_api(link, credentials) {
            Ok(entity) => return Ok(entity),
            Err(e) => tracing::warn!("Spotify Web API failed, using the public page instead: {e}"),
        }
    }
    fetch_public(link)
}

fn client() -> Result<Client, String> {
    Client::builder()
        .timeout(HTTP_TIMEOUT)
        .connect_timeout(Duration::from_secs(10))
        .user_agent(BROWSER_UA)
        .build()
        .map_err(|e| format!("Could not create the HTTP client: {e}"))
}

fn network_error(context: &str, e: reqwest::Error) -> String {
    if e.is_timeout() {
        format!("{context}: the request timed out.")
    } else {
        format!("{context}: {}", e.without_url())
    }
}

// ---- Web API -----------------------------------------------------------------------------------

fn fetch_api(link: &SpotifyLink, credentials: &Credentials) -> Result<FetchedEntity, String> {
    let client = client()?;
    let token = api_token(&client, credentials)?;
    match link.kind {
        SpotifyKind::Album => api_album(&client, &token, link),
        SpotifyKind::Track => api_track(&client, &token, link),
    }
}

fn api_token(client: &Client, credentials: &Credentials) -> Result<String, String> {
    let basic = base64::engine::general_purpose::STANDARD
        .encode(format!("{}:{}", credentials.client_id, credentials.client_secret));
    let response = client
        .post(TOKEN_URL)
        .header(reqwest::header::AUTHORIZATION, format!("Basic {basic}"))
        .form(&[("grant_type", "client_credentials")])
        .send()
        .map_err(|e| network_error("Could not reach Spotify", e))?;
    let status = response.status();
    if !status.is_success() {
        return Err(match status.as_u16() {
            400 | 401 | 403 => "Spotify rejected the Client ID/Secret. Check them in Settings > Music.".to_string(),
            429 => "Spotify is rate-limiting requests. Try again in a minute.".to_string(),
            code => format!("Spotify could not issue a token (HTTP {code})."),
        });
    }
    let body: Value = response
        .json()
        .map_err(|e| format!("Spotify sent an unreadable token response: {e}"))?;
    body.get("access_token")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "Spotify did not return an access token.".to_string())
}

fn api_get(client: &Client, token: &str, url: &str) -> Result<Value, String> {
    let response = client
        .get(url)
        .bearer_auth(token)
        .send()
        .map_err(|e| network_error("Could not reach Spotify", e))?;
    let status = response.status();
    if !status.is_success() {
        return Err(match status.as_u16() {
            401 => "Spotify rejected the access token.".to_string(),
            403 => "Spotify refused this request (the app may not have Web API access).".to_string(),
            404 => "Spotify could not find this track or album.".to_string(),
            429 => "Spotify is rate-limiting requests. Try again in a minute.".to_string(),
            code => format!("Spotify answered with HTTP {code}."),
        });
    }
    response
        .json()
        .map_err(|e| format!("Spotify sent unreadable data: {e}"))
}

fn api_album(client: &Client, token: &str, link: &SpotifyLink) -> Result<FetchedEntity, String> {
    let first = api_get(client, token, &format!("{API}/albums/{}", link.id))?;
    let title = first
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Untitled album")
        .to_string();
    let artist = first_artist(&first).unwrap_or_default();
    let cover_url = largest_image(&first);
    let mut tracks = Vec::new();

    let mut page = first;
    for _ in 0..20 {
        if let Some(items) = page
            .get("tracks")
            .and_then(|t| t.get("items"))
            .and_then(Value::as_array)
        {
            for item in items {
                let Some(track) = api_track_item(item, &title, &artist) else {
                    continue;
                };
                tracks.push(track);
            }
        }
        let next = page.get("tracks").and_then(|t| t.get("next")).and_then(Value::as_str);
        let Some(next) = next else { break };
        // Never send the bearer token to a host other than Spotify's API.
        if !next.starts_with("https://api.spotify.com/") {
            tracing::warn!("Spotify returned an unexpected pagination URL; stopping at this page");
            break;
        }
        page = api_get(client, token, next)?;
    }
    if tracks.is_empty() {
        return Err("Spotify returned no tracks for this album.".to_string());
    }
    Ok(FetchedEntity {
        kind: SpotifyKind::Album,
        title,
        artist,
        cover_url,
        tracks,
        source: "Spotify Web API",
    })
}

fn api_track(client: &Client, token: &str, link: &SpotifyLink) -> Result<FetchedEntity, String> {
    let value = api_get(client, token, &format!("{API}/tracks/{}", link.id))?;
    let title = value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Untitled track")
        .to_string();
    let artist = first_artist(&value).unwrap_or_default();
    let album = value
        .get("album")
        .and_then(|a| a.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let cover_url = value.get("album").and_then(largest_image);
    let duration_secs = value.get("duration_ms").and_then(millis).unwrap_or(0.0);
    if duration_secs <= 0.0 {
        return Err("Spotify did not report the duration of this track.".to_string());
    }
    Ok(FetchedEntity {
        kind: SpotifyKind::Track,
        title: if album.is_empty() { title.clone() } else { album.clone() },
        artist: artist.clone(),
        cover_url,
        tracks: vec![FetchedTrack {
            spotify_id: Some(link.id.clone()),
            title,
            artist,
            album,
            duration_secs,
        }],
        source: "Spotify Web API",
    })
}

fn api_track_item(item: &Value, album: &str, album_artist: &str) -> Option<FetchedTrack> {
    let title = item.get("name").and_then(Value::as_str)?.trim().to_string();
    if title.is_empty() {
        return None;
    }
    let artist = first_artist(item).unwrap_or_else(|| album_artist.to_string());
    let duration_secs = item.get("duration_ms").and_then(millis).unwrap_or(0.0);
    Some(FetchedTrack {
        spotify_id: item
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_string),
        title,
        artist,
        album: album.to_string(),
        duration_secs,
    })
}

fn first_artist(value: &Value) -> Option<String> {
    value
        .get("artists")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(|a| a.get("name"))
        .and_then(Value::as_str)
        .map(|s| decode_entities(s.trim()))
        .filter(|s| !s.is_empty())
}

/// URL of the widest image in an object that has `images: [{url,width,height}]`.
fn largest_image(value: &Value) -> Option<String> {
    let images = value.get("images")?.as_array()?;
    images
        .iter()
        .max_by_key(|i| i.get("width").and_then(Value::as_u64).unwrap_or(0))
        .and_then(|i| i.get("url"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn millis(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_u64().map(|v| v as f64))
        .filter(|ms| ms.is_finite() && *ms > 0.0)
        .map(|ms| ms / 1000.0)
}

// ---- Public embed page -------------------------------------------------------------------------

fn fetch_public(link: &SpotifyLink) -> Result<FetchedEntity, String> {
    let client = client()?;
    let url = format!("{EMBED}/{}/{}", link.kind.as_str(), link.id);
    let response = client
        .get(&url)
        .send()
        .map_err(|e| network_error("Could not reach Spotify", e))?;
    if response.status().as_u16() == 404 {
        return Err("Spotify could not find this track or album.".to_string());
    }
    if !response.status().is_success() {
        return Err(format!(
            "Spotify answered with HTTP {} for this link.",
            response.status().as_u16()
        ));
    }
    let html = response
        .text()
        .map_err(|e| format!("Could not read the Spotify page: {e}"))?;
    let data = extract_next_data(&html).ok_or_else(|| {
        "Spotify did not return album data. The public page layout may have changed; add Spotify \
         Client ID/Secret in Settings > Music to use the Web API."
            .to_string()
    })?;
    let value: Value = serde_json::from_str(data).map_err(|e| format!("Spotify sent unreadable album data: {e}"))?;
    let entity = find_entity(&value).ok_or_else(|| {
        "The Spotify page did not contain a track or album. Is this a playlist or podcast link?".to_string()
    })?;
    parse_embed_entity(entity, link.kind)
}

/// The JSON blob of a Next.js page (`<script id="__NEXT_DATA__" ...>...</script>`).
pub fn extract_next_data(html: &str) -> Option<&str> {
    let marker = html.find("id=\"__NEXT_DATA__\"")?;
    let start = html[marker..].find('>')? + marker + 1;
    let end = html[start..].find("</script>")? + start;
    let data = html[start..end].trim();
    (!data.is_empty()).then_some(data)
}

/// Find the entity object. The documented path is `props.pageProps.state.data.entity`; the deep
/// search keeps the parser alive if Spotify moves the object one level.
fn find_entity(value: &Value) -> Option<&Value> {
    if let Some(entity) = value
        .get("props")
        .and_then(|v| v.get("pageProps"))
        .and_then(|v| v.get("state"))
        .and_then(|v| v.get("data"))
        .and_then(|v| v.get("entity"))
    {
        return Some(entity);
    }
    fn walk(value: &Value, depth: usize) -> Option<&Value> {
        if depth == 0 {
            return None;
        }
        if let Some(obj) = value.as_object() {
            if obj.contains_key("trackList") || obj.get("type").and_then(Value::as_str) == Some("track") {
                return Some(value);
            }
            for child in obj.values() {
                if let Some(found) = walk(child, depth - 1) {
                    return Some(found);
                }
            }
        } else if let Some(array) = value.as_array() {
            for child in array {
                if let Some(found) = walk(child, depth - 1) {
                    return Some(found);
                }
            }
        }
        None
    }
    walk(value, 12)
}

fn parse_embed_entity(entity: &Value, kind: SpotifyKind) -> Result<FetchedEntity, String> {
    let title = entity
        .get("title")
        .or_else(|| entity.get("name"))
        .and_then(Value::as_str)
        .map(|s| decode_entities(s.trim()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| match kind {
            SpotifyKind::Album => "Untitled album".to_string(),
            SpotifyKind::Track => "Untitled track".to_string(),
        });
    let artist = entity
        .get("subtitle")
        .and_then(Value::as_str)
        .map(|s| decode_entities(s.trim()))
        .filter(|s| !s.is_empty())
        .or_else(|| first_artist(entity))
        .unwrap_or_default();
    let cover_url = entity
        .get("visualIdentity")
        .and_then(|v| v.get("image"))
        .and_then(|images| images.as_array())
        .and_then(|images| {
            images
                .iter()
                .max_by_key(|i| i.get("maxWidth").and_then(Value::as_u64).unwrap_or(0))
        })
        .and_then(|i| i.get("url"))
        .and_then(Value::as_str)
        .map(str::to_string);

    match kind {
        SpotifyKind::Track => {
            let duration_secs = entity.get("duration").and_then(millis).unwrap_or(0.0);
            if duration_secs <= 0.0 {
                return Err("Spotify did not report the duration of this track.".to_string());
            }
            let album = entity
                .get("albumOfTrack")
                .and_then(|a| a.get("name"))
                .and_then(Value::as_str)
                .map(|s| decode_entities(s.trim()))
                .unwrap_or_default();
            let track_artist = first_artist(entity).unwrap_or_else(|| artist.clone());
            Ok(FetchedEntity {
                kind,
                title: if album.is_empty() { title.clone() } else { album.clone() },
                artist: artist.clone(),
                cover_url,
                tracks: vec![FetchedTrack {
                    spotify_id: entity.get("id").and_then(Value::as_str).map(str::to_string),
                    title,
                    artist: track_artist,
                    album,
                    duration_secs,
                }],
                source: "Spotify public page",
            })
        }
        SpotifyKind::Album => {
            let items = entity
                .get("trackList")
                .and_then(Value::as_array)
                .ok_or_else(|| "Spotify returned no track list for this album.".to_string())?;
            let album_title = entity_title(entity);
            let mut tracks = Vec::with_capacity(items.len());
            for item in items {
                let title = item
                    .get("title")
                    .and_then(Value::as_str)
                    .map(|s| decode_entities(s.trim()))
                    .filter(|s| !s.is_empty());
                let Some(title) = title else { continue };
                let duration_secs = item.get("duration").and_then(millis).unwrap_or(0.0);
                let item_artist = item
                    .get("subtitle")
                    .and_then(Value::as_str)
                    .map(|s| decode_entities(s.trim()))
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| artist.clone());
                let spotify_id = item
                    .get("uri")
                    .and_then(Value::as_str)
                    .and_then(|uri| uri.rsplit(':').next())
                    .filter(|id| !id.is_empty())
                    .map(str::to_string);
                tracks.push(FetchedTrack {
                    spotify_id,
                    title,
                    artist: item_artist,
                    album: album_title.clone(),
                    duration_secs,
                });
            }
            if tracks.is_empty() {
                return Err("Spotify returned no playable tracks for this album.".to_string());
            }
            Ok(FetchedEntity {
                kind,
                title,
                artist,
                cover_url,
                tracks,
                source: "Spotify public page",
            })
        }
    }
}

fn entity_title(entity: &Value) -> String {
    entity
        .get("title")
        .or_else(|| entity.get("name"))
        .and_then(Value::as_str)
        .map(|s| decode_entities(s.trim()))
        .unwrap_or_default()
}

/// Minimal HTML entity decoding for titles. JSON escapes are already handled by serde.
pub fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(idx) = rest.find('&') {
        out.push_str(&rest[..idx]);
        let tail = &rest[idx..];
        let (replacement, consumed) = if let Some(end) = tail.find(';') {
            let entity = &tail[1..end];
            let value = match entity {
                "amp" => Some('&'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "nbsp" => Some(' '),
                other => numeric_entity(other),
            };
            (value, end + 1)
        } else {
            (None, 1)
        };
        match replacement {
            Some(value) => out.push(value),
            None => out.push('&'),
        }
        rest = &tail[consumed..];
    }
    out.push_str(rest);
    out
}

fn numeric_entity(entity: &str) -> Option<char> {
    let digits = entity.strip_prefix('#')?;
    let hex = digits.strip_prefix('x').or_else(|| digits.strip_prefix('X'));
    let value = if let Some(hex) = hex {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        digits.parse::<u32>().ok()?
    };
    char::from_u32(value)
}

/// A version of a title that is more likely to match on SoundCloud/YouTube: remaster/edition
/// suffixes, bracketed qualifiers and trailing " - Live" style tags are removed. Used only for
/// the search query; the original title is still shown and its duration still guards the match.
pub fn clean_title(title: &str) -> String {
    const QUALIFIERS: &[&str] = &[
        "remaster",
        "remastered",
        "deluxe",
        "expanded",
        "anniversary",
        "edition",
        "version",
        "mono",
        "stereo",
        "bonus",
        "explicit",
        "digital",
        "reissue",
        "from the",
        "soundtrack",
        "motion picture",
        "radio edit",
        "single edit",
        "album edit",
        "demo",
        "live",
        "feat",
        "ft.",
    ];
    let mut cleaned = String::with_capacity(title.len());
    let mut depth = 0i32;
    let mut chunk = String::new();
    let mut opener = '(';
    for c in title.chars() {
        match c {
            '(' | '[' => {
                if depth == 0 {
                    chunk.clear();
                    opener = c;
                } else {
                    chunk.push(c);
                }
                depth += 1;
            }
            ')' | ']' => {
                depth = (depth - 1).max(0);
                if depth == 0 {
                    let lower = chunk.to_lowercase();
                    if QUALIFIERS.iter().any(|q| lower.contains(q)) {
                        // Remaster/edition/live/... qualifiers only hurt the search.
                    } else {
                        cleaned.push(' ');
                        cleaned.push(opener);
                        cleaned.push_str(&chunk);
                        cleaned.push(if opener == '(' { ')' } else { ']' });
                        cleaned.push(' ');
                    }
                } else {
                    chunk.push(c);
                }
            }
            _ if depth > 0 => chunk.push(c),
            _ => cleaned.push(c),
        }
    }
    let mut result = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    // Trailing " - Live", " - Remastered 2009", ... segments.
    if let Some((head, tail)) = result.rsplit_once(" - ") {
        let lower = tail.to_lowercase();
        if QUALIFIERS.iter().any(|q| lower.starts_with(q)) || lower.starts_with("from ") {
            result = head.trim().to_string();
        }
    }
    // A leading "Artist - " from a title that already contains the artist stays untouched; the
    // downloader builds "Artist - Title" itself.
    if result.is_empty() {
        title.trim().to_string()
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_common_spotify_link_shapes() {
        let album = parse_link("https://open.spotify.com/album/0ETFjACtuP2ADo6LFhL6HN?si=abc").unwrap();
        assert_eq!(album.kind, SpotifyKind::Album);
        assert_eq!(album.id, "0ETFjACtuP2ADo6LFhL6HN");

        let track = parse_link("https://open.spotify.com/intl-ru/track/4cOdK2wGLETKBW3PvgPWqT").unwrap();
        assert_eq!(track.kind, SpotifyKind::Track);
        assert_eq!(track.id, "4cOdK2wGLETKBW3PvgPWqT");

        let embed = parse_link("open.spotify.com/embed/album/0ETFjACtuP2ADo6LFhL6HN").unwrap();
        assert_eq!(embed.kind, SpotifyKind::Album);

        let uri = parse_link("spotify:track:4cOdK2wGLETKBW3PvgPWqT").unwrap();
        assert_eq!(uri.kind, SpotifyKind::Track);
    }

    #[test]
    fn rejects_other_hosts_and_entities() {
        assert!(parse_link("https://example.com/album/0ETFjACtuP2ADo6LFhL6HN").is_err());
        assert!(parse_link("https://open.spotify.com/playlist/0ETFjACtuP2ADo6LFhL6HN").is_err());
        assert!(parse_link("https://open.spotify.com/album/../../etc/passwd").is_err());
        assert!(parse_link("").is_err());
    }

    #[test]
    fn cleans_remaster_and_edition_suffixes() {
        assert_eq!(clean_title("Come Together - Remastered 2009"), "Come Together");
        assert_eq!(clean_title("Song (Deluxe Edition)"), "Song");
        assert_eq!(clean_title("Song (Live at Wembley)"), "Song");
        assert_eq!(clean_title("Song (feat. Someone)"), "Song");
        assert_eq!(clean_title("Song (Radio Edit)"), "Song");
        assert_eq!(clean_title("Song (Original Mix)"), "Song (Original Mix)");
        assert_eq!(clean_title("Song [Bonus Track]"), "Song");
        assert_eq!(clean_title("The Live Song"), "The Live Song");
    }

    #[test]
    fn parses_album_embed_json() {
        let entity = serde_json::json!({
            "type": "album",
            "title": "Abbey Road (Remastered 2009)",
            "subtitle": "The Beatles",
            "trackList": [
                {"uri":"spotify:track:t1","title":"Come Together","subtitle":"The Beatles","duration":259946},
                {"uri":"spotify:track:t2","title":"Something","subtitle":"The Beatles","duration":182293}
            ],
            "visualIdentity": {"image": [
                {"url":"https://img/cover300","maxWidth":300},
                {"url":"https://img/cover640","maxWidth":640}
            ]}
        });
        let parsed = parse_embed_entity(&entity, SpotifyKind::Album).unwrap();
        assert_eq!(parsed.title, "Abbey Road (Remastered 2009)");
        assert_eq!(parsed.artist, "The Beatles");
        assert_eq!(parsed.cover_url.as_deref(), Some("https://img/cover640"));
        assert_eq!(parsed.tracks.len(), 2);
        assert_eq!(parsed.tracks[0].title, "Come Together");
        assert!((parsed.tracks[0].duration_secs - 259.946).abs() < 0.01);
        assert_eq!(parsed.tracks[0].spotify_id.as_deref(), Some("t1"));
    }

    #[test]
    fn parses_track_embed_json() {
        let entity = serde_json::json!({
            "type": "track",
            "name": "Never Gonna Give You Up",
            "id": "4cOdK2wGLETKBW3PvgPWqT",
            "duration": 213573,
            "artists": [{"name": "Rick Astley"}],
            "visualIdentity": {"image": [{"url":"https://img/t","maxWidth":640}]}
        });
        let parsed = parse_embed_entity(&entity, SpotifyKind::Track).unwrap();
        assert_eq!(parsed.title, "Never Gonna Give You Up");
        assert_eq!(parsed.artist, "Rick Astley");
        assert_eq!(parsed.tracks[0].duration_secs, 213.573);
    }

    #[test]
    fn extracts_next_data_script() {
        let html = r#"<html><script id="__NEXT_DATA__" type="application/json">{"a":1}</script></html>"#;
        assert_eq!(extract_next_data(html), Some(r#"{"a":1}"#));
        assert_eq!(extract_next_data("<html></html>"), None);
    }

    #[test]
    fn decodes_basic_entities() {
        assert_eq!(decode_entities("Rock &amp; Roll"), "Rock & Roll");
        assert_eq!(decode_entities("Don&#39;t"), "Don't");
        assert_eq!(decode_entities("Don&#x27;t"), "Don't");
        assert_eq!(decode_entities("A & B"), "A & B");
    }

    #[test]
    fn credentials_fall_back_to_nothing_when_half_configured() {
        assert!(resolve_credentials("", "").is_none());
        assert!(resolve_credentials("id-only", "").is_none());
    }
}
