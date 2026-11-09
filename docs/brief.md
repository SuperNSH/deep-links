# Deep links: brief

Universal Links on iOS and macOS and App Links on Android, routed to app screens. Custom URL schemes are already in NSH core.

## Options

- Native: associated domains with NSUserActivity continueUserActivity (not in NSH core)
- Native: Android autoVerify https intent filter with assetlinks.json (not in NSH core)
- Port React Navigation's linking model for the route table
- Reference only: app_links, expo-linking

## Recommended path

Add the missing native handlers for verified web links. Route them through a validated route table with a pending-link slot that survives login.

## Edge cases to design for

- Apple's CDN caches apple-app-site-association; a failed fetch can persist for days
- Android needs android:autoVerify and a per-subdomain assetlinks.json
- Verified web links are safer than custom schemes for auth flows
- Link parameters are untrusted: validate against a schema, allowlist hosts

## Research status

Research done 2026-11-09 by web search. Versions and licences are unverified until checked on the registry before pinning. Full catalogue: the research notes for the SuperNSH library set.
