import { execa } from "execa";

/** Install dependencies in a generated workspace with bun. */
export async function installWorkspace(
    dir: string,
    opts: { quiet?: boolean } = {},
): Promise<void> {
    await execa("bun", ["install"], {
        cwd: dir,
        stdio: opts.quiet ? "ignore" : "inherit",
    });
}
