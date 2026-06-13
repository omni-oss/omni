// Generator fixture — records its invocation (and a summary of the context
// it received) on a global registry so the spec can assert it ran.
export default function generatorA(ctx) {
    const g = globalThis;
    (g.__OMNI_GEN_CALLS__ ??= []).push({
        name: "a",
        isDryRun: ctx.isDryRun,
        hasSys: Boolean(ctx.sys),
        hasLog: Boolean(ctx.log),
    });
}
