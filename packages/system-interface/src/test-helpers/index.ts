import fsPromises from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { test as vitestTest } from "vitest";

export const test = vitestTest.extend<{ tempDirPath: string; tempDir: string }>(
    {
        tempDirPath: [
            // Vitest reads fixture dependencies from the destructured parameter.
            // eslint-disable-next-line no-empty-pattern
            async ({}, run) => {
                const uuid = crypto.randomUUID();
                const tempdir = path.join(os.tmpdir(), `vitest-test-${uuid}`);
                await run(tempdir);
            },
            {
                scope: "test",
            },
        ],
        tempDir: [
            async ({ tempDirPath: tempDir }, run) => {
                await fsPromises.mkdir(tempDir);
                try {
                    await run(tempDir);
                } finally {
                    await removeTempDir(tempDir);
                }
            },
            {
                scope: "test",
            },
        ],
    },
);

export const it = test;

async function removeTempDir(tempDir: string): Promise<void> {
    const maxAttempts = 5;
    for (let attempts = 1; attempts <= maxAttempts; attempts += 1) {
        try {
            await fsPromises.rm(tempDir, { recursive: true, force: true });
            return;
        } catch (err) {
            if (attempts === maxAttempts) {
                console.warn(
                    `Failed to remove dir ${tempDir} after ${attempts} attempts:`,
                    err,
                );
                if (err instanceof Error && !err.message.includes("EBUSY")) {
                    throw err;
                }
                return;
            }
            await new Promise((resolve) => setTimeout(resolve, 100));
        }
    }
}
