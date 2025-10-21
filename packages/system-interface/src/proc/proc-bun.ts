import type { Process } from ".";
import { NodeProcess } from "./proc-node";

export class BunProcess implements Process {
    private fallbackProc: NodeProcess = new NodeProcess();

    currentDir(): string {
        return this.fallbackProc.currentDir();
    }

    setCurrentDir(dir: string): void {
        this.fallbackProc.setCurrentDir(dir);
    }
}
