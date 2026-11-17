import { NativeDeepLinks } from "nsh-deep-links/native";
import type { DeepLinksBackend } from "./backend";
import { asDeepLinksError } from "./errors";
import type { LinkKind } from "./links";

const KINDS: LinkKind[] = ["universal", "app", "custom", "web"];

/**
 * The Rust crate's platform intake (`nsh-deep-links/native`): Universal Links on Apple, the launch
 * App Link on Android, custom schemes through NSH core. The app or package must list the crate
 * under `"rust"` in `nsh.json`; importing this module without it fails the bundle.
 */
export function rustBackend(): DeepLinksBackend {
  return new RustBackend();
}

class RustBackend implements DeepLinksBackend {
  private readonly native = new NativeDeepLinks();

  async initialUrl(): Promise<string | null> {
    try {
      return (await this.native.initialUrl()) ?? null;
    } catch (error) {
      throw asDeepLinksError(error);
    }
  }

  async nextUrl(): Promise<string> {
    try {
      return await this.native.nextUrl();
    } catch (error) {
      throw asDeepLinksError(error);
    }
  }

  cancelWait(): void {
    this.native.cancelWait();
  }

  webLinkKind(): LinkKind | null {
    const kind = this.native.webLinkKind();
    return KINDS.find((known) => known === kind) ?? null;
  }
}
