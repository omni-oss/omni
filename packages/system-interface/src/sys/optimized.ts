import type { FileSystem } from "@/fs";
import { OptimizedFileSystem } from "@/fs/optimized";
import type { Process } from "@/proc";
import { OptimizedProcess } from "@/proc/optimized";
import type { System } from "./interfaces";

export class OptimizedSystem implements System {
    private constructor(
        public fs: FileSystem,
        public proc: Process,
    ) {}

    static async create(): Promise<OptimizedSystem> {
        return new OptimizedSystem(
            await OptimizedFileSystem.create(),
            await OptimizedProcess.create(),
        );
    }
}
