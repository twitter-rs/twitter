//! Offline viewer and media downloader for X (Twitter).
//!
//! Provides a [`Twitter`] client that fetches posts (tweets) and downloads
//! their media from X. Use it from the CLI binary or as a library:
//!
//! ```no_run
//! use twitter::{Target, Twitter};
//! # use twitter::CookieJar;
//!
//! let tw = Twitter::new().unwrap();
//! let user = tw.user("nasa").unwrap();
//! let tweets = tw.timeline(&user, Some(10)).unwrap();
//! for t in &tweets {
//!     println!("{}: {}", t.id, t.text);
//! }
//! ```
//!
//! (The example above requires network access to x.com; pass cookies via
//! [`Twitter::from_cookies_file`] to access protected accounts.)
//!
//! Authentication is optional: pass cookies (Netscape `cookies.txt` format,
//! containing `auth_token` and `ct0`) to access protected accounts and to
//! avoid rate limits. Without cookies, public posts are fetched via X's
//! public syndication endpoint.

pub mod api;
pub mod cookies;
pub mod download;
pub mod error;
pub mod models;
pub mod viewer;

pub use api::{Api, DEFAULT_UA};
pub use cookies::CookieJar;
pub use download::Store;
pub use error::Error;
pub use models::{Media, MediaKind, Tweet, User};

use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::{json, Value};
use url::Url;

use api::QueryIds;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    User(String),
    Tweet(String),
}

impl Target {
    pub fn parse(input: &str) -> Result<Target, Error> {
        let s = input.trim().strip_prefix('@').unwrap_or(input.trim());
        if let Some(t) = parse_tweet_url(s)? {
            return Ok(t);
        }
        if s.len() >= 15 && s.chars().all(|c| c.is_ascii_digit()) {
            return Ok(Target::Tweet(s.to_string()));
        }
        Ok(Target::User(s.to_string()))
    }
}

fn parse_tweet_url(s: &str) -> Result<Option<Target>, Error> {
    let url = match Url::parse(s) {
        Ok(u) => u,
        Err(_) => return Ok(None),
    };
    let host = url.host_str().unwrap_or("");
    if host != "x.com"
        && host != "twitter.com"
        && !host.ends_with(".x.com")
        && !host.ends_with(".twitter.com")
    {
        return Err(Error::InvalidTarget(format!("not an x.com URL: {s}")));
    }
    let segs: Vec<&str> = url
        .path_segments()
        .map(|s| s.filter(|x| !x.is_empty()).collect())
        .unwrap_or_default();
    match segs.as_slice() {
        [_, "status", id] => Ok(Some(Target::Tweet(id.to_string()))),
        ["i", "web", "status", id] => Ok(Some(Target::Tweet(id.to_string()))),
        [user] => Ok(Some(Target::User(user.to_string()))),
        _ => Err(Error::InvalidTarget(format!("unrecognized x.com URL: {s}"))),
    }
}

pub struct Twitter {
    api: Api,
    query_ids: Mutex<QueryIds>,
    refreshed: AtomicBool,
}

impl Twitter {
    /// Create a client without cookies (public syndication access only).
    pub fn new() -> Result<Twitter, Error> {
        Self::with_cookies(CookieJar::default())
    }

    /// Create a client with cookies parsed from a Netscape-format cookies file
    /// (e.g. exported with `auth_token` and `ct0` entries).
    pub fn from_cookies_file(path: impl AsRef<Path>) -> Result<Twitter, Error> {
        let raw = std::fs::read_to_string(path)?;
        let cookies = CookieJar::parse_file(&raw)?;
        Self::with_cookies(cookies)
    }

    /// Create a client from an inline `key=value; key2=value2` cookie string.
    pub fn from_cookie_header(header: &str) -> Result<Twitter, Error> {
        let cookies = CookieJar::parse_header(header)?;
        Self::with_cookies(cookies)
    }

    pub fn with_cookies(cookies: CookieJar) -> Result<Twitter, Error> {
        Ok(Twitter {
            api: Api::new(cookies)?,
            query_ids: Mutex::new(QueryIds::default()),
            refreshed: AtomicBool::new(false),
        })
    }

    pub fn cookies(&self) -> &CookieJar {
        self.api.cookies()
    }

    /// Re-extract the current GraphQL query ids from the live x.com web
    /// bundle. Called automatically when a request fails with stale ids.
    pub fn refresh_query_ids(&self) -> Result<(), Error> {
        let ids = api::discover_query_ids(&self.api)?;
        *self.query_ids.lock().unwrap() = ids;
        Ok(())
    }

    fn qids(&self) -> QueryIds {
        self.query_ids.lock().unwrap().clone()
    }

    fn with_retry<T, F>(&self, f: F) -> Result<T, Error>
    where
        F: Fn() -> Result<T, Error>,
    {
        let mut attempts = 0;
        loop {
            match f() {
                Ok(v) => return Ok(v),
                Err(Error::RateLimited) => {
                    attempts += 1;
                    if attempts >= 4 {
                        return Err(Error::RateLimited);
                    }
                    std::thread::sleep(Duration::from_secs(5 * attempts));
                }
                Err(Error::InvalidRequest(_)) if !self.refreshed.swap(true, Ordering::SeqCst) => {
                    let _ = self.refresh_query_ids();
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Look up a user by screen name (e.g. `"nasa"`).
    pub fn user(&self, screen_name: &str) -> Result<User, Error> {
        let vars = json!({
            "screen_name": screen_name,
            "withSafetyModeUserFields": true,
            "withSuperFollowsUserFields": true
        });
        let resp = self.with_retry(|| {
            self.api
                .graphql(&self.qids().user_by_screen_name, "UserByScreenName", &vars)
        })?;
        let result = &resp["data"]["user"]["result"];
        if result["__typename"].as_str() == Some("UserUnavailable") {
            return Err(Error::Api(format!("user @{screen_name} is unavailable")));
        }
        let legacy = &result["legacy"];
        Ok(User {
            id: result["rest_id"].as_str().unwrap_or("").to_string(),
            screen_name: legacy["screen_name"]
                .as_str()
                .or(result["core"]["screen_name"].as_str())
                .unwrap_or(screen_name)
                .to_string(),
            name: legacy["name"]
                .as_str()
                .or(result["core"]["name"].as_str())
                .unwrap_or(screen_name)
                .to_string(),
        })
    }

    /// Fetch a user's timeline, paginating until exhausted or `max` posts
    /// collected.
    pub fn timeline(&self, user: &User, max: Option<usize>) -> Result<Vec<Tweet>, Error> {
        let mut tweets: Vec<Tweet> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut cursor: Option<String> = None;
        loop {
            let mut vars = json!({
                "userId": user.id,
                "count": 40,
                "includePromotedContent": false,
                "withQuickPromoteEligibilityTweetFields": true,
                "withVoice": true,
                "withVideos": true
            });
            if let Some(c) = &cursor {
                vars["cursor"] = Value::String(c.clone());
            }
            let resp = self.with_retry(|| {
                self.api
                    .graphql(&self.qids().user_tweets, "UserTweets", &vars)
            })?;
            let result = &resp["data"]["user"]["result"];
            if result["__typename"].as_str() == Some("UserUnavailable") {
                break;
            }
            let Some(instructions) = result["timeline"]["timeline"]["instructions"].as_array()
            else {
                return Err(Error::Api("unexpected UserTweets response".into()));
            };
            let mut next_cursor = None;
            for inst in instructions {
                let Some(entries) = inst["entries"].as_array() else {
                    continue;
                };
                for entry in entries {
                    let content = &entry["content"];
                    match content["entryType"].as_str() {
                        Some("TimelineTimelineItem") | Some("TimelinePinEntry") => {
                            collect_tweet(content, &mut seen, &mut tweets);
                        }
                        Some("TimelineTimelineModule") => {
                            if let Some(items) = content["items"].as_array() {
                                for item in items {
                                    collect_tweet(
                                        &item["item"]["itemContent"],
                                        &mut seen,
                                        &mut tweets,
                                    );
                                }
                            }
                        }
                        Some("TimelineTimelineCursor")
                            if content["cursorType"].as_str() == Some("Bottom") =>
                        {
                            next_cursor = content["value"].as_str().map(str::to_string);
                        }
                        _ => {}
                    }
                }
            }
            if let Some(m) = max {
                if tweets.len() >= m {
                    tweets.truncate(m);
                    break;
                }
            }
            match next_cursor {
                Some(c) if !c.is_empty() && cursor.as_deref() != Some(&c) => {
                    cursor = Some(c);
                    std::thread::sleep(Duration::from_millis(250));
                }
                _ => break,
            }
        }
        Ok(tweets)
    }

    /// Fetch a single post by id, e.g. `"2075726968335618499"`.
    /// Falls back to the public syndication endpoint when the authenticated
    /// API fails or the post is not accessible with the configured cookies.
    pub fn tweet(&self, id: &str) -> Result<Tweet, Error> {
        let vars = json!({
            "tweetId": id,
            "with_rux_injections": false,
            "includePromotedContent": true,
            "withCommunity": true,
            "withQuickPromoteEligibilityTweetFields": true,
            "withBirdwatchNotes": true,
            "withVoice": true
        });
        let result = self.with_retry(|| {
            self.api.graphql(
                &self.qids().tweet_result_by_rest_id,
                "TweetResultByRestId",
                &vars,
            )
        });
        if let Ok(resp) = result {
            let node = &resp["data"]["tweetResult"]["result"];
            if !node.is_null() {
                if let Ok(t) = Tweet::from_graphql_result(node) {
                    return Ok(t);
                }
            }
        }
        self.tweet_via_syndication(id)
    }

    fn tweet_via_syndication(&self, id: &str) -> Result<Tweet, Error> {
        let token = syndication_token(id)?;
        let url = format!(
            "{}/tweet-result?id={id}&lang=en&token={token}",
            api::SYNDICATION_ORIGIN
        );
        let v = self.api.get_json(&url)?;
        Tweet::from_syndication(&v)
    }

    /// Download all media of a post into `dir/media/`, skipping files that
    /// already exist. Returns the saved file paths.
    pub fn download_media(
        &self,
        tweet: &Tweet,
        dir: &Path,
    ) -> Result<Vec<std::path::PathBuf>, Error> {
        let media_dir = dir.join("media");
        std::fs::create_dir_all(&media_dir)?;
        let mut saved = Vec::new();
        for (i, m) in tweet.media.iter().enumerate() {
            let name = media_file_name(tweet.id.as_str(), i, m);
            let path = media_dir.join(&name);
            if path.exists() {
                if path.metadata()?.len() == 0 {
                    std::fs::remove_file(&path)?;
                } else {
                    saved.push(path);
                    continue;
                }
            }
            match self.stream_with_retry(&m.url, &path) {
                Ok(()) => saved.push(path),
                Err(e) => eprintln!("  warn: {} ({name})", e),
            }
            if let Some(thumb) = &m.thumbnail {
                let thumb_path = media_dir.join(poster_file_name(tweet.id.as_str(), i, m));
                if !thumb_path.exists() {
                    let _ = self.stream_with_retry(thumb, &thumb_path);
                }
            }
        }
        Ok(saved)
    }

    fn stream_with_retry(&self, url: &str, path: &std::path::Path) -> Result<(), Error> {
        let alt = strip_tag(url);
        let mut attempts = 0;
        loop {
            let mut result = self.api.stream_to(url, path);
            if let (Err(Error::RateLimited), Some(alt)) = (&result, alt.as_deref()) {
                let _ = std::fs::remove_file(path);
                result = self.api.stream_to(alt, path);
            }
            match result {
                Ok(()) => return Ok(()),
                Err(Error::RateLimited) => {
                    attempts += 1;
                    if attempts >= 4 {
                        return Err(Error::RateLimited);
                    }
                    std::thread::sleep(Duration::from_secs(5 * attempts));
                }
                Err(e) => {
                    let _ = std::fs::remove_file(path);
                    attempts += 1;
                    if attempts >= 4 {
                        return Err(e);
                    }
                    std::thread::sleep(Duration::from_secs(2 * attempts));
                }
            }
        }
    }
}

/// File name used for a tweet's media item, e.g. `2075726968335618499_1.mp4`.
pub fn media_file_name(tweet_id: &str, index: usize, media: &Media) -> String {
    let mut ext = models::extension_for(&media.url);
    if ext.is_empty() {
        ext = match media.kind {
            MediaKind::Photo => "jpg".into(),
            MediaKind::Video | MediaKind::Gif => "mp4".into(),
        };
    }
    format!("{tweet_id}_{}.{ext}", index + 1)
}

pub fn poster_file_name(tweet_id: &str, index: usize, media: &Media) -> String {
    let ext = media
        .thumbnail
        .as_deref()
        .map(models::extension_for)
        .filter(|e| !e.is_empty())
        .unwrap_or_else(|| "jpg".into());
    format!("{tweet_id}_{}_poster.{ext}", index + 1)
}

fn collect_tweet(content: &Value, seen: &mut HashSet<String>, out: &mut Vec<Tweet>) {
    let result = &content["itemContent"]["tweet_results"]["result"];
    if result.is_null() {
        return;
    }
    if let Ok(t) = Tweet::from_graphql_result(result) {
        if seen.insert(t.id.clone()) {
            out.push(t);
        }
    }
}

fn syndication_token(id: &str) -> Result<String, Error> {
    let n: u64 = id
        .parse()
        .map_err(|_| Error::InvalidTarget(format!("invalid tweet id: {id}")))?;
    Ok(format!("tw{}", URL_SAFE_NO_PAD.encode(n.to_be_bytes())))
}

/// CDN video URLs carry a session-bound `?tag=` parameter that is often
/// rejected (429) when replayed; the file itself is served without it.
fn strip_tag(url: &str) -> Option<String> {
    let (base, query) = url.split_once('?')?;
    let parts: Vec<&str> = query.split('&').collect();
    let stripped: Vec<&str> = parts
        .iter()
        .copied()
        .filter(|p| !p.starts_with("tag="))
        .collect();
    if stripped.len() == parts.len() {
        return None;
    }
    Some(if stripped.is_empty() {
        base.to_string()
    } else {
        format!("{base}?{}", stripped.join("&"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syndication_token_matches_live_algorithm() {
        assert_eq!(
            syndication_token("2075726968335618499").unwrap(),
            "twHM52rhZX0cM"
        );
    }

    #[test]
    fn parses_targets() {
        assert_eq!(
            Target::parse("https://x.com/nasa/status/123?s=1").unwrap(),
            Target::Tweet("123".into())
        );
        assert_eq!(
            Target::parse("https://twitter.com/i/web/status/456").unwrap(),
            Target::Tweet("456".into())
        );
        assert_eq!(
            Target::parse("https://x.com/nasa").unwrap(),
            Target::User("nasa".into())
        );
        assert_eq!(Target::parse("@nasa").unwrap(), Target::User("nasa".into()));
        assert_eq!(Target::parse("nasa").unwrap(), Target::User("nasa".into()));
        assert_eq!(
            Target::parse("2075726968335618499").unwrap(),
            Target::Tweet("2075726968335618499".into())
        );
        assert!(Target::parse("https://example.com/foo").is_err());
    }

    #[test]
    fn media_names() {
        let m = Media {
            kind: MediaKind::Photo,
            url: "https://pbs.twimg.com/media/x.jpg?name=orig".into(),
            thumbnail: None,
            width: None,
            height: None,
        };
        assert_eq!(media_file_name("42", 0, &m), "42_1.jpg");
        assert_eq!(poster_file_name("42", 0, &m), "42_1_poster.jpg");
    }

    #[test]
    fn strips_tag_param() {
        assert_eq!(
            strip_tag("https://video.twimg.com/x/v.mp4?tag=14").as_deref(),
            Some("https://video.twimg.com/x/v.mp4")
        );
        assert_eq!(
            strip_tag("https://video.twimg.com/x/v.mp4?tag=14&a=1").as_deref(),
            Some("https://video.twimg.com/x/v.mp4?a=1")
        );
        assert_eq!(strip_tag("https://video.twimg.com/x/v.mp4"), None);
    }
}
