import fs from "node:fs";
import os from "node:os";
import { OptimizedProcess } from "./optimized";
import { declareProcTests } from "./test-declarations";

declareProcTests({
    name: "Node",
    args: process.argv,
    currentDir: fs.realpathSync(process.cwd()),
    env: process.env,
    newCurrentDir: fs.realpathSync(os.tmpdir()),
    proc: await OptimizedProcess.create(),
});
