import { declareSysTests } from "./test-declarations";
import { VirtualSystem } from "./virtual";

declareSysTests({
    sys: await VirtualSystem.create(),
    isRealSystem: false,
    name: "Virtual",
});
