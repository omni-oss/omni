import type { FileSystem } from "@/fs";
import type { Process } from "@/proc";

export interface System {
    fs: FileSystem;
    proc: Process;
}
