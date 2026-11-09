# nsh-deep-links

Universal Links on iOS and macOS and App Links on Android, routed to app screens. Custom URL schemes are already in NSH core.

Part of the Native Shell (NSH) capability set. Same layout as the other SuperNSH repos: a Rust crate, docs in `docs/`, CI on GitHub Actions, and a web status page on GitHub Pages at https://supernsh.github.io/deep-links/.

## Status

**Scaffold. Not implemented.** The crate exposes one placeholder function and one test. Nothing here talks to a device yet. Read [docs/brief.md](docs/brief.md) for the planned approach and [docs/parity.md](docs/parity.md) for what is done.

## Planned approach

Add the missing native handlers for verified web links. Route them through a validated route table with a pending-link slot that survives login.

## Layout

- `src/lib.rs`: placeholder crate.
- `docs/brief.md`: options, recommended path and edge cases from the research.
- `docs/decisions.md`: open decisions.
- `docs/parity.md`: feature status.
- `site/`: static status page, published by `.github/workflows/pages.yml`.
- `.zcode/workflow.ts`: the phases this area will run through.

## Checks

Every compile and test runs in GitHub Actions. Nothing is built on a developer machine.

## Licence

MIT. See [LICENSE](LICENSE).

Repository: https://github.com/SuperNSH/deep-links
