import { defineConfig } from "oxlint";

const baseConfig = createConfig();

export default baseConfig;

export function createConfig() {
    return defineConfig({
        plugins: ["jsdoc", "promise", "vitest"],
        rules: {
            // Vitest accepts an optional assertion failure message.
            "vitest/valid-expect": ["warn", { maxArgs: 2 }],
            // Our runtime-matrix helper declares real tests; setup hooks may assert prerequisites.
            "vitest/no-standalone-expect": [
                "warn",
                {
                    additionalTestBlockFunctions: [
                        "itPerRuntime",
                        "beforeAll",
                        "beforeEach",
                    ],
                },
            ],
        },
    });
}
