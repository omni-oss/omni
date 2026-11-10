import solidV2 from "eslint-plugin-solid/configs/v2";
import { defineConfig } from "oxlint";

export function createConfig() {
    return defineConfig({
        jsPlugins: ["eslint-plugin-solid"],
        settings: solidV2.settings,
        rules: solidV2.rules,
    });
}

export default createConfig();
