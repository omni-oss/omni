import os from "node:os";
import { BunProcess } from "./proc-bun";
import { declareProcTests } from "./proc-test-declarations";

declareProcTests({
    name: "Bun",
    args: process.argv,
    currentDir: process.cwd(),
    env: process.env,
    newCurrentDir: os.tmpdir(),
    proc: new BunProcess(),
    skip: typeof Bun === "undefined",
});
