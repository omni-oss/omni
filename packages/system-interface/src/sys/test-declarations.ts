import path from "node:path";
import { describe, expect } from "vitest";
import { it } from "@/test-helpers";
import type { System } from "./interfaces";

export type SystemTestDeclarationsArgs = {
    name: string;
    sys: System;
    skip?: boolean;
    isRealSystem?: boolean;
};

export function declareSysTests(args: SystemTestDeclarationsArgs): void {
    const isRealSystem = args.isRealSystem ?? true;

    async function withFixture(
        dir: string,
        test: () => Promise<void>,
    ): Promise<void> {
        if (!(await args.sys.fs.pathExists(dir))) {
            await args.sys.fs.createDirectory(dir, {
                recursive: true,
            });
        }
        try {
            await test();
        } finally {
            if (await args.sys.fs.pathExists(dir)) {
                await args.sys.fs.remove(dir, {
                    recursive: true,
                });
            }
        }
    }

    describe
        .skipIf(args.skip ?? false)
        .sequential(`System ${args.name}`, () => {
            async function expectAbleToCreateFileInCurrentDirectory(
                dir: string,
            ) {
                args.sys.proc.setCurrentDir(dir);
                await args.sys.fs.writeStringToFile(
                    path.join(dir, "test.txt"),
                    "test",
                );
                const contents = await args.sys.fs.readFileAsString(
                    path.join(dir, "test.txt"),
                );
                expect(contents).toBe("test");
            }

            if (isRealSystem) {
                it(`should create file in current directory`, async ({
                    realDir,
                }) => {
                    await expectAbleToCreateFileInCurrentDirectory(realDir);
                });
            } else {
                it(`should create file in current directory`, async ({
                    tempDir,
                }) => {
                    await withFixture(tempDir, () =>
                        expectAbleToCreateFileInCurrentDirectory(tempDir),
                    );
                });
            }
        });
}
