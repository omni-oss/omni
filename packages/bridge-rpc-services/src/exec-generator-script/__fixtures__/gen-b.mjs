/** biome-ignore-all lint/suspicious/noTsIgnore: test fixture */
/** biome-ignore-all lint/suspicious/noAssignInExpressions: test fixture */
// @ts-ignore
// Generator fixture — records its invocation on a global registry.
export default function generatorB(ctx) {
    const g = globalThis;
    // @ts-ignore
    (g.__OMNI_GEN_CALLS__ ??= []).push({
        name: "b",
        isDryRun: ctx.isDryRun,
        hasSys: Boolean(ctx.sys),
        hasLog: Boolean(ctx.log),
    });
}
