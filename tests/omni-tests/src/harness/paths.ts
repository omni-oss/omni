/**
 * Locate the omni repository root from anywhere inside the test package.
 *
 * Walks up the directory tree looking for the workspace `Cargo.toml` (the file
 * that declares `[workspace]`). The result is memoized for the lifetime of the
 * test process.
 */

import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";

let cachedRepoRoot: string | undefined;

/**
 * Return the absolute path to the omni repo root.
 *
 * @param startDir Directory to start searching from (defaults to this file's
 *   directory). Mainly useful for tests of the harness itself.
 * @throws if no workspace `Cargo.toml` is found above `startDir`.
 */
export function findRepoRoot(startDir: string = import.meta.dirname): string {
    if (cachedRepoRoot && startDir === import.meta.dirname) {
        return cachedRepoRoot;
    }

    let dir = startDir;
    // eslint-disable-next-line no-constant-condition
    while (true) {
        const cargoToml = join(dir, "Cargo.toml");
        if (
            existsSync(cargoToml) &&
            readFileSync(cargoToml, "utf8").includes("[workspace]")
        ) {
            if (startDir === import.meta.dirname) {
                cachedRepoRoot = dir;
            }
            return dir;
        }

        const parent = dirname(dir);
        if (parent === dir) {
            throw new Error(
                `Could not locate the omni repo root: no workspace Cargo.toml ` +
                    `found at or above ${startDir}.`,
            );
        }
        dir = parent;
    }
}
