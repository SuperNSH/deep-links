# Deep links: goals

The contract every area builds against. API names here refine the predetermined set
(`DeepLinks`, `Link`, `LinkKind`, `RouteTable`, `RouteMatch`, `PendingLink`,
`DeepLinksError`, `ErrorKind`, `Backend`, `FakeBackend`, the `association` module, the
`platform` modules) and keep them. The TypeScript package mirrors every name and every
behavior one-to-one; a behavior lands in both, with a test in both.

## Goals

1. **Verified web links, the layer NSH core lacks.** Universal Links on iOS and macOS
   (`NSUserActivity` with a `webpageURL`) and App Links on Android (`autoVerify` https
   intents), delivered through the crate's `Backend` trait into one dispatch pipeline.
2. **Custom schemes stay wrapped, not rebuilt.** Every custom-scheme link goes through
   `nsh_core::url_scheme` (`check_url`, `check_scheme`, `schemes_from_json`,
   `MAX_URL_LEN` = 16 KiB, `MAX_PENDING` = 16). This crate adds no scheme logic of its
   own; it re-exports the core limits so a core change breaks our tests, not silent
   parity.
3. **A validated route table.** `RouteTable::from_config` takes prefixes plus a
   pattern-to-screen config (React Navigation's linking model: `:param`,
   `:param(regex)`, `:param?`, `:param+`, `:param*`, `*` wildcard, nested `screens`,
   per-param `parse` schemas) and validates it up front, rejecting bad patterns,
   conflicting screens and extraneous keys with coded errors. `match` returns a
   `RouteMatch { screen, params }`. The `https://host` prefixes are also the host
   allowlist: an https link whose host matches no https prefix is refused, not routed.
4. **A pending-link slot that survives login.** A link that arrives before the app can
   route it (login gate not yet open) is held in `PendingLink`; `claim` takes it after
   login, `clear` drops it. The slot holds one link; a newer hold replaces the older
   one without an error, the oldest going, as core's `MAX_PENDING` rule does.
5. **Association-file tooling, offline.** `association::apple_app_site_association` and
   `association::assetlinks_json` build and validate the two statement formats:
   correct `appID` (team prefix) and package placement, one statement file per
   subdomain for Android's `autoVerify`. Validators take bytes the caller already has;
   the library never fetches — Apple's CDN caches a failed
   `apple-app-site-association` fetch for days, so verification state is never guessed
   at runtime.
6. **Every call resolves exactly once** — success, a coded failure, or a cancel. Never
   a hang, never a second resolution, never a panic: no `unwrap`/`expect`/`panic!`/
   `unreachable!` outside `#[cfg(test)]`, and platform bridges are cfg-gated so every
   other target compiles without them.

### Platform scope

| Platform | Verified web links | Custom schemes |
|---|---|---|
| iOS, macOS | Universal Links | wrapped from core |
| Android | App Links | wrapped from core |
| Windows, Linux | `ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM` | wrapped from core |
| Web | page-address origin rules only; the initial page address is the only intake | none |

`platform::unsupported` is constructible on every target, so its coded refusals are
testable on any CI runner.

### API surface (refined)

- `DeepLinks::new(backend, routes)` — one delivery pipeline over a `Backend` impl and
  a validated `RouteTable`. One instance handles links; `initial` and `next` never
  redeliver the same link.
- `initial() -> Option<Link>` — the link that launched the app, readable more than
  once (app_links' `getInitialLink`); `None` when the app was not started from one.
- `next()` — resolves exactly once with the next delivered `Link`; one outstanding
  `next` at a time (a second resolves `ERR_DEEP_LINKS_BUSY`); `stop()` resolves it
  with `ERR_DEEP_LINKS_CANCELLED`; an optional deadline resolves
  `ERR_DEEP_LINKS_TIMEOUT`. A delivery arriving after the call resolved is dropped.
- `pending() -> Option<Link>` — the held link, if any.
- `Link { url, kind }`, `LinkKind { Universal, App, Custom }` — the kind comes from
  how the system delivered the link and is never downgraded to `Custom` to force a
  match (verified links are the safer surface for auth flows).
- Dispatch order: length and character checks (core's rules, applied to verified links
  too) → for https, host allowlist (the table's https prefixes, host compared
  case-insensitively) → `LinkKind` classification → `RouteTable::match` → deliver to
  the outstanding `next`, else hold in order — at most the re-exported `MAX_PENDING`
  links, the oldest going past it, core's rule mirrored. A link held for login sits in
  `PendingLink`, a separate single-link slot.
- `Backend` is the platform trait (shaped after biometrics' `backend.rs`); `FakeBackend`
  scripts deliveries, cancels, timeouts, duplicates and platform failures, and the TS
  mock `memoryBackend()` mirrors that script surface; `rustBackend()` binds the crate
  through `nsh-deep-links/native`.

Not in the first release, deliberately: building a path from app state (React
Navigation's `getPathFromState`), runtime scheme registration (the Tauri plugin's
`register`/`unregister`), runtime association fetching, and web verified-link intake
beyond the page address.

## Acceptance criteria

Each criterion names the CI gate in `.github/workflows/ci.yml` that proves it — the
gates this change-set adds when the workspace conversion lands: the 3-OS matrix, the
cross-check job, and the `crates/nsh-deep-links` and `packages/nsh-deep-links` jobs.
Today's ci.yml runs fmt, clippy and test on one ubuntu job at the repo root, so until
the new gates land and a run passes, every criterion below counts as not started;
`docs/parity.md` records the runs, and no claim is made before one passes.

1. The workspace is clean: `cargo fmt --all -- --check` and
   `cargo clippy --all-targets -- -D warnings` pass on the linux, macOS and Windows
   matrix.
2. Every bridge compiles for its target: the cross-check job clippy-passes
   `aarch64-apple-ios`, `aarch64-linux-android` and `wasm32-unknown-unknown`.
3. The Rust suite passes: `cargo test --all-targets` in `crates/nsh-deep-links`,
   including one inline or integration test per edge case below, named exactly as
   listed.
4. The TS suite passes: `nsh check` and `nsh test` in `packages/nsh-deep-links`, with
   the same edge cases under the same names, driven through `memoryBackend()`.
5. Every code in the error enum below is asserted by at least one named edge-case
   test in each language, `ERR_DEEP_LINKS_BACKEND` through
   `a_platform_failure_resolves_as_backend` (proven by gates 3 and 4).
6. Route matching and association output are identical across languages: both suites
   assert the same fixtures and the exact serialized JSON the builders emit (proven by
   gates 3 and 4).
7. The wrapped limits stay core's:
   `a_url_longer_than_max_url_len_is_rejected_as_too_large` asserts against the
   re-exported `MAX_URL_LEN` and `links_waiting_past_max_pending_drop_the_oldest`
   against the re-exported `MAX_PENDING`, never local copies (proven by gates 3
   and 4).

## Error enum

`DeepLinksError { code, kind, message }`; `code` is the stable string apps match on.
Add-only: codes are never renamed or reused. The thirteen baseline codes cover every
failure mode the references surface (see decisions.md for the mapping choices); no
additions were needed.

| Code | `ErrorKind` | Fires when |
|---|---|---|
| `ERR_DEEP_LINKS_INVALID_URL` | `InvalidUrl` | The URL is not absolute or not a URL at all (core's `check_url` parse failure), or it carries whitespace or control characters. |
| `ERR_DEEP_LINKS_URL_TOO_LARGE` | `UrlTooLarge` | The URL exceeds `MAX_URL_LEN` (16 KiB), verified links included. |
| `ERR_DEEP_LINKS_DISALLOWED_SCHEME` | `DisallowedScheme` | The link's scheme is not one the app claims, or a config tries to claim a reserved scheme (`http`, `https`, `file`, `mailto`, … core's `check_scheme` list). |
| `ERR_DEEP_LINKS_HOST_NOT_ALLOWED` | `HostNotAllowed` | An https link's host matches no `https://` prefix in the route table, or (web) an address is not of the page's origin. |
| `ERR_DEEP_LINKS_NO_ROUTE` | `NoRoute` | The URL is valid and allowed but matches no pattern — including a path segment whose percent-escape cannot decode, which React Navigation also treats as no match. |
| `ERR_DEEP_LINKS_INVALID_PARAMETER` | `InvalidParameter` | A link parameter is untrusted input and fails its `parse` schema; the whole match is refused rather than routed with a bad value. |
| `ERR_DEEP_LINKS_INVALID_ARGUMENT` | `InvalidArgument` | The caller's config is wrong: an invalid pattern (`:` mid-segment, unclosed `(`, duplicate param, combined modifiers), a claim whose scheme is not of URL-scheme form (core's `check_scheme`), two screens claiming one pattern, an extraneous key, a top-level path containing params, an empty package or appID to an association builder. |
| `ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM` | `UnsupportedPlatform` | The platform cannot do what the link asks — a verified web link on Windows or Linux, a custom scheme on the web. |
| `ERR_DEEP_LINKS_BUSY` | `Busy` | A `next` is already outstanding, or a delivery arrives while one is being resolved. |
| `ERR_DEEP_LINKS_CANCELLED` | `Cancelled` | `stop()` resolves the outstanding `next`; the system or the user cancelled the platform handoff. |
| `ERR_DEEP_LINKS_TIMEOUT` | `Timeout` | `next`'s optional deadline passes with no delivery, or a platform call exceeds its own deadline because the OS never answered. |
| `ERR_DEEP_LINKS_ASSOCIATION_INVALID` | `AssociationInvalid` | An association statement fails verification: wrong team prefix or appID in `apple-app-site-association`, wrong package in `assetlinks.json`, or a malformed statement shape. |
| `ERR_DEEP_LINKS_BACKEND` | `Backend` | The platform failed underneath us — FFI, JNI, COM, D-Bus or the JS bridge — and no more specific code applies. |

## Edge cases

Every case maps to exactly one test, in Rust (`crates/nsh-deep-links/tests/`) and in
TS (`packages/nsh-deep-links/tests/`), under the same name. The names are frozen: the
test-suite area implements these exact identifiers, driven through the public API plus
`FakeBackend` / `memoryBackend()` — never FFI, a JVM, the DOM or the network. All
tests run on any CI runner: platform-specific behavior is reached through the
constructible-anywhere `platform::unsupported` or through scripted backends.

| Case | Expected behavior | Test name |
|---|---|---|
| Cold-start link | The launch link resolves through `initial()` exactly once; `next()` never redelivers it. | `a_cold_start_link_dispatches_exactly_once` |
| No launch link | `initial()` is `None` when the app was not started from a link. | `initial_is_none_when_no_link_started_the_app` |
| Link while running | A link delivered after startup resolves through `next()`. | `a_link_while_running_dispatches_through_next` |
| Cancel | `stop()` resolves the outstanding `next()` with `ERR_DEEP_LINKS_CANCELLED`, exactly once; a delivery arriving after is dropped. | `stop_cancels_a_pending_next_exactly_once` |
| Timeout | A `next()` whose deadline passes with no delivery resolves `ERR_DEEP_LINKS_TIMEOUT`. | `a_next_waiting_past_its_deadline_resolves_as_timeout` |
| Duplicate delivery | A second delivery while one resolves maps to `ERR_DEEP_LINKS_BUSY`; the first still resolves with its link. | `a_duplicate_delivery_while_one_resolves_maps_to_busy` |
| Concurrent listeners | A second `next()` while one is outstanding resolves `ERR_DEEP_LINKS_BUSY`. | `a_second_next_while_one_is_outstanding_is_busy` |
| Malformed URL | A relative, empty or unparseable URL is refused with `ERR_DEEP_LINKS_INVALID_URL`, not dispatched. | `a_malformed_url_is_rejected_as_invalid_url` |
| Oversized URL | A URL longer than the re-exported `MAX_URL_LEN` (16 KiB) is refused with `ERR_DEEP_LINKS_URL_TOO_LARGE`. | `a_url_longer_than_max_url_len_is_rejected_as_too_large` |
| Whitespace and controls | A URL containing any whitespace or control character is refused with `ERR_DEEP_LINKS_INVALID_URL` (core's rule, also for verified links). | `a_url_with_whitespace_or_control_characters_is_rejected` |
| Unclaimed scheme | A link of a scheme the app does not claim is refused with `ERR_DEEP_LINKS_DISALLOWED_SCHEME`. | `a_link_of_an_unclaimed_scheme_is_rejected_as_disallowed` |
| Reserved scheme claim | A config claiming a reserved scheme (`https`, `file`, `mailto`, …) is refused with `ERR_DEEP_LINKS_DISALLOWED_SCHEME`. | `a_reserved_scheme_in_a_claim_is_rejected_as_disallowed` |
| Unclaimed host | An https link to a host with no matching table prefix is refused with `ERR_DEEP_LINKS_HOST_NOT_ALLOWED`. | `an_https_link_to_an_unlisted_host_is_rejected_as_host_not_allowed` |
| Case-insensitive scheme and host | `MYAPP://` matches `myapp://`; `https://Example.COM` matches the `https://example.com` prefix; params stay as written. | `scheme_and_host_match_case_insensitively` |
| Verified over custom | A delivery the system reports as a verified web link keeps its `Universal`/`App` kind and its https route; it is never downgraded to a custom scheme to force a match (auth flows rely on this). | `a_verified_web_link_is_preferred_over_a_custom_scheme_link` |
| Unsupported platform | A verified web link handed to the unsupported backend resolves `ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM`. | `a_verified_web_link_on_an_unsupported_platform_resolves_unsupported` |
| Platform failure | A delivery the backend reports as a platform failure (FFI, JNI, COM, D-Bus, the JS bridge) resolves `ERR_DEEP_LINKS_BACKEND`; `FakeBackend` and `memoryBackend()` script the failure. | `a_platform_failure_resolves_as_backend` |
| No matching route | A valid, allowed URL whose path matches no pattern resolves `ERR_DEEP_LINKS_NO_ROUTE`. | `a_link_whose_path_matches_no_route_is_rejected_as_no_route` |
| Query and fragment | Query params are parsed into `RouteMatch.params` and the fragment is kept on `Link.url` uninterpreted. | `query_params_and_fragments_are_parsed_and_preserved` |
| Path params win | A path param overrides a query param of the same name (React Navigation's rule). | `path_params_win_over_same_named_query_params` |
| Trailing slash | `/orders/42/` matches the same route as `/orders/42`; multiple slashes collapse, as React Navigation normalizes. | `a_trailing_slash_matches_the_same_route` |
| Empty path | The bare prefix (`https://example.com/`) matches the root-level `''` pattern, like React Navigation's empty-path handling. | `an_empty_path_matches_the_root_route` |
| Percent-encoding | Segments decode before matching and schema validation, hex digits compare case-insensitively (`%2f` = `%2F`), and an undecodable escape fails as `ERR_DEEP_LINKS_NO_ROUTE`. | `percent_encoded_segments_decode_before_matching` |
| Specificity ordering | Static segments match before regex params, regex before plain params, plain before repeats, repeats before wildcard. | `more_specific_patterns_match_before_wildcards` |
| Untrusted params | A param failing its `parse` schema refuses the match with `ERR_DEEP_LINKS_INVALID_PARAMETER` — never routed with a bad value. | `a_param_failing_its_parse_schema_rejects_as_invalid_parameter` |
| Invalid pattern | `RouteTable::from_config` refuses a bad pattern with `ERR_DEEP_LINKS_INVALID_ARGUMENT`, naming the pattern. | `an_invalid_pattern_in_the_route_table_is_rejected_as_invalid_argument` |
| Conflicting patterns | Two screens claiming one pattern are refused at `from_config` with `ERR_DEEP_LINKS_INVALID_ARGUMENT`. | `conflicting_patterns_for_two_screens_reject_as_invalid_argument` |
| Pending survives login | A link held before login is still claimable after a simulated login completes; login does not clear the slot. | `a_pending_link_survives_a_simulated_login` |
| Claim clears the slot | `claim` returns the held link once; a second claim gets `None`; `clear` drops it. | `claiming_a_pending_link_clears_the_slot` |
| Held-links overflow | Links waiting with no outstanding `next` are held in order, at most the re-exported `MAX_PENDING` (16); past it the oldest goes, without an error — core's rule, mirrored. | `links_waiting_past_max_pending_drop_the_oldest` |
| Wrong team | An `apple-app-site-association` whose `appID` team prefix or bundle id is not the expected one fails validation with `ERR_DEEP_LINKS_ASSOCIATION_INVALID`. | `an_apple_association_for_the_wrong_team_is_invalid` |
| Wrong package | An `assetlinks.json` whose `target` package is not the expected one fails validation with `ERR_DEEP_LINKS_ASSOCIATION_INVALID`. | `an_assetlinks_statement_for_the_wrong_package_is_invalid` |
| Per-subdomain statements | The builder emits one statement file per subdomain listed (Android `autoVerify` needs each subdomain to serve its own `assetlinks.json`). | `assetlinks_statements_are_built_per_subdomain` |
| Offline validation | Validators take bytes and never fetch: a bad cached CDN response cannot be refreshed or guessed by this library, and the test asserts no fetch path is taken. | `association_validation_is_offline_and_never_fetches` |
| Exactly once | `initial`, `next`, `stop` and `claim` each resolve exactly once across success, coded failure and cancel; a delivery after resolution is dropped. | `every_call_resolves_exactly_once` |

Thirty-five tests per language. The brief's four edge cases are covered: Apple CDN
caching (offline validation), per-subdomain assetlinks (statements built per
subdomain), verified links for auth (verified over custom), untrusted parameters
(parse-schema refusal plus the host allowlist cases).
