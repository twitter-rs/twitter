use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("http {status}: {body}")]
    Http { status: u16, body: String },
    #[error("rate limited by X, try again later")]
    RateLimited,
    #[error("authentication failed (403) - cookies may be expired")]
    Forbidden,
    #[error("not found (404)")]
    NotFound,
    #[error("invalid request (graphql query ids may be stale): {0}")]
    InvalidRequest(String),
    #[error("invalid cookies: {0}")]
    Cookies(String),
    #[error("invalid target: {0}")]
    InvalidTarget(String),
    #[error("tweet not available: {0}")]
    TweetUnavailable(String),
    #[error("api error: {0}")]
    Api(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}
