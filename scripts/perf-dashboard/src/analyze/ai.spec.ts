import { describe, expect, it } from "vitest";
import type { Report } from "../chart/ir";
import { makeRun } from "../testing/run-factory";
import {
    type AiConfig,
    annotateReportWithAi,
    ENV_KEYS,
    resolveAiConfig,
} from "./ai";
import { crossTool } from "./cross-tool";

const BASE = ENV_KEYS.baseUrl;
const MODEL = ENV_KEYS.model;
const KEY = ENV_KEYS.apiKey;

describe("resolveAiConfig", () => {
    it("is silently off when nothing is set", () => {
        const { config, warnings } = resolveAiConfig({}, {});
        expect(config).toBeNull();
        expect(warnings).toEqual([]);
    });

    it("resolves fully from env", () => {
        const { config } = resolveAiConfig(
            {},
            { [BASE]: "https://ai.test", [MODEL]: "gpt", [KEY]: "sk-1" },
        );
        expect(config).toEqual({
            baseUrl: "https://ai.test",
            model: "gpt",
            apiKey: "sk-1",
            mode: "all-at-once",
        });
    });

    it("lets CLI overrides win over env", () => {
        const { config } = resolveAiConfig(
            { baseUrl: "https://flag.test" },
            { [BASE]: "https://env.test", [MODEL]: "gpt", [KEY]: "sk-1" },
        );
        expect(config?.baseUrl).toBe("https://flag.test");
    });

    it("warns and disables when partially configured, naming the gaps", () => {
        const { config, warnings } = resolveAiConfig(
            { baseUrl: "https://ai.test" },
            {},
        );
        expect(config).toBeNull();
        expect(warnings.some((w) => w.includes("model"))).toBe(true);
        expect(warnings.some((w) => w.includes("API key"))).toBe(true);
        expect(warnings.some((w) => w.includes("AI_ANALYSIS_MODEL"))).toBe(
            true,
        );
        expect(warnings).toContain("AI analysis disabled.");
    });

    it("resolves mode from env", () => {
        const { config } = resolveAiConfig(
            {},
            {
                [BASE]: "https://ai.test",
                [MODEL]: "gpt",
                [KEY]: "sk-1",
                AI_ANALYSIS_MODE: "per-graph",
            },
        );
        expect(config?.mode).toBe("per-graph");
    });

    it("lets CLI mode override env mode", () => {
        const { config } = resolveAiConfig(
            {
                baseUrl: "https://ai.test",
                model: "gpt",
                apiKey: "sk-1",
                mode: "per-graph",
            },
            { AI_ANALYSIS_MODE: "all-at-once" },
        );
        expect(config?.mode).toBe("per-graph");
    });

    it("warns on invalid mode and falls back to all-at-once", () => {
        const { config, warnings } = resolveAiConfig(
            {
                baseUrl: "https://ai.test",
                model: "gpt",
                apiKey: "sk-1",
                mode: "bad-mode",
            },
            {},
        );
        expect(config?.mode).toBe("all-at-once");
        expect(warnings.some((w) => w.includes("unknown mode"))).toBe(true);
        expect(warnings.some((w) => w.includes('"bad-mode"'))).toBe(true);
    });
});

function sampleReport(): Report {
    const run = makeRun({
        version: "0.4.0",
        os: "linux",
        preset: "full",
        scenarios: [
            {
                name: "scale-300",
                tools: [
                    { tool: "omni", warm: 700, cold: 8000, resources: true },
                    { tool: "turbo", warm: 500, cold: 6000 },
                ],
            },
        ],
    });
    return {
        title: "r",
        generatedAt: "t",
        meta: { spotlight: "0.4.0" },
        views: [crossTool([run], { version: "0.4.0" })],
    };
}

/** Returns a fetch that always responds with a fixed completion body. */
function okFetch(content: string): typeof fetch {
    return (async () =>
        new Response(JSON.stringify({ choices: [{ message: { content } }] }), {
            status: 200,
        })) as unknown as typeof fetch;
}

describe("annotateReportWithAi", () => {
    it("all-at-once (default): makes a single request that fills all charts and the report", async () => {
        const report = sampleReport();
        const chartCount = report.views.flatMap((v) => v.charts).length;
        let calls = 0;
        const cfg: AiConfig = {
            baseUrl: "https://ai.test",
            model: "gpt",
            apiKey: "sk-1",
            fetchImpl: (async () => {
                calls++;
                const charts = Object.fromEntries(
                    Array.from({ length: chartCount }, (_, i) => [
                        `g${i}`,
                        `AI chart ${i}.`,
                    ]),
                );
                const content = JSON.stringify({
                    report: "AI overall.",
                    charts,
                });
                return new Response(
                    JSON.stringify({ choices: [{ message: { content } }] }),
                    { status: 200 },
                );
            }) as unknown as typeof fetch,
        };

        await annotateReportWithAi(report, cfg);

        expect(calls).toBe(1);
        expect(report.aiAnalysis).toBe("AI overall.");
        expect(
            report.views[0]?.charts.every((c) =>
                c.aiAnalysis?.startsWith("AI chart"),
            ),
        ).toBe(true);
    });

    it("all-at-once: warns and leaves aiAnalysis unset when JSON cannot be parsed", async () => {
        const report = sampleReport();
        const warnings: string[] = [];
        const cfg: AiConfig = {
            baseUrl: "https://ai.test",
            model: "gpt",
            apiKey: "sk-1",
            fetchImpl: okFetch("This is not JSON at all."),
        };

        await annotateReportWithAi(report, cfg, {
            onWarn: (m) => warnings.push(m),
        });

        expect(report.aiAnalysis).toBeUndefined();
        expect(report.views[0]?.charts[0]?.aiAnalysis).toBeUndefined();
        expect(warnings.some((w) => w.includes("could not parse JSON"))).toBe(
            true,
        );
    });

    it("all-at-once: tolerates a JSON response wrapped in a code fence", async () => {
        const report = sampleReport();
        const chartCount = report.views.flatMap((v) => v.charts).length;
        const charts = Object.fromEntries(
            Array.from({ length: chartCount }, (_, i) => [
                `g${i}`,
                `AI chart ${i}.`,
            ]),
        );
        const inner = `\`\`\`json\n${JSON.stringify({ report: "AI overall.", charts })}\n\`\`\``;
        const cfg: AiConfig = {
            baseUrl: "https://ai.test",
            model: "gpt",
            apiKey: "sk-1",
            fetchImpl: okFetch(inner),
        };

        await annotateReportWithAi(report, cfg);

        expect(report.aiAnalysis).toBe("AI overall.");
        expect(
            report.views[0]?.charts.every((c) =>
                c.aiAnalysis?.startsWith("AI chart"),
            ),
        ).toBe(true);
    });

    it("per-graph: makes one request per chart plus one for the report", async () => {
        const report = sampleReport();
        const chartCount = report.views.flatMap((v) => v.charts).length;
        let calls = 0;
        const cfg: AiConfig = {
            baseUrl: "https://ai.test",
            model: "gpt",
            apiKey: "sk-1",
            mode: "per-graph",
            fetchImpl: (async () => {
                calls++;
                return new Response(
                    JSON.stringify({
                        choices: [{ message: { content: "AI insight." } }],
                    }),
                    { status: 200 },
                );
            }) as unknown as typeof fetch,
        };

        await annotateReportWithAi(report, cfg);

        // N charts + 1 overall report call.
        expect(calls).toBe(chartCount + 1);
        expect(report.aiAnalysis).toBe("AI insight.");
        expect(
            report.views[0]?.charts.every(
                (c) => c.aiAnalysis === "AI insight.",
            ),
        ).toBe(true);
    });

    it("warns and leaves aiAnalysis unset on request failure", async () => {
        const report = sampleReport();
        const warnings: string[] = [];
        const failing: AiConfig = {
            baseUrl: "https://ai.test",
            model: "gpt",
            apiKey: "sk-1",
            // 400 is non-retryable → fails fast without retries.
            fetchImpl: (async () =>
                new Response("bad", {
                    status: 400,
                })) as unknown as typeof fetch,
        };

        await annotateReportWithAi(report, failing, {
            onWarn: (m) => warnings.push(m),
        });

        expect(report.aiAnalysis).toBeUndefined();
        expect(report.views[0]?.charts[0]?.aiAnalysis).toBeUndefined();
        expect(
            warnings.some((w) => w.includes("AI request failed (400)")),
        ).toBe(true);
    });
});
