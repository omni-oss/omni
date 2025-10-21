import os from "node:os";
import { OptimizedProcess } from "./optimized";
import { declareProcTests } from "./test-declarations";

declareProcTests({
    name: "Node",
    args: process.argv,
    currentDir: process.cwd(),
    env: process.env,
    newCurrentDir: os.tmpdir(),
    proc: await OptimizedProcess.create(),
});
