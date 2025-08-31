import fsSync from "node:fs";
import fs from "node:fs/promises";
import nodePath from "node:path";
import type { VirtualFileSystem } from "./fs";

export async function copyToVfsIfExists(
    vfs: VirtualFileSystem,
    path: string,
    basePath: string,
) {
    const filePath = nodePath.join(basePath, path);
    if (fsSync.existsSync(filePath)) {
        vfs.writeFile(path, await fs.readFile(filePath, "utf-8"));
    }
}
