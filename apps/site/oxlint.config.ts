import { createConfig } from "@omni-oss/oxlint-config/base";
import solidV2 from "eslint-plugin-solid/configs/v2";
import { defineConfig } from "oxlint";

export default defineConfig({
    extends: [createConfig()],
    jsPlugins: ["eslint-plugin-solid", "@pandacss/eslint-plugin/oxlint"],
    ignorePatterns: ["**/*.gen.*", "dist"],
    settings: solidV2.settings,
    rules: solidV2.rules,
});
