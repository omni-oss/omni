import fsPromises from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { test as vitestTest } from "vitest";

export const test = vitestTest.extend<{ tempDir: string; realDir: string }>({
    // biome-ignore lint/correctness/noEmptyPattern: This is a Vitest extension
    async tempDir({}, run) {
        const uuid = crypto.randomUUID();
        const tempdir = path.join(os.tmpdir(), `vitest-test-${uuid}`);

        await run(tempdir);
    },
    async realDir({ tempDir }, run) {
        await fsPromises.mkdir(tempDir);
        try {
            await run(tempDir);
        } finally {
            await fsPromises.rm(tempDir, { recursive: true });
        }
    },
});

export const it = test;
