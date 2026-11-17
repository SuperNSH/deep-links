import { expect, test } from "nsh/test";
import { DeepLinks, MAX_PENDING, MAX_URL_LEN, memoryBackend, unsupportedBackend } from "../src";
import { code, delivered, routes, setup, thrown } from "./helpers";

// The delivery and pipeline edge cases of docs/goals.md, named as the Rust suite names them. The
// deadline case runs on the app's manual clock in app.test.tsx.

test("a_cold_start_link_dispatches_exactly_once", async () => {
  const { backend, links } = setup();
  backend.setInitial("myapp://orders/42");
  const first = await links.initial();
  expect(first?.kind).toBe("custom");
  expect(first?.route).toEqual({ screen: "Order", params: { id: 42 } });
  expect(await links.initial()).toEqual(first);
  expect(backend.initialReads()).toBe(1);
  // `next` never hands the launch link out again: it waits for a new one.
  const wait = links.next();
  expect(backend.waiting()).toBe(true);
  expect(backend.deliveries()).toBe(0);
  await links.stop();
  expect(await code(wait)).toBe("ERR_DEEP_LINKS_CANCELLED");
});

test("initial_is_none_when_no_link_started_the_app", async () => {
  const { links } = setup();
  expect(await links.initial()).toBeNull();
  expect(await links.initial()).toBeNull();
});

test("a_link_while_running_dispatches_through_next", async () => {
  const { backend, links } = setup();
  const link = await delivered(backend, links, "myapp://chat/jane");
  expect(link.url).toBe("myapp://chat/jane");
  expect(link.route.screen).toBe("Chat");
  expect(backend.deliveries()).toBe(1);
});

test("stop_cancels_a_pending_next_exactly_once", async () => {
  const { backend, links } = setup();
  const wait = links.next();
  expect(await links.stop()).toBe(true);
  backend.deliver("myapp://chat/late");
  expect(await code(wait)).toBe("ERR_DEEP_LINKS_CANCELLED");
  expect(await code(links.next())).toBe("ERR_DEEP_LINKS_CANCELLED");
  expect(backend.queued()).toEqual(["myapp://chat/late"]);
  expect(await links.stop()).toBe(false);
});

test("a_duplicate_delivery_while_one_resolves_maps_to_busy", async () => {
  const { backend, links } = setup();
  const wait = links.next();
  backend.deliver("myapp://chat/jane");
  expect(thrown(() => backend.deliver("myapp://chat/jane"))).toBe("ERR_DEEP_LINKS_BUSY");
  expect((await wait).url).toBe("myapp://chat/jane");
  // The duplicate was not held for a later wait.
  void links.next();
  expect(backend.waiting()).toBe(true);
  expect(backend.queued()).toEqual([]);
});

test("a_second_next_while_one_is_outstanding_is_busy", async () => {
  const { backend, links } = setup();
  const first = links.next();
  expect(await code(links.next())).toBe("ERR_DEEP_LINKS_BUSY");
  backend.deliver("myapp://chat/jane");
  expect((await first).route.screen).toBe("Chat");
});

test("a_malformed_url_is_rejected_as_invalid_url", async () => {
  const { backend, links } = setup();
  for (const url of ["", "/relative", "orders/42", "myapp", "://orders"]) {
    expect(await code(delivered(backend, links, url))).toBe("ERR_DEEP_LINKS_INVALID_URL");
  }
});

test("a_url_longer_than_max_url_len_is_rejected_as_too_large", async () => {
  expect(MAX_URL_LEN).toBe(16 * 1024);
  const { backend, links } = setup("universal");
  const custom = `myapp://chat/${"a".repeat(MAX_URL_LEN)}`;
  expect(await code(delivered(backend, links, custom))).toBe("ERR_DEEP_LINKS_URL_TOO_LARGE");
  const web = `https://example.com/chat/${"a".repeat(MAX_URL_LEN)}`;
  expect(await code(delivered(backend, links, web))).toBe("ERR_DEEP_LINKS_URL_TOO_LARGE");
  const head = "myapp://chat/";
  const fits = `${head}${"a".repeat(MAX_URL_LEN - head.length)}`;
  expect((await delivered(backend, links, fits)).route.screen).toBe("Chat");
});

test("a_url_with_whitespace_or_control_characters_is_rejected", async () => {
  const { backend, links } = setup("universal");
  for (const url of [
    "myapp://chat/ja ne",
    "myapp://chat/jane\n",
    "myapp://chat/\tjane",
    "myapp://chat/jane\u007f",
    "myapp://chat/jane ",
    "https://example.com/chat/ja ne",
    " myapp://chat/jane",
  ]) {
    expect(await code(delivered(backend, links, url))).toBe("ERR_DEEP_LINKS_INVALID_URL");
  }
});

test("a_link_of_an_unclaimed_scheme_is_rejected_as_disallowed", async () => {
  const { backend, links } = setup();
  for (const url of ["other://chat/jane", "mailto:someone@example.com", "file:///etc/passwd", "javascript:alert(1)"]) {
    expect(await code(delivered(backend, links, url))).toBe("ERR_DEEP_LINKS_DISALLOWED_SCHEME");
  }
});

test("an_https_link_to_an_unlisted_host_is_rejected_as_host_not_allowed", async () => {
  const { backend, links } = setup("app");
  for (const url of [
    "https://evil.com/orders/42",
    "https://example.com.evil.com/orders/42",
    "https://sub.example.com/orders/42",
    "https://example.com:8443/orders/42",
    "http://example.com/orders/42",
  ]) {
    expect(await code(delivered(backend, links, url))).toBe("ERR_DEEP_LINKS_HOST_NOT_ALLOWED");
  }
});

test("scheme_and_host_match_case_insensitively", async () => {
  const { backend, links } = setup("universal");
  const custom = await delivered(backend, links, "MYAPP://chat/Jane");
  expect(custom.kind).toBe("custom");
  expect(custom.route.params.author).toBe("Jane");
  const web = await delivered(backend, links, "https://Example.COM/chat/JaNe");
  expect(web.kind).toBe("universal");
  expect(web.route.params.author).toBe("JaNe");
  expect(await code(delivered(backend, links, "myapp://CHAT/jane"))).toBe("ERR_DEEP_LINKS_NO_ROUTE");
});

test("a_verified_web_link_is_preferred_over_a_custom_scheme_link", async () => {
  for (const kind of ["universal", "app"] as const) {
    const { backend, links } = setup(kind);
    const link = await delivered(backend, links, "https://example.com/auth/callback?code=1");
    expect(link.kind).toBe(kind);
    expect(link.route.screen).toBe("Login");
    expect(link.url.startsWith("https://")).toBe(true);
    const custom = await delivered(backend, links, "myapp://auth/callback?code=1");
    expect(custom.kind).toBe("custom");
  }
});

test("a_verified_web_link_on_an_unsupported_platform_resolves_unsupported", async () => {
  const links = new DeepLinks(unsupportedBackend(), routes());
  expect(thrown(() => links.resolve("https://example.com/orders/42"))).toBe("ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM");
  expect(await code(links.next())).toBe("ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM");
  expect(await links.initial()).toBeNull();
  // A platform with no verified links of its own answers the same for a delivered one.
  const { backend, links: plain } = setup(null);
  expect(await code(delivered(backend, plain, "https://example.com/orders/42"))).toBe(
    "ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM",
  );
});

test("a_platform_failure_resolves_as_backend", async () => {
  const { backend, links } = setup();
  const wait = links.next();
  backend.failPlatform();
  expect(await code(wait)).toBe("ERR_DEEP_LINKS_BACKEND");
  const broken = memoryBackend();
  broken.failInitial("ERR_DEEP_LINKS_BACKEND");
  expect(await code(new DeepLinks(broken, routes()).initial())).toBe("ERR_DEEP_LINKS_BACKEND");
});

test("a_link_whose_path_matches_no_route_is_rejected_as_no_route", async () => {
  const { backend, links } = setup("universal");
  for (const url of ["myapp://invoices/42", "https://example.com/orders", "https://example.com/orders/42/lines"]) {
    expect(await code(delivered(backend, links, url))).toBe("ERR_DEEP_LINKS_NO_ROUTE");
  }
});

test("query_params_and_fragments_are_parsed_and_preserved", async () => {
  const { backend, links } = setup();
  const link = await delivered(backend, links, "myapp://orders/42?tab=lines&note=a%20b&flag#top");
  expect(link.url).toBe("myapp://orders/42?tab=lines&note=a%20b&flag#top");
  expect(link.route.params).toEqual({ id: 42, tab: "lines", note: "a b", flag: "" });
});

test("path_params_win_over_same_named_query_params", async () => {
  const { backend, links } = setup();
  const link = await delivered(backend, links, "myapp://chat/jane?author=mallory&x=1");
  expect(link.route.params).toEqual({ author: "jane", x: "1" });
});

test("a_trailing_slash_matches_the_same_route", async () => {
  const { backend, links } = setup();
  for (const url of ["myapp://orders/42", "myapp://orders/42/", "myapp://orders//42", "myapp:///orders/42//"]) {
    expect((await delivered(backend, links, url)).route).toEqual({ screen: "Order", params: { id: 42 } });
  }
});

test("an_empty_path_matches_the_root_route", async () => {
  const { backend, links } = setup("app");
  for (const url of ["https://example.com/", "https://example.com", "myapp://", "https://example.com/?ref=mail"]) {
    expect((await delivered(backend, links, url)).route.screen).toBe("Home");
  }
});

test("percent_encoded_segments_decode_before_matching", async () => {
  const { backend, links } = setup();
  expect((await delivered(backend, links, "myapp://chat/jane%20doe%2Fx")).route.params.author).toBe("jane doe/x");
  expect((await delivered(backend, links, "myapp://caf%c3%a9")).route.screen).toBe("Cafe");
  expect((await delivered(backend, links, "myapp://caf%C3%A9")).route.screen).toBe("Cafe");
  expect((await delivered(backend, links, "myapp://orders/%34%32")).route.params.id).toBe(42);
  for (const url of ["myapp://chat/%zz", "myapp://chat/%ff", "myapp://chat/%4"]) {
    expect(await code(delivered(backend, links, url))).toBe("ERR_DEEP_LINKS_NO_ROUTE");
  }
});

test("a_pending_link_survives_a_simulated_login", async () => {
  const { backend, links } = setup();
  const order = await delivered(backend, links, "myapp://orders/42");
  links.pending().hold(order);
  const login = await delivered(backend, links, "myapp://auth/callback?code=xyz");
  expect(login.route.screen).toBe("Login");
  expect(links.pending().held()).toEqual(order);
  expect(links.pending().claim()).toEqual(order);
});

test("claiming_a_pending_link_clears_the_slot", () => {
  const { links } = setup();
  const pending = links.pending();
  const first = links.resolve("myapp://orders/1");
  const second = links.resolve("myapp://orders/2");
  pending.hold(first);
  pending.hold(second);
  expect(pending.claim()).toEqual(second);
  expect(pending.claim()).toBeNull();
  pending.hold(second);
  pending.clear();
  expect(pending.held()).toBeNull();
  expect(pending.claim()).toBeNull();
});

test("links_waiting_past_max_pending_drop_the_oldest", async () => {
  expect(MAX_PENDING).toBe(16);
  const { backend, links } = setup();
  for (let i = 0; i <= MAX_PENDING; i += 1) backend.deliver(`myapp://orders/${i}`);
  for (let i = 1; i <= MAX_PENDING; i += 1) {
    expect((await links.next()).route.params.id).toBe(i);
  }
  void links.next();
  expect(backend.waiting()).toBe(true);
});

test("every_call_resolves_exactly_once", async () => {
  const { backend, links } = setup();
  backend.setInitial("myapp://orders/7");
  const launch = await links.initial();
  expect(await links.initial()).toEqual(launch);
  expect(backend.initialReads()).toBe(1);
  const link = await delivered(backend, links, "myapp://chat/jane");
  expect(link.route.screen).toBe("Chat");
  backend.failPlatform();
  expect(await code(links.next())).toBe("ERR_DEEP_LINKS_BACKEND");
  expect(backend.deliveries()).toBe(1);
  links.pending().hold(link);
  expect(links.pending().claim()).toEqual(link);
  expect(links.pending().claim()).toBeNull();
  // A settled wait ignores later answers: the stop that follows cancels only the new wait.
  let settled = 0;
  const wait = links.next().then(
    () => (settled += 1),
    () => (settled += 1),
  );
  expect(await links.stop()).toBe(true);
  backend.cancelWait();
  await wait;
  expect(settled).toBe(1);
  expect(await links.stop()).toBe(false);
  expect(await code(links.next())).toBe("ERR_DEEP_LINKS_CANCELLED");
});
