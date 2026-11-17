import { expect, test } from "nsh/test";
import { RouteTable } from "../src";
import { thrown } from "./helpers";

test("more_specific_patterns_match_before_wildcards", () => {
  const routes = RouteTable.fromConfig({
    prefixes: ["myapp://"],
    screens: {
      Anything: "*",
      Rest: "files/:rest*",
      Many: "files/:path+",
      Name: "files/:name",
      Numbered: "files/:id(\\d+)",
      New: "files/new",
    },
  });
  for (const [url, screen] of [
    ["myapp://files/new", "New"],
    ["myapp://files/42", "Numbered"],
    ["myapp://files/report", "Name"],
    ["myapp://files/a/b", "Many"],
    ["myapp://files", "Rest"],
    ["myapp://other/x", "Anything"],
    ["myapp://", "Anything"],
  ] as const) {
    expect(routes.matchUrl(url).screen).toBe(screen);
  }
  expect(routes.matchUrl("myapp://files/a/b").params.path).toBe("a/b");
});

test("a_param_failing_its_parse_schema_rejects_as_invalid_parameter", () => {
  const routes = RouteTable.fromConfig({
    prefixes: ["myapp://"],
    screens: {
      Order: { path: "orders/:id", parse: { id: "integer", gift: "boolean" } },
      Price: { path: "price/:amount", parse: { amount: "number" } },
      Anything: "*",
    },
  });
  for (const url of [
    "myapp://orders/abc",
    "myapp://orders/4.5",
    "myapp://orders/42?gift=yes",
    "myapp://price/1e400",
    "myapp://price/0x10",
  ]) {
    expect(thrown(() => routes.matchUrl(url))).toBe("ERR_DEEP_LINKS_INVALID_PARAMETER");
  }
  expect(routes.matchUrl("myapp://orders/42?gift=true").params).toEqual({ id: 42, gift: true });
  expect(routes.matchUrl("myapp://price/-2.5").params.amount).toBe(-2.5);
});

test("an_invalid_pattern_in_the_route_table_is_rejected_as_invalid_argument", () => {
  for (const pattern of [
    "orders/a:b",
    "orders/:id(",
    "orders/(x)",
    "orders/:id)",
    "orders/a?",
    "orders/:id?+",
    "orders/:id+?",
    "orders/:",
    "orders/:id/:id",
    "orders/:id([)",
  ]) {
    let detail = "";
    try {
      RouteTable.fromConfig({ prefixes: ["myapp://"], screens: { Order: pattern } });
    } catch (error) {
      expect(error).toMatchObject({ code: "ERR_DEEP_LINKS_INVALID_ARGUMENT" });
      detail = (error as { detail: string }).detail;
    }
    expect(detail.includes(pattern)).toBe(true);
  }
  for (const config of [
    { prefixes: ["myapp://"], screens: { Order: { path: "o", params: {} } } },
    { prefixes: ["myapp://"], path: "app/:tenant", screens: {} },
    { prefixes: ["myapp://"], screens: { Order: { alias: ["o"] } } },
    { prefixes: ["myapp://"], linking: true },
  ]) {
    expect(thrown(() => RouteTable.fromConfig(config))).toBe("ERR_DEEP_LINKS_INVALID_ARGUMENT");
  }
});

test("conflicting_patterns_for_two_screens_reject_as_invalid_argument", () => {
  expect(
    thrown(() => RouteTable.fromConfig({ prefixes: ["myapp://"], screens: { One: "orders/:id", Two: "orders//:id/" } })),
  ).toBe("ERR_DEEP_LINKS_INVALID_ARGUMENT");
  expect(
    thrown(() =>
      RouteTable.fromConfig({
        prefixes: ["myapp://"],
        screens: { Shop: { path: "shop", screens: { Cart: "cart" } }, Cart2: "shop/cart" },
      }),
    ),
  ).toBe("ERR_DEEP_LINKS_INVALID_ARGUMENT");
  const alias = RouteTable.fromConfig({
    prefixes: ["myapp://"],
    screens: { Order: { path: "orders/:id", alias: ["orders/:id", "o/:id"] } },
  });
  expect(alias.matchUrl("myapp://o/1").screen).toBe("Order");
});

test("a_reserved_scheme_in_a_claim_is_rejected_as_disallowed", () => {
  for (const scheme of ["mailto", "file", "javascript", "tel", "data", "nsh", "Intent"]) {
    expect(thrown(() => RouteTable.fromConfig({ prefixes: [`${scheme}://`], screens: {} }))).toBe(
      "ERR_DEEP_LINKS_DISALLOWED_SCHEME",
    );
  }
  expect(thrown(() => RouteTable.fromConfig({ prefixes: ["1app://"], screens: {} }))).toBe(
    "ERR_DEEP_LINKS_INVALID_ARGUMENT",
  );
  const fine = RouteTable.fromConfig({ prefixes: ["MyApp://", "https://example.com", "myapp://deep"] });
  expect(fine.claimedSchemes()).toEqual(["myapp"]);
});
