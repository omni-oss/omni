/** biome-ignore-all lint/suspicious/noTsIgnore: test fixture */
/** biome-ignore-all lint/suspicious/noAssignInExpressions: test fixture */
// @ts-ignore
// Generator fixture — records its invocation (and a summary of the context
// it received) on a global registry so the spec can assert it ran.
export default function generatorA(ctx) {
    const g = globalThis;
    // @ts-ignore
    (g.__OMNI_GEN_CALLS__ ??= []).push({
        name: "a",
        isDryRun: ctx.isDryRun,
        hasSys: Boolean(ctx.sys),
        hasLog: Boolean(ctx.log),
        data: ctx.data,
    });
}
