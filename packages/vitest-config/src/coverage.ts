import type { InlineConfig } from "vitest/node";

/**
 * Shared coverage defaults (V8 provider). Inert unless a run is started with
 * `--coverage`, so merging this into a config never changes normal test runs.
 *
 * Reports land in a per-package `coverage/` directory as a terminal summary,
 * an lcov file (for CI/Codecov), and a browseable HTML report.
 */
export const coverage: NonNullable<InlineConfig["coverage"]> = {
    provider: "v8",
    reporter: ["text", "lcov", "html"],
    reportsDirectory: "./coverage",
    include: ["src/**"],
    exclude: [
        "src/**/__tests__/**",
        "src/**/*.{test,spec}.*",
        "src/**/*.d.ts",
        // Type-only barrels and entrypoints carry no executable logic.
        "src/index.ts",
    ],
};

export default coverage;
