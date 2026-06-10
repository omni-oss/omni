import { throwIfBunNotAvailable } from "@/platform-helpers";
import type { ArgsList, Process, ProcessEnv } from ".";
import { NodeProcess } from "./node";

export class BunProcess implements Process {
    private fallbackProc: NodeProcess = new NodeProcess();

    currentDir(): string {
        return this.fallbackProc.currentDir();
    }

    setCurrentDir(dir: string): Promise<void> {
        return this.fallbackProc.setCurrentDir(dir);
    }

    args(): ArgsList {
        throwIfBunNotAvailable();

        return Bun.argv;
    }

    env(): ProcessEnv {
        throwIfBunNotAvailable();

        return Bun.env;
    }
}
