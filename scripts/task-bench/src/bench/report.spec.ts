import { describe, expect, it } from "vitest";
import type { ToolInfo } from "../tools";
import { renderToolInfo } from "./report";

const info: ToolInfo = {
    tool: "turbo",
    version: "2.10.3",
    daemon: true,
    provisioning: "workspace-dependency",
    supportedVersions: ["^2.0.0"],
    description: "Vercel Turborepo. Runs a persistent daemon.",
};

describe("renderToolInfo", () => {
    it("renders version and the key attributes for each tool", () => {
        const text = renderToolInfo([info]).join("\n");
        expect(text).toContain("**turbo** 2.10.3");
        expect(text).toContain("daemon: yes");
        expect(text).toContain("provisioning: workspace-dependency");
        expect(text).toContain("supported: ^2.0.0");
        expect(text).toContain("Vercel Turborepo");
    });

    it("shows '?' for a missing version and 'no' for no daemon", () => {
        const text = renderToolInfo([
            { ...info, tool: "omni", version: null, daemon: false },
        ]).join("\n");
        expect(text).toContain("**omni** ?");
        expect(text).toContain("daemon: no");
    });

    it("returns nothing for an empty list", () => {
        expect(renderToolInfo([])).toEqual([]);
    });
});
