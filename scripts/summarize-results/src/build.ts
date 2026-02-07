export {};

await Bun.build({
    entrypoints: ["./src/index.ts"],
    outdir: "./bin",
    target: "node",
    minify: true,
});
