use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct User {
    pub id: String,
    pub screen_name: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Photo,
    Video,
    Gif,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Media {
    pub kind: MediaKind,
    /// Direct download URL (original quality photo, or best mp4 variant).
    pub url: String,
    /// Thumbnail/poster URL (videos and gifs).
    pub thumbnail: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tweet {
    pub id: String,
    pub author: User,
    pub text: String,
    pub created_at: Option<String>,
    #[serde(default)]
    pub is_retweet: bool,
    pub media: Vec<Media>,
    pub url: String,
}

impl Tweet {
    pub fn from_graphql_result(v: &Value) -> Result<Tweet, Error> {
        let mut r = v;
        match r["__typename"].as_str() {
            Some("Tweet") => {}
            Some("TweetWithVisibilityResults") => r = &r["tweet"],
            Some("TweetUnavailable") => {
                let reason = r["reason"].as_str().unwrap_or("unavailable");
                return Err(Error::TweetUnavailable(reason.to_string()));
            }
            Some(other) => {
                return Err(Error::Api(format!("unexpected tweet result type: {other}")))
            }
            None => return Err(Error::Api("missing __typename in tweet result".into())),
        }
        let legacy = &r["legacy"];
        let id = r["rest_id"]
            .as_str()
            .or(legacy["id_str"].as_str())
            .unwrap_or("")
            .to_string();
        if id.is_empty() {
            return Err(Error::Api("tweet missing id".into()));
        }
        let u = &r["core"]["user_results"]["result"];
        let ulegacy = &u["legacy"];
        let author = User {
            id: u["rest_id"].as_str().unwrap_or("").to_string(),
            screen_name: ulegacy["screen_name"]
                .as_str()
                .or(u["core"]["screen_name"].as_str())
                .unwrap_or("")
                .to_string(),
            name: ulegacy["name"]
                .as_str()
                .or(u["core"]["name"].as_str())
                .unwrap_or("")
                .to_string(),
        };
        let text = legacy["full_text"].as_str().unwrap_or("").to_string();
        let created_at = legacy["created_at"].as_str().map(str::to_string);
        let is_retweet = legacy["retweeted_status_result"].is_object();
        let mut media = Vec::new();
        media_from_entities(&legacy["extended_entities"], &mut media);
        if media.is_empty() {
            media_from_entities(&legacy["entities"], &mut media);
        }
        let url = format!("https://x.com/{}/status/{}", author.screen_name, id);
        Ok(Tweet {
            id,
            author,
            text,
            created_at,
            is_retweet,
            media,
            url,
        })
    }

    pub fn from_syndication(v: &Value) -> Result<Tweet, Error> {
        if let Some(errors) = v["errors"].as_array() {
            if let Some(e) = errors.first() {
                let msg = e["message"].as_str().unwrap_or("unavailable");
                return Err(Error::TweetUnavailable(msg.to_string()));
            }
        }
        let id = v["id_str"].as_str().unwrap_or("").to_string();
        if id.is_empty() {
            return Err(Error::Api("syndication response missing id_str".into()));
        }
        let u = &v["user"];
        let author = User {
            id: u["id_str"].as_str().unwrap_or("").to_string(),
            screen_name: u["screen_name"].as_str().unwrap_or("").to_string(),
            name: u["name"].as_str().unwrap_or("").to_string(),
        };
        let text = v["text"].as_str().unwrap_or("").to_string();
        let created_at = v["created_at"].as_str().map(str::to_string);
        let is_retweet = text.starts_with("RT @");
        let mut media = Vec::new();
        if let Some(details) = v["mediaDetails"].as_array() {
            for m in details {
                media_from_media_detail(m, &mut media);
            }
        }
        let url = format!("https://x.com/{}/status/{}", author.screen_name, id);
        Ok(Tweet {
            id,
            author,
            text,
            created_at,
            is_retweet,
            media,
            url,
        })
    }
}

fn media_from_entities(entities: &Value, out: &mut Vec<Media>) {
    let Some(arr) = entities.get("media").and_then(Value::as_array) else {
        return;
    };
    for m in arr {
        let Some(base) = m["media_url_https"]
            .as_str()
            .or(m["media_url"].as_str())
            .map(str::to_string)
        else {
            continue;
        };
        let width = m["original_info"]["width"].as_u64().map(|v| v as u32);
        let height = m["original_info"]["height"].as_u64().map(|v| v as u32);
        if m["type"].as_str() == Some("photo") {
            out.push(Media {
                kind: MediaKind::Photo,
                url: format!("{base}?name=orig"),
                thumbnail: None,
                width,
                height,
            });
        }
        let kind = match m["type"].as_str() {
            Some("animated_gif") => Some(MediaKind::Gif),
            Some("video") => Some(MediaKind::Video),
            _ => None,
        };
        if let (Some(kind), Some(url)) = (kind, best_mp4(&m["video_info"]["variants"])) {
            out.push(Media {
                kind,
                url,
                thumbnail: Some(base),
                width,
                height,
            });
        }
    }
}

fn media_from_media_detail(m: &Value, out: &mut Vec<Media>) {
    let Some(base) = m["media_url_https"].as_str().map(str::to_string) else {
        return;
    };
    let width = m["width"].as_u64().map(|v| v as u32);
    let height = m["height"].as_u64().map(|v| v as u32);
    if m["type"].as_str() == Some("photo") {
        out.push(Media {
            kind: MediaKind::Photo,
            url: format!("{base}?name=orig"),
            thumbnail: None,
            width,
            height,
        });
    }
    let kind = match m["type"].as_str() {
        Some("animated_gif") => Some(MediaKind::Gif),
        Some("video") => Some(MediaKind::Video),
        _ => None,
    };
    if let (Some(kind), Some(url)) = (kind, best_mp4(&m["video_info"]["variants"])) {
        out.push(Media {
            kind,
            url,
            thumbnail: Some(base),
            width,
            height,
        });
    }
}

fn best_mp4(variants: &Value) -> Option<String> {
    variants
        .as_array()?
        .iter()
        .filter(|v| v["content_type"].as_str() == Some("video/mp4"))
        .max_by_key(|v| v["bitrate"].as_u64().unwrap_or(0))
        .and_then(|v| v["url"].as_str().map(str::to_string))
}

pub fn extension_for(url: &str) -> String {
    let path = url.split('?').next().unwrap_or(url);
    let last = path.rsplit('/').next().unwrap_or("");
    let ext = last.rsplit('.').next().unwrap_or("");
    if (2..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        ext.to_lowercase()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn picks_best_mp4_variant() {
        let v = json!([
            {"content_type": "application/x-mpegURL", "url": "x.m3u8"},
            {"content_type": "video/mp4", "bitrate": 832000, "url": "low.mp4"},
            {"content_type": "video/mp4", "bitrate": 2176000, "url": "high.mp4"},
        ]);
        assert_eq!(best_mp4(&v), Some("high.mp4".to_string()));
    }

    #[test]
    fn unwraps_visibility_results() {
        let v = json!({
            "__typename": "TweetWithVisibilityResults",
            "tweet": {
                "__typename": "Tweet",
                "rest_id": "123",
                "core": {"user_results": {"result": {
                    "rest_id": "42",
                    "legacy": {"screen_name": "bob", "name": "Bob"}
                }}},
                "legacy": {
                    "full_text": "hello world",
                    "created_at": "Wed Jul 10 23:40:49 +0000 2026",
                    "extended_entities": {"media": [
                        {"type": "photo", "media_url_https": "https://pbs.twimg.com/media/x.jpg"}
                    ]}
                }
            }
        });
        let t = Tweet::from_graphql_result(&v).unwrap();
        assert_eq!(t.id, "123");
        assert_eq!(t.text, "hello world");
        assert_eq!(t.author.screen_name, "bob");
        assert_eq!(t.media.len(), 1);
        assert_eq!(t.media[0].kind, MediaKind::Photo);
        assert!(t.media[0].url.ends_with("?name=orig"));
    }

    #[test]
    fn marks_retweets() {
        let v = json!({
            "__typename": "Tweet",
            "rest_id": "1",
            "core": {"user_results": {"result": {"rest_id": "2", "legacy": {"screen_name": "a", "name": "A"}}}},
            "legacy": {
                "full_text": "RT @x: hi",
                "retweeted_status_result": {"result": {"__typename": "Tweet"}}
            }
        });
        assert!(Tweet::from_graphql_result(&v).unwrap().is_retweet);
    }

    #[test]
    fn syndication_errors() {
        let v = json!({"errors": [{"message": "Could not find tweet with id: 5"}]});
        assert!(matches!(
            Tweet::from_syndication(&v),
            Err(Error::TweetUnavailable(_))
        ));
    }

    #[test]
    fn extracts_extension() {
        assert_eq!(
            extension_for("https://pbs.twimg.com/media/x.jpg?name=orig"),
            "jpg"
        );
        assert_eq!(
            extension_for("https://video.twimg.com/a/b.mp4?tag=14"),
            "mp4"
        );
        assert_eq!(extension_for("https://pbs.twimg.com/thumb/x"), "");
    }
}
