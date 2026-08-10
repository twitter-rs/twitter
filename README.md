# twitter — offline viewer and archiver for X (Twitter)

A Rust package (library + CLI) for keeping a personal, offline archive of
posts and media from X (Twitter): download everything from an account, or
individual posts by URL, then browse the archive in a self-contained HTML
viewer — no internet required afterwards.

This package is **not affiliated with, endorsed by, or associated with
Twitter, Inc. or X Corp.** It is an independent, unofficial tool that uses
publicly available web endpoints for personal archiving. Archive content
only if you are entitled to keep it, and respect the platform's terms.

## What it does

- **Archives accounts**: fetch a user's timeline (paginating through the
  entire history, or limited with `--max`) and save every post plus all
  attached photos, videos and GIFs in original quality.
- **Archives individual posts**: from a URL
  (`https://x.com/user/status/123...`) or a bare post id.
- **Browses offline**: generates a self-contained `index.html` with a
  search box, embedded post data and locally stored media. Open the file
  directly, or serve it over HTTP with `twitter view --serve`.
- **Keeps your archive current**: re-running merges new posts into the
  existing archive; files already present are skipped.
- **Works for protected accounts**: when provided with your session cookies
  (`auth_token` / `ct0`), posts you are allowed to see are archived.
  Without cookies, public posts are still archived via X's public
  syndication endpoint.

## Install

```sh
cargo install twitter
```

or build from source:

```sh
git clone https://github.com/twitter-rs/twitter.git
cd twitter
cargo build --release
```

## CLI usage

```
Usage: twitter [OPTIONS] <COMMAND>

Commands:
  download  Download posts and media from accounts (@user) or post URLs
  info      Print post or account info as JSON
  view      Build the offline HTML viewer (and optionally serve it)
```

### Download everything from an account

```sh
twitter download @nasa
```

### Download individual posts by URL

```sh
twitter download https://x.com/nasa/status/2075726968335618499
```

### Options

```sh
twitter download @nasa --max 50 --no-retweets          # newest 50 original posts
twitter download @nasa -o nasa_archive                 # custom output dir
twitter download https://x.com/... -c cookies.txt      # custom cookies file
twitter info https://x.com/nasa/status/123             # show post JSON
twitter view -o nasa_archive --open                    # open the viewer
twitter view -o nasa_archive --serve 8080              # serve over HTTP
```

### Output layout

```
twitter_data/
├── index.html     # offline viewer (open in any browser)
├── posts.json     # all archived posts as JSON (newest first)
└── media/         # downloaded photos / videos / gifs, one file per item
```

Media files are named `<post_id>_<n>.<ext>`; videos keep a matching
`<post_id>_<n>_poster.jpg` thumbnail for the viewer.

## Library usage

```rust
use twitter::{Target, Twitter};

let tw = Twitter::from_cookies_file("cookies.txt")?;   // optional auth
let user = tw.user("nasa")?;
let posts = tw.timeline(&user, Some(20))?;             // newest 20 posts
for post in &posts {
    tw.download_media(post, std::path::Path::new("twitter_data"))?;
}
```

Public API: `Twitter` (fetch posts/timelines/users, download media),
`Store` (merge/save archives), `viewer` (offline HTML viewer + tiny HTTP
server), plus the `Tweet`, `User`, `Media` models.

## Cookies

Copy your `auth_token` and `ct0` cookies (and ideally the rest of your
session cookies) from your browser into a Netscape-format `cookies.txt`
file, e.g.:

```
.x.com	TRUE	/	TRUE	1801772568	auth_token	<your token>
.x.com	TRUE	/	TRUE	1801772569	ct0	<your token>
```

`--cookies cookies.txt` is the default; pass `-c <file>` to override.
Keep the file private — it grants access to your account.

## Notes

- GraphQL query ids used by the web client change from time to time; the
  tool re-extracts the current ones from the live web bundle automatically
  when a request fails, so archives keep working without updates.
- Photos are saved in original quality (`?name=orig`); videos are saved as
  the highest-bitrate MP4 variant.
- Be polite: requests are paced and retried with backoff on rate limits.

## License

MIT
