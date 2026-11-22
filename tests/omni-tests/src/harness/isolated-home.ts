/**
 * A per-worker home and config directory for spawned `omni` processes, so
 * tests never share the developer's (or each other's) fallback keystore.
 */

import { existsSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";

let root: string | undefined;

function isolatedRoot(): string {
    if (root === undefined) {
        const dir = mkdtempSync(join(tmpdir(), "omni-e2e-home-"));
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
 * Toolchain homes that default to a location under the user's home. Tools
 * such as rustup's `rustfmt` and `cargo` shims find their installation there,
 * so redirecting the home alone makes them try to install a toolchain (and
 * fail without a network). Keep the real locations.
 */
function toolchainHomes(): Record<string, string> {
    const real = homedir();
    const homes: Record<string, string> = {};
    for (const [name, dir] of [
        ["RUSTUP_HOME", ".rustup"],
        ["CARGO_HOME", ".cargo"],
    ] as const) {
        const configured = process.env[name];
        const location = configured ?? join(real, dir);
        if (configured !== undefined || existsSync(location)) {
            homes[name] = location;
        }
    }
    return homes;
}

/**
 * Environment that points the user's home and config directories at this
 * worker's private directory. Callers' own `env` values take precedence, so
 * tests that deliberately share a config directory keep working.
 *
 * On Windows the `directories` crate reads the known-folder API rather than
 * these variables, so they are set for child tools but do not redirect omni's
 * keystore there.
 */
export function isolatedHomeEnv(): Record<string, string> {
    const home = isolatedRoot();
    const config = join(home, ".config");
    const appData = join(home, "AppData", "Roaming");
    const localAppData = join(home, "AppData", "Local");
    for (const dir of [config, appData, localAppData]) {
        mkdirSync(dir, { recursive: true });
    }
    return {
        ...toolchainHomes(),
        HOME: home,
        USERPROFILE: home,
        XDG_CONFIG_HOME: config,
        APPDATA: appData,
        LOCALAPPDATA: localAppData,
    };
}
