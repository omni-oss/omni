/**
 * A per-worker config directory for spawned `omni` processes, so tests never
 * share the developer's (or each other's) fallback keystore.
 *
 * Only `XDG_CONFIG_HOME` is redirected; `HOME` is left alone on purpose. The
 * file keystore finds its directory through `XDG_CONFIG_HOME` on Linux, which
 * is where it is used. Redirecting `HOME` breaks tools that keep their state
 * under it: rustup's `rustfmt`/`cargo` shims try to install a toolchain, and
 * the macOS Keychain is looked up under the home directory, so keyring access
 * failed there. On macOS and Windows the keystore location does not come from
 * environment variables, so this isolates nothing there; macOS uses the
 * Keychain and Windows the known-folder API.
 */

import { mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

let root: string | undefined;

function isolatedRoot(): string {
    if (root === undefined) {
        const dir = mkdtempSync(join(tmpdir(), "omni-e2e-config-"));
        process.once("exit", () => {
            try {
                rmSync(dir, { recursive: true, force: true });
            } catch {
                // Best-effort cleanup of a temp directory.
            }
        });
        root = dir;
    }
    return root;
}

/**
 * Environment that points the config directory at this worker's private
 * directory. Callers' own `env` values take precedence, so tests that
 * deliberately share a config directory keep working.
 */
export function isolatedHomeEnv(): Record<string, string> {
    const config = join(isolatedRoot(), ".config");
    mkdirSync(config, { recursive: true });
    return { XDG_CONFIG_HOME: config };
}
