import "vitest";

declare module "vitest" {
    interface TestTags {
        tags:
            | "generator"
            | "prompt"
            | "input"
            | "mcp"
            | "hashing"
            | "caching"
            | "execution"
            | "output";
    }
}
