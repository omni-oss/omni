import { throwIfBunNotAvailable } from "@/platform-helpers";
import type { ArgsList, Env, Process } from ".";
import { ObjectEnv } from "./env";
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

    env(): Env {
        throwIfBunNotAvailable();

        return new ObjectEnv(Bun.env);
    }
}
