import path from "node:path";
import { describe, expect } from "vitest";
import { it } from "@/test-helpers";
import type { FileSystem } from "./interfaces";

export type FileSystemTestDeclarationsArgs = {
    name: string;
    skip?: boolean;
    fs: FileSystem;
    useRealDir?: boolean;
};

export function declareFsTests(args: FileSystemTestDeclarationsArgs): void {
    const useRealDir = args.useRealDir ?? true;

    async function withFixture(
        dir: string,
        test: () => Promise<void>,
    ): Promise<void> {
        if (!(await args.fs.pathExists(dir))) {
            await args.fs.createDirectory(dir, {
                recursive: true,
            });
        }
        try {
            await test();
        } finally {
            if (await args.fs.pathExists(dir)) {
                await args.fs.remove(dir, {
                    recursive: true,
                });
            }
        }
    }

    describe.skipIf(args.skip ?? false)(`FileSystem ${args.name}`, () => {
        async function expectAbleToWriteFile(dir: string) {
            const p = path.join(dir, "test.txt");
            await args.fs.writeStringToFile(p, "test");
            const contents = await args.fs.readFileAsString(p);
            expect(contents).toBe("test");
        }

        if (useRealDir) {
            it("should be able to write file", async ({ realDir }) => {
                await expectAbleToWriteFile(realDir);
            });
        } else {
            it("should be able to write file", async ({ tempDir }) => {
                await withFixture(tempDir, () =>
                    expectAbleToWriteFile(tempDir),
                );
            });
        }

        async function expectAbleToReadFile(dir: string) {
            const p = path.join(dir, "test.txt");
            await args.fs.writeStringToFile(p, "test");
            const contents = await args.fs.readFileAsString(p);
            expect(contents).toBe("test");
        }

        if (useRealDir) {
            it("should be able to read a file", async ({ realDir }) => {
                await expectAbleToReadFile(realDir);
            });
        } else {
            it("should be able to read a file", async ({ tempDir }) => {
                await withFixture(tempDir, () => expectAbleToReadFile(tempDir));
            });
        }

        async function expectAbleToAppendToFile(dir: string) {
            const p = path.join(dir, "test.txt");
            await args.fs.writeStringToFile(p, "test");
            await args.fs.appendStringToFile(p, "test");
            const contents = await args.fs.readFileAsString(p);
            expect(contents).toBe("testtest");
        }

        if (useRealDir) {
            it("should be able to append to a file", async ({ realDir }) => {
                await expectAbleToAppendToFile(realDir);
            });
        } else {
            it("should be able to append to a file", async ({ tempDir }) => {
                await withFixture(tempDir, () =>
                    expectAbleToAppendToFile(tempDir),
                );
            });
        }

        async function expectAbleToRemoveAFile(dir: string) {
            const p = path.join(dir, "test.txt");
            await args.fs.writeStringToFile(p, "test");
            await args.fs.remove(p);
            expect(await args.fs.pathExists(p)).toBe(false);
        }

        if (useRealDir) {
            it("should be able to remove a file", async ({ realDir }) => {
                await expectAbleToRemoveAFile(realDir);
            });
        } else {
            it("should be able to remove a file", async ({ tempDir }) => {
                await withFixture(tempDir, () =>
                    expectAbleToRemoveAFile(tempDir),
                );
            });
        }

        async function expectAbleToCreateDirectory(dir: string) {
            const p = path.join(dir, "test");
            await args.fs.createDirectory(p);
            expect(await args.fs.pathExists(p)).toBe(true);
        }

        if (useRealDir) {
            it("should be able to create a directory", async ({ realDir }) => {
                await expectAbleToCreateDirectory(realDir);
            });
        } else {
            it("should be able to create a directory", async ({ tempDir }) => {
                await withFixture(tempDir, () =>
                    expectAbleToCreateDirectory(tempDir),
                );
            });
        }

        async function expectAbleToRemoveADirectory(dir: string) {
            const p = path.join(dir, "test");
            await args.fs.createDirectory(p);
            await args.fs.remove(p, {
                recursive: true,
            });
            expect(await args.fs.pathExists(p)).toBe(false);
        }

        if (useRealDir) {
            it("should be able to remove a directory", async ({ realDir }) => {
                await expectAbleToRemoveADirectory(realDir);
            });
        } else {
            it("should be able to remove a directory", async ({ tempDir }) => {
                await withFixture(tempDir, () =>
                    expectAbleToRemoveADirectory(tempDir),
                );
            });
        }

        async function expectAbleToReadDirectory(dir: string) {
            const p = path.join(dir, "test");
            await args.fs.createDirectory(p);
            const contents = await args.fs.readDirectory(p);
            expect(contents).toEqual([]);
        }

        if (useRealDir) {
            it("should be able to read a directory", async ({ realDir }) => {
                await expectAbleToReadDirectory(realDir);
            });
        } else {
            it("should be able to read a directory", async ({ tempDir }) => {
                await withFixture(tempDir, () =>
                    expectAbleToReadDirectory(tempDir),
                );
            });
        }

        async function expectAbleToReadDirectoryWithFiles(dir: string) {
            const p = path.join(dir, "test");
            await args.fs.createDirectory(p);
            await args.fs.writeStringToFile(path.join(p, "test.txt"), "test");
            const contents = await args.fs.readDirectory(p);
            expect(contents).toEqual(["test.txt"]);
        }

        if (useRealDir) {
            it("should be able to read a directory with files", async ({
                realDir,
            }) => {
                await expectAbleToReadDirectoryWithFiles(realDir);
            });
        } else {
            it("should be able to read a directory with files", async ({
                tempDir,
            }) => {
                await withFixture(tempDir, () =>
                    expectAbleToReadDirectoryWithFiles(tempDir),
                );
            });
        }

        async function expectAbleToReadDirectoryWithDirectories(dir: string) {
            const p = path.join(dir, "test");
            await args.fs.createDirectory(p);
            await args.fs.createDirectory(path.join(p, "test"));
            const contents = await args.fs.readDirectory(p);
            expect(contents).toEqual(["test"]);
        }

        if (useRealDir) {
            it("should be able to read a directory with directories", async ({
                realDir,
            }) => {
                await expectAbleToReadDirectoryWithDirectories(realDir);
            });
        } else {
            it("should be able to read a directory with directories", async ({
                tempDir,
            }) => {
                await withFixture(tempDir, () =>
                    expectAbleToReadDirectoryWithDirectories(tempDir),
                );
            });
        }

        async function expectAbleToCopyFile(dir: string) {
            const p = path.join(dir, "test.txt");
            await args.fs.writeStringToFile(p, "test");
            await args.fs.copy(p, path.join(dir, "test2.txt"));
            const contents = await args.fs.readFileAsString(
                path.join(dir, "test2.txt"),
            );
            expect(contents).toBe("test");
        }

        if (useRealDir) {
            it("should be able to copy a file", async ({ realDir }) => {
                await expectAbleToCopyFile(realDir);
            });
        } else {
            it("should be able to copy a file", async ({ tempDir }) => {
                await withFixture(tempDir, () => expectAbleToCopyFile(tempDir));
            });
        }

        async function expectAbleToRenameFile(dir: string) {
            const p = path.join(dir, "test.txt");
            await args.fs.writeStringToFile(p, "test");
            await args.fs.rename(p, path.join(dir, "test2.txt"));
            const contents = await args.fs.readFileAsString(
                path.join(dir, "test2.txt"),
            );
            expect(contents).toBe("test");
        }

        if (useRealDir) {
            it("should be able to rename a file", async ({ realDir }) => {
                await expectAbleToRenameFile(realDir);
            });
        } else {
            it("should be able to rename a file", async ({ tempDir }) => {
                await withFixture(tempDir, () =>
                    expectAbleToRenameFile(tempDir),
                );
            });
        }

        async function expectAbleToStatFile(dir: string) {
            const p = path.join(dir, "test.txt");
            await args.fs.writeStringToFile(p, "test");
            const stats = await args.fs.stat(p);
            expect(stats.isFile()).toBe(true);
        }

        if (useRealDir) {
            it("should be able to stat a file", async ({ realDir }) => {
                await expectAbleToStatFile(realDir);
            });
        } else {
            it("should be able to stat a file", async ({ tempDir }) => {
                await withFixture(tempDir, () => expectAbleToStatFile(tempDir));
            });
        }

        async function expectAbleToStatDirectory(dir: string) {
            const p = path.join(dir, "test");
            await args.fs.createDirectory(p);
            const stats = await args.fs.stat(p);
            expect(stats.isDirectory()).toBe(true);
        }

        if (useRealDir) {
            it("should be able to stat a directory", async ({ realDir }) => {
                await expectAbleToStatDirectory(realDir);
            });
        } else {
            it("should be able to stat a directory", async ({ tempDir }) => {
                await withFixture(tempDir, () =>
                    expectAbleToStatDirectory(tempDir),
                );
            });
        }
    });
}
