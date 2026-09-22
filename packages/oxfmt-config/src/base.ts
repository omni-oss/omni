import { defineConfig } from "oxfmt";

const baseConfig = createConfig();

export default baseConfig;

export function createConfig() {
    return defineConfig({
        useTabs: false,
        tabWidth: 4,
        printWidth: 80,
        singleQuote: false,
        jsxSingleQuote: false,
        quoteProps: "as-needed",
        trailingComma: "all",
        semi: true,
        arrowParens: "always",
        bracketSameLine: false,
        bracketSpacing: true,
        endOfLine: "lf",
        sortPackageJson: true,
        sortImports: true,
    });
}
