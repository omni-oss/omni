/** biome-ignore-all lint/suspicious/noTsIgnore: test fixture */
// Generator fixture — records its invocation on a global registry.
// @ts-ignore
export default function generatorB(ctx) {
    const g = globalThis;
    (g.__OMNI_GEN_CALLS__ ??= []).push({
        name: "b",
        isDryRun: ctx.isDryRun,
        hasSys: Boolean(ctx.sys),
        hasLog: Boolean(ctx.log),
    });
}
