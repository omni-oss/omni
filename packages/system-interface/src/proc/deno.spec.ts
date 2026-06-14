import fs from "node:fs";
import os from "node:os";
import { DenoProcess } from "./deno";
import { declareProcTests } from "./test-declarations";

declareProcTests({
    name: "Deno",
    args: process.argv,
    currentDir: fs.realpathSync(process.cwd()),
    env: process.env,
    newCurrentDir: fs.realpathSync(os.tmpdir()),
    proc: new DenoProcess(),
    skip: typeof Deno === "undefined",
});
