# nsh-deep-links

Universal Links on iOS and macOS, App Links on Android, and the app's custom URL schemes for
Native Shell (NSH) 0.6.0 apps: each link checked, classified and routed to a screen, and
delivered exactly once.

Part of the NSH capability set: a Rust crate, a TypeScript package that mirrors it, docs in
`docs/`, CI on GitHub Actions, and a status page at https://supernsh.github.io/deep-links/.

## Install

In the app's directory:

```sh
nsh add nsh-deep-links
```

`nsh add` needs NSH main or the nightly: the 0.6.0 and 0.6.1 releases do not have it. In a Rust app, `cargo add nsh-deep-links` works with any of them.

TSX imports `nsh-deep-links`, `nsh-deep-links/rust` and `nsh-deep-links/memory`.

## Status

Implemented and green in CI: run https://github.com/SuperNSH/deep-links/actions/runs/44376314593 (commit 274d269, "build: move to NSH 0.6.0")
passed all nine jobs: format, rust on Ubuntu, macOS and Windows, clippy cross-checks for
`aarch64-apple-ios`, `aarch64-apple-ios-sim`, `aarch64-linux-android` and
`wasm32-unknown-unknown`, and the TypeScript job (`nsh check`, `nsh build` with no warning,
`nsh test`). The macOS job was cancelled in the queue once and passed on a re-run of the same
run. Nothing has run on a device or simulator: the Apple and Android bridges are only
compile-checked. The Apple bridge changed after that run (it now chains NSH 0.6.0's own
`continueUserActivity`), and its macOS test has not run in CI yet.

What a platform delivers, and the two gaps a crate cannot close (an iOS cold-start Universal
Link, an App Link reaching a running Android app), are in
[docs/goals.md](docs/goals.md#platform-scope).

## Use

Rust:

```rust
use nsh_deep_links::{DeepLinks, RouteTable};

let routes = RouteTable::from_config(&serde_json::json!({
    "prefixes": ["https://example.com", "myapp://"],
    "screens": { "Order": { "path": "orders/:id", "parse": { "id": "integer" } } }
}))?;
let links = DeepLinks::platform(routes);
if let Some(link) = links.initial().await? {
    // link.kind, link.route.screen, link.route.params
}
let next = links.next().await?;
```

TypeScript (the app lists the crate under `"rust"` in `nsh.json`):

```ts
import { DeepLinks, RouteTable } from "nsh-deep-links";

const links = DeepLinks.platform(
  RouteTable.fromConfig({
    prefixes: ["https://example.com", "myapp://"],
    screens: { Order: { path: "orders/:id", parse: { id: "integer" } } },
  }),
);
const launch = await links.initial();
const next = await links.next();
```

The app still declares what the system needs: its schemes in `nsh.json` `schemes`, the
`applinks:` associated domains in its entitlements, and an `android:autoVerify` intent filter.
`nsh_deep_links::association` (TS: `assetlinksFiles`, `appleAppSiteAssociationFiles`) builds and
checks the files the web server must serve, one per host.

## Layout

- `crates/nsh-deep-links`: the crate. `links.rs` (`DeepLinks`, the pipeline), `routes.rs`
  (`RouteTable`), `association.rs`, `backend.rs` + `inbox.rs` (the platform seam and its
  delivery rules), `fake.rs` (`FakeBackend`), `native.rs` (`NativeDeepLinks` for TSX), and
  `platform/` (`apple`, `android`, `windows`, `linux`, `web`, `unsupported`, and `core_links`,
  the NSH core intake they share). Tests in `tests/`.
- `packages/nsh-deep-links`: the TypeScript package, `src/` mirroring the crate, `tests/`, and
  an `App.tsx` demo on a memory backend.
- `docs/goals.md`: the contract, edge cases and their tests. `docs/parity.md`: the reference
  APIs mapped, and status. `docs/decisions.md`: decisions. `docs/brief.md`: the research brief.
- `site/`: the status page, published by `.github/workflows/pages.yml`.

## Checks

`.github/workflows/ci.yml` runs `cargo fmt --check`, clippy and `cargo test` on Linux, macOS
and Windows, clippy for the iOS, Android and wasm targets, and `nsh check`, `nsh build` (no
warnings) and `nsh test` in `packages/nsh-deep-links`.

## Licence

MIT. See [LICENSE](LICENSE).

Repository: https://github.com/SuperNSH/deep-links
