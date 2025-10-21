import process from "node:process";
import type { Process } from ".";

export class NodeProcess implements Process {
    currentDir(): string {
        return process.cwd();
    }

    setCurrentDir(dir: string): void {
        process.chdir(dir);
    }
}
