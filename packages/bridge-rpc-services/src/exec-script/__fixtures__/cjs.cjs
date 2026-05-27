// CommonJS fixture for loader.spec.ts.
// In Node ESM interop, `await import('./cjs.cjs')` surfaces as
//   { default: { kind: "cjs", ... }, ...named }
module.exports = { kind: "cjs", id: 7 };
