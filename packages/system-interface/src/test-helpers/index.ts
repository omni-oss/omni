import fsPromises from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { test as vitestTest } from "vitest";

export const test = vitestTest.extend<{ tempDirPath: string; tempDir: string }>(
    {
        // biome-ignore lint/correctness/noEmptyPattern: This is a Vitest extension
        tempDirPath: [
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
                    let attempts = 0;
                    let maxAttempts = 5;
                    while (true) {
                        try {
                            attempts += 1;
                            await fsPromises.rm(tempDir, {
                                recursive: true,
                                force: true,
                            });
                            break;
                        } catch (err) {
                            if (attempts >= maxAttempts) {
                                if (
                                    err instanceof Error &&
                                    !err.message.includes("EBUSY")
                                ) {
                                    console.warn(
                                        `Failed to remove dir ${tempDir} after ${attempts} attempts:`,
                                        err,
                                    );
                                    throw err;
                                }
                                break;
                            } else {
                                await new Promise((resolve) =>
                                    setTimeout(resolve, 100),
                                );
                            }
                        }
                    }
                }
            },
            {
                scope: "test",
            },
        ],
    },
);

export const it = test;
