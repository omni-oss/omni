#!/usr/bin/env node
import { mkdir, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { Command } from "@commander-js/extra-typings";
import { FLAG_KEYS } from "@/analyze/ai";
import { description, name, version } from "../../package.json";
import { resolveAiConfig } from "../analyze";
import { normalize, parseSuite } from "../ingest";
import { buildReport, type ViewKind } from "../pipeline";
import type { RenderOutput } from "../render";
import { getRenderer } from "../render";
import type { DataSource } from "../sources";
import { GitHubDataSource, LocalFsDataSource } from "../sources";

const program = new Command();

program.name(name).version(version).description(description);

program
    .command("inspect")
    .description(
        "Normalize local benchmark output and print the flattened runs.",
    )
    .requiredOption(
        "-p, --path <path...>",
        "data.json file(s) or directory of runs",
    )
    .option("--version-label <version>", "override the omni version")
    .option("--target <target>", "override the target")
    .action(async (opts) => {
        const source = new LocalFsDataSource({
            paths: opts.path,
            ...(opts.versionLabel ? { version: opts.versionLabel } : {}),
            ...(opts.target ? { target: opts.target } : {}),
        });
        const refs = await source.listRuns();
        const runs = [];
        for (const ref of refs) {
            const { json } = await source.fetchRaw(ref);
            runs.push(
                ...normalize(ref, parseSuite(json), source.descriptor.id),
            );
        }
        console.log(JSON.stringify({ runs }, null, 2));
    });

program
    .command("report")
    .description("Build the full report (all views) and render it.")
    .option("--source <source>", "data source (local-fs | github)", "local-fs")
    .option(
        "-p, --path <path...>",
        "[local-fs] data.json file(s) or directory of runs",
    )
    .option("--repo <owner/repo>", "[github] published comparison repo")
    .option("--ref <ref>", "[github] restrict to a single tag/branch/sha")
    .option("--token <token>", "[github] API token (or set GITHUB_TOKEN)")
    .option(
        "--views <views>",
        "comma-separated views (cross-tool,version-history)",
    )
    .option("--spotlight <version>", "omni version to spotlight (cross-tool)")
    .option(
        "--renderer <renderers>",
        "comma-separated output renderers (json,markdown,html)",
        "json",
    )
    .option("--json-file-name <name>", "[json] override the output file name")
    .option(
        "--markdown-file-name <name>",
        "[markdown] override the output file name",
    )
    .option("--html-file-name <name>", "[html] override the output file name")
    .option("-o, --out <dir>", "write output files to this directory")
    .option(
        `${FLAG_KEYS.baseUrl} <url>`,
        "[ai] chat-completions base URL (or AI_ANALYSIS_API_BASE_URL)",
    )
    .option(
        `${FLAG_KEYS.model} <model>`,
        "[ai] model name (or AI_ANALYSIS_MODEL)",
    )
    .option(
        `${FLAG_KEYS.apiKey} <key>`,
        "[ai] API key (or AI_ANALYSIS_API_KEY)",
    )
    .option(
        "--ai-analysis-mode <mode>",
        "[ai] all-at-once (default) | per-graph (or AI_ANALYSIS_MODE)",
    )
    .action(async (opts) => {
        const source = buildSource(opts);
        const fileNames: Record<string, string | undefined> = {
            json: opts.jsonFileName,
            markdown: opts.markdownFileName,
            html: opts.htmlFileName,
        };
        // De-duplicate renderer ids, preserving order.
        const rendererIds = [
            ...new Set(
                opts.renderer
                    .split(",")
                    .map((r) => r.trim())
                    .filter((r) => r),
            ),
        ];
        const renderers = rendererIds.map((id) => {
            const fileName = fileNames[id];
            return getRenderer(id, fileName ? { fileName } : {});
        });

        // Optional AI analysis: env then flags (flags win); warn if incomplete.
        const { config: aiConfig, warnings } = resolveAiConfig({
            baseUrl: opts.aiAnalysisApiBaseUrl,
            model: opts.aiAnalysisModel,
            apiKey: opts.aiAnalysisApiKey,
            mode: opts.aiAnalysisMode,
        });
        for (const w of warnings) console.warn(w);

        const report = await buildReport({
            source,
            ...(opts.views
                ? {
                      views: opts.views
                          .split(",")
                          .map((v) => v.trim()) as ViewKind[],
                  }
                : {}),
            ...(opts.spotlight ? { version: opts.spotlight } : {}),
            ...(aiConfig ? { ai: aiConfig } : {}),
            onWarn: (msg) => console.warn(msg),
        });

        const outputs = await Promise.all(
            renderers.map((r) => r.render(report)),
        );
        await emit({ files: outputs.flatMap((o) => o.files) }, opts.out);
    });

interface ReportOpts {
    source: string;
    path?: string[];
    repo?: string;
    ref?: string;
    token?: string;
}

function buildSource(opts: ReportOpts): DataSource {
    if (opts.source === "github") {
        if (!opts.repo?.includes("/")) {
            console.error(
                "--repo <owner/repo> is required for --source github",
            );
            process.exit(1);
        }
        const [owner, repo] = opts.repo.split("/", 2) as [string, string];
        const token = opts.token ?? process.env.GITHUB_TOKEN;
        return new GitHubDataSource({
            owner,
            repo,
            ...(opts.ref ? { ref: opts.ref } : {}),
            ...(token ? { token } : {}),
        });
    }
    if (opts.source === "local-fs") {
        if (!opts.path || opts.path.length === 0) {
            console.error("--path is required for --source local-fs");
            process.exit(1);
        }
        return new LocalFsDataSource({ paths: opts.path });
    }
    console.error(`unknown --source "${opts.source}"`);
    process.exit(1);
}

async function emit(output: RenderOutput, outDir?: string): Promise<void> {
    if (!outDir) {
        const multiple = output.files.length > 1;
        for (const file of output.files) {
            if (multiple) console.log(`==> ${file.path} <==`);
            console.log(
                typeof file.content === "string"
                    ? file.content
                    : Buffer.from(file.content).toString("utf8"),
            );
        }
        return;
    }
    for (const file of output.files) {
        const dest = join(outDir, file.path);
        await mkdir(dirname(dest), { recursive: true });
        await writeFile(dest, file.content);
    }
    console.log(
        `Wrote ${output.files.length} file(s) to ${outDir}: ${output.files
            .map((f) => f.path)
            .join(", ")}`,
    );
}

program.parseAsync();
