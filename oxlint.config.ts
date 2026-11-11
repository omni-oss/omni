import { createConfig } from "@omni-oss/oxlint-config/base";
import { ignorePatterns } from "@omni-oss/oxlint-config/ignores";
import { defineConfig } from "oxlint";

export default defineConfig({
    options: {
        reportUnusedDisableDirectives: "deny",
        typeAware: true,
        // TypeScript diagnostics come from `check:types` (tsc). Running them here
        // too reports false mixed `src`/`dist` errors once dependencies are built.
        typeCheck: false,
    },
    extends: [createConfig()],
    ignorePatterns,
});
