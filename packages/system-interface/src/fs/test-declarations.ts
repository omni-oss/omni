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
    const createTempDir = args.useRealDir ?? true;

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
        async function expectAbleToWriteStringFile(dir: string) {
            const p = path.join(dir, "test.txt");
            await args.fs.writeStringToFile(p, "test");
            const contents = await args.fs.readFileAsString(p);
            expect(contents).toBe("test");
        }

        if (createTempDir) {
            it("should be able to write string to file", async ({
                tempDir,
            }) => {
                await expectAbleToWriteStringFile(tempDir);
            });
        } else {
            it("should be able to write string to file", async ({
                tempDirPath,
            }) => {
                await withFixture(tempDirPath, () =>
                    expectAbleToWriteStringFile(tempDirPath),
                );
            });
        }

        async function expectAbleToWriteBytesFile(dir: string) {
            const p = path.join(dir, "test.txt");
            await args.fs.writeBytesToFile(p, Buffer.from("test"));
            const contents = await args.fs.readFileAsString(p);
            expect(contents).toBe("test");
        }

        if (createTempDir) {
            it("should be able to write bytes to file", async ({ tempDir }) => {
                await expectAbleToWriteBytesFile(tempDir);
            });
        } else {
            it("should be able to write bytes to file", async ({
                tempDirPath,
            }) => {
                await withFixture(tempDirPath, () =>
                    expectAbleToWriteBytesFile(tempDirPath),
                );
            });
        }

        async function expectAbleToReadFileAsString(dir: string) {
            const p = path.join(dir, "test.txt");
            await args.fs.writeStringToFile(p, "test");
            const contents = await args.fs.readFileAsString(p);
            expect(contents).toBe("test");
        }

        if (createTempDir) {
            it("should be able to read a file as string", async ({
                tempDir,
            }) => {
                await expectAbleToReadFileAsString(tempDir);
            });
        } else {
            it("should be able to read a file as string", async ({
                tempDirPath,
            }) => {
                await withFixture(tempDirPath, () =>
                    expectAbleToReadFileAsString(tempDirPath),
                );
            });
        }

        async function expectAbleToReadFileAsBytes(dir: string) {
            const p = path.join(dir, "test.txt");
            await args.fs.writeStringToFile(p, "test");
            const contents = await args.fs.readFileAsBytes(p);
            expect(contents).toEqual(new Uint8Array(Buffer.from("test")));
        }

        if (createTempDir) {
            it("should be able to read a file as bytes", async ({
                tempDir,
            }) => {
                await expectAbleToReadFileAsBytes(tempDir);
            });
        } else {
            it("should be able to read a file as bytes", async ({
                tempDirPath,
            }) => {
                await withFixture(tempDirPath, () =>
                    expectAbleToReadFileAsBytes(tempDirPath),
                );
            });
        }

        async function expectAbleToAppendToFile(dir: string) {
            const p = path.join(dir, "test.txt");
            await args.fs.writeStringToFile(p, "test");
            await args.fs.appendStringToFile(p, "test");
            const contents = await args.fs.readFileAsString(p);
            expect(contents).toBe("testtest");
        }

        if (createTempDir) {
            it("should be able to append to a file", async ({ tempDir }) => {
                await expectAbleToAppendToFile(tempDir);
            });
        } else {
            it("should be able to append to a file", async ({
                tempDirPath,
            }) => {
                await withFixture(tempDirPath, () =>
                    expectAbleToAppendToFile(tempDirPath),
                );
            });
        }

        async function expectAbleToRemoveAFile(dir: string) {
            const p = path.join(dir, "test.txt");
            await args.fs.writeStringToFile(p, "test");
            await args.fs.remove(p);
            expect(await args.fs.pathExists(p)).toBe(false);
        }

        if (createTempDir) {
            it("should be able to remove a file", async ({ tempDir }) => {
                await expectAbleToRemoveAFile(tempDir);
            });
        } else {
            it("should be able to remove a file", async ({ tempDirPath }) => {
                await withFixture(tempDirPath, () =>
                    expectAbleToRemoveAFile(tempDirPath),
                );
            });
        }

        async function expectAbleToCreateDirectory(dir: string) {
            const p = path.join(dir, "test");
            await args.fs.createDirectory(p, { recursive: true });
            expect(await args.fs.pathExists(p)).toBe(true);
        }

        if (createTempDir) {
            it("should be able to create a directory", async ({ tempDir }) => {
                await expectAbleToCreateDirectory(tempDir);
            });
        } else {
            it("should be able to create a directory", async ({
                tempDirPath,
            }) => {
                await withFixture(tempDirPath, () =>
                    expectAbleToCreateDirectory(tempDirPath),
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

        if (createTempDir) {
            it("should be able to remove a directory", async ({ tempDir }) => {
                await expectAbleToRemoveADirectory(tempDir);
            });
        } else {
            it("should be able to remove a directory", async ({
                tempDirPath,
            }) => {
                await withFixture(tempDirPath, () =>
                    expectAbleToRemoveADirectory(tempDirPath),
                );
            });
        }

        async function expectAbleToReadDirectory(dir: string) {
            const p = path.join(dir, "test");
            await args.fs.createDirectory(p);
            const contents = await args.fs.readDirectory(p);
            expect(contents).toEqual([]);
        }

        if (createTempDir) {
            it("should be able to read a directory", async ({ tempDir }) => {
                await expectAbleToReadDirectory(tempDir);
            });
        } else {
            it("should be able to read a directory", async ({
                tempDirPath,
            }) => {
                await withFixture(tempDirPath, () =>
                    expectAbleToReadDirectory(tempDirPath),
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

        if (createTempDir) {
            it("should be able to read a directory with files", async ({
                tempDir,
            }) => {
                await expectAbleToReadDirectoryWithFiles(tempDir);
            });
        } else {
            it("should be able to read a directory with files", async ({
                tempDirPath,
            }) => {
                await withFixture(tempDirPath, () =>
                    expectAbleToReadDirectoryWithFiles(tempDirPath),
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

        if (createTempDir) {
            it("should be able to read a directory with directories", async ({
                tempDir,
            }) => {
                await expectAbleToReadDirectoryWithDirectories(tempDir);
            });
        } else {
            it("should be able to read a directory with directories", async ({
                tempDirPath,
            }) => {
                await withFixture(tempDirPath, () =>
                    expectAbleToReadDirectoryWithDirectories(tempDirPath),
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

        if (createTempDir) {
            it("should be able to copy a file", async ({ tempDir }) => {
                await expectAbleToCopyFile(tempDir);
            });
        } else {
            it("should be able to copy a file", async ({ tempDirPath }) => {
                await withFixture(tempDirPath, () =>
                    expectAbleToCopyFile(tempDirPath),
                );
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

        if (createTempDir) {
            it("should be able to rename a file", async ({ tempDir }) => {
                await expectAbleToRenameFile(tempDir);
            });
        } else {
            it("should be able to rename a file", async ({ tempDirPath }) => {
                await withFixture(tempDirPath, () =>
                    expectAbleToRenameFile(tempDirPath),
                );
            });
        }

        async function expectAbleToStatFile(dir: string) {
            const p = path.join(dir, "test.txt");
            await args.fs.writeStringToFile(p, "test");
            const stats = await args.fs.stat(p);
            expect(stats.isFile()).toBe(true);
        }

        if (createTempDir) {
            it("should be able to stat a file", async ({ tempDir }) => {
                await expectAbleToStatFile(tempDir);
            });
        } else {
            it("should be able to stat a file", async ({ tempDirPath }) => {
                await withFixture(tempDirPath, () =>
                    expectAbleToStatFile(tempDirPath),
                );
            });
        }

        async function expectAbleToStatDirectory(dir: string) {
            const p = path.join(dir, "test");
            await args.fs.createDirectory(p);
            const stats = await args.fs.stat(p);
            expect(stats.isDirectory()).toBe(true);
        }

        if (createTempDir) {
            it("should be able to stat a directory", async ({ tempDir }) => {
                await expectAbleToStatDirectory(tempDir);
            });
        } else {
            it("should be able to stat a directory", async ({
                tempDirPath,
            }) => {
                await withFixture(tempDirPath, () =>
                    expectAbleToStatDirectory(tempDirPath),
                );
            });
        }
    });
}
