import { throwIfDenoNotAvailable } from "@/platform-helpers";
import type { ArgsList, Process, ProcessEnv } from ".";

export class DenoProcess implements Process {
    currentDir(): string {
        throwIfDenoNotAvailable();

        return Deno.cwd();
    }

    setCurrentDir(dir: string): Promise<void> {
        throwIfDenoNotAvailable();

        Deno.chdir(dir);
        return Promise.resolve();
    }

    args(): ArgsList {
        throwIfDenoNotAvailable();

        return Deno.args;
    }

    env(): ProcessEnv {
        throwIfDenoNotAvailable();

        return Deno.env.toObject();
    }
}
