/**
 * Interactive `omni_prompt` (requestty) widgets, driven through the PTY harness.
 *
 * These run real requestty prompts via `omni generator run`: a generator's
 * `prompts` render the widgets, and its `add-content` action bakes the answers
 * into a file we can read back - which proves the keystrokes were actually
 * received and parsed. Always `waitFor` the prompt message before sending keys.
 *
 * The generators here keep `remember` unset and declare no target overrides, so
 * their session is empty and no post-run "save session?" confirm appears; the
 * run completes as soon as the prompts are answered. Each run passes `-o out`
 * (and usually `-n <name>`) so only the widget under test is interactive.
 *
 * Pinned to `crates/omni_prompt/src/prompt.rs` and `.../make/prompts.rs`.
 */

import { describe, expect, it } from "vitest";
import {
    makeWorkspace,
    promptGeneratorSpec,
    spawnOmniPty,
    type WorkspaceSpec,
} from "@/harness";

type Json = Record<string, unknown>;

interface GeneratorDef {
    description?: string;
    prompts?: Json[];
    actions: Json[];
}

/**
 * Build a workspace exposing one local generator source plus the given
 * generators (keyed by name). Mirrors the shape of {@link promptGeneratorSpec}
 * but lets each test declare its own prompts/actions.
 */
function generatorWorkspace(generators: Record<string, GeneratorDef>) {
    const projects: Record<string, Json> = {};
    for (const [name, def] of Object.entries(generators)) {
        projects[`generators/${name}/generator.omni.yaml`] = {
            name,
            ...(def.description !== undefined
                ? { description: def.description }
                : {}),
            ...(def.prompts !== undefined ? { prompts: def.prompts } : {}),
            actions: def.actions,
        };
    }

    const spec: WorkspaceSpec = {
        workspace: {
            projects: ["**"],
            generators: [{ source: "local", path: "generators/**" }],
        },
        projects,
        // `generator run` writes its sources lockfile here without creating it.
        files: { ".omni/sources/generator/.keep": "" },
    };

    return makeWorkspace(spec);
}

/** A single generator named `g` whose action writes `content` to `result.txt`. */
function singleGenerator(prompts: Json[], content: string) {
    return generatorWorkspace({
        g: {
            prompts,
            actions: [
                {
                    type: "add-content",
                    output_path: "result.txt",
                    content,
                },
            ],
        },
    });
}

describe("+prompt @tui (text prompt)", () => {
    it("drives a requestty text prompt and bakes the answer into output", async () => {
        const ws = makeWorkspace(promptGeneratorSpec());

        const pty = spawnOmniPty(
            [
                "generator",
                "run",
                "-n",
                "greeter",
                "-o",
                "out",
                "--save-session",
            ],
            { cwd: ws.cwd },
        );

        // The prompt only reads input once requestty has rendered it.
        await pty.waitFor("Who to greet?");
        pty.type("omni");
        pty.press("enter");

        const exit = await pty.waitForExit();

        expect(exit.exitCode).toBe(0);
        expect(ws.read("out/greeting.txt")).toContain("Hello omni!");
    });
});

describe("+prompt @tui (select prompt)", () => {
    it("selecting a different generator from the list runs that one", async () => {
        const ws = generatorWorkspace({
            alpha: {
                actions: [
                    {
                        type: "add-content",
                        output_path: "alpha.txt",
                        content: "alpha",
                    },
                ],
            },
            beta: {
                actions: [
                    {
                        type: "add-content",
                        output_path: "beta.txt",
                        content: "beta",
                    },
                ],
            },
        });

        // No -n: omni renders the generator-selection `select` widget.
        const pty = spawnOmniPty(["generator", "run", "-o", "out"], {
            cwd: ws.cwd,
        });

        await pty.waitFor("Select generator");

        // Discovery order isn't guaranteed, so read the rendered list to learn
        // which generator sits below the (highlighted) first one.
        const screen = pty.screen();
        const order = ["alpha", "beta"]
            .map((name) => ({ name, at: screen.indexOf(name) }))
            .filter((entry) => entry.at >= 0)
            .sort((a, b) => a.at - b.at)
            .map((entry) => entry.name);
        expect(order).toHaveLength(2);
        const [first, second] = order;

        // Move down to the second option and select it.
        pty.press("down");
        pty.press("enter");

        const exit = await pty.waitForExit();

        expect(exit.exitCode).toBe(0);
        expect(ws.exists(`out/${second}.txt`)).toBe(true);
        expect(ws.exists(`out/${first}.txt`)).toBe(false);
    });
});

describe("+prompt @tui (multi_select prompt)", () => {
    it("space toggles options and enter submits the selected values", async () => {
        const ws = singleGenerator(
            [
                {
                    type: "multi-select",
                    name: "tags",
                    message: "Pick tags",
                    options: [
                        { name: "a", value: "a" },
                        { name: "b", value: "b" },
                        { name: "c", value: "c" },
                    ],
                },
            ],
            '{{ prompts.tags | join(sep="-") }}',
        );

        const pty = spawnOmniPty(["generator", "run", "-n", "g", "-o", "out"], {
            cwd: ws.cwd,
        });

        await pty.waitFor("Pick tags");
        pty.press("space"); // toggle the first option (a)
        pty.press("down");
        pty.press("space"); // toggle the second option (b)
        pty.press("enter");

        const exit = await pty.waitForExit();

        expect(exit.exitCode).toBe(0);
        // Values are emitted in option order, not selection order.
        expect(ws.read("out/result.txt")).toBe("a-b");
    });
});

describe("+prompt @tui (confirm prompt)", () => {
    function confirmWorkspace() {
        return singleGenerator(
            [
                {
                    type: "confirm",
                    name: "flag",
                    message: "Proceed?",
                    default: true,
                },
            ],
            "{{ prompts.flag }}",
        );
    }

    it("pressing n answers false", async () => {
        const ws = confirmWorkspace();

        const pty = spawnOmniPty(["generator", "run", "-n", "g", "-o", "out"], {
            cwd: ws.cwd,
        });

        await pty.waitFor("Proceed?");
        pty.type("n");
        pty.press("enter");

        const exit = await pty.waitForExit();

        expect(exit.exitCode).toBe(0);
        expect(ws.read("out/result.txt")).toBe("false");
    });

    it("pressing enter accepts the (true) default", async () => {
        const ws = confirmWorkspace();

        const pty = spawnOmniPty(["generator", "run", "-n", "g", "-o", "out"], {
            cwd: ws.cwd,
        });

        await pty.waitFor("Proceed?");
        pty.press("enter");

        const exit = await pty.waitForExit();

        expect(exit.exitCode).toBe(0);
        expect(ws.read("out/result.txt")).toBe("true");
    });
});

describe("+prompt @tui (validation)", () => {
    it("re-prompts on invalid input and proceeds once valid", async () => {
        const ws = singleGenerator(
            [
                {
                    type: "text",
                    name: "code",
                    message: "Enter code",
                    validate: [
                        {
                            condition: "{{ value == 'abc' }}",
                            error_message: "must be abc",
                        },
                    ],
                },
            ],
            "{{ prompts.code }}",
        );

        const pty = spawnOmniPty(["generator", "run", "-n", "g", "-o", "out"], {
            cwd: ws.cwd,
        });

        await pty.waitFor("Enter code");
        // Submit an invalid value first; the validator rejects and re-prompts.
        pty.type("ab");
        pty.press("enter");
        await pty.waitFor("must be abc");

        // Append to the existing input so it becomes the valid "abc".
        pty.type("c");
        pty.press("enter");

        const exit = await pty.waitForExit();

        expect(exit.exitCode).toBe(0);
        expect(ws.read("out/result.txt")).toBe("abc");
    });
});

describe("+prompt @tui (if-skip & password)", () => {
    it("an `if: false` prompt is skipped and never rendered", async () => {
        const ws = singleGenerator(
            [
                {
                    type: "text",
                    name: "hidden",
                    message: "SHOULD-NOT-APPEAR",
                    if: false,
                    default: "x",
                },
                { type: "text", name: "shown", message: "Type value" },
            ],
            "hidden={{ prompts.hidden | default(value='MISSING') }} shown={{ prompts.shown }}",
        );

        const pty = spawnOmniPty(["generator", "run", "-n", "g", "-o", "out"], {
            cwd: ws.cwd,
        });

        await pty.waitFor("Type value");
        // The skipped prompt's message must never have been drawn.
        expect(pty.text()).not.toContain("SHOULD-NOT-APPEAR");
        pty.type("here");
        pty.press("enter");

        const exit = await pty.waitForExit();

        expect(exit.exitCode).toBe(0);
        expect(ws.read("out/result.txt")).toBe("hidden=MISSING shown=here");
    });

    it("a password prompt captures input without echoing it", async () => {
        const secret = "topsecret-123";
        const ws = singleGenerator(
            [{ type: "password", name: "secret", message: "Enter secret" }],
            "secret={{ prompts.secret }}",
        );

        const pty = spawnOmniPty(["generator", "run", "-n", "g", "-o", "out"], {
            cwd: ws.cwd,
        });

        await pty.waitFor("Enter secret");
        pty.type(secret);
        pty.press("enter");

        const exit = await pty.waitForExit();

        expect(exit.exitCode).toBe(0);
        // The value was captured...
        expect(ws.read("out/result.txt")).toBe(`secret=${secret}`);
        // ...but the plaintext never appeared on screen.
        expect(pty.text()).not.toContain(secret);
    });
});
