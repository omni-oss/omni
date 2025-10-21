import { throwIfDenoNotAvailable } from "@/platform-helpers";
import type { Process } from ".";

export class DenoProcess implements Process {
    currentDir(): string {
        throwIfDenoNotAvailable();

        return Deno.cwd();
    }

    setCurrentDir(dir: string): void {
        throwIfDenoNotAvailable();

        Deno.chdir(dir);
    }
}
