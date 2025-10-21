import { throwIfBunNotAvailable } from "@/platform-helpers";
import type { Process, ProcessEnv } from ".";
import { NodeProcess } from "./node";

export class BunProcess implements Process {
    private fallbackProc: NodeProcess = new NodeProcess();

    currentDir(): string {
        return this.fallbackProc.currentDir();
    }

    setCurrentDir(dir: string): void {
        this.fallbackProc.setCurrentDir(dir);
    }

    args(): string[] {
        throwIfBunNotAvailable();

        return Bun.argv;
    }

    env(): ProcessEnv {
        throwIfBunNotAvailable();

        return Bun.env;
    }
}
