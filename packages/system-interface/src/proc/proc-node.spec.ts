import os from "node:os";
import { NodeProcess } from "./proc-node";
import { declareProcTests } from "./proc-test-declarations";

declareProcTests({
    name: "Node",
    args: process.argv,
    currentDir: process.cwd(),
    env: process.env,
    newCurrentDir: os.tmpdir(),
    proc: new NodeProcess(),
});
