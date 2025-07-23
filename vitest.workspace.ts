import type { TestProjectConfiguration } from "vitest/config";

export default [
    "./**/vitest.config.{unit,e2e,integration,api,ui}.{ts,js}",
] satisfies TestProjectConfiguration[];
