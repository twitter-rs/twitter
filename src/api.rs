use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::Value;

use crate::cookies::CookieJar;
use crate::Error;

pub const API_ORIGIN: &str = "https://x.com";
pub const SYNDICATION_ORIGIN: &str = "https://cdn.syndication.twimg.com";
pub const DEFAULT_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";
const PUBLIC_BEARER: &str = "AAAAAAAAAAAAAAAAAAAAANRILgAAAAAAnNwIzUejRCOuH5E6I8xnZz4puTs%3D1Zv7ttfk8LF81IUq16cHjhLTvJu4FA33AGWWjCpTnA";

#[derive(Debug, Clone)]
pub struct QueryIds {
    pub user_by_screen_name: String,
    pub user_tweets: String,
    pub tweet_result_by_rest_id: String,
}

impl Default for QueryIds {
    fn default() -> Self {
        Self {
            user_by_screen_name: "Gb-d6r0vxPOADdG62OEBpQ".into(),
            user_tweets: "SXVCYB8XHSS25nzIljNtZA".into(),
            tweet_result_by_rest_id: "GZsN2Pc4knAoit6pXa4HSA".into(),
        }
    }
}

pub struct Api {
    http: Client,
    media: Client,
    cookies: CookieJar,
}

impl Api {
    pub fn new(cookies: CookieJar) -> Result<Self, Error> {
        let http = Client::builder()
            .user_agent(DEFAULT_UA)
            .timeout(Duration::from_secs(120))
            .connect_timeout(Duration::from_secs(20))
            .build()?;
        let media = Client::builder()
            .user_agent(DEFAULT_UA)
            .timeout(Duration::from_secs(7200))
            .connect_timeout(Duration::from_secs(20))
            .build()?;
        Ok(Self {
            http,
            media,
            cookies,
        })
    }

    pub fn cookies(&self) -> &CookieJar {
        &self.cookies
    }

    pub fn graphql(&self, query_id: &str, op: &str, variables: &Value) -> Result<Value, Error> {
        let url = format!("{API_ORIGIN}/i/api/graphql/{query_id}/{op}");
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {PUBLIC_BEARER}")).unwrap(),
        );
        headers.insert("x-twitter-active-user", HeaderValue::from_static("yes"));
        headers.insert("x-twitter-client-language", HeaderValue::from_static("en"));
        headers.insert(
            "cookie",
            HeaderValue::from_str(&self.cookies.to_header())
                .unwrap_or_else(|_| HeaderValue::from_static("")),
        );
        if let Some(ct0) = self.cookies.csrf_token() {
            headers.insert(
                "x-csrf-token",
                HeaderValue::from_str(ct0).unwrap_or_else(|_| HeaderValue::from_static("")),
            );
        }
        let resp = self
            .http
            .get(&url)
            .headers(headers)
            .query(&[("variables", variables.to_string())])
            .send()?;
        parse_json_response(resp)
    }

    pub fn get_json(&self, url: &str) -> Result<Value, Error> {
        let resp = self.http.get(url).send()?;
        parse_json_response(resp)
    }

    pub fn get_bytes(&self, url: &str) -> Result<Vec<u8>, Error> {
        let resp = self.http.get(url).send()?;
        match resp.status().as_u16() {
            200 => Ok(resp.bytes()?.to_vec()),
            404 => Err(Error::NotFound),
            429 => Err(Error::RateLimited),
            s => Err(Error::Http {
                status: s,
                body: String::new(),
            }),
        }
    }

    /// Stream a response body directly to disk (for large media files).
    pub fn stream_to(&self, url: &str, path: &std::path::Path) -> Result<(), Error> {
        let mut resp = self.media.get(url).send()?;
        match resp.status().as_u16() {
            200 => {
                let mut file = std::fs::File::create(path)?;
                resp.copy_to(&mut file)?;
                Ok(())
            }
            404 => Err(Error::NotFound),
            429 => Err(Error::RateLimited),
            s => Err(Error::Http {
                status: s,
                body: String::new(),
            }),
        }
    }
}

fn parse_json_response(resp: reqwest::blocking::Response) -> Result<Value, Error> {
    let status = resp.status().as_u16();
    let body = resp.text().unwrap_or_default();
    match status {
        200 => {
            let v: Value = serde_json::from_str(&body).map_err(|_| {
                Error::Api(format!(
                    "invalid json response: {}",
                    body.chars().take(200).collect::<String>()
                ))
            })?;
            if let Some(errors) = v["errors"].as_array() {
                if let Some(e) = errors.first() {
                    let code = e["code"].as_i64().unwrap_or(0);
                    let msg = e["message"].as_str().unwrap_or("graphql error");
                    if code == 368 || msg.contains("invalid") {
                        return Err(Error::InvalidRequest(msg.to_string()));
                    }
                    if code == 63 || code == 50 {
                        return Err(Error::Forbidden);
                    }
                    return Err(Error::Api(msg.to_string()));
                }
            }
            Ok(v)
        }
        429 => Err(Error::RateLimited),
        403 => Err(Error::Forbidden),
        404 => Err(Error::NotFound),
        400 => Err(Error::InvalidRequest(body.chars().take(200).collect())),
        s => Err(Error::Http {
            status: s,
            body: body.chars().take(200).collect(),
        }),
    }
}

static BUNDLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"src="([^"]*responsive-web/client-web/main\.[a-f0-9]+\.js)""#).unwrap()
});
static QUERY_ID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"queryId:"([A-Za-z0-9_-]{15,})",operationName:"([A-Za-z0-9_]+)""#).unwrap()
});

/// Extracts current GraphQL query ids from the x.com web client bundle.
pub fn discover_query_ids(api: &Api) -> Result<QueryIds, Error> {
    let mut home = api.get_bytes(&format!("{API_ORIGIN}/home"))?;
    if home.is_empty() {
        home = api.get_bytes(&format!("{API_ORIGIN}/"))?;
    }
    let home = String::from_utf8_lossy(&home);
    let bundle_url = BUNDLE_RE
        .captures(&home)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .ok_or_else(|| Error::Api("could not find main bundle in x.com html".into()))?;
    let js = api.get_bytes(&bundle_url)?;
    let js = String::from_utf8_lossy(&js);

    let mut ids = QueryIds::default();
    let mut found = 0;
    for cap in QUERY_ID_RE.captures_iter(&js) {
        let qid = cap[1].to_string();
        match &cap[2] {
            "UserTweets" => {
                ids.user_tweets = qid;
                found += 1;
            }
            "UserByScreenName" => {
                ids.user_by_screen_name = qid;
                found += 1;
            }
            "TweetResultByRestId" => {
                ids.tweet_result_by_rest_id = qid;
                found += 1;
            }
            _ => {}
        }
    }
    if found == 0 {
        return Err(Error::Api(
            "could not extract query ids from the x.com web bundle".into(),
        ));
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_ids_are_present() {
        let ids = QueryIds::default();
        assert!(ids.user_tweets.len() >= 15);
        assert!(ids.user_by_screen_name.len() >= 15);
        assert!(ids.tweet_result_by_rest_id.len() >= 15);
    }

    #[test]
    fn query_id_regex_matches_bundle_format() {
        let js =
            r#"queryId:"SXVCYB8XHSS25nzIljNtZA",operationName:"UserTweets",operationType:"query""#;
        let caps = QUERY_ID_RE.captures(js).unwrap();
        assert_eq!(&caps[1], "SXVCYB8XHSS25nzIljNtZA");
        assert_eq!(&caps[2], "UserTweets");
    }
}
