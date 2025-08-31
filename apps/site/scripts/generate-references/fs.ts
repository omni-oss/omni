import fs from "node:fs/promises";
import nodePath from "node:path";

// File system types using discriminated union
export type VirtualFile = {
    type: "file";
    content: string;
};

export type VirtualDirectory = {
    type: "directory";
    children: Record<string, VirtualNode>;
};

export type VirtualNode = VirtualFile | VirtualDirectory;

export type FileTree = Record<string, VirtualNode>;

export interface FileSystem {
    writeFile(path: string, content: string): Promise<void>;
    readFile(path: string): Promise<string | undefined>;
    listFiles(): Promise<string[]>;
    getFileTree(): Promise<FileTree>;
    clear(): Promise<void>;
}

// Virtual file system implementation
export class VirtualFileSystem implements FileSystem {
    private files: Map<string, string> = new Map();

    async writeFile(path: string, content: string) {
        this.files.set(path, content);
    }

    async readFile(path: string) {
        return this.files.get(path);
    }

    async listFiles() {
        return Array.from(this.files.keys()).sort();
    }

    async getFileTree() {
        const tree: FileTree = {};

        for (const [path, content] of this.files.entries()) {
            const parts = path.split("/");
            let current: Record<string, VirtualNode> = tree;

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
        this.files.clear();
    }

    async writeFilesToDisk(basePath: string) {
        await writeFilesToDisk(this, basePath);
    }
}

// Function to write files to disk from a VirtualFileSystem
async function writeFilesToDisk(vfs: VirtualFileSystem, basePath: string) {
    for (const path of await vfs.listFiles()) {
        const content = await vfs.readFile(path);
        if (!content) continue;

        const filePath = nodePath.join(basePath, path);
        console.log(`Writing file: ${filePath}`);
        await fs.mkdir(nodePath.dirname(filePath), { recursive: true });
        await fs.writeFile(filePath, content);
    }
}
