import { DeepLinks, DeepLinksError, type Link, type LinkKind, type MemoryDeepLinks, RouteTable, memoryBackend } from "../src";

/** The route table every delivery test uses; the Rust suite's fixture, field for field. */
export function routes(): RouteTable {
  return RouteTable.fromConfig({
    prefixes: ["https://example.com", "myapp://"],
    screens: {
      Home: "",
      Order: { path: "orders/:id", parse: { id: "integer" } },
      Chat: "chat/:author",
      Login: "auth/callback",
      Cafe: "caf%C3%A9",
    },
  });
}

export function setup(kind: LinkKind | null = null): { backend: MemoryDeepLinks; links: DeepLinks } {
  const backend = memoryBackend();
  backend.setWebLinkKind(kind);
  return { backend, links: new DeepLinks(backend, routes()) };
}

/** The link a delivered URL resolves with, through `next`. */
export function delivered(backend: MemoryDeepLinks, links: DeepLinks, url: string): Promise<Link> {
  const wait = links.next();
  backend.deliver(url);
  return wait;
}

/** The code a promise rejects with, or "no error". */
export async function code(answer: Promise<unknown>): Promise<string> {
  try {
    await answer;
    return "no error";
  } catch (error) {
    return error instanceof DeepLinksError ? error.code : `not coded: ${String(error)}`;
  }
}

/** The code a call throws, or "no error". */
export function thrown(call: () => unknown): string {
  try {
    call();
    return "no error";
  } catch (error) {
    return error instanceof DeepLinksError ? error.code : `not coded: ${String(error)}`;
  }
}
