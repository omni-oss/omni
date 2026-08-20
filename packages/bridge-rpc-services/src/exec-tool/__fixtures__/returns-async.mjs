// Tool fixture — async default export that returns after a macrotask.
export default async function returnsAsync(ctx) {
    await new Promise((resolve) => setTimeout(resolve, 5));
    return { ok: true, who: ctx.inputs };
}
