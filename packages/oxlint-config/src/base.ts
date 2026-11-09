import { defineConfig } from "oxlint";

const baseConfig = createConfig();

export default baseConfig;

export function createConfig() {
    return defineConfig({
        plugins: ["jsdoc", "promise", "vitest"],
        rules: {
            // Logging APIs use tagged template calls for their side effects.
            "no-unused-expressions": ["warn", { allowTaggedTemplates: true }],
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
