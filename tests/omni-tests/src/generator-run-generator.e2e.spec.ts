/**
 * `run_generator` action – `use_input_defaults` propagation.
 *
 * Cements the behavior fixed in the commit that added `use_input_defaults` to
 * `HandlerContext`, `ExecuteActionsArgs`, and `RunConfig`, and changed
 * `run_generator.rs` from hardcoding `use_input_defaults: false` to forwarding
 * `ctx.use_input_defaults`.
 *
 * Before the fix, `--use-defaults` was honoured for the top-level generator but
 * silently suppressed for every generator invoked transitively via a
 * `run_generator` action. After the fix the flag propagates through the whole
 * call chain so sub-generators correctly use their declared defaults.
 *
 * Pinned to:
 *   crates/omni_generator/src/action_handlers/run_generator.rs
 *   crates/omni_generator/src/execute_actions.rs
 *   crates/omni_generator/src/run.rs
 */

import { describe, expect, it } from "vitest";
import { makeWorkspace, runOmni, type WorkspaceSpec } from "@/harness";

const GENERATOR_SOURCE = [{ source: "local", path: "generators/**" }];

/**
 * A workspace with a `parent` generator (no inputs) that invokes a `child`
 * generator via `run_generator`. The child has a `subject` string input with
 * default "world" and writes `Hello {{ inputs.subject }}!` to `greeting.txt`.
 */
function parentChildSpec(): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            generators: GENERATOR_SOURCE,
        },
        projects: {
            "generators/child/generator.omni.yaml": {
                name: "child",
                inputs: [
                    {
                        type: "string",
                        name: "subject",
                        message: "Who to greet?",
                        default: "world",
                    },
                ],
                actions: [
                    {
                        type: "add-content",
                        output_path: "greeting.txt",
                        content: "Hello {{ inputs.subject }}!",
                    },
                ],
            },
            "generators/parent/generator.omni.yaml": {
                name: "parent",
                actions: [{ type: "run-generator", generator: "child" }],
            },
        },
        files: { ".omni/sources/generator/.keep": "" },
    };
}

describe("+generator @cli (run-generator / use_input_defaults)", {
    tags: ["generator"],
}, () => {
    it("--use-defaults propagates into a sub-generator's inputs", async () => {
        // The parent has no inputs; the child has `subject` defaulting to
        // "world". With --use-defaults the child must pick up that default
        // instead of trying to prompt.
        const ws = makeWorkspace(parentChildSpec());

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "parent",
                "-o",
                "out",
                "--use-defaults",
                "--save-session=false",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        expect(ws.read("out/greeting.txt")).toBe("Hello world!");
    });

    it("explicit input_values on run_generator override the sub-generator's default", async () => {
        // Even when --use-defaults is active, an explicit value supplied via
        // `input_values.values` in the run_generator action takes precedence.
        const ws = makeWorkspace({
            workspace: {
                projects: ["**"],
                generators: GENERATOR_SOURCE,
            },
            projects: {
                "generators/child/generator.omni.yaml": {
                    name: "child",
                    inputs: [
                        {
                            type: "string",
                            name: "subject",
                            message: "Who to greet?",
                            default: "world",
                        },
                    ],
                    actions: [
                        {
                            type: "add-content",
                            output_path: "greeting.txt",
                            content: "Hello {{ inputs.subject }}!",
                        },
                    ],
                },
                "generators/parent/generator.omni.yaml": {
                    name: "parent",
                    actions: [
                        {
                            type: "run-generator",
                            generator: "child",
                            input_values: { values: { subject: "Custom" } },
                        },
                    ],
                },
            },
            files: { ".omni/sources/generator/.keep": "" },
        });

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "parent",
                "-o",
                "out",
                "--use-defaults",
                "--save-session=false",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        // "Custom" must win over the "world" default.
        expect(ws.read("out/greeting.txt")).toBe("Hello Custom!");
    });

    it("--use-defaults propagates through multiple layers of run_generator", async () => {
        // root → middle → leaf: the `leaf` generator has two inputs with
        // defaults. With --use-defaults those defaults must reach the leaf even
        // though it is two run_generator hops away.
        const ws = makeWorkspace({
            workspace: {
                projects: ["**"],
                generators: GENERATOR_SOURCE,
            },
            projects: {
                "generators/leaf/generator.omni.yaml": {
                    name: "leaf",
                    inputs: [
                        {
                            type: "string",
                            name: "greeting",
                            message: "Greeting?",
                            default: "Hello",
                        },
                        {
                            type: "string",
                            name: "subject",
                            message: "Subject?",
                            default: "earth",
                        },
                    ],
                    actions: [
                        {
                            type: "add-content",
                            output_path: "result.txt",
                            content:
                                "{{ inputs.greeting }} {{ inputs.subject }}!",
                        },
                    ],
                },
                "generators/middle/generator.omni.yaml": {
                    name: "middle",
                    actions: [{ type: "run-generator", generator: "leaf" }],
                },
                "generators/root/generator.omni.yaml": {
                    name: "root",
                    actions: [{ type: "run-generator", generator: "middle" }],
                },
            },
            files: { ".omni/sources/generator/.keep": "" },
        });

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "root",
                "-o",
                "out",
                "--use-defaults",
                "--save-session=false",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        // Both of leaf's defaults must be honoured through the 3-level chain.
        expect(ws.read("out/result.txt")).toBe("Hello earth!");
    });

    it("parent and sub-generator both use defaults when --use-defaults is passed", async () => {
        // Verifies that --use-defaults is applied to the top-level generator's
        // own inputs AND propagated to a nested sub-generator's inputs.
        const ws = makeWorkspace({
            workspace: {
                projects: ["**"],
                generators: GENERATOR_SOURCE,
            },
            projects: {
                "generators/child/generator.omni.yaml": {
                    name: "child",
                    inputs: [
                        {
                            type: "string",
                            name: "subject",
                            message: "Who to greet?",
                            default: "world",
                        },
                    ],
                    actions: [
                        {
                            type: "add-content",
                            output_path: "child.txt",
                            content: "Hello {{ inputs.subject }}!",
                        },
                    ],
                },
                "generators/parent/generator.omni.yaml": {
                    name: "parent",
                    inputs: [
                        {
                            type: "string",
                            name: "prefix",
                            message: "Prefix?",
                            default: "Greetings",
                        },
                    ],
                    actions: [
                        {
                            type: "add-content",
                            output_path: "parent.txt",
                            content: "{{ inputs.prefix }}",
                        },
                        { type: "run-generator", generator: "child" },
                    ],
                },
            },
            files: { ".omni/sources/generator/.keep": "" },
        });

        const result = await runOmni(
            [
                "generator",
                "run",
                "-n",
                "parent",
                "-o",
                "out",
                "--use-defaults",
                "--save-session=false",
            ],
            { cwd: ws.cwd },
        );

        expect(result).toHaveSucceeded();
        // Parent uses its own default for `prefix`…
        expect(ws.read("out/parent.txt")).toBe("Greetings");
        // …and use_input_defaults is propagated so the child uses its default too.
        expect(ws.read("out/child.txt")).toBe("Hello world!");
    });
});
