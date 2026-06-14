import fs from "node:fs";
import os from "node:os";
import { BunProcess } from "./bun";
import { declareProcTests } from "./test-declarations";

declareProcTests({
    name: "Bun",
    args: process.argv,
    currentDir: fs.realpathSync(process.cwd()),
    env: process.env,
    newCurrentDir: fs.realpathSync(os.tmpdir()),
    proc: new BunProcess(),
    skip: typeof Bun === "undefined",
});
