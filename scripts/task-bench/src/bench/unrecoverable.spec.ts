import { describe, expect, it } from "vitest";
import { unrecoverableExitReason } from "./unrecoverable";

describe("unrecoverableExitReason", () => {
    it("flags the Windows process-init failure (0xC0000142)", () => {
        const reason = unrecoverableExitReason(3221225794, "win32");
        expect(reason).toMatch(/STATUS_DLL_INIT_FAILED/);
    });

    it("is platform-specific: the Windows code is recoverable on linux", () => {
        expect(unrecoverableExitReason(3221225794, "linux")).toBeNull();
    });

    it("flags a SIGKILL/OOM exit on linux and darwin", () => {
        expect(unrecoverableExitReason(137, "linux")).toMatch(/OOM|SIGKILL/);
        expect(unrecoverableExitReason(137, "darwin")).toMatch(/SIGKILL/);
    });

    it("returns null for ordinary failures and success", () => {
        expect(unrecoverableExitReason(0, "win32")).toBeNull();
        expect(unrecoverableExitReason(1, "win32")).toBeNull();
        expect(unrecoverableExitReason(2, "linux")).toBeNull();
    });
});
