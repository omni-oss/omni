/// <reference types="node" />

import { fileURLToPath } from "node:url";

import { createConfig } from "@omni-oss/oxlint-config/base";
import { ignorePatterns } from "@omni-oss/oxlint-config/ignores";
import { createConfig as createJsxConfig } from "@omni-oss/oxlint-config/jsx";
import { createConfig as createSolidConfig } from "@omni-oss/oxlint-config/solid";
import { defineConfig } from "oxlint";

// The lint task runs from the workspace root; Panda otherwise searches that directory.
process.env.PANDA_CONFIG_PATH ??= fileURLToPath(
    new URL("./panda.config.ts", import.meta.url),
);

const solidConfig = createSolidConfig();

export default defineConfig({
    extends: [createConfig(), createJsxConfig(), solidConfig],
    // oxlint does not inherit `settings` from extended configs.
    settings: solidConfig.settings,
    jsPlugins: ["@pandacss/eslint-plugin/oxlint"],
    ignorePatterns,
});
