/** biome-ignore-all lint/suspicious/noTsIgnore: test fixture */
/** biome-ignore-all lint/suspicious/noAssignInExpressions: test fixture */
// @ts-ignore

// Generator fixture — an async default export. Records its invocation only
// after awaiting a macrotask, so the spec can prove the runner awaits it.
export default async function generatorAsync(ctx) {
    await new Promise((resolve) => setTimeout(resolve, 5));
    const g = globalThis;
    // @ts-ignore
    (g.__OMNI_GEN_CALLS__ ??= []).push({
        name: "async",
        isDryRun: ctx.isDryRun,
        hasSys: Boolean(ctx.sys),
        hasLog: Boolean(ctx.log),
    });
}
