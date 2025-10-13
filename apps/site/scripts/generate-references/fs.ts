import fs from "node:fs/promises";
import nodePath from "node:path";

// File system types using discriminated union
export type FsFile = {
    type: "file";
    content: string;
};

export type FsDirectory = {
    type: "directory";
    children: Record<string, FsNode>;
};

export type FsNode = FsFile | FsDirectory;

export type FileTree = Record<string, FsNode>;

export interface FileSystem {
    writeFile(path: string, content: string): Promise<void>;
    readFile(path: string): Promise<string | undefined>;
    listFiles(): Promise<string[]>;
    getFileTree(): Promise<FileTree>;
    clear(): Promise<void>;
}

// Virtual file system implementation
export class VirtualFileSystem implements FileSystem {
    private files: Record<string, string> = {};

    async writeFile(path: string, content: string) {
        this.files[path] = content;
    }

    async readFile(path: string) {
        return this.files[path];
    }

    async listFiles() {
        return Object.keys(this.files);
    }

    async getFileTree() {
        const tree: FileTree = {};

        for (const [path, content] of Object.entries(this.files)) {
            const parts = path.split("/");
            let current: Record<string, FsNode> = tree;

            for (let i = 0; i < parts.length; i++) {
                const part = parts[i];
                if (i === parts.length - 1) {
                    // File
                    current[part] = {
                        type: "file",
                        content: content,
                    };
                } else {
                    // Directory
                    if (!current[part]) {
                        current[part] = {
                            type: "directory",
                            children: {},
                        };
                    }
                    if (current[part].type === "directory") {
                        current = current[part].children;
                    }
                }
            }
        }

        return tree;
    }

    async clear() {
        for (const key of Object.keys(this.files)) {
            delete this.files[key];
        }
    }

    async writeFilesToDisk(basePath: string) {
        await writeFilesToDisk(this, basePath);
    }
}

// Function to write files to disk from a VirtualFileSystem
async function writeFilesToDisk(vfs: VirtualFileSystem, basePath: string) {
    const files = await vfs.listFiles();

    console.log(`Writing files ${files.length} to disk...`);

    for (const path of files) {
        const content = await vfs.readFile(path);
        if (!content) {
            console.error(`File not found: ${path}`);
        }

        const filePath = nodePath.join(basePath, path);
        console.log(`Writing file: ${filePath}`);
        await fs.mkdir(nodePath.dirname(filePath), { recursive: true });
        await fs.writeFile(filePath, content);
    }
}
