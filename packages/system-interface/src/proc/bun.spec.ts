import os from "node:os";
import { BunProcess } from "./bun";
import { declareProcTests } from "./test-declarations";

declareProcTests({
    name: "Bun",
    args: process.argv,
    currentDir: process.cwd(),
    env: process.env,
    newCurrentDir: os.tmpdir(),
    proc: new BunProcess(),
    skip: typeof Bun === "undefined",
});
