import { createConfig } from "@omni-oss/oxlint-config/base";
import { ignorePatterns } from "@omni-oss/oxlint-config/ignores";
import { defineConfig } from "oxlint";

export default defineConfig({
    options: {
        reportUnusedDisableDirectives: "deny",
        typeAware: true,
        typeCheck: true,
    },
    extends: [createConfig()],
    ignorePatterns,
});
