// Imports two bare specifiers. In the boundary test these are mapped by an
// injected resolver to `node_modules`-shaped paths so the walker records the
// package root as a bounded grant boundary without walking its internals.
import "exports-fixture";
import "exports-fixture/sub";
export const consumer = true;
