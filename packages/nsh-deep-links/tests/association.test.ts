import { expect, test } from "nsh/test";
import {
  appleAppSiteAssociation,
  appleAppSiteAssociationFiles,
  assetlinksFiles,
  assetlinksJson,
  validateAppleAppSiteAssociation,
  validateAssetlinks,
} from "../src";
import { thrown } from "./helpers";

const PACKAGE = "com.supernsh.shop";
const APP_ID = "1A2B3C4D5E.com.supernsh.shop";
const FINGERPRINT =
  "aa:bb:cc:dd:ee:ff:00:11:22:33:44:55:66:77:88:99:aa:bb:cc:dd:ee:ff:00:11:22:33:44:55:66:77:88:99";

// Byte for byte the JSON the Rust suite asserts (crates/nsh-deep-links/tests/association.rs).
const ASSETLINKS = `[
  {
    "relation": [
      "delegate_permission/common.handle_all_urls"
    ],
    "target": {
      "namespace": "android_app",
      "package_name": "com.supernsh.shop",
      "sha256_cert_fingerprints": [
        "AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99"
      ]
    }
  }
]`;

const APPLE = `{
  "applinks": {
    "details": [
      {
        "appIDs": [
          "1A2B3C4D5E.com.supernsh.shop"
        ],
        "components": [
          {
            "/": "/orders/*"
          }
        ]
      }
    ]
  }
}`;

test("an_apple_association_for_the_wrong_team_is_invalid", () => {
  const file = appleAppSiteAssociation([APP_ID], ["/orders/*"]);
  expect(file).toBe(APPLE);
  validateAppleAppSiteAssociation(file, APP_ID);
  for (const expected of ["9Z8Y7X6W5V.com.supernsh.shop", "1A2B3C4D5E.com.supernsh.other"]) {
    expect(thrown(() => validateAppleAppSiteAssociation(file, expected))).toBe("ERR_DEEP_LINKS_ASSOCIATION_INVALID");
  }
  const legacy = `{"applinks": {"apps": [], "details": [{"appID": "9Z8Y7X6W5V.com.supernsh.shop", "paths": ["*"]}]}}`;
  expect(thrown(() => validateAppleAppSiteAssociation(legacy, APP_ID))).toBe("ERR_DEEP_LINKS_ASSOCIATION_INVALID");
  expect(thrown(() => validateAppleAppSiteAssociation("{}", APP_ID))).toBe("ERR_DEEP_LINKS_ASSOCIATION_INVALID");
  expect(thrown(() => validateAppleAppSiteAssociation(file, "shop"))).toBe("ERR_DEEP_LINKS_INVALID_ARGUMENT");
  expect(thrown(() => appleAppSiteAssociation([""]))).toBe("ERR_DEEP_LINKS_INVALID_ARGUMENT");
});

test("an_assetlinks_statement_for_the_wrong_package_is_invalid", () => {
  const file = assetlinksJson(PACKAGE, [FINGERPRINT]);
  expect(file).toBe(ASSETLINKS);
  validateAssetlinks(file, PACKAGE);
  expect(thrown(() => validateAssetlinks(file, "com.supernsh.other"))).toBe("ERR_DEEP_LINKS_ASSOCIATION_INVALID");
  const misplaced = `[{"relation": ["delegate_permission/common.handle_all_urls"], "package_name": "com.supernsh.shop", "target": {"namespace": "android_app", "package_name": "com.supernsh.other", "sha256_cert_fingerprints": ["AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99"]}}]`;
  expect(thrown(() => validateAssetlinks(misplaced, PACKAGE))).toBe("ERR_DEEP_LINKS_ASSOCIATION_INVALID");
  expect(thrown(() => assetlinksJson("", [FINGERPRINT]))).toBe("ERR_DEEP_LINKS_INVALID_ARGUMENT");
});

test("assetlinks_statements_are_built_per_subdomain", () => {
  const files = assetlinksFiles(["example.com", "www.example.com", "Shop.Example.com"], PACKAGE, [FINGERPRINT]);
  expect(files.map((file) => file.url)).toEqual([
    "https://example.com/.well-known/assetlinks.json",
    "https://www.example.com/.well-known/assetlinks.json",
    "https://shop.example.com/.well-known/assetlinks.json",
  ]);
  expect(files.every((file) => file.body === ASSETLINKS)).toBe(true);
  expect(appleAppSiteAssociationFiles(["example.com"], [APP_ID], ["/orders/*"])).toEqual([
    { url: "https://example.com/.well-known/apple-app-site-association", body: APPLE },
  ]);
  for (const hosts of [["*.example.com"], ["example.com", "EXAMPLE.COM"], []]) {
    expect(thrown(() => assetlinksFiles(hosts, PACKAGE, [FINGERPRINT]))).toBe("ERR_DEEP_LINKS_INVALID_ARGUMENT");
  }
});

test("association_validation_is_offline_and_never_fetches", () => {
  // Any fetch fails the test: the validators take bytes and must never reach for the network.
  const realFetch = globalThis.fetch;
  let fetched = 0;
  globalThis.fetch = (() => {
    fetched += 1;
    throw new Error("association validation fetched");
  }) as typeof fetch;
  try {
    const files = assetlinksFiles(["shop.invalid"], PACKAGE, [FINGERPRINT]);
    validateAssetlinks(files[0]?.body ?? "", PACKAGE);
    for (const cached of ["", "<html>Not Found</html>", '{"error": "timeout"}']) {
      expect(thrown(() => validateAssetlinks(cached, PACKAGE))).toBe("ERR_DEEP_LINKS_ASSOCIATION_INVALID");
      expect(thrown(() => validateAppleAppSiteAssociation(cached, APP_ID))).toBe("ERR_DEEP_LINKS_ASSOCIATION_INVALID");
    }
    const apple = appleAppSiteAssociationFiles(["shop.invalid"], [APP_ID]);
    validateAppleAppSiteAssociation(apple[0]?.body ?? "", APP_ID);
  } finally {
    globalThis.fetch = realFetch;
  }
  expect(fetched).toBe(0);
});
