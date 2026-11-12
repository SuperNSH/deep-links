# Decisions

## Open

None. The three decisions this repo opened with are resolved below.

## Decided

### 1. NSH version: nsh-core 0.5.0 from crates.io

`crates/nsh-deep-links` depends on `nsh-core = "0.5.0"`. Until 0.5.0 it depended on
`nsh-core = { git = "https://github.com/longbridge/nsh" }`: docs.rs for nsh-core 0.4.0
(checked 2026-11-10) published no `url_scheme` module, and the custom-scheme core this
crate wraps — `check_url`, `check_scheme`, `schemes_from_json`, `MAX_URL_LEN`,
`MAX_PENDING` — existed only in the repository. 0.5.0 (2026-11-11) publishes the module,
and its `url_scheme.rs` is byte-identical to the `main` commit the git dependency last
resolved to (`7cd0801`), so the pin moved to the release as planned.

### 2. Library choice: nothing imported; an org-native crate with reference parity

None of app_links, React Navigation or the Tauri deep-link plugin becomes a
dependency. app_links is a Flutter plugin speaking Dart method channels, React
Navigation's linking model is TypeScript inside its own NavigationContainer, and the
Tauri plugin is coupled to the Tauri runtime — none can be linked into an NSH Rust
crate, and the one layer they would supply (custom-scheme intake) already exists in
`nsh_core::url_scheme`. So the crate is written in-house and mirrors its references by
name and behavior, the same trade biometrics made when it named its API after
local_auth: the route table is ported from React Navigation's linking model
(`packages/core/src/getStateFromPath.tsx`, `getPatternParts.tsx`,
`validatePathConfig.tsx` — pattern syntax, specificity ordering, config validation),
the delivery plumbing keeps app_links' names where sensible (`initial` is
`getInitialLink`, `next` is the `uriLinkStream` events, one delivery resolving once),
and the associated-domain config follows Tauri's `AssociatedDomain` shape
(`plugins/deep-link/src/config.rs`). The clones under `.refs/` are the parity
references `docs/parity.md` maps row by row; nothing is vendored from them.

### 3. Platforms in scope for the first release

- **Apple (iOS + macOS):** Universal Links via `NSUserActivity`, plus custom schemes
  wrapped from core.
- **Android:** App Links (`android:autoVerify` https intents) plus wrapped custom
  schemes.
- **Windows + Linux:** wrapped custom schemes only. A verified web link resolves
  `ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM` — the desktop platforms give no verified-web
  contract this library can honor, and the Tauri plugin answers `UnsupportedPlatform`
  the same way where a platform cannot do the job.
- **Web:** association-file tooling plus page-address origin rules
  (`url_scheme`'s `PAGE`/`QUIET`/`BASE` and same-origin semantics); the initial page
  address is the only link intake, matching app_links' web feature note.

Evidence: `docs/brief.md` names exactly these native handlers as what NSH core lacks
(options 1-2); app_links' feature matrix shows desktop web-link support is partial
(Linux is marked unknown), which we scope out rather than guess at.

### Canonical repositories

The five references resolved to these clones under `.refs/`, all read-only:

- https://github.com/llfbandit/app_links
- https://github.com/react-navigation/react-navigation
- https://github.com/tauri-apps/plugins-workspace
- https://github.com/longbridge/nsh
- https://github.com/SuperNSH/biometrics

React Navigation's linking API lives inside the monorepo's `packages/native`
(NavigationContainer's `linking` prop, `useLinking`, `extractPathFromURL`) and
`packages/core` (`getStateFromPath`, `getPathFromState`, `getPatternParts`,
`validatePathConfig`) — there is no separate linking repository to clone; the
`packages/routers` package holds the navigators' state machines, not the path
matching.

### Error codes: the baseline thirteen, no additions

`docs/goals.md` is the contract for the full enum. Every failure mode the references
surface maps onto an existing baseline code, so nothing was added. The mappings that
could have demanded new codes, and why they did not: a percent-escape that fails to
decode is `ERR_DEEP_LINKS_NO_ROUTE` (React Navigation returns no state for the path,
the same outcome); association-builder inputs that are empty or malformed are
`ERR_DEEP_LINKS_INVALID_ARGUMENT` (a caller error) while statements that verify
against the wrong team, appID or package are `ERR_DEEP_LINKS_ASSOCIATION_INVALID`
(a verification failure); platform FFI, JNI, COM, D-Bus and I/O failures are
`ERR_DEEP_LINKS_BACKEND`. The table is add-only from here; codes are never renamed or
reused.
