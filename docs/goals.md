# Deep links: goals

The contract the crate and the TypeScript package build against. The names are the
predetermined set (`DeepLinks`, `Link`, `LinkKind`, `RouteTable`, `RouteMatch`,
`PendingLink`, `DeepLinksError`, `ErrorKind`, `Backend`, `FakeBackend`, the `association`
module, the `platform` modules). The TypeScript package mirrors every name and behavior,
camelCased; a behavior lands in both, with a test in both.

## Goals

1. **Verified web links, the layer NSH core lacks.** Universal Links on iOS and macOS
   (`NSUserActivity` of type `NSUserActivityTypeBrowsingWeb`) and App Links on Android
   (`autoVerify` https intents), delivered through the crate's `Backend` trait into one
   dispatch pipeline. What each platform actually delivers is in "Platform scope" below.
2. **Custom schemes stay wrapped, not rebuilt.** Every custom-scheme link comes from NSH
   core's `UrlScheme` intake (`next_url_scheme_open`, `app_url`) and is checked with
   `nsh_core::url_scheme::check_url`; prefixes are checked with `check_scheme`. The crate
   re-exports core's `MAX_URL_LEN` (16 KiB) and `MAX_PENDING` (16); the TS package reads
   both, and calls `check_url` / `check_scheme`, through the native module, so neither
   language keeps a copy.
3. **A validated route table.** `RouteTable::from_config` takes prefixes, an optional
   param-free root `path`, and a pattern-to-screen config ported from React Navigation's
   linking model: `:param`, `:param(regex)`, `:param?`, `:param+`, `:param*`, `*`, nested
   `screens`, `alias`, `exact`, and per-param `parse` types. It is validated up front: bad
   patterns, conflicting screens and extraneous keys are coded errors. `match_url` returns a
   `RouteMatch { screen, params }`. The `https://host` prefixes are also the host allowlist.
4. **A pending-link slot that survives login.** `PendingLink` holds one link the app cannot
   route yet; `claim` takes it once, `clear` drops it, and a newer `hold` replaces the held
   link without an error.
5. **Association-file tooling, offline.** Builders and validators for
   `apple-app-site-association` and `assetlinks.json`, one file per host. Validators take
   the bytes the caller already has and the name of the app they must vouch for; nothing
   fetches.
6. **Every call resolves exactly once** — success, a coded failure, or a cancel. Never a
   hang, never a second resolution, never a panic: no `unwrap`/`expect`/`panic!` outside
   tests, and platform bridges are cfg-gated so every other target compiles without them.

### Platform scope

| Platform | Verified web links | Custom schemes | How |
|---|---|---|---|
| macOS | Universal Links while running, through `next`; a launch link only if deep links were first read before the system handed it over | from NSH core | puts itself in front of the host's `application:continueUserActivity:restorationHandler:` (NSH 0.7.1's macOS host implements it for Handoff and web links) at runtime, when deep links are first read; the host's own method runs after ours |
| iOS | Universal Links while running, through `next`; **a cold-start Universal Link is not seen** | from NSH core | puts itself in front of the host's `scene:continueUserActivity:` (NSH 0.7.1's iOS host implements it for Handoff and web links) at runtime, when deep links are first read; the host's own method runs after ours. The cold-start activity sits in the scene's connection options, which NSH's host reads before any crate code runs and passes to core (`nsh-platform-ios/src/app.rs`), so this crate does not see it |
| Android | the launch App Link, through `initial`; **an App Link reaching a running app is not seen** | from NSH core | reads the activity's launch intent over JNI; NSH 0.7.1's activity hands `onNewIntent` URLs to core, which keeps claimed schemes and declared links; this crate reads schemes only, so `https` never reaches it |
| Windows, Linux | `ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM` | from NSH core | no verified-web-link contract on the desktop |
| Web | the page's address at load, through `initial`, kind `web`; `next` is `ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM` | none | core's `app_url` |
| Anything else | `ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM` | none | `platform::unsupported::UnsupportedBackend` |

The two gaps need the host to hand the crate the activity or intent; a crate cannot add
that. `platform::unsupported::UnsupportedBackend` (TS: `unsupportedBackend()`) is public on
every target, so its coded refusals are testable on any runner.

On the native platforms the launch link for custom schemes is core's address the first time
deep links are read (`app_url`); core also queued that link for the first listener, and the
crate skips that one copy so `next` never hands it out again.

### API surface

- `DeepLinks::new(backend, routes)` / `DeepLinks::platform(routes)`
  (TS: `new DeepLinks(backend, routes)`, `DeepLinks.platform(routes)`).
- `initial() -> Option<Link>` — the launch link (app_links' `getInitialLink`), read from the
  platform once; every later call answers the same.
- `next()` — resolves once with the next `Link`. One wait at a time: a second call is
  `ERR_DEEP_LINKS_BUSY`. Dropping the Rust future cancels the wait.
- `next_within(deadline)` (TS: `nextWithin(ms)`) — `next` that resolves
  `ERR_DEEP_LINKS_TIMEOUT` when the deadline passes; a link arriving later waits for the
  next call and is not lost. A zero deadline is `ERR_DEEP_LINKS_INVALID_ARGUMENT`. On wasm
  the deadline is not armed (no timer driver there), and the web's wait resolves at once.
- `stop()` — ends delivery: the waiting `next` resolves `ERR_DEEP_LINKS_CANCELLED`, so does
  every later `next`, and links arriving afterwards are never handed out. Build a new
  `DeepLinks` to listen again.
- `pending() -> PendingLink` with `hold`, `claim`, `clear`, `held`.
- `resolve(url) -> Link` — the whole pipeline on a URL the app already has.
- `Link { url, kind, route }`; `LinkKind { Universal, App, Custom, Web }` (TS strings
  `"universal" | "app" | "custom" | "web"`). The kind comes from how the system delivered
  the link and is never downgraded to make a match.
- Dispatch order: core's `check_url` against `http`/`https` (so core's length and character
  rules apply to verified links too) → host allowlist → the platform's verified-link kind,
  else `ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM` → otherwise core's `check_url` against the
  claimed schemes → `RouteTable::match_url`. A URL core refuses for every scheme is named by
  why: too long, whitespace or control characters, not an absolute URL, else an unclaimed
  scheme.
- Links that arrive with no `next` waiting are held in order by the backend, at most
  `MAX_PENDING`, the oldest going. A copy of the link the last wait resolved with, arriving
  before the next wait starts, is the system firing twice: the backend refuses it with
  `ERR_DEEP_LINKS_BUSY` (`FakeBackend::deliver` returns it) and does not hold it.
- `Backend` is the platform trait (`initial_url`, `next_url`, `cancel_wait`,
  `web_link_kind`); `FakeBackend` (TS `memoryBackend()`) scripts the launch link,
  deliveries, duplicates, platform failures and cancels; TS `rustBackend()` binds the crate's
  `NativeDeepLinks` through `nsh-deep-links/native`.
- Route params: path params, then query params; a path param wins over a query param of
  the same name. Segments decode strictly (`%zz` or bytes that are not UTF-8 are
  `ERR_DEEP_LINKS_NO_ROUTE`); a repeat param's value is its segments joined by `/`; an
  absent optional param is left out. `parse` types are `"string"`, `"number"` (finite
  decimal), `"integer"` (whole, within ±2^53) and `"boolean"` (`true`/`false`); a value that
  fails its type refuses the whole match with `ERR_DEEP_LINKS_INVALID_PARAMETER` and never
  falls through to a looser pattern. Equally specific patterns tie-break by screen name, so
  both languages agree whatever map order their JSON has. Regexes are compiled by each
  language's own engine (Rust `regex`, JS `RegExp` with `u`): keep to their common subset.

Not in the first release, deliberately: building a path from app state (React Navigation's
`getPathFromState`), function or Standard Schema `parse` values, `initialRouteName`,
`shared`, runtime scheme registration (the Tauri plugin's `register`/`unregister`), runtime
association fetching, and web intake beyond the page address.

## Acceptance criteria

Each criterion names the gate in `.github/workflows/ci.yml` that proves it. All seven
pass in CI run https://github.com/SuperNSH/deep-links/actions/runs/44376314593; see `docs/parity.md`.

1. The workspace is clean: `cargo fmt --all -- --check` and
   `cargo clippy -p nsh-deep-links --all-targets -- -D warnings` pass on Linux, macOS and
   Windows.
2. Every bridge compiles for its target: the cross-check job clippy-passes
   `aarch64-apple-ios`, `aarch64-apple-ios-sim`, `aarch64-linux-android` and
   `wasm32-unknown-unknown`.
3. The Rust suite passes: `cargo test -p nsh-deep-links --all-targets`, including one
   integration test per edge case below, named exactly as listed.
4. The TS suite passes: `nsh check`, `nsh build` with no warning, and `nsh test` in
   `packages/nsh-deep-links`, with the same edge cases under the same names.
5. Every code in the error enum is asserted by at least one named edge-case test in each
   language.
6. Route matching and association output are identical across languages: both suites
   assert the same fixtures and the exact JSON the builders emit.
7. The wrapped limits stay core's: the oversized-URL and held-links tests assert against
   the re-exported `MAX_URL_LEN` and `MAX_PENDING`, never local copies.

## Error enum

`DeepLinksError` carries `code()`, `kind()`, `message()` and an optional `detail()`; `code`
is the stable string apps match on. Add-only: codes are never renamed or reused.

| Code | `ErrorKind` | Fires when |
|---|---|---|
| `ERR_DEEP_LINKS_INVALID_URL` | `InvalidUrl` | The URL is not absolute or not a URL, or it carries whitespace or control characters. |
| `ERR_DEEP_LINKS_URL_TOO_LARGE` | `UrlTooLarge` | The URL exceeds `MAX_URL_LEN` bytes, verified links included. |
| `ERR_DEEP_LINKS_DISALLOWED_SCHEME` | `DisallowedScheme` | The link's scheme is not one the app claims, or a prefix claims a reserved scheme (`mailto`, `file`, … core's `check_scheme` list). |
| `ERR_DEEP_LINKS_HOST_NOT_ALLOWED` | `HostNotAllowed` | An `http(s)` link's host (and port) matches no `http(s)` prefix of the same scheme. |
| `ERR_DEEP_LINKS_NO_ROUTE` | `NoRoute` | No prefix or pattern matches, or a path segment's percent-escape does not decode. |
| `ERR_DEEP_LINKS_INVALID_PARAMETER` | `InvalidParameter` | A param fails its `parse` type; the whole match is refused. |
| `ERR_DEEP_LINKS_INVALID_ARGUMENT` | `InvalidArgument` | The caller's config or input is wrong: a bad pattern, an extraneous key, two screens on one pattern, params in the root path, a scheme not of URL-scheme form, a zero deadline, a bad package, app ID, fingerprint or host for an association builder. |
| `ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM` | `UnsupportedPlatform` | A verified web link where the platform has none, or a wait on a platform that delivers no links. |
| `ERR_DEEP_LINKS_BUSY` | `Busy` | A `next` is already waiting, or the system fired the link a wait just resolved with again. |
| `ERR_DEEP_LINKS_CANCELLED` | `Cancelled` | `stop()` ended delivery, or the wait was cancelled. |
| `ERR_DEEP_LINKS_TIMEOUT` | `Timeout` | `next_within`'s deadline passed with no link. |
| `ERR_DEEP_LINKS_ASSOCIATION_INVALID` | `AssociationInvalid` | An association file is malformed, or no statement in it vouches for the expected package or app ID. |
| `ERR_DEEP_LINKS_BACKEND` | `Backend` | The platform failed underneath (JNI, the JS bridge), or custom-scheme intake was asked for outside an NSH engine on a platform with no other intake. |

## Edge cases

Every case maps to one test, in Rust (`crates/nsh-deep-links/tests/{links,routes,association}.rs`)
and TS (`packages/nsh-deep-links/tests/{links,routes,association}.test.ts`, the deadline case
in `app.test.tsx`), under the same name, driven through the public API with `FakeBackend` /
`memoryBackend()`: never FFI, a JVM, the DOM or the network. Rust waits run on tokio's
paused clock; the TS deadline case runs on the app's manual clock.

| Case | Expected behavior | Test name |
|---|---|---|
| Cold-start link | `initial()` resolves the launch link, read from the platform once; `next()` never hands it out again. | `a_cold_start_link_dispatches_exactly_once` |
| No launch link | `initial()` is `None` when no link started the app. | `initial_is_none_when_no_link_started_the_app` |
| Link while running | A link delivered after startup resolves through `next()`. | `a_link_while_running_dispatches_through_next` |
| Cancel | `stop()` resolves the waiting `next()` with `ERR_DEEP_LINKS_CANCELLED`, once; later calls are cancelled too and a link arriving after is never handed out. | `stop_cancels_a_pending_next_exactly_once` |
| Timeout | A deadline that passes with no link resolves `ERR_DEEP_LINKS_TIMEOUT`; the gate frees and a later link is not lost. | `a_next_waiting_past_its_deadline_resolves_as_timeout` |
| Duplicate delivery | The same link fired again while the first resolves is refused `ERR_DEEP_LINKS_BUSY` and not held; the first still resolves with its link. | `a_duplicate_delivery_while_one_resolves_maps_to_busy` |
| Concurrent listeners | A second `next()` while one waits is `ERR_DEEP_LINKS_BUSY`. | `a_second_next_while_one_is_outstanding_is_busy` |
| Malformed URL | Empty, relative or unparseable: `ERR_DEEP_LINKS_INVALID_URL`. | `a_malformed_url_is_rejected_as_invalid_url` |
| Oversized URL | Longer than `MAX_URL_LEN`, custom or verified: `ERR_DEEP_LINKS_URL_TOO_LARGE`; exactly the limit is taken. | `a_url_longer_than_max_url_len_is_rejected_as_too_large` |
| Whitespace and controls | Any whitespace or control character, verified links too: `ERR_DEEP_LINKS_INVALID_URL`. | `a_url_with_whitespace_or_control_characters_is_rejected` |
| Unclaimed scheme | `ERR_DEEP_LINKS_DISALLOWED_SCHEME`. | `a_link_of_an_unclaimed_scheme_is_rejected_as_disallowed` |
| Reserved scheme claim | A prefix of a reserved scheme: `ERR_DEEP_LINKS_DISALLOWED_SCHEME`; a malformed one: `ERR_DEEP_LINKS_INVALID_ARGUMENT`. | `a_reserved_scheme_in_a_claim_is_rejected_as_disallowed` |
| Unclaimed host | Another host, a lookalike suffix, another port or `http` for an `https` prefix: `ERR_DEEP_LINKS_HOST_NOT_ALLOWED`. | `an_https_link_to_an_unlisted_host_is_rejected_as_host_not_allowed` |
| Case-insensitive scheme and host | `MYAPP://` matches `myapp://`, `https://Example.COM` the `https://example.com` prefix; params and paths keep their case. | `scheme_and_host_match_case_insensitively` |
| Verified over custom | A verified delivery keeps its `Universal`/`App` kind and its https route. | `a_verified_web_link_is_preferred_over_a_custom_scheme_link` |
| Unsupported platform | The unsupported backend refuses a verified link and every wait with `ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM`. | `a_verified_web_link_on_an_unsupported_platform_resolves_unsupported` |
| Platform failure | A scripted platform failure, on `next` or `initial`, resolves `ERR_DEEP_LINKS_BACKEND`. | `a_platform_failure_resolves_as_backend` |
| No matching route | `ERR_DEEP_LINKS_NO_ROUTE`. | `a_link_whose_path_matches_no_route_is_rejected_as_no_route` |
| Query and fragment | Query params join `RouteMatch.params`; the fragment stays on `Link.url`, uninterpreted. | `query_params_and_fragments_are_parsed_and_preserved` |
| Path params win | A path param overrides a query param of the same name. | `path_params_win_over_same_named_query_params` |
| Trailing slash | Trailing and repeated slashes match the same route. | `a_trailing_slash_matches_the_same_route` |
| Empty path | The bare prefix matches the `''` pattern. | `an_empty_path_matches_the_root_route` |
| Percent-encoding | Segments decode before matching and typing; `%c3%a9` = `%C3%A9`; an undecodable escape is `ERR_DEEP_LINKS_NO_ROUTE`. | `percent_encoded_segments_decode_before_matching` |
| Specificity ordering | Static before regex params, regex before plain, plain before one-or-more, one-or-more before zero-or-more, then the wildcard; config order does not decide. | `more_specific_patterns_match_before_wildcards` |
| Untrusted params | A param failing its `parse` type refuses the match with `ERR_DEEP_LINKS_INVALID_PARAMETER`, never falling through. | `a_param_failing_its_parse_schema_rejects_as_invalid_parameter` |
| Invalid pattern | `from_config` refuses a bad pattern with `ERR_DEEP_LINKS_INVALID_ARGUMENT`, naming it. | `an_invalid_pattern_in_the_route_table_is_rejected_as_invalid_argument` |
| Conflicting patterns | Two screens on one pattern, nested ones included: `ERR_DEEP_LINKS_INVALID_ARGUMENT`. | `conflicting_patterns_for_two_screens_reject_as_invalid_argument` |
| Pending survives login | A held link is still claimable after a login completes through its own link. | `a_pending_link_survives_a_simulated_login` |
| Claim clears the slot | `claim` hands the link out once; a newer `hold` replaces; `clear` drops. | `claiming_a_pending_link_clears_the_slot` |
| Held-links overflow | Past `MAX_PENDING` held links the oldest goes, without an error. | `links_waiting_past_max_pending_drop_the_oldest` |
| Wrong team | An `apple-app-site-association` for another team or bundle: `ERR_DEEP_LINKS_ASSOCIATION_INVALID`. | `an_apple_association_for_the_wrong_team_is_invalid` |
| Wrong package | An `assetlinks.json` for another package: `ERR_DEEP_LINKS_ASSOCIATION_INVALID`. | `an_assetlinks_statement_for_the_wrong_package_is_invalid` |
| Per-subdomain statements | One file per host, at its own `/.well-known/` URL. | `assetlinks_statements_are_built_per_subdomain` |
| Offline validation | Validators take bytes; a stale or failed CDN answer is judged as bytes; TS asserts `fetch` is never called. | `association_validation_is_offline_and_never_fetches` |
| Exactly once | `initial`, `next`, `stop` and `claim` each resolve once; a failure after a resolution reaches the next call only. | `every_call_resolves_exactly_once` |
