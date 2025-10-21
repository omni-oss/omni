import { OptimizedSystem } from "./optimized";
import { declareSysTests } from "./test-declarations";

declareSysTests({
    sys: await OptimizedSystem.create(),
    isRealSystem: true,
    name: "Optimized",
});
