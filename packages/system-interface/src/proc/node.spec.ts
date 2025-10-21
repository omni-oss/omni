import os from "node:os";
import { NodeProcess } from "./node";
import { declareProcTests } from "./test-declarations";

declareProcTests({
    name: "Node",
    args: process.argv,
    currentDir: process.cwd(),
    env: process.env,
    newCurrentDir: os.tmpdir(),
    proc: new NodeProcess(),
});
