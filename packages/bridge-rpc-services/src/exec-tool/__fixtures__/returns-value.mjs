// Tool fixture — returns a JSON value derived from its inputs.
export default function returnsValue(ctx) {
    return { echoed: ctx.inputs, count: 2, hasSys: Boolean(ctx.sys), hasLog: Boolean(ctx.log) };
}
