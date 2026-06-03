export type Runtime = "node" | "bun" | "deno";

export const RUNTIME: Runtime = detectRuntime();

function detectRuntime(): Runtime {
    return typeof (globalThis as Record<string, unknown>).Deno !== "undefined"
        ? "deno"
        : typeof (globalThis as Record<string, unknown>).Bun !== "undefined"
          ? "bun"
          : "node";
}
