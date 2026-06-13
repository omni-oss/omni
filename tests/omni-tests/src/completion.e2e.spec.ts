/**
 * `omni completion [-s <shell>]` - emits a shell completion script via
 * `clap_complete`. Pinned to `crates/omni_cli_core/src/commands/completion.rs`.
 *
 * Caveat: the script's program name is `build::PROJECT_NAME`, which resolves to
 * the crate name `omni_cli_core` (not `omni`). Tests assert the actual emitted
 * name. Also note that completion generation builds the full clap command tree;
 * in debug builds clap's debug assertions currently abort on a duplicate `-d`
 * short option in `cache prune`, so these rely on the (preferred) release binary.
 */

import { describe, expect, it } from "vitest";
import { runOmni } from "@/harness";

const PROGRAM_NAME = "omni_cli_core";

// A distinctive marker proving each shell's script was generated.
const SHELL_MARKERS: Record<string, string | RegExp> = {
    bash: "complete -F",
    zsh: "#compdef",
    fish: "complete -c",
    powershell: "Register-ArgumentCompleter",
    elvish: "edit:completion",
};

describe("+completion @output (script generation)", () => {
    it("defaults to bash and emits a completion script", async () => {
        const result = await runOmni(["completion"]);

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining(SHELL_MARKERS.bash as string);
    });

    for (const [shell, marker] of Object.entries(SHELL_MARKERS)) {
        it(`-s/--shell ${shell} emits a valid script`, async () => {
            const result = await runOmni(["completion", "-s", shell]);

            expect(result).toHaveSucceeded();
            if (marker instanceof RegExp) {
                expect(result).toMatchOutput(marker);
            } else {
                expect(result).toOutputContaining(marker);
            }
        });
    }

    it("rejects an unknown shell with value-enum help", async () => {
        const result = await runOmni(["completion", "-s", "notashell"]);

        expect(result).toHaveExitCode(2);
        expect(result).toHaveStderrContaining("invalid value 'notashell'");
    });

    it("references the program name", async () => {
        const result = await runOmni(["completion", "-s", "bash"]);

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining(PROGRAM_NAME);
    });
});
