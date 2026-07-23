import { throwIfDenoNotAvailable } from "@/platform-helpers";
import type { ArgsList, Env, Process } from ".";
import { ObjectEnv } from "./env";

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

    env(): Env {
        throwIfDenoNotAvailable();

        return new ObjectEnv(Deno.env.toObject());
    }
}
