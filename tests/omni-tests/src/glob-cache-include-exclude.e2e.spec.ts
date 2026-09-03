/**
 * Structured include/exclude globs on a cache key field, end to end.
 *
 * `cache.key.files` accepts the include/exclude object form. A file the
 * include glob walks but an exclude glob matches is dropped from the hashed
 * input set, so editing it must not move the project hash, while editing an
 * included file must. This pins the collector's `include && !exclude` matching
 * to the observable cache key.
 */

import { describe, expect, it } from "vitest";
import { makeWorkspace, runOmni, type WorkspaceSpec } from "@/harness";

const RAW_HASH_PATTERN = /^[A-Za-z0-9]{40,}$/;

function cacheGlobSpec(files: unknown): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            app: {
                name: "app",
                cache: { key: { files } },
                tasks: { build: 'echo "build"' },
            },
        },
        files: {
            "app/src/a.txt": "a\n",
            "app/src/gen/b.txt": "b\n",
        },
    };
}

async function projectHash(cwd: string): Promise<string> {
    const result = await runOmni(
        ["hash", "-r", "project", "app", "-t", "build"],
        { cwd },
    );
    expect(result).toHaveSucceeded();
    expect(result.stdout).toMatch(RAW_HASH_PATTERN);
    return result.stdout;
}

describe("+hashing @e2e (cache include/exclude globs)", {
    tags: ["hashing"],
}, () => {
    it("an excluded file never changes the hash; an included file does", async () => {
        const ws = makeWorkspace(
            cacheGlobSpec({ include: ["src/**"], exclude: ["src/gen/**"] }),
        );

        const base = await projectHash(ws.cwd);

        // Editing an excluded file leaves the hashed input set untouched.
        ws.write("app/src/gen/b.txt", "b changed\n");
        expect(await projectHash(ws.cwd)).toBe(base);

        // Editing an included file moves the hash.
        ws.write("app/src/a.txt", "a changed\n");
        expect(await projectHash(ws.cwd)).not.toBe(base);
    });
});
