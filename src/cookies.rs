use std::collections::BTreeMap;

use crate::Error;

#[derive(Debug, Clone, Default)]
pub struct CookieJar {
    cookies: BTreeMap<String, String>,
}

impl CookieJar {
    pub fn parse_file(text: &str) -> Result<Self, Error> {
        let mut jar = Self::default();
        for raw in text.lines() {
            let mut line = raw.trim();
            if line.is_empty() {
                continue;
            }
            line = line.strip_prefix("#HttpOnly_").unwrap_or(line).trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split('\t').map(str::trim).collect();
            if fields.len() >= 7 {
                jar.cookies
                    .insert(fields[5].to_string(), fields[6].to_string());
            }
        }
        Ok(jar)
    }

    pub fn parse_header(text: &str) -> Result<Self, Error> {
        let mut jar = Self::default();
        for part in text.split(';') {
            let mut it = part.trim().splitn(2, '=');
            if let (Some(k), Some(v)) = (it.next(), it.next()) {
                let k = k.trim();
                if !k.is_empty() {
                    jar.cookies.insert(k.to_string(), v.trim().to_string());
                }
            }
        }
        Ok(jar)
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.cookies.get(name).map(String::as_str)
    }

    pub fn csrf_token(&self) -> Option<&str> {
        self.get("ct0")
    }

    pub fn to_header(&self) -> String {
        self.cookies
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    pub fn is_empty(&self) -> bool {
        self.cookies.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NETSCAPE: &str = "# Netscape HTTP Cookie File
.x.com\tTRUE\t/\tTRUE\t1801772568\tauth_token\t814f8264645554ba0d4325bb3947230480e7ccfc
#HttpOnly_.x.com\tTRUE\t/\tTRUE\t1801772569\tct0\tabc123
x.com\tTRUE\t/\tFALSE\t0\tlang\ten
";

    #[test]
    fn parses_netscape_file() {
        let jar = CookieJar::parse_file(NETSCAPE).unwrap();
        assert_eq!(
            jar.get("auth_token"),
            Some("814f8264645554ba0d4325bb3947230480e7ccfc")
        );
        assert_eq!(jar.get("ct0"), Some("abc123"));
        assert_eq!(jar.get("lang"), Some("en"));
    }

    #[test]
    fn parses_header() {
        let jar = CookieJar::parse_header("auth_token=abc; ct0=def; lang=en").unwrap();
        assert_eq!(jar.get("auth_token"), Some("abc"));
        assert_eq!(jar.get("ct0"), Some("def"));
        assert_eq!(jar.csrf_token(), Some("def"));
    }

    #[test]
    fn builds_header() {
        let jar = CookieJar::parse_header("a=1; b=2").unwrap();
        let h = jar.to_header();
        assert!(h.contains("a=1"));
        assert!(h.contains("b=2"));
    }
}
