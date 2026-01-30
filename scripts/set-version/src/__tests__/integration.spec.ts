import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { describe, expect, test } from "vitest";
import { BUILT_IN_PROFILES, findConfig, setVersion } from "..";

describe("setVersion", () => {
    test("should set version at the given directory", async () => {
        const tempdir = await mktemp();
        const packageJsonPath = path.join(tempdir, "package.json");
        await fs.writeFile(
            packageJsonPath,
            JSON.stringify(
                {
                    name: "test",
                    version: "0.0.0",
                },
                null,
                4,
            ),
            {
                encoding: "utf-8",
            },
        );

        const version = "1.0.0";
        try {
            await setVersion(tempdir, version, BUILT_IN_PROFILES);
            const packageJsonContent = await fs.readFile(packageJsonPath, {
                encoding: "utf-8",
            });
            const packageJson = JSON.parse(packageJsonContent);
            expect(packageJson.version).toEqual(version);
        } finally {
            await fs.rm(tempdir, { recursive: true });
        }
    });
});

describe("findConfig", () => {
    test("should find config at the given directory", async () => {
        const tempdir = await mktemp();
        const packageJsonPath = path.join(tempdir, "set-version.json");
        const data = {
            profiles: BUILT_IN_PROFILES,
        };
        await fs.writeFile(packageJsonPath, JSON.stringify(data, null, 4), {
            encoding: "utf-8",
        });

        try {
            const config = await findConfig(tempdir, true);
            expect(config).toEqual(data);
        } finally {
            await fs.rm(tempdir, { recursive: true });
        }
    });
});

async function mktemp() {
    const tempdir = await fs.mkdtemp(path.join(os.tmpdir(), "vitest-test-"));
    await fs.mkdir(tempdir, { recursive: true });

    return tempdir;
}
