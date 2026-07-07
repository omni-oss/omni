import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { type HarnessConfigInput, resolveConfig } from "./config";
import { buildModel, type OmniRenderOptions, renderOmni } from "./model";

// Must match the fixed options in the Rust parity test (tests/parity.rs).
const OPTIONS: OmniRenderOptions = {
    taskCommandTemplate: "node ./task.mjs {task_id}",
    projectCacheKeyFiles: ["package.json", "task.mjs", "src/**/*.js"],
};

const GOLDEN_DIR = join(
    dirname(fileURLToPath(import.meta.url)),
    "../../../crates/omni_workspace_gen/tests/golden",
);

/** Convert the wasm model's camelCase keys back to the core's snake_case, so
 * both languages can assert against a single golden. */
function snakeKeys(value: unknown): unknown {
    if (Array.isArray(value)) return value.map(snakeKeys);
    if (value && typeof value === "object") {
        const out: Record<string, unknown> = {};
        for (const [key, val] of Object.entries(value)) {
            const snake = key.replace(/[A-Z]/g, (m) => `_${m.toLowerCase()}`);
            out[snake] = snakeKeys(val);
        }
        return out;
    }
    return value;
}

const CASES: Array<[string, HarnessConfigInput]> = [
    [
        "isolated",
        {
            projects: 4,
            tasksPerProject: 2,
            seed: 1,
            dependency: { strategy: "isolated", edgeProbability: 0.35 },
        },
    ],
    [
        "chain",
        {
            projects: 5,
            tasksPerProject: 2,
            seed: 1,
            dependency: { strategy: "chain", edgeProbability: 0.35 },
        },
    ],
    [
        "fan-out",
        {
            projects: 4,
            tasksPerProject: 3,
            seed: 1,
            dependency: { strategy: "fan-out", edgeProbability: 0.35 },
        },
    ],
    [
        "layered",
        {
            projects: 8,
            tasksPerProject: 3,
            seed: 1,
            dependency: { strategy: "layered", edgeProbability: 0.35 },
        },
    ],
    [
        "random",
        {
            projects: 10,
            tasksPerProject: 2,
            seed: 1234,
            dependency: { strategy: "random", edgeProbability: 0.4 },
        },
    ],
];

describe("cross-language parity", () => {
    for (const [name, input] of CASES) {
        it(`matches the ${name} golden (model + rendered omni)`, () => {
            const golden = JSON.parse(
                readFileSync(join(GOLDEN_DIR, `${name}.json`), "utf8"),
            );
            const model = buildModel(resolveConfig(input));

            expect(snakeKeys(model)).toEqual(golden.model);

            const omni = Object.fromEntries(renderOmni(model, OPTIONS));
            expect(omni).toEqual(golden.omni);
        });
    }
});
