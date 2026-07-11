import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { LocalFsDataSource } from "../sources";
import { normalize } from "./normalize";
import { parseSuite } from "./schema";

const FIXTURE = fileURLToPath(
    new URL("../__fixtures__/linux-full.data.json", import.meta.url),
);

describe("LocalFsDataSource + normalize (M1 end-to-end)", () => {
    it("infers (version, target) from the payload for a bare data.json", async () => {
        const source = new LocalFsDataSource({ paths: [FIXTURE] });
        const refs = await source.listRuns();

        expect(refs).toHaveLength(1);
        expect(refs[0]?.version).toBe("0.4.1");
        expect(refs[0]?.target).toBe("x86_64-unknown-linux-gnu");
    });

    it("honors explicit version/target overrides", async () => {
        const source = new LocalFsDataSource({
            paths: [FIXTURE],
            version: "dev",
            target: "x86_64-unknown-linux-gnu",
        });
        const [ref] = await source.listRuns();
        expect(ref?.version).toBe("dev");
    });

    it("filters by version in listRuns", async () => {
        const source = new LocalFsDataSource({ paths: [FIXTURE] });
        expect(await source.listRuns({ versions: ["nope"] })).toHaveLength(0);
        expect(await source.listRuns({ versions: ["0.4.1"] })).toHaveLength(1);
    });

    it("flattens the suite into a normalized run", async () => {
        const run = await loadRun();

        expect(run.source).toBe("local-fs");
        expect(run.preset).toBe("full");
        expect(run.os).toBe("linux");
        expect(run.concurrency).toBe(8);
        expect(run.taskBenchVersion).toBe("0.1.0");
    });

    it("emits duration points for every tool/warmth and resources only when present", async () => {
        const run = await loadRun();

        const omniWarmDuration = run.points.find(
            (p) =>
                p.coord.tool === "omni" &&
                p.coord.warmth === "warm" &&
                p.metric === "durationMs",
        );
        expect(omniWarmDuration?.median).toBe(715);
        expect(omniWarmDuration?.n).toBe(3);
        expect(omniWarmDuration?.hasResources).toBe(true);

        // omni carries resource metrics …
        const omniRss = run.points.find(
            (p) =>
                p.coord.tool === "omni" &&
                p.coord.warmth === "cold" &&
                p.metric === "peakRssBytes",
        );
        expect(omniRss?.median).toBe(120000000);

        // … turbo does not.
        const turboResourcePoints = run.points.filter(
            (p) => p.coord.tool === "turbo" && p.metric !== "durationMs",
        );
        expect(turboResourcePoints).toHaveLength(0);
        const turboDuration = run.points.filter(
            (p) => p.coord.tool === "turbo" && p.metric === "durationMs",
        );
        expect(turboDuration).toHaveLength(2); // cold + warm
        expect(turboDuration.every((p) => p.hasResources === false)).toBe(true);
    });

    it("marks the errored tool and records warnings without throwing", async () => {
        const run = await loadRun();

        const nxPoints = run.points.filter((p) => p.coord.tool === "nx");
        expect(nxPoints.length).toBeGreaterThan(0);
        expect(nxPoints.every((p) => p.errored)).toBe(true);
        expect(nxPoints.every((p) => p.n === 0)).toBe(true);

        expect(run.warnings.some((w) => w.includes("nx errored"))).toBe(true);
        expect(run.warnings.some((w) => w.includes("no samples"))).toBe(true);
    });

    it("captures platform specs and tool info", async () => {
        const run = await loadRun();
        expect(run.platform.os.platform).toBe("linux");
        expect(run.platform.cpus.length).toBe(4);
        expect(run.platform.memory.totalBytes).toBe(16777216000);
        const omniInfo = run.toolInfo.find((t) => t.tool === "omni");
        expect(omniInfo?.version).toBe("0.4.1");
        expect(omniInfo?.provisioning).toBe("host-binary");
    });

    it("carries the omni tool version onto its coordinates", async () => {
        const run = await loadRun();
        const omni = run.points.find((p) => p.coord.tool === "omni");
        expect(omni?.coord.toolVersion).toBe("0.4.1");
        const nx = run.points.find((p) => p.coord.tool === "nx");
        expect(nx?.coord.toolVersion).toBeNull();
    });
});

async function loadRun() {
    const source = new LocalFsDataSource({ paths: [FIXTURE] });
    const [ref] = await source.listRuns();
    if (!ref) throw new Error("expected a run ref");
    const { json } = await source.fetchRaw(ref);
    const [run] = normalize(ref, parseSuite(json), source.descriptor.id);
    if (!run) throw new Error("expected a normalized run");
    return run;
}
