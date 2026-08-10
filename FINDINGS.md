# Findings — X (Twitter) archiving, August 2026

This file documents everything learned while building and testing the
`twitter` crate. It is a record of what works today (2026-08), what is dead,
and how the pieces fit together.

## Status summary

- The GraphQL web API is the only live data path for posts and timelines.
  The legacy v1.1 REST API (`x.com/i/api/1.1/statuses/*.json`) is **dead**
  (404/403 for every endpoint tested).
- Guest access is gone: profile/video pages are client-rendered shells and
  the API requires `auth_token` + `ct0` cookies. Without cookies the public
  **syndication** endpoint still serves individual posts.
- GraphQL query ids rotate with web client deploys; the crate re-extracts
  them from the live web bundle automatically when a request fails.

## Endpoints (verified working, 2026-08)

All on `https://x.com/i/api/graphql/{queryId}/{operation}` with cookies
(`auth_token`, `ct0`), `x-csrf-token: <ct0>`, and the public web bearer
token. Query ids observed live (subject to rotation — the crate re-discovers
them):

| Operation | Query id (2026-08) |
|---|---|
| `TweetResultByRestId` | `GZsN2Pc4knAoit6pXa4HSA` |
| `UserByScreenName` | `Gb-d6r0vxPOADdG62OEBpQ` |
| `UserTweets` | `SXVCYB8XHSS25nzIljNtZA` |
| `TweetDetail` | `XMOz5h24KAZ86qKffKTLdQ` |
| `UserByRestId` | `xvmVfRLmnr1alc5f2dib0Q` |
| `UserTweetsAndReplies` | `qUpkZU6eN8MbtQb7rC_pYg` |

Discovery: the ids live in the web client bundle
(`https://abs.twimg.com/responsive-web/client-web/main.<hash>.js`) in the
form `queryId:"...",operationName:"..."`. The bundle URL is referenced from
`https://x.com/home` HTML.

## Response shapes (2026)

- Tweet results may be `__typename: "Tweet"`, `"TweetWithVisibilityResults"`
  (unwrap `.tweet`), or `"TweetUnavailable"`. Text is `legacy.full_text`,
  media in `legacy.extended_entities.media[]`.
- Media: `photo` (use `media_url_https?name=orig` for the original upload),
  `video` / `animated_gif` (pick the highest-bitrate `video/mp4` variant from
  `video_info.variants`; skip `application/x-mpegURL`).
- User objects changed shape: `core.user_results.result.legacy` was removed
  for some endpoints; name/screen_name now live in
  `core.user_results.result.core.{name,screen_name}`.
- Timelines: `UserTweets` returns `data.user.result.timeline.timeline.
  instructions[]` (note the doubled `timeline`). Paginate with the
  `TimelineTimelineCursor` entry with `cursorType: "Bottom"` (pass as
  `cursor`). Entries are `TimelineTimelineItem`, `TimelinePinEntry`,
  `TimelineTimelineModule` (module items are usually suggested users, not
  tweets). `TimelineClearCache`/`TimelineAddEntries` instructions are noise.

## Syndication fallback (no auth)

`GET https://cdn.syndication.twimg.com/tweet-result?id={id}&lang=en&token={token}`

- `token = "tw" + base64url(big-endian 8 bytes of the id)` — verified against
  a live post (`2075726968335618499` → `twHM52rhZX0cM`).
- Works for public posts; protected/deleted posts return an error object.
- Media in `mediaDetails[]` (`media_url_https`, `video_info.variants`).

## Media download notes

- `pbs.twimg.com` photos: plain URL serves ~160 KB; `?name=orig` serves the
  original upload (~1.8 MB for a 5568×3712 photo).
- Video CDN URLs carry a session-bound `?tag=` query parameter that the CDN
  rejects (HTTP 429) when replayed outside the session that issued it;
  stripping `tag` fixes it (the file itself is public). The crate retries
  with the stripped URL on 429.
- A single 4K video can be ~3.2 GB; downloads must stream to disk and must
  not use a short total-request timeout (use a generous read/socket timeout).

## Misc

- The old `x.com/i/api/1.1/` endpoints (settings, user_timeline, show,
  users/show) returned 404/403 — removed server-side.
- Verification was done end-to-end: real posts downloaded (including the
  3.2 GB 4K video), accounts paginated (@NASA, @charlidamelio), and the
  viewer regenerated and served over HTTP.
