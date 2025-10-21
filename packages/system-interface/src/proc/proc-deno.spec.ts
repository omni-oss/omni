import os from "node:os";
import { DenoProcess } from "./proc-deno";
import { declareProcTests } from "./proc-test-declarations";

declareProcTests({
    name: "Deno",
    args: process.argv,
    currentDir: process.cwd(),
    env: process.env,
    newCurrentDir: os.tmpdir(),
    proc: new DenoProcess(),
    skip: typeof Deno === "undefined",
});
