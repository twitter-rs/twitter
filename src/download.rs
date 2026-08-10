use std::path::Path;

use crate::viewer;
use crate::{Error, Tweet};

/// Local offline store: holds downloaded posts and writes `posts.json` plus
/// the offline HTML viewer into the output directory.
#[derive(Debug, Clone, Default)]
pub struct Store {
    tweets: Vec<Tweet>,
}

impl Store {
    /// Load an existing store from `dir/posts.json`, if present.
    pub fn load(dir: &Path) -> Result<Store, Error> {
        let path = dir.join("posts.json");
        if path.exists() {
            let raw = std::fs::read_to_string(&path)?;
            let tweets = serde_json::from_str(&raw).unwrap_or_default();
            Ok(Store { tweets })
        } else {
            Ok(Store::default())
        }
    }

    pub fn tweets(&self) -> &[Tweet] {
        &self.tweets
    }

    /// Add a post; returns `true` if it was new.
    pub fn add(&mut self, tweet: Tweet) -> bool {
        if self.tweets.iter().any(|t| t.id == tweet.id) {
            false
        } else {
            self.tweets.push(tweet);
            true
        }
    }

    pub fn len(&self) -> usize {
        self.tweets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tweets.is_empty()
    }

    /// Write `posts.json` (newest first) and regenerate the HTML viewer.
    pub fn save(&self, dir: &Path) -> Result<(), Error> {
        std::fs::create_dir_all(dir)?;
        let mut sorted = self.tweets.clone();
        sorted.sort_by(|a, b| b.id.cmp(&a.id));
        std::fs::write(dir.join("posts.json"), serde_json::to_vec_pretty(&sorted)?)?;
        viewer::write_index(dir, &sorted)?;
        Ok(())
    }
}
