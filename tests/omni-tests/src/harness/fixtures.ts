/**
 * Reusable workspace fixtures for common e2e scenarios.
 *
 * These mirror the shapes in `crates/omni_cli_core/test_fixtures` so tests can
 * spin up a representative workspace without re-describing it each time. Each
 * factory returns a {@link WorkspaceSpec} you can pass to {@link makeWorkspace}
 * (and tweak/spread as needed).
 */

import type { WorkspaceSpec } from "./workspace";

/**
 * A minimal single-project workspace with `build` and `test` echo tasks.
 */
export function singleProjectSpec(): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            app: {
                name: "app",
                tasks: {
                    build: 'echo "build app"',
                    test: 'echo "test app"',
                },
            },
        },
    };
}

/**
 * Two projects where `project-1#run` depends on `project-2#list` and a local
 * `build` task - a portable analogue of the `project-1`/`project-2` fixtures
 * used to exercise dependency ordering.
 */
export function dependencyChainSpec(): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            "project-1": {
                name: "project-1",
                tasks: {
                    run: {
                        exec: 'echo "run project-1"',
                        dependencies: ["project-2#list", "build"],
                    },
                    build: 'echo "build project-1"',
                },
                dependencies: ["project-2"],
            },
            "project-2": {
                name: "project-2",
                tasks: {
                    list: 'echo "list project-2"',
                    build: 'echo "build project-2"',
                },
            },
        },
    };
}

/**
 * A base (template) project and a child that `extends` it - useful for config
 * merge / `extends` / `base: true` behaviors.
 */
export function extendsSpec(): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            "base.omni.yaml": {
                name: "base",
                base: true,
                tasks: { "from-base": 'echo "from base"' },
            },
            child: {
                name: "child",
                extends: ["../base.omni.yaml"],
                tasks: { own: 'echo "own task"' },
            },
        },
    };
}

/**
 * One project per supported config extension (yaml / yml / json / toml) - pins
 * that omni discovers and loads `project.omni.{ext}` regardless of format.
 * Each project name matches its extension so tests can assert which loaded.
 */
export function multiFormatProjectsSpec(): WorkspaceSpec {
    return {
        workspace: { projects: ["**"] },
        projects: {
            "yaml-app/project.omni.yaml": {
                name: "yaml-app",
                tasks: { greet: 'echo "hello from yaml"' },
            },
            "yml-app/project.omni.yml": {
                name: "yml-app",
                tasks: { greet: 'echo "hello from yml"' },
            },
            "json-app/project.omni.json": {
                name: "json-app",
                tasks: { greet: 'echo "hello from json"' },
            },
            "toml-app/project.omni.toml": {
                name: "toml-app",
                tasks: { greet: 'echo "hello from toml"' },
            },
        },
    };
}

/**
 * A workspace exposing a local generator source plus a single generator that
 * asks one `text` prompt and writes the answer into a file via an inline
 * `add-content` action. Driving this with the PTY harness exercises the real
 * `omni_prompt`/requestty interactive path end to end (see the `+prompt @tui`
 * backlog): run it, answer the prompt, then assert the generated file baked the
 * answer in.
 *
 * The generator is named `greeter`; its prompt is `subject` ("Who to greet?"),
 * and it writes `greeting.txt` containing `Hello <subject>!`.
 */
export function promptGeneratorSpec(): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            generators: [{ source: "local", path: "generators/**" }],
        },
        projects: {
            "generators/greeter/generator.omni.yaml": {
                name: "greeter",
                description: "POC text-prompt generator",
                prompts: [
                    { type: "text", name: "subject", message: "Who to greet?" },
                ],
                actions: [
                    {
                        type: "add-content",
                        output_path: "greeting.txt",
                        content: "Hello {{ prompts.subject }}!",
                    },
                ],
            },
        },
        // `omni generator run` writes a generator-sources lockfile to
        // `.omni/sources/generator/lock.json` and does not create the dir, so
        // seed it (a real workspace would already have `.omni`).
        files: { ".omni/sources/generator/.keep": "" },
    };
}

/**
 * A workspace with a local generator source and a `scaffold` generator suited
 * to non-interactive `omni generator run` tests. The generator:
 *   - asks one `text` prompt `subject` (default `"world"`, `remember: true` so
 *     it is persisted to the session),
 *   - declares a `dest` target rooted at `@output/src` (so `-t dest=<path>` can
 *     redirect output and prove target overrides), and
 *   - writes `<target dest>/greeting.txt` containing `Hello <subject>!`.
 *
 * A sibling `app` project is included so `-p/--project` can target a project
 * directory. The `.omni/sources/generator` dir is seeded because `generator run`
 * writes its sources lockfile there without creating the directory.
 */
export function scaffoldGeneratorSpec(): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            generators: [{ source: "local", path: "generators/**" }],
        },
        projects: {
            app: { name: "app", tasks: { build: 'echo "build app"' } },
            "generators/scaffold/generator.omni.yaml": {
                name: "scaffold",
                description: "scaffolds a greeting file",
                prompts: [
                    {
                        type: "text",
                        name: "subject",
                        message: "Who to greet?",
                        default: "world",
                        remember: true,
                    },
                ],
                targets: { dest: "@output/src" },
                actions: [
                    {
                        type: "add-content",
                        output_path: "greeting.txt",
                        target: "dest",
                        content: "Hello {{ prompts.subject }}!",
                    },
                ],
            },
        },
        files: { ".omni/sources/generator/.keep": "" },
    };
}
