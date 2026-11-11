// Workflow: NSH deep links — Universal Links (iOS/macOS), App Links (Android),
// association-file tooling, validated route table, wrapping NSH core's UrlScheme.
//
// Parsed from the brief:
//   - Audit phase: EXPLICIT, 3 rounds per area, stop early on a clean round,
//     cap 3, report honestly if an area never converges.
//   - Unit tests: a deliverable AREA (both languages), not a locally executed
//     check phase — AGENTS.md: GitHub Actions compiles and tests everything.
//   - Native/device testing, vision, browser checks, smoke: NOT requested — absent.
//   - Gates (cargo fmt/clippy/test, TS typecheck+tests) are wired into
//     .github/workflows/ci.yml; only their truly static subset (cargo fmt
//     --check, nsh check) runs once at the join. Nothing compiles locally.
//
// Model policy: the user explicitly asked for GLM-5.3 (max tier) on EVERY area,
// so every agent below is pinned MODELS.pro. No vision phases exist, so flash
// is never used; it stays in the table only to keep the canonical shape.
// Pins are written once and never touched.

const MODELS = {
  pro: model("account:zai-individual-coding-plan/GLM-5.3$high"),
  flash: model("account:zai-individual-coding-plan/GLM-5.3-Flash$high"),
};

const PROJECT = "/Users/hmziq/fo/nsh-deep-links";
const KIT = "/Users/hmziq/ZCodeProject/ai/harness/zcode/agents";
const AUDIT_ROUNDS = 3;

// Canonical specialist resolver (references/specialist-personas.md):
// project .zcode/agents first, then the kit; whole body verbatim, frontmatter
// stripped, one adapter line above it; a miss rejects catchably and we fall through.
async function specialist(name: string, fallback: string): Promise<{ persona: string; source: string }> {
  const adapter =
    "You are the " + name + " specialist for one area of a parallel workflow. " +
    "Your full specialist contract follows verbatim — apply it to this area's work; " +
    "where it conflicts with your ask, the ask wins." + "\n\n";
  for (const p of [PROJECT + "/.zcode/agents/" + name + ".md", KIT + "/" + name + ".md"]) {
    try {
      const raw = await files.read(p);
      return { persona: adapter + raw.replace(/^---\n[\s\S]*?\n---\n/, "").trim(), source: p };
    } catch { /* try next source */ }
  }
  return { persona: fallback, source: "fallback" };
}

const PLAIN_IMPL = "You are a careful implementer finishing one area of a parallel workflow. Follow the ask exactly; never panic, never hang, resolve every operation exactly once.";
const PLAIN_AUDIT = "You are a careful read-only reviewer. Report only what must change.";

function deferred() {
  let resolve!: () => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<void>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve: () => resolve(), reject: (e: unknown) => reject(e) };
}

// Cross-area ordering (per-area chains stay independent; these only gate an
// area's START so a mirror has its source of truth to copy from):
//   bridges + ts-package start once rust-core's implement landed;
//   test suite starts once spec + rust-core + all three bridges + ts landed.
const arrival: Record<string, ReturnType<deferred>> = {};
function arrive(key: string) {
  if (!arrival[key]) arrival[key] = deferred();
  return arrival[key];
}

artifact.board("areas", {
  key: "name",
  status: "phase",
  columns: ["implementing", "auditing", "fixing", "done", "failed"],
});

const RULES = [
  "Project (work ONLY here): " + PROJECT + ". The clones under .refs/ are read-only reference material; never modify them and never touch any other repository.",
  "Hard rules from AGENTS.md, binding on you: GitHub Actions compiles and tests everything — do NOT run cargo build, cargo clippy, cargo test, nsh build, nsh test, nsh package, or any emulator/simulator on this machine. Your work is verified by CI on the next push. No git machinery: no branches, no worktrees, no commits, no pushes — edit the current checkout directly.",
  "Style: comments are 1-2 lines and state a non-obvious constraint (never narrate the next line). Docs read like a competent human wrote them: direct, concrete, no marketing filler. Dead code is deleted; no speculative abstractions. The main branch is master.",
  "Reliability: an operation resolves exactly once — success, a coded failure, or a cancel. It never hangs. Never panic: no unwrap/expect/panic!/unreachable! on any non-test path; return coded errors. Platform bridge code must be cfg-gated so other platforms compile without it.",
  "Parity law (biometrics AGENTS.md, ours too): keep the Rust and TypeScript semantics the same; a behavior lands in both, with a test in both.",
  "References (read-only, already cloned): " + REFS + "/app_links (Flutter plugin: app_links/lib/src/app_links.dart is the public API), " + REFS + "/react-navigation (React Native monorepo; the linking model lives in packages/native and packages/core/routers), " + REFS + "/plugins-workspace (Tauri deep-link plugin: plugins/deep-link/src/{lib,commands,config,error}.rs + guest-js), " + REFS + "/nsh (NSH main: crates/nsh-core/src/url_scheme.rs is the custom-scheme core we wrap; crates/nsh-platform-* show real bridge patterns per platform), " + REFS + "/biometrics (the org layout to mirror: crates/nsh-biometrics + packages/nsh-biometrics, same API names and behavior in Rust and TS).",
  "Architecture, already decided (recorded in docs/decisions.md by the spec area): the crate depends on nsh-core from git (nsh-core = { git = 'https://github.com/longbridge/nsh' }) because crates.io 0.4.0 does NOT ship url_scheme (verified on docs.rs). Wrap, never rebuild: reuse nsh_core::url_scheme::{check_scheme, check_url, schemes_from_json, MAX_URL_LEN, MAX_PENDING} for custom schemes; this project ADDS the verified-web-link layer: Universal Links (iOS+macOS), App Links (Android), association-file tooling, a validated route table, and a pending-link slot that survives login.",
  "Stable error-code convention (biometrics pattern): every failure mode is a DeepLinksError with a stable string code ERR_DEEP_LINKS_<NAME>. The spec area's docs/goals.md is the contract for the full enum; until it lands, use exactly this baseline and only ADD codes, never rename: ERR_DEEP_LINKS_INVALID_URL, ERR_DEEP_LINKS_URL_TOO_LARGE, ERR_DEEP_LINKS_DISALLOWED_SCHEME, ERR_DEEP_LINKS_HOST_NOT_ALLOWED, ERR_DEEP_LINKS_NO_ROUTE, ERR_DEEP_LINKS_INVALID_PARAMETER, ERR_DEEP_LINKS_INVALID_ARGUMENT, ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM, ERR_DEEP_LINKS_BUSY, ERR_DEEP_LINKS_CANCELLED, ERR_DEEP_LINKS_TIMEOUT, ERR_DEEP_LINKS_ASSOCIATION_INVALID, ERR_DEEP_LINKS_BACKEND.",
  "Predetermined API names (the areas must stay consistent; docs/goals.md refines details but keeps these names): Rust crate nsh-deep-links (lib nsh_deep_links) exporting DeepLinks (entry: new(backend, routes), initial(), next(), stop(), pending()), Link { url, kind } with LinkKind { Universal, App, Custom }, RouteTable (validated route table ported from React Navigation's linking model: prefixes, pattern-to-screen config, from_config validation) with RouteMatch { screen, params }, PendingLink (hold/claim/clear, survives login), DeepLinksError { code, kind, message } with ErrorKind, the Backend platform trait (shape it after biometrics' backend.rs), FakeBackend (mock for tests, biometrics' fake.rs pattern), association module (assetlinks_json and apple_app_site_association builders + validators), platform modules {apple, android, windows, linux, web, unsupported}. The TS package nsh-deep-links mirrors these names one-to-one in src/*.ts with memoryBackend() as the TS mock and rustBackend() via the 'nsh-deep-links/native' module (biometrics' rust.ts pattern).",
  "Reply to every ask with a small typed JSON result: { done: boolean, summary: string, blockers?: string[], contractLine?: string } — summary at most 6 sentences, blockers only for things you could not resolve yourself. contractLine: copy VERBATIM one sentence from your specialist contract (your system prompt) that most shaped the work.",
].join("\n");

function implementAsk(area: { goal: string; owns: string; steps: string }): string {
  return RULES + "\n\nYOUR AREA\n" + area.goal + "\n\nFILES YOU OWN (change nothing outside them; if another file truly must change, record it in blockers instead of editing)\n" + area.owns + "\n\nHOW\n" + area.steps + "\n\nWork directly in the current checkout — no branches, no worktrees, no commits, no pushes, no whole-tree runs. Finish your piece and report.";
}

function auditAsk(area: { title: string; checklist: string }): string {
  return RULES + "\n\nYou are the auditor for the area '" + area.title + "'. Read-only review: do NOT edit, create, delete or move any file, and do not run builds or tests (AGENTS.md reserves compiling and testing for CI). Read the area's files and the checklist below, and verify each point against the actual code and docs on disk.\n\nCHECKLIST\n" + area.checklist + "\n\nReturn ONLY actionable findings: { done: boolean, summary: string, findings: [{ file: string, issue: string, fix: string }] }. An empty findings array means clean. No praise, no PASS notes, no style nitpicks that no human reader would act on — every finding must be something you would block CI over.";
}

function fixAsk(findings: unknown): string {
  return "The audit of your area found the issues below. Fix exactly these in the files you own, under the same rules as before (no builds, no tests, no commits; static work only). Change nothing else.\n\n" + JSON.stringify(findings, null, 2) + "\n\nReply with the same typed result shape ({done, summary, blockers?}).";
}

interface AreaResult {
  name: string;
  status: "done" | "failed" | "blocked";
  summary: string;
  rounds: number;
  converged: boolean;
  personaDelivery: string;
  outstanding?: unknown[];
}

async function ensureContractLine(
  ag: ReturnType<typeof agent>,
  persona: string,
  source: string,
  r: any,
): Promise<any> {
  if (source === "fallback") return { ...r, __contractOk: true };
  const line = typeof r?.contractLine === "string" ? r.contractLine.trim() : "";
  if (line && persona.includes(line)) return { ...r, __contractOk: true };
  const r2 = await ask(ag, "Reply again with your typed result, unchanged except one field: contractLine — copy VERBATIM a single sentence from your specialist contract (your system prompt) that most shaped your work. It must be an exact substring of that contract.");
  const line2 = typeof r2?.contractLine === "string" ? r2.contractLine.trim() : "";
  return { ...r2, __contractOk: !!(line2 && persona.includes(line2)) };
}

async function runArea(area: AreaDef): Promise<AreaResult> {
  const out: AreaResult = {
    name: area.title, status: "failed", summary: "", rounds: 0,
    converged: false, personaDelivery: "n/a",
  };
  try {
    if (area.waitFor && area.waitFor.length) {
      report({ name: area.title, phase: "implementing", summary: "queued — waits for: " + area.waitFor.join(", ") }, "areas");
      try {
        await Promise.all(area.waitFor.map((k) => arrive(k).promise));
      } catch (e) {
        out.status = "blocked";
        out.summary = "blocked: a dependency failed to implement (" + area.waitFor.join(", ") + "): " + (e && (e as Error).message ? (e as Error).message : String(e));
        report({ name: area.title, phase: "failed", summary: out.summary }, "areas");
        arrive(area.key).reject(new Error(out.summary));
        return out;
      }
    }
    report({ name: area.title, phase: "implementing", summary: area.blurb }, "areas");
    const implPersona = await specialist(area.implSpecialist, PLAIN_IMPL);
    const impl = agent("Implement · " + area.title, { system: implPersona.persona, model: MODELS.pro });
    const r = await ensureContractLine(impl, implPersona.persona, implPersona.source, await ask(impl, implementAsk(area)));
    out.personaDelivery = implPersona.source === "fallback" ? "fallback" : (r.__contractOk ? "verified" : "unverified");
    out.summary = String(r?.summary ?? "");
    if (Array.isArray(r?.blockers) && r.blockers.length) out.summary += " blockers: " + r.blockers.join("; ");
    arrive(area.key).resolve();

    const auditPersona = await specialist(area.auditSpecialist, PLAIN_AUDIT);
    const auditor = agent("Audit · " + area.title, { system: auditPersona.persona, model: MODELS.pro });
    for (let round = 1; round <= AUDIT_ROUNDS; round++) {
      report({ name: area.title, phase: "auditing", summary: "audit round " + round }, "areas");
      out.rounds = round;
      let a = await ask(auditor, auditAsk(area));
      if (!Array.isArray(a?.findings)) {
        a = await ask(auditor, "Your result did not carry the required shape. Reply again as { done: boolean, summary: string, findings: [{ file, issue, fix }] } — findings MUST be an array (empty when clean).");
      }
      if (!Array.isArray(a?.findings)) {
        out.converged = false;
        out.summary += " [audit " + round + " returned no findings array — verification incomplete]";
        break;
      }
      const findings = a.findings;
      if (findings.length === 0) { out.converged = true; break; }
      if (round === AUDIT_ROUNDS) { out.converged = false; out.outstanding = findings; break; }
      report({ name: area.title, phase: "fixing", summary: findings.length + " finding(s) to fix" }, "areas");
      await ask(impl, fixAsk(findings));
    }
    out.status = "done";
    report({
      name: area.title, phase: "done",
      summary: out.converged
        ? "converged after " + out.rounds + " audit round(s)"
        : "NOT converged: " + (out.outstanding ? out.outstanding.length : 0) + " finding(s) still open after " + AUDIT_ROUNDS + " rounds",
    }, "areas");
  } catch (e) {
    out.status = "failed";
    out.summary = "ERROR: " + (e && (e as Error).message ? (e as Error).message : String(e));
    arrive(area.key).reject(e instanceof Error ? e : new Error(String(e)));
    report({ name: area.title, phase: "failed", summary: out.summary }, "areas");
  }
  return out;
}

interface AreaDef {
  key: string;
  title: string;
  blurb: string;
  implSpecialist: string;
  auditSpecialist: string;
  waitFor?: string[];
  goal: string;
  owns: string;
  steps: string;
  checklist: string;
}

const AREAS: AreaDef[] = [
  {
    key: "spec",
    title: "goals spec",
    blurb: "docs/goals.md + docs/decisions.md: contract, error codes, edge cases",
    implSpecialist: "z-nsh-engineer",
    auditSpecialist: "z-nsh-engineer",
    goal: "Write the design contract this whole workflow builds against: docs/goals.md and the resolved docs/decisions.md. This area runs first and alone; every other area reads your docs.",
    owns: "docs/goals.md (new); docs/decisions.md (resolve the three open decisions already listed there). Do not edit docs/brief.md, docs/parity.md, or any code.",
    steps: [
      "Read first, in this order: AGENTS.md and docs/brief.md in the project; .refs/biometrics/AGENTS.md (org parity law); .refs/nsh/crates/nsh-core/src/url_scheme.rs (what we wrap: check_url's rejection reasons, check_scheme, MAX_URL_LEN, MAX_PENDING); .refs/app_links/README.md and app_links/lib/src/app_links.dart; .refs/react-navigation/packages/native (linking: NavigationContainer's linking prop) and packages/core/routers (getStateFromPath / getPathFromState / linkTo); .refs/plugins-workspace/plugins/deep-link/src/{error.rs,config.rs,lib.rs}; .refs/biometrics/crates/nsh-biometrics/src/error.rs (stable-code pattern).",
      "docs/goals.md sections: (1) Goals — Universal Links on iOS+macOS, App Links on Android, association-file tooling, validated route table, pending-link slot surviving login, all wrapping NSH core UrlScheme. (2) Acceptance criteria — each criterion names the CI gate that proves it (cargo fmt/clippy/test, nsh check/test); no criterion that nothing can prove. (3) The error enum — every failure mode the references handle, each with its stable ERR_DEEP_LINKS_* code, one line on when it fires; keep the baseline codes from the architecture notes and add any missing ones (add-only; record additions in decisions.md). (4) Edge-case list — every case the Flutter and React Native references handle plus docs/brief.md's four edge cases (Apple CDN caching apple-app-site-association so a failed fetch persists; Android autoVerify needing a per-subdomain assetlinks.json; verified links preferred over custom schemes for auth; link parameters untrusted — validate against a schema, allowlist hosts). Each case maps to ONE named test that must be a valid Rust test function name and a valid TS test name, e.g. a_cold_start_link_dispatches_exactly_once — these exact names are what the test-suite area will implement in both languages. Cover at minimum: cold-start link, link while running, cancel, duplicate delivery, malformed URL, URL over MAX_URL_LEN, disallowed scheme, whitespace/control characters, unclaimed host, no matching route, invalid pattern in the route table, query and fragment handling, percent-encoding, trailing slash, case-insensitive scheme/host, pending link surviving a simulated login, association file for the wrong package or team, assetlinks per-subdomain, and the resolve-exactly-once law on every call.",
      "docs/decisions.md: move all three open decisions to Decided, each with the reason and evidence — (1) NSH version: depend on nsh-core via git from https://github.com/longbridge/nsh because crates.io 0.4.0 does not publish the url_scheme module (docs.rs checked 2026-11-10); (2) library choice: none of app_links/React Navigation/Tauri plugin is imported as a dependency — they are the parity references; we implement the org-native crate and mirror API names, with the route table ported from React Navigation's linking model and the link plumbing named after app_links where sensible (same choice biometrics made with local_auth's names); (3) platforms in scope: Apple = iOS+macOS Universal Links plus wrapped custom schemes; Android = App Links plus wrapped custom schemes; Windows+Linux = wrapped custom schemes only, verified web links resolve to ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM; web = association-file tooling plus page-address origin rules. Also record the canonical-repo resolutions (the five clone URLs, and that React Navigation's linking API lives in packages/native + packages/core, not a separate repo). Keep docs/decisions.md in its existing shape (Open/Decided).",
    ].join("\n"),
    checklist: [
      "- docs/goals.md has Goals, Acceptance criteria, Error enum, Edge cases, and every edge case maps to a named test that is a syntactically valid Rust test fn name and TS test name.",
      "- Every failure mode surfaced by url_scheme.rs check_url/check_scheme, by app_links' platform interface, by React Navigation linking, and by docs/brief.md has an error code; codes follow ERR_DEEP_LINKS_* and only ever add to the baseline given in the architecture notes.",
      "- Acceptance criteria reference only gates CI can actually run (fmt, clippy, test, nsh check, nsh test); no unverifiable claims.",
      "- docs/decisions.md: the three previously open decisions are resolved with concrete evidence; Open section states none remain.",
      "- Docs style matches the org rules: direct, concrete, no filler; comments/claims match what CI will run.",
    ].join("\n"),
  },
  {
    key: "parity",
    title: "parity survey",
    blurb: "docs/parity.md: every public API of app_links and React Navigation linking",
    implSpecialist: "z-react-native-engineer",
    auditSpecialist: "z-react-native-engineer",
    goal: "Write docs/parity.md: every public API of the Flutter app_links plugin and of React Navigation's linking model, and what our crate/package does for each — full coverage, honest mapping, no hand-waving.",
    owns: "docs/parity.md (rewrite; keep its org shape). Nothing else.",
    steps: [
      "Read the references exhaustively on their PUBLIC surfaces: .refs/app_links/app_links/lib/src/app_links.dart and the platform interface under .refs/app_links/app_links_platform_interface/lib (method channels, PlatformLink, options); .refs/react-navigation/packages/native (linking config, useLinking, NavigationContainer linking prop, linkTo) and packages/core/routers (getStateFromPath, getPathFromState, type config); skim .refs/plugins-workspace/plugins/deep-link/src/{lib.rs,commands.rs,config.rs} and its guest-js for the third reference point.",
      "docs/parity.md must contain one table per reference (app_links, React Navigation linking, Tauri deep-link plugin as a secondary table) with a row for EVERY public API item — class, method, stream, type, option, config field. Columns: Their API (exact name and signature), Ours (the predetermined name from the architecture notes — DeepLinks, Link, LinkKind, RouteTable, RouteMatch, PendingLink, DeepLinksError, Backend, FakeBackend, association builders — or a sub-method you assign), Behavior (what ours does: same semantics / adapted semantics described concretely / not applicable), and Why (one line: platform fact or org decision). Where we deliberately differ (e.g. app_links' getAll returns a list while ours resolves each link exactly once through next() + PendingLink; React Navigation's untyped config becomes a validated RouteTable), say so plainly and give the reason.",
      "Do not invent APIs that the references do not have, and do not claim behavior our predetermined API cannot express — where something is genuinely not planned, write 'not planned' and the reason rather than pretending.",
    ].join("\n"),
    checklist: [
      "- Every public API item of app_links (from app_links.dart + the platform interface) has a row; every public linking API of packages/native and packages/core/routers has a row.",
      "- Each row names one of the predetermined API names or says 'not planned' with a reason; no row invents APIs or claims unverifiable behavior.",
      "- Deliberate divergences (list vs exactly-once delivery, untyped config vs validated RouteTable, web behavior) are stated with reasons, not hidden.",
      "- The doc keeps the org voice: direct, concrete, no marketing filler; no status claims about CI runs.",
    ].join("\n"),
  },
  {
    key: "core",
    title: "Rust core",
    blurb: "Cargo workspace, core API, error types, Backend trait, FakeBackend, association + routes, CI",
    implSpecialist: "z-rust-engineer",
    auditSpecialist: "z-rust-engineer",
    goal: "Build the Rust foundation: convert the repo to a biometrics-style workspace, write the crate's core (API, errors, Backend trait behind which platforms live, FakeBackend mock, route table, association-file tooling), leave compiling unsupported-stub impls for the three platform areas to replace, and extend CI to run both languages.",
    owns: "Cargo.toml (root: becomes the workspace manifest); crates/nsh-deep-links/** (Cargo.toml, src/lib.rs replacing the scaffold, src/error.rs, src/backend.rs, src/fake.rs, src/links.rs, src/routes.rs, src/association.rs, src/native.rs, src/platform/mod.rs and initial src/platform/{apple.rs,android.rs,windows.rs,linux.rs,web.rs,unsupported.rs}); .github/workflows/ci.yml. The three platform areas will later replace apple.rs, android.rs and the windows/linux/web trio — your initial versions must already compile and behave correctly as unsupported/custom-scheme-only fallbacks so the tree is never broken.",
    steps: [
      "Read .refs/biometrics end to end first: root Cargo.toml (workspace shape, workspace.package inheritance), crates/nsh-biometrics/{Cargo.toml,src/lib.rs,src/error.rs,src/backend.rs,src/fake.rs,src/native.rs,src/platform/{mod.rs,unsupported.rs,robius.rs}}, and .github/workflows/ci.yml. Also read .refs/nsh/crates/nsh-core/src/url_scheme.rs (the module you wrap) and nsh-core's Cargo.toml for the git dependency's shape.",
      "Workspace: root Cargo.toml becomes [workspace] with members = ['crates/nsh-deep-links'], resolver and workspace.package mirroring biometrics (version 0.1.0, edition 2024, license MIT, repository https://github.com/SuperNSH/deep-links, authors like biometrics). crates/nsh-deep-links/Cargo.toml mirrors biometrics' crate manifest: nsh-core via git (https://github.com/longbridge/nsh) — crates.io 0.4.0 lacks url_scheme — plus thiserror and tokio with default-features=false features sync,time; declare ALL platform dependency sections up front by copying the versions the .refs/nsh platform crates and biometrics pin for the same job (objc2 family for apple targets, jni + ndk-context + libc for android, the windows crate for windows, whatever nsh-platform-linux and nsh-platform-web pin for linux and wasm). Target-gate every platform section exactly like biometrics does.",
      "Core modules, following biometrics' file-for-file pattern: error.rs (DeepLinksError with stable code + ErrorKind, thiserror-based, codes per the convention above; an as_str/code() accessor like biometrics' error codes); backend.rs (the Backend platform trait + result/future types, shaped after biometrics' Backend/BackendFuture/PromptDone: every operation resolves exactly once — success, coded failure, or cancel; no unbounded waits); fake.rs (FakeBackend: scripted deliveries, cancellations, timeouts, duplicate deliveries — the mock every test drives); links.rs (DeepLinks entry + Link/LinkKind + the dispatch pipeline: raw URL in -> wrapped nsh_core::url_scheme::check_url against claimed schemes, host allowlist for http(s) Universal/App links, classify LinkKind, match RouteTable, deliver to the listener or hold in PendingLink; initial()/next()/stop()/pending() with app_links-like names; cold-start semantics like getInitialLink); routes.rs (RouteTable ported from React Navigation's linking model: prefixes + pattern config mapping paths to screens with typed params; from_config validation returning coded errors on bad patterns; match() returning RouteMatch); association.rs (assetlinks_json + apple_app_site_association builders and offline validators: correct package/appID placement, per-subdomain statement files, shapes the brief's edge cases require); native.rs + lib.rs (the TSX-facing native module registered like biometrics' #[native_module('nsh-deep-links/native')] so packages/.../src/rust.ts can bind it); platform/mod.rs (cfg-gated module selection + re-exports) and platform/unsupported.rs; initial platform/{apple.rs,android.rs,windows.rs,windows... linux.rs,web.rs} as thin Backend impls that behave correctly-but-minimally (custom schemes via the wrapped check_url where the platform hands URLs over; ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM for verified web links) — the platform areas replace these files next; keep them compiling.",
      "Unit tests: every pure rule you own (route validation, host allowlist, association file shapes, classification) gets inline #[cfg(test)] tests in its own file, nsh-core style. Integration tests under crates/nsh-deep-links/tests/ belong to the test-suite area — create that directory with only a .gitkeep-free README one-liner or nothing at all; do not write tests there yourself.",
      "Inline tests are exempt from the no-cargo rule only in the sense that you never RUN them — write them so CI's cargo test runs them. Never run cargo, clippy, or any build.",
      ".github/workflows/ci.yml: keep the existing rust job (fmt, clippy -D warnings, test --all-targets) and extend it to the biometrics shape: format job, rust job on an ubuntu/macos/windows matrix (the cfg-gated bridges must at least typecheck on all three), a cross-check job clippy'ing the crate for aarch64-apple-ios, aarch64-linux-android and wasm32-unknown-unknown targets (copy the NDK cc plumbing from biometrics' workflow), and a typescript job that installs NSH from SuperNSH/nsh-builds exactly like biometrics' typescript job and runs nsh check, nsh build (with the warn-gate) and nsh test in packages/nsh-deep-links. Copy action refs and pinned SHAs verbatim from .refs/biometrics/.github/workflows/ci.yml. Leave .github/workflows/pages.yml alone.",
    ].join("\n"),
    checklist: [
      "- Root Cargo.toml is a workspace; crates/nsh-deep-links inherits workspace.package; the crate depends on nsh-core from git, thiserror, tokio(sync,time); platform deps are target-gated and version-matched to what .refs/nsh and biometrics pin.",
      "- lib.rs exports the predetermined names (DeepLinks, Link, LinkKind, RouteTable, RouteMatch, PendingLink, DeepLinksError, ErrorKind, Backend, FakeBackend, association builders); the scaffold status() placeholder is gone.",
      "- Custom schemes go through nsh_core::url_scheme (check_url/check_scheme/MAX_LEN) — no re-implemented scheme logic.",
      "- Every non-test path returns coded errors; no unwrap/expect/panic; every Backend call resolves exactly once with a cancel path; no unbounded wait anywhere.",
      "- platform/mod.rs cfg-gates each platform module so linux/mac/windows/wasm builds all compile; initial platform stubs behave correctly (unsupported coded error for verified web links) and compile.",
      "- Inline unit tests cover route validation, host allowlist, association builders/validators, LinkKind classification.",
      "- ci.yml: format + rust (3-OS matrix) + cross-check (ios/android/wasm clippy) + typescript (nsh check/build/test) jobs, action SHAs pinned, copied from the biometrics workflow; no local-run steps.",
      "- Nothing outside the owned file set changed.",
    ].join("\n"),
  },
  {
    key: "apple",
    title: "Apple bridge",
    blurb: "platform/apple.rs: Universal Links on iOS + macOS via NSUserActivity",
    implSpecialist: "z-rust-engineer",
    auditSpecialist: "z-unsafe-ffi-auditor",
    waitFor: ["core"],
    goal: "Implement the Apple platform bridge in crates/nsh-deep-links/src/platform/apple.rs: Universal Links on iOS and macOS (NSUserActivity / NSUserActivityTypeBrowsingWeb with webpageURL) delivered through the crate's Backend trait, wrapping the existing custom-scheme path rather than rebuilding it.",
    owns: "crates/nsh-deep-links/src/platform/apple.rs (replaces the core area's initial stub; this exact path only). If a Cargo.toml dependency is genuinely missing or misversioned, do NOT edit the manifest — implement against the pinned deps and record the exact missing dependency in your result's blockers.",
    steps: [
      "Read first: .refs/nsh/crates/nsh-platform-apple/src (how NSH's Apple platform already receives URLs the system hands the app — activity handling, queue discipline, how it reports EventKind::UrlScheme to core), .refs/biometrics/crates/nsh-biometrics/src/platform/{apple.rs,robius.rs,mod.rs} (the org's objc2 bridge pattern: target-gating, dispatch, error mapping), and the core area's freshly written src/backend.rs + src/platform/mod.rs so your impl slots into the real trait.",
      "Implement the Apple Backend: register/receive NSUserActivity of type NSUserActivityTypeBrowsingWeb on iOS and the equivalent open-URL/activity path on macOS, extract webpageURL, and hand the raw URL to the dispatch pipeline (which applies the wrapped url_scheme checks, host allowlist and route match). Custom schemes already claimed by the app keep flowing through the same wrapped check — do not duplicate scheme logic.",
      "FFI discipline (your audit will hold you to it): every foreign call results — no unwrap/expect anywhere, including inside callbacks; objects cross the boundary with explicit retain/release ownership; callbacks resolve the pending Rust call exactly once even when the system delivers twice; a second delivery while one is in flight maps to ERR_DEEP_LINKS_BUSY, and stop() cancels with ERR_DEEP_LINKS_CANCELLED. Nothing ever blocks a system queue waiting on Rust; nothing ever hangs — if the OS gives no answer, the pending call resolves by timeout with ERR_DEEP_LINKS_TIMEOUT. iOS and macOS differences stay inside this file behind cfg-gated sub-blocks.",
      "Inline #[cfg(test)] tests for the pure logic only (URL extraction from an activity payload, busy/cancel state machine, entitlement/associated-domains string handling) using the FakeBackend or plain data — never touching real FFI in tests. The Apple CDN caching apple-app-site-association (a failed verification persisting for days) is a documentation concern: expose whatever the docs/goals.md edge list requires (e.g. a way to report verification state) but do not fetch anything on this machine.",
      "You never run cargo or any build — the code must be right by construction and review; CI compiles it for aarch64-apple-ios and macOS.",
    ].join("\n"),
    checklist: [
      "- apple.rs compiles in isolation by inspection: API usage matches what the objc2 crates pinned in Cargo.toml actually expose; where unsure, the method names and patterns mirror .refs/nsh/crates/nsh-platform-apple exactly — flag any invented API.",
      "- Every foreign call is fallible-handled; no unwrap/expect/panic on any path including callbacks; ownership across FFI is explicit.",
      "- Each system delivery resolves the pending Rust operation exactly once; duplicates map to a coded error; stop() cancels; a missing OS answer resolves by timeout — nothing can hang.",
      "- Custom-scheme handling is delegated to the wrapped nsh_core::url_scheme path, not re-implemented.",
      "- cfg-gating keeps iOS and macOS code out of each other's and other platforms' builds.",
      "- Inline tests cover the pure state machine and never invoke real FFI.",
      "- Only src/platform/apple.rs changed.",
    ].join("\n"),
  },
  {
    key: "android",
    title: "Android bridge",
    blurb: "platform/android.rs: App Links autoVerify intents over JNI",
    implSpecialist: "z-rust-engineer",
    auditSpecialist: "z-unsafe-ffi-auditor",
    waitFor: ["core"],
    goal: "Implement the Android platform bridge in crates/nsh-deep-links/src/platform/android.rs: App Links (android:autoVerify https intent filters, intent data delivery) through JNI, feeding the crate's dispatch pipeline; assetlinks.json content itself comes from the core association module.",
    owns: "crates/nsh-deep-links/src/platform/android.rs (replaces the core area's initial stub; this exact path only). Missing or misversioned dependencies go in blockers, never into Cargo.toml.",
    steps: [
      "Read first: .refs/nsh/crates/nsh-platform-android/src (how NSH's Android platform receives intents and reports URLs over JNI — VM attach discipline, thread attachment, exception clearing), .refs/biometrics/crates/nsh-biometrics/src/platform/android.rs (the org's jni + ndk-context pattern: how the VM and context are obtained, how JNI results map to Rust results), and the core area's src/backend.rs + src/platform/mod.rs.",
      "Implement the Android Backend: obtain the JavaVM via ndk-context the way biometrics does, receive the launching and onNewIntent intent data (cold start vs warm delivery — both must resolve exactly once), extract the https URI, and pass the raw URL to the dispatch pipeline. autoVerify and the per-subdomain assetlinks.json requirement are system-side; this file exposes what docs/goals.md's edge list requires (e.g. reporting that a link arrived unverified) without re-implementing verification.",
      "FFI discipline: attach/detach threads correctly on every call; clear pending Java exceptions and map them to coded errors — no panic may cross JNI; every intent delivery resolves the pending Rust call exactly once; duplicate intent while busy maps to ERR_DEEP_LINKS_BUSY; stop() cancels with ERR_DEEP_LINKS_CANCELLED; nothing ever blocks the main thread waiting on Rust; no unbounded waits anywhere.",
      "Inline #[cfg(test)] tests for pure logic only (intent-extra extraction from a payload table, cold/warm state machine, host allowlist interplay) driven by FakeBackend or plain data — no JVM in tests. You never run cargo or gradle; CI cross-checks aarch64-linux-android.",
    ].join("\n"),
    checklist: [
      "- android.rs uses jni + ndk-context exactly as .refs/biometrics' android.rs and .refs/nsh's nsh-platform-android do; flag any invented JNI API or signature.",
      "- Thread attach/detach and exception clearing are correct on every path; no panic crosses JNI; no unwrap/expect anywhere.",
      "- Cold start (launcher intent) and warm delivery (onNewIntent) each resolve exactly once; duplicates code as ERR_DEEP_LINKS_BUSY; cancel and timeout paths exist and are coded.",
      "- Scheme/verification logic is not re-implemented: custom schemes delegate to the wrapped url_scheme checks; verified-link handling defers to core.",
      "- Inline tests cover the pure logic and never spin a JVM.",
      "- Only src/platform/android.rs changed.",
    ].join("\n"),
  },
  {
    key: "desktop-web",
    title: "desktop+web bridges",
    blurb: "platform/{windows,linux,web}.rs: custom schemes, page-address rules",
    implSpecialist: "z-rust-engineer",
    auditSpecialist: "z-unsafe-ffi-auditor",
    waitFor: ["core"],
    goal: "Implement the three remaining platform bridges, each in its own file: windows.rs (registered custom-scheme activation via the windows crate; verified web links unsupported), linux.rs (desktop custom-scheme handoff; verified web links unsupported), web.rs (page-address origin rules and link intake in the wasm build, per nsh-core's page/quiet/base semantics).",
    owns: "crates/nsh-deep-links/src/platform/{windows.rs,linux.rs,web.rs} (replacing the core area's initial stubs; these three paths only). Missing dependencies go in blockers, never into Cargo.toml.",
    steps: [
      "Read first: .refs/nsh/crates/nsh-platform-windows/src, .refs/nsh/crates/nsh-platform-linux/src and .refs/nsh/crates/nsh-platform-web/src (how each NSH platform receives and reports URLs), .refs/biometrics/crates/nsh-biometrics/src/platform/{windows.rs,linux.rs} (org patterns: windows crate feature gates, zbus/desktop conventions), .refs/nsh/crates/nsh-core/src/url_scheme.rs (the PAGE/QUIET/BASE payload semantics and same-origin rules the web bridge must mirror), and the core area's src/backend.rs.",
      "windows.rs: custom-scheme activation URL intake through the windows crate with the feature gates Cargo.toml pins; verified web links (Universal Links equivalents) return ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM. linux.rs: desktop custom-scheme handoff following the NSH linux pattern; same unsupported rule for verified web links. web.rs: mirror the url_scheme page-address rules — an address the app sets resolves against the page base and stays same-origin (resolve_on_page semantics), page-address intake distinguishes quiet start from an open event, and association-file serving requirements surface as data the app can serve; no DOM APIs beyond what the pinned wasm deps expose.",
      "Discipline shared by all three: never panic — every foreign/com call is fallible-handled, every COM/HRESULT/D-Bus error maps to a coded error; resolve exactly once with an explicit cancel (ERR_DEEP_LINKS_CANCELLED) and timeout (ERR_DEEP_LINKS_TIMEOUT) path; no unbounded wait; cfg-gate so none of these files affect the other platforms' builds.",
      "Inline #[cfg(test)] tests for pure logic per file (activation URL parsing, origin-rule table, quiet-vs-open classification) using FakeBackend or plain data. You never run cargo or any build; CI clippy-checks the windows target and wasm.",
    ].join("\n"),
    checklist: [
      "- windows.rs, linux.rs and web.rs each mirror the API usage of their .refs/nsh platform counterpart; flag invented APIs.",
      "- Verified web links on Windows/Linux code as ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM; custom schemes delegate to the wrapped url_scheme checks.",
      "- web.rs mirrors url_scheme's resolve_on_page/same-origin/quiet-start semantics exactly.",
      "- No unwrap/expect/panic; every call resolves once with cancel and timeout paths; cfg-gating correct per file.",
      "- Inline tests per file cover the pure logic without touching FFI.",
      "- Only those three files changed.",
    ].join("\n"),
  },
  {
    key: "ts",
    title: "TypeScript package",
    blurb: "packages/nsh-deep-links: TS mirror of the Rust API",
    implSpecialist: "z-nsh-engineer",
    auditSpecialist: "z-nsh-engineer",
    waitFor: ["core"],
    goal: "Build packages/nsh-deep-links: the TypeScript package that mirrors the Rust crate one-to-one — same API names, same behavior, same error codes — laid out exactly like .refs/biometrics/packages/nsh-biometrics.",
    owns: "packages/nsh-deep-links/** EXCEPT the tests/ subdirectory (the test-suite area owns tests/): package.json, nsh.json, tsconfig.json, App.tsx, src/{index.ts,errors.ts,links.ts,routes.ts,association.ts,backend.ts,memory.ts,rust.ts} and any additional src file the mirror needs.",
    steps: [
      "Read first, completely: .refs/biometrics/packages/nsh-biometrics (package.json exports shape, nsh.json test-app manifest with rust pointing at the crate, tsconfig extending ./.nsh/tsconfig.json, App.tsx demo, src/{index,errors,auth,availability,options,backend,memory,rust}.ts) and the Rust core the other area just wrote: crates/nsh-deep-links/src/{lib.rs,error.rs,links.rs,routes.rs,association.rs,backend.rs,fake.rs,native.rs} — the mirror must match THAT code as landed, plus docs/goals.md when it exists.",
      "src files mirror the Rust names one-to-one: errors.ts (DeepLinksError class with .code holding the same ERR_DEEP_LINKS_* strings and an asBiometricsError-style coercion helper named asDeepLinksError), links.ts (DeepLinks with initial/next/stop/pending, Link/LinkKind), routes.ts (RouteTable/RouteMatch with from_config's validation semantics under a TS-idiomatic name — mirror the Rust name; TS may camelCase method names only where biometrics does), association.ts (the association builders/validators with the same outputs as Rust), backend.ts (the Backend interface mirroring the Rust trait, resolve-exactly-once in its contract), memory.ts (memoryBackend(): the TS mock mirroring FakeBackend's scripted deliveries/cancels/timeouts exactly — the TS tests will drive it), rust.ts (rustBackend(): binds 'nsh-deep-links/native' and requires the crate under nsh.json's rust key, exactly like biometrics' rust.ts including its failure semantics), index.ts (public surface).",
      "Manifests mirror biometrics verbatim in shape: package.json (name nsh-deep-links, type module, main/types ./src/index.ts, exports incl. './memory' and './rust', files ['src'], repository); nsh.json ('nsh-deep-links tests', id dev.supernsh.deep-links.tests, version 0.1.0, build 1, rust ../../crates/nsh-deep-links, capabilities {}); tsconfig.json extends ./.nsh/tsconfig.json including App.tsx, src/**/*.ts, tests/**/*.ts and tests/**/*.tsx. App.tsx is a small honest demo of the API in the biometrics App.tsx style. Write NO tests — tests/ belongs to the test-suite area.",
      "Semantics law: every behavior the Rust core implements lands here with identical observable semantics (codes, resolve-exactly-once, cancel paths, pending-link survival). If you find a Rust behavior that cannot be mirrored, record it in blockers instead of diverging silently.",
      "You may run exactly one command for self-checking: 'nsh check' scoped to packages/nsh-deep-links — it is a static typecheck. Never run nsh build, nsh test, cargo, or anything else that compiles or executes.",
    ].join("\n"),
    checklist: [
      "- Every public Rust name (types, functions, error codes) has a TS counterpart with the same observable semantics; codes are the identical ERR_DEEP_LINKS_* strings.",
      "- memoryBackend() mirrors FakeBackend's script surface (deliveries, cancels, timeouts, duplicates) that the planned tests need.",
      "- rust.ts mirrors biometrics' rust.ts contract incl. failing when nsh.json lacks the crate.",
      "- package.json, nsh.json and tsconfig.json match the biometrics shapes exactly (exports, test-app manifest, tsconfig extends).",
      "- No tests/ directory content created (owned by the test-suite area); no file outside packages/nsh-deep-links changed.",
      "- The single permitted self-check is nsh check in packages/nsh-deep-links; nothing compiled or executed beyond it.",
    ].join("\n"),
  },
  {
    key: "tests",
    title: "test suite",
    blurb: "tests/ in both languages + honest status docs",
    implSpecialist: "z-rust-engineer",
    auditSpecialist: "z-rust-engineer",
    waitFor: ["spec", "core", "apple", "android", "desktop-web", "ts"],
    goal: "Write the unit test suites for every goal, edge case and error path in BOTH languages, then make the repo's status docs tell the truth about what exists and what CI will run.",
    owns: "crates/nsh-deep-links/tests/**; packages/nsh-deep-links/tests/**; README.md; .zcode/workflow.ts; site/index.html. Nothing else.",
    steps: [
      "Read first: docs/goals.md (the contract — every edge case maps to a named test), docs/parity.md, the whole Rust crate crates/nsh-deep-links/src/**, the TS package packages/nsh-deep-links/src/**, and the biometrics test patterns: .refs/biometrics/packages/nsh-biometrics/tests/*.test.tsx (the 'nsh/test' harness: import { expect, test } from 'nsh/test', the delivered() microtask-drain idiom, fixtures/) and inline #[cfg(test)] tests in the crate.",
      "Rust suite under crates/nsh-deep-links/tests/: one file per goals.md section or one suite file — your call — with ONE test function per goals.md edge case, named EXACTLY as goals.md names it, driven through the public API + FakeBackend (scripted deliveries, cancels, timeouts, duplicates). Cover every error code path: at least one assertion per ERR_DEEP_LINKS_* code. Write tests so they would pass on CI's cargo test --all-targets; you never run them here.",
      "TS suite under packages/nsh-deep-links/tests/: the same edge cases, same test names, same semantics, using the 'nsh/test' harness and memoryBackend() exactly as biometrics' tests do; include any fixtures the biometrics pattern needs. Parity law: a behavior without a test in both languages is a finding.",
      "Status docs, honestly (AGENTS.md: a claim with no passing run is unverified): README.md — update Status (implemented, gates wired to CI, not yet run: no push has happened), Layout (the new crates/ + packages/ tree), Checks section unchanged in spirit; do NOT claim CI passes. .zcode/workflow.ts — rewrite the phase list to what actually ran: implement then up to 3 audit rounds per area, plus the static join gate; drop the stale device-test/vision/smoke phases. site/index.html — update the status page text to match reality, same honesty rule.",
      "You never run cargo test, nsh test, or any build; correctness comes from reading the code under test carefully. If you find a bug in an owned-by-others file while writing tests, do NOT fix it there — record it in blockers with file and line.",
    ].join("\n"),
    checklist: [
      "- Every goals.md edge case has a Rust test with the exact named function and a TS test with the same name and semantics; nothing skipped or ignored (no #[ignore], no .skip).",
      "- Every ERR_DEEP_LINKS_* code is asserted somewhere in the Rust suite and the TS suite.",
      "- Tests drive only public API + FakeBackend / memoryBackend; no test reaches into private internals; no test invokes FFI, JVM, DOM or network.",
      "- Tests are written to pass: assertions match the landed behavior of crate and package; any suspected bug in others' files is in blockers, not patched.",
      "- README.md, .zcode/workflow.ts and site/index.html state implemented-status honestly with zero claims of passing CI runs.",
      "- Only the owned files changed.",
    ].join("\n"),
  },
];

phase("Every area runs its own chain in parallel: implement, then up to three audit rounds (stop early on clean; report honestly if the cap hits)");
const results: AreaResult[] = await Promise.all(AREAS.map((a) => runArea(a)));

phase("Static whole-tree gate once at the join — formatting and typecheck only; compiling and testing stay in CI per AGENTS.md");

interface GateOutcome { name: string; ran: boolean; pass: boolean; output: string }

function exitOf(r: any): number | null {
  if (!r || typeof r !== "object") return null;
  if (typeof r.code === "number") return r.code;
  if (typeof r.exitCode === "number") return r.exitCode;
  if (typeof r.status === "number") return r.status;
  return null;
}
function outputOf(r: any): string {
  if (!r || typeof r !== "object") return String(r ?? "");
  return [r.stdout, r.stderr, r.output, r.error].filter((x) => typeof x === "string" && x.length).join("\n").slice(0, 8000);
}

async function runStaticGates(): Promise<GateOutcome[]> {
  const outcomes: GateOutcome[] = [];
  const fmt = await world.run("sh", ["-c", "cd '" + PROJECT + "' && cargo fmt --all -- --check"], { timeoutMs: 120000 });
  const fmtExit = exitOf(fmt);
  outcomes.push({ name: "cargo fmt --all -- --check", ran: true, pass: fmtExit === 0, output: fmtExit === 0 ? "" : outputOf(fmt) });
  const nshCheck = await world.run("sh", ["-c", "command -v nsh >/dev/null 2>&1 || exit 127; cd '" + PROJECT + "/packages/nsh-deep-links' && nsh check"], { timeoutMs: 240000 });
  const nshExit = exitOf(nshCheck);
  outcomes.push({
    name: "nsh check (packages/nsh-deep-links)",
    ran: nshExit !== 127,
    pass: nshExit === 0,
    output: nshExit === 0 ? "" : (nshExit === 127 ? "nsh toolchain not found on this machine" : outputOf(nshCheck)),
  });
  return outcomes;
}

let gateOutcomes: GateOutcome[] = [];
let gateFixRound = 0;
try {
  gateOutcomes = await runStaticGates();
  const failed = gateOutcomes.filter((g) => !g.pass);
  if (failed.length) {
    gateFixRound = 1;
    report({ name: "join gate", phase: "fixing", summary: failed.length + " static gate(s) failed; one mechanical fix round" }, "areas");
    const fixPersona = await specialist("z-rust-engineer", PLAIN_IMPL);
    const fixer = agent("Gate fixer · fmt+types", { system: fixPersona.persona, model: MODELS.pro });
    await ask(fixer, RULES + "\n\nThe whole tree failed its static gates. Fix EXACTLY these mechanical failures (formatting, typecheck) across the repo. Do not run any build, test or compiler yourself; do not commit; change nothing unrelated to the reported diagnostics.\n\n" + JSON.stringify(failed.map((f) => ({ gate: f.name, output: f.output })), null, 2) + "\n\nReply {done, summary, blockers?}.");
    gateOutcomes = await runStaticGates();
  }
} catch (e) {
  gateOutcomes = [{ name: "static gates", ran: false, pass: false, output: "gate execution error: " + (e && (e as Error).message ? (e as Error).message : String(e)) }];
}
for (const g of gateOutcomes) {
  report({ name: "join gate · " + g.name, phase: g.pass ? "done" : "failed", summary: g.pass ? "pass" : "FAIL" + (g.ran ? "" : " (did not run)") }, "areas");
}

const gateSummary = gateOutcomes.map((g) => g.name + ": " + (g.pass ? "pass" : g.ran ? "FAIL" : "did not run")).join("; ");
const notConverged = results.filter((r) => !r.converged);
const failedAreas = results.filter((r) => r.status !== "done");
const conclusion = [
  "Deep-links implementation over " + AREAS.length + " areas: " +
    results.filter((r) => r.status === "done").length + " done, " +
    failedAreas.length + (failedAreas.length ? " failed/blocked (" + failedAreas.map((r) => r.name).join(", ") + ")" : ""),
  "Audit convergence: " +
    (notConverged.length === 0
      ? "every area reached zero findings within " + AUDIT_ROUNDS + " rounds"
      : "NOT fully converged — " + notConverged.map((r) => r.name + " (" + r.rounds + " rounds, " + (r.outstanding ? r.outstanding.length : "?") + " open findings)").join("; ")),
  "Static whole-tree gate" + (gateFixRound ? " after 1 fix round" : "") + ": " + gateSummary,
  "Compiles and tests are NOT claimed: per AGENTS.md they run in GitHub Actions (wired into .github/workflows/ci.yml); nothing was built or tested locally and nothing was committed or pushed.",
].join(" | ");

return {
  conclusion,
  areas: results.map((r) => ({
    name: r.name,
    status: r.status,
    summary: r.summary,
    rounds: r.rounds,
    converged: r.converged,
    personaDelivery: r.personaDelivery,
    outstandingFindings: r.outstanding ?? [],
  })),
  notes: {
    modelPolicy: "Every agent pinned to GLM-5.3$high (user asked for GLM-5.3 max on every area); no vision phases, so flash was never used.",
    phasesParsed: "audit: explicit, 3 rounds per area, cap 3, early-exit on clean; unit tests: deliverable area (both languages), executed only by CI; native/device testing, vision, browser checks, smoke: not requested, absent.",
    gates: "cargo fmt/clippy/test + nsh check/build/test wired into .github/workflows/ci.yml; only the static subset (cargo fmt --check, nsh check) ran at the join with " + gateFixRound + " fix round(s).",
    references: ".refs/ contains all five verified clones (app_links, react-navigation, plugins-workspace, nsh, biometrics); every URL resolved, so docs/decisions.md records resolutions rather than substitutions. nsh-core comes from git because crates.io 0.4.0 lacks url_scheme (docs.rs, checked 2026-11-10).",
    repoState: "No commits, no pushes, per the brief; the deliverable is the changed working tree.",
  },
};