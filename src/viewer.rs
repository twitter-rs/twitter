use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;

use serde::Serialize;

use crate::{media_file_name, poster_file_name, Error, MediaKind, Tweet};

#[derive(Serialize)]
struct ViewMedia {
    kind: &'static str,
    file: String,
    poster: Option<String>,
}

#[derive(Serialize)]
struct ViewTweet<'a> {
    id: &'a str,
    url: &'a str,
    text: &'a str,
    created_at: Option<&'a str>,
    author_name: &'a str,
    author_screen_name: &'a str,
    media: Vec<ViewMedia>,
}

/// Build the self-contained offline viewer page (no external resources).
pub fn build_index_html(tweets: &[Tweet]) -> Result<String, Error> {
    let mut view: Vec<ViewTweet> = Vec::with_capacity(tweets.len());
    for t in tweets {
        let mut media = Vec::with_capacity(t.media.len());
        for (i, m) in t.media.iter().enumerate() {
            media.push(ViewMedia {
                kind: match m.kind {
                    MediaKind::Photo => "Photo",
                    MediaKind::Video => "Video",
                    MediaKind::Gif => "Gif",
                },
                file: media_file_name(&t.id, i, m),
                poster: (m.kind != MediaKind::Photo && m.thumbnail.is_some())
                    .then(|| poster_file_name(&t.id, i, m)),
            });
        }
        view.push(ViewTweet {
            id: &t.id,
            url: &t.url,
            text: &t.text,
            created_at: t.created_at.as_deref(),
            author_name: &t.author.name,
            author_screen_name: &t.author.screen_name,
            media,
        });
    }
    let json = serde_json::to_string(&view)?
        .replace('<', "\\u003c")
        .replace('>', "\\u003e");
    Ok(format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Twitter Offline Viewer</title>
<style>
  :root {{ color-scheme: dark; }}
  * {{ box-sizing: border-box; }}
  body {{ margin: 0; background: #000; color: #e7e9ea; font: 15px/1.45 -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; }}
  header {{ position: sticky; top: 0; z-index: 10; background: rgba(0,0,0,.92); backdrop-filter: blur(6px); border-bottom: 1px solid #2f3336; padding: 12px 20px; display: flex; gap: 16px; align-items: center; flex-wrap: wrap; }}
  h1 {{ font-size: 18px; margin: 0; }}
  #count {{ color: #71767b; font-size: 13px; }}
  #q {{ flex: 1; min-width: 220px; background: #16181c; border: 1px solid #2f3336; border-radius: 999px; color: #e7e9ea; padding: 8px 16px; font-size: 14px; outline: none; }}
  #q:focus {{ border-color: #1d9bf0; }}
  main {{ max-width: 680px; margin: 0 auto; padding: 12px 8px 60px; }}
  article {{ border-bottom: 1px solid #2f3336; padding: 16px 12px; }}
  .head {{ display: flex; gap: 8px; align-items: baseline; flex-wrap: wrap; }}
  .sn, .t {{ color: #71767b; font-size: 13px; }}
  .text {{ white-space: pre-wrap; overflow-wrap: anywhere; margin: 8px 0 12px; }}
  .grid {{ display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 8px; }}
  .grid img, .grid video {{ width: 100%; border-radius: 14px; background: #16181c; display: block; max-height: 480px; object-fit: contain; }}
  a.src {{ color: #1d9bf0; text-decoration: none; font-size: 13px; word-break: break-all; }}
  a.src:hover {{ text-decoration: underline; }}
  .empty {{ color: #71767b; text-align: center; padding: 60px 0; }}
</style>
</head>
<body>
<header>
  <h1>X / Twitter — Offline</h1>
  <span id="count"></span>
  <input id="q" type="search" placeholder="Search posts or accounts…" autocomplete="off">
</header>
<main id="posts"></main>
<script>
const POSTS = {json};
const $ = (id) => document.getElementById(id);
function esc(s) {{ const d = document.createElement('div'); d.textContent = s == null ? '' : s; return d.innerHTML; }}
function render() {{
  const q = $('q').value.toLowerCase();
  const list = POSTS.filter(p => !q || p.text.toLowerCase().includes(q) || p.author_name.toLowerCase().includes(q) || p.author_screen_name.toLowerCase().includes(q));
  $('count').textContent = list.length + ' posts';
  const main = $('posts');
  main.innerHTML = '';
  if (!list.length) {{
    main.innerHTML = '<div class="empty">No posts. Run <code>twitter download</code> first.</div>';
    return;
  }}
  for (const p of list) {{
    const art = document.createElement('article');
    const head = document.createElement('div');
    head.className = 'head';
    head.innerHTML = '<strong>' + esc(p.author_name) + '</strong><span class="sn">@' + esc(p.author_screen_name) + '</span><span class="t">' + esc(p.created_at) + '</span>';
    art.appendChild(head);
    const txt = document.createElement('p');
    txt.className = 'text';
    txt.textContent = p.text;
    art.appendChild(txt);
    if (p.media.length) {{
      const grid = document.createElement('div');
      grid.className = 'grid';
      for (const m of p.media) {{
        if (m.kind === 'Photo') {{
          const img = document.createElement('img');
          img.loading = 'lazy';
          img.src = 'media/' + m.file;
          grid.appendChild(img);
        }} else if (m.kind === 'Gif') {{
          const v = document.createElement('video');
          v.autoplay = true; v.loop = true; v.muted = true; v.playsInline = true;
          v.src = 'media/' + m.file;
          grid.appendChild(v);
        }} else {{
          const v = document.createElement('video');
          v.controls = true; v.preload = 'metadata';
          v.src = 'media/' + m.file;
          if (m.poster) v.poster = 'media/' + m.poster;
          grid.appendChild(v);
        }}
      }}
      art.appendChild(grid);
    }}
    const a = document.createElement('a');
    a.className = 'src';
    a.href = p.url;
    a.target = '_blank';
    a.rel = 'noopener';
    a.textContent = p.url;
    art.appendChild(a);
    main.appendChild(art);
  }}
}}
$('q').addEventListener('input', render);
render();
</script>
</body>
</html>
"#
    ))
}

/// Write `index.html` for the given posts into `dir`.
pub fn write_index(dir: &Path, tweets: &[Tweet]) -> Result<(), Error> {
    let html = build_index_html(tweets)?;
    std::fs::write(dir.join("index.html"), html)?;
    Ok(())
}

/// Serve the offline viewer over HTTP on 127.0.0.1:`port`. Blocks forever.
pub fn serve(dir: &Path, port: u16) -> Result<(), Error> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    println!(
        "Serving {} at http://127.0.0.1:{} (Ctrl-C to stop)",
        dir.display(),
        port
    );
    let root = dir.to_path_buf();
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let root = root.clone();
        thread::spawn(move || {
            let _ = handle_connection(stream, &root);
        });
    }
    Ok(())
}

fn handle_connection(mut stream: TcpStream, root: &Path) -> Result<(), Error> {
    let mut buf = [0u8; 8192];
    let n = stream.read(&mut buf)?;
    let head = String::from_utf8_lossy(&buf[..n]);
    let Some(request_line) = head.lines().next() else {
        return Ok(());
    };
    let mut parts = request_line.split_whitespace();
    if parts.next() != Some("GET") {
        return Ok(());
    }
    let Some(raw_path) = parts.next() else {
        return Ok(());
    };
    let path = raw_path.split('?').next().unwrap_or("/");
    let rel = path.trim_start_matches('/');
    if rel.split('/').any(|c| c == ".." || c == ".") {
        let response = status_response("403 Forbidden", "Forbidden");
        stream.write_all(&response)?;
        return Ok(());
    }
    let response = match resolve(root, rel) {
        Some(path) => match std::fs::read(&path) {
            Ok(bytes) => {
                let ct = content_type(&path);
                format!("HTTP/1.1 200 OK\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len())
                    .into_bytes()
                    .into_iter()
                    .chain(bytes)
                    .collect::<Vec<u8>>()
            }
            Err(_) => status_response("404 Not Found", "Not Found"),
        },
        None => status_response("404 Not Found", "Not Found"),
    };
    stream.write_all(&response)?;
    Ok(())
}

fn resolve(root: &Path, rel: &str) -> Option<PathBuf> {
    let path = if rel.is_empty() {
        root.join("index.html")
    } else {
        root.join(rel)
    };
    path.is_file().then_some(path)
}

fn status_response(status: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript",
        Some("json") => "application/json",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mp3") => "audio/mpeg",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Media, MediaKind, User};

    #[test]
    fn html_escapes_json() {
        let t = Tweet {
            id: "1".into(),
            author: User {
                id: "2".into(),
                screen_name: "a".into(),
                name: "A".into(),
            },
            text: "<script>alert(1)</script>".into(),
            created_at: None,
            is_retweet: false,
            media: vec![],
            url: "https://x.com/a/status/1".into(),
        };
        let html = build_index_html(&[t]).unwrap();
        assert!(html.contains("\\u003cscript\\u003e"));
        assert!(!html.contains("<script>alert"));
    }

    #[test]
    fn viewer_media_names() {
        let m = Media {
            kind: MediaKind::Video,
            url: "https://video.twimg.com/x/v.mp4?tag=1".into(),
            thumbnail: Some("https://pbs.twimg.com/t/thumb.jpg".into()),
            width: None,
            height: None,
        };
        let t = Tweet {
            id: "9".into(),
            author: User {
                id: "2".into(),
                screen_name: "a".into(),
                name: "A".into(),
            },
            text: "x".into(),
            created_at: None,
            is_retweet: false,
            media: vec![m],
            url: "https://x.com/a/status/9".into(),
        };
        let html = build_index_html(&[t]).unwrap();
        assert!(html.contains("9_1.mp4"));
        assert!(html.contains("9_1_poster.jpg"));
    }
}
