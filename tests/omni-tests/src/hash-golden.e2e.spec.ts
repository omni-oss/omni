/**
 * Golden workspace-hash snapshot.
 *
 * `omni hash -r workspace` folds every task's resolved cache input/output files
 * into one content hash. This test pins that hash for a fixed multi-project
 * fixture so any accidental change to how the collector builds or matches its
 * globs is caught. The fixture uses project-rooted and workspace-rooted
 * includes, nested `**`, output globs, and files that must not match, and it
 * contains no `!` entry, so it stays on the include-only path.
 *
 * The hash depends on the OS (path handling), so the snapshot is stored per
 * platform (see `resolveSnapshotPath` in vitest.config.e2e.ts). Commit the
 * value on each platform CI runs on; a later change to the hash then fails the
 * test. A platform with no committed snapshot is not pinned until one is added.
 */

import { existsSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { makeWorkspace, runOmni } from "@/harness";

describe("+hash @hashing (golden workspace hash)", () => {
    it("hashes a fixed multi-project workspace to a committed value", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                "libs/alpha": {
                    name: "alpha",
                    tasks: {
                        build: {
                            exec: "echo build",
                            cache: {
                                key: {
                                    files: [
                                        "src/**/*.rs",
                                        "@workspace/shared/**/*.toml",
                                    ],
                                },
                                output: { files: ["dist/**"] },
                            },
                        },
                    },
                },
                "apps/beta": {
                    name: "beta",
                    tasks: {
                        build: {
                            exec: "echo build",
                            cache: {
                                key: { files: ["src/**/*.ts"] },
                                output: { files: ["dist/**/*.js"] },
                            },
                        },
                    },
                },
            },
            files: {
                "shared/config.toml": "k = 1\n",
                "libs/alpha/src/lib.rs": "pub fn a() {}\n",
                "libs/alpha/src/inner/mod.rs": "pub fn b() {}\n",
                "libs/alpha/src/notes.txt": "skip\n",
                "apps/beta/src/index.ts": "export const x = 1;\n",
                "apps/beta/src/util/helper.ts": "export const y = 2;\n",
                "apps/beta/src/README.md": "skip\n",
            },
        });

        // The hash must be stable across runs before the snapshot means anything.
        const first = await runOmni(["hash", "-r", "workspace"], {
            cwd: ws.cwd,
        });
        const second = await runOmni(["hash", "-r", "workspace"], {
            cwd: ws.cwd,
        });

        expect(first).toHaveSucceeded();
        expect(first.stdout).toBe(second.stdout);

        // The snapshot is per-platform. On a platform whose snapshot is not
        // committed yet, a local run writes it (commit it to start pinning the
        // value), but CI never writes a missing snapshot, so skip the assertion
        // there to keep the runner green until the value is committed.
        const snapshotFile = join(
            dirname(fileURLToPath(import.meta.url)),
            "__snapshots__",
            `${basename(fileURLToPath(import.meta.url))}.${process.platform}.snap`,
        );
        const inCI = process.env.CI != null && process.env.CI !== "";

        if (existsSync(snapshotFile) || !inCI) {
            expect(first.stdout).toMatchSnapshot();
        } else {
            console.warn(
                `no committed workspace-hash snapshot for platform "${process.platform}"; ` +
                    "run the e2e suite locally on it with -u to pin the value",
            );
        }
    });
});
