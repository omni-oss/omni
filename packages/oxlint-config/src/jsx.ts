import { defineConfig } from "oxlint";

export function createConfig() {
    return defineConfig({ plugins: ["jsx-a11y"] });
}

export default createConfig();
