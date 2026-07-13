#!/usr/bin/env bun
/**
 * analyze-perf.ts — analyze a `perf.data` recording produced by the
 * `omni_bench` profiling task (`cargo flamegraph ... -c "record ..."`).
 *
 * Design: perf does the heavy lifting, this script just analyzes its output.
 * `perf.data` is a binary format, and decoding + symbol resolution + call-graph
 * aggregation are exactly what `perf report` already does well, so this script
 * shells out to:
 *
 *     perf report --stdio -g none --no-inline \
 *       -F overhead_children,overhead,dso,symbol --sort dso,symbol
 *
 * and parses the resulting (already-ranked) per-symbol table. It then layers on
 * the things `perf report` can't do:
 *
 *   - Collapse Rust generic monomorphizations (`foo<A>`, `foo<B>` -> `foo`),
 *     the single biggest source of noise in these profiles.
 *   - Filter to a subset of symbols (e.g. omni's own code).
 *   - Re-rank by self vs. inclusive cost and group by shared object.
 *   - Emit a machine-readable JSON summary.
 *
 * `--no-inline` is passed by default: resolving DWARF inline frames is by far
 * the slowest part of any perf post-processing pass (minutes vs. seconds when
 * the profile lives on a slow/FUSE filesystem), and it is not needed for a flat
 * self/inclusive/DSO ranking. Pass `--inline` if you specifically want inlined
 * frames folded into their callers' attribution.
 *
 * For call-path/flamegraph views, use the `flamegraph.svg` this crate already
 * generates, or `perf report -g --stdio` directly.
 *
 * Usage:
 *   bun run analyze-perf.ts [perf.data] [options]
 *
 * Examples:
 *   # Top 30 functions by self time in the default perf.data next to the crate
 *   bun run scripts/analyze-perf.ts
 *
 *   # Focus on omni's own code, collapsing generic monomorphizations
 *   bun run scripts/analyze-perf.ts -f 'omni_|task_execution' --collapse-generics
 *
 *   # Inclusive ranking, top 50, plus a per-DSO breakdown
 *   bun run scripts/analyze-perf.ts --mode total -n 50 --by-dso
 *
 *   # JSON for further processing
 *   bun run scripts/analyze-perf.ts --json > report.json
 */

import { existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const CRATE_DIR = resolve(SCRIPT_DIR, "..");

interface Options {
    input: string;
    top: number;
    mode: "self" | "total";
    filter: RegExp | null;
    collapseGenerics: boolean;
    byDso: boolean;
    minPercent: number;
    inline: boolean;
    json: boolean;
}

function printHelp(): void {
    console.log(`analyze-perf.ts — analyze a perf.data recording (via \`perf report\`)

Usage:
  bun run analyze-perf.ts [perf.data] [options]

Options:
  -i, --input <file>        Path to perf.data (default: ./perf.data, then
                            <crate>/perf.data).
  -n, --top <N>             Number of entries to show per table (default: 30).
      --mode <self|total>   Which table to show first: self (exclusive) or
                            total (inclusive) time (default: self).
  -f, --filter <regex>      Only include symbols matching this regex, e.g.
                            'omni_|task_execution'.
      --collapse-generics   Strip <...> generic parameters so monomorphized
                            copies of a function merge into one entry.
      --by-dso              Add a breakdown grouped by shared object (DSO).
      --min-percent <p>     Hide table rows below this percent (default: 0.5).
      --inline              Resolve DWARF inline frames (much slower; off by
                            default).
      --json                Emit a JSON summary instead of formatted reports.
  -h, --help                Show this help.

For call-path/flamegraph views, use this crate's flamegraph.svg or
\`perf report -g --stdio -i <perf.data>\`.
`);
}

function parseArgs(argv: string[]): Options {
    const opts: Options = {
        input: "",
        top: 30,
        mode: "self",
        filter: null,
        collapseGenerics: false,
        byDso: false,
        minPercent: 0.5,
        inline: false,
        json: false,
    };

    const positional: string[] = [];

    for (let i = 0; i < argv.length; i++) {
        const arg = argv[i];
        const next = () => {
            const v = argv[++i];
            if (v === undefined) fail(`missing value for ${arg}`);
            return v;
        };

        switch (arg) {
            case "-h":
            case "--help":
                printHelp();
                process.exit(0);
                break;
            case "-i":
            case "--input":
                opts.input = next();
                break;
            case "-n":
            case "--top":
                opts.top = parsePositiveInt(next(), arg);
                break;
            case "--mode": {
                const v = next();
                if (v !== "self" && v !== "total") {
                    fail(`--mode must be 'self' or 'total', got '${v}'`);
                }
                opts.mode = v;
                break;
            }
            case "-f":
            case "--filter":
                try {
                    opts.filter = new RegExp(next());
                } catch (e) {
                    fail(`invalid --filter regex: ${(e as Error).message}`);
                }
                break;
            case "--collapse-generics":
                opts.collapseGenerics = true;
                break;
            case "--by-dso":
                opts.byDso = true;
                break;
            case "--min-percent":
                opts.minPercent = Number.parseFloat(next());
                if (Number.isNaN(opts.minPercent)) {
                    fail("--min-percent must be a number");
                }
                break;
            case "--inline":
                opts.inline = true;
                break;
            case "--json":
                opts.json = true;
                break;
            default:
                if (arg.startsWith("-")) fail(`unknown option: ${arg}`);
                positional.push(arg);
        }
    }

    if (!opts.input) opts.input = positional[0] ?? "";
    if (!opts.input) {
        opts.input = existsSync("perf.data")
            ? "perf.data"
            : resolve(CRATE_DIR, "perf.data");
    }

    return opts;
}

function parsePositiveInt(value: string, flag: string): number {
    const n = Number.parseInt(value, 10);
    if (!Number.isInteger(n) || n <= 0) {
        fail(`${flag} must be a positive integer, got '${value}'`);
    }
    return n;
}

function fail(message: string): never {
    console.error(`error: ${message}`);
    process.exit(1);
}

/** One row of `perf report`'s aggregated per-(dso,symbol) table. */
interface PerfRow {
    childrenPct: number;
    selfPct: number;
    dso: string;
    symbol: string;
}

interface Report {
    rows: PerfRow[];
    event: string;
    sampleCount: string;
    totalEvents: number;
}

// Data row, e.g.:
//   "    24.26%     0.00%  libc.so.6   [.] __GI___clone3            -      -"
// Groups: children%, self%, dso, then "[x] symbol ...trailing IPC".
const ROW = /^\s*(-?[\d.]+)%\s+(-?[\d.]+)%\s+(\S+)\s+\[[^\]]\]\s+(.*)$/;
// The (no-LBR) recordings this script targets always render the trailing IPC /
// IPC-coverage columns as dashes; strip them so only the symbol name remains.
const TRAILING_IPC = /\s{2,}-\s+-\s*$/;
const EVENT_COUNT = /Event count \(approx\.\):\s*(\d+)/;
const SAMPLES = /Samples:\s*(\S+)\s+of event '([^']+)'/;

/** Remove <...> generic parameters, innermost first, until none remain. */
function stripGenerics(symbol: string): string {
    let prev: string;
    let cur = symbol;
    do {
        prev = cur;
        cur = cur.replace(/<[^<>]*>/g, "");
    } while (cur !== prev);
    // Turbofish `foo::<T>::bar` collapses to `foo::::bar`; normalize the
    // resulting colon runs back to a single path separator.
    return cur.replace(/:{3,}/g, "::").trim();
}

async function runPerfReport(opts: Options): Promise<Report> {
    const args = [
        "report",
        "-i",
        opts.input,
        "--stdio",
        "-g",
        "none",
        opts.inline ? "--inline" : "--no-inline",
        "-F",
        "overhead_children,overhead,dso,symbol",
        "--sort",
        "dso,symbol",
        "--percent-limit",
        "0",
    ];

    const proc = Bun.spawn(["perf", ...args], {
        stdout: "pipe",
        stderr: "pipe",
    });

    const stdout = await new Response(proc.stdout).text();
    const exitCode = await proc.exited;
    if (exitCode !== 0) {
        const stderr = await new Response(proc.stderr).text();
        fail(
            `\`perf report\` exited with code ${exitCode}` +
                (stderr.trim() ? `:\n${stderr.trim()}` : ""),
        );
    }

    const report: Report = {
        rows: [],
        event: "events",
        sampleCount: "?",
        totalEvents: 0,
    };

    for (const line of stdout.split("\n")) {
        if (line.startsWith("#")) {
            const ec = EVENT_COUNT.exec(line);
            if (ec) report.totalEvents = Number.parseInt(ec[1], 10);
            const s = SAMPLES.exec(line);
            if (s) {
                report.sampleCount = s[1];
                report.event = s[2];
            }
            continue;
        }
        const m = ROW.exec(line);
        if (!m) continue;

        let symbol = m[4].replace(TRAILING_IPC, "").trim();
        if (opts.collapseGenerics && symbol && !symbol.startsWith("[")) {
            symbol = stripGenerics(symbol) || symbol;
        }

        report.rows.push({
            childrenPct: Number.parseFloat(m[1]),
            selfPct: Number.parseFloat(m[2]),
            dso: m[3],
            symbol,
        });
    }

    if (report.rows.length === 0) {
        fail(
            "perf report produced no rows. Was perf.data recorded with call " +
                "graphs (e.g. `perf record --call-graph fp -g`)?",
        );
    }

    return report;
}

interface Row {
    name: string;
    selfPct: number;
    childrenPct: number;
}

/** Aggregate perf rows by symbol, summing self and inclusive percentages. */
function aggregateBySymbol(report: Report, filter: RegExp | null): Row[] {
    const bySymbol = new Map<string, Row>();
    for (const r of report.rows) {
        if (filter && !filter.test(r.symbol)) continue;
        const existing = bySymbol.get(r.symbol);
        if (existing) {
            existing.selfPct += r.selfPct;
            existing.childrenPct += r.childrenPct;
        } else {
            bySymbol.set(r.symbol, {
                name: r.symbol,
                selfPct: r.selfPct,
                childrenPct: r.childrenPct,
            });
        }
    }
    return [...bySymbol.values()];
}

/** Aggregate self percentages by shared object. */
function aggregateByDso(report: Report): Row[] {
    const byDso = new Map<string, Row>();
    for (const r of report.rows) {
        const existing = byDso.get(r.dso);
        if (existing) {
            existing.selfPct += r.selfPct;
            existing.childrenPct += r.childrenPct;
        } else {
            byDso.set(r.dso, {
                name: r.dso,
                selfPct: r.selfPct,
                childrenPct: r.childrenPct,
            });
        }
    }
    return [...byDso.values()];
}

function formatCount(n: number): string {
    if (n >= 1e9) return `${(n / 1e9).toFixed(2)}G`;
    if (n >= 1e6) return `${(n / 1e6).toFixed(2)}M`;
    if (n >= 1e3) return `${(n / 1e3).toFixed(2)}K`;
    return n.toFixed(0);
}

function truncate(s: string, max: number): string {
    return s.length <= max ? s : `${s.slice(0, max - 1)}…`;
}

function printTable(
    title: string,
    rows: Row[],
    key: "selfPct" | "childrenPct",
    opts: Options,
    report: Report,
    label = "symbol",
): void {
    console.log(`\n${title}`);
    console.log("─".repeat(title.length));

    const sorted = [...rows].sort((a, b) => b[key] - a[key]);
    const shown = sorted
        .filter((r) => r[key] >= opts.minPercent)
        .slice(0, opts.top);

    if (shown.length === 0) {
        console.log("(no entries above threshold)");
        return;
    }

    const eventLabel = report.event;
    console.log(`${"%".padStart(7)}  ${eventLabel.padStart(10)}  ${label}`);
    for (const r of shown) {
        const pct = `${r[key].toFixed(2)}%`.padStart(7);
        const abs = report.totalEvents
            ? formatCount((r[key] / 100) * report.totalEvents)
            : "-";
        console.log(`${pct}  ${abs.padStart(10)}  ${truncate(r.name, 100)}`);
    }
}

function printReports(report: Report, opts: Options): void {
    const symbols = aggregateBySymbol(report, opts.filter);

    console.log(`perf analysis: ${opts.input}`);
    console.log(
        `event: ${report.event}   samples: ${report.sampleCount}   ` +
            `${report.event}: ${formatCount(report.totalEvents)} ` +
            `(${report.totalEvents})   symbols: ${symbols.length}`,
    );
    if (opts.filter) console.log(`filter: /${opts.filter.source}/`);
    if (opts.collapseGenerics) console.log("generics: collapsed");
    if (opts.inline) console.log("inline frames: resolved");

    const selfTable = () =>
        printTable(
            "Top functions by SELF time (exclusive)",
            symbols,
            "selfPct",
            opts,
            report,
        );
    const totalTable = () =>
        printTable(
            "Top functions by TOTAL time (inclusive)",
            symbols,
            "childrenPct",
            opts,
            report,
        );

    if (opts.mode === "self") {
        selfTable();
        totalTable();
    } else {
        totalTable();
        selfTable();
    }

    if (opts.byDso) {
        // DSO grouping is about where code lives; the symbol filter doesn't apply.
        printTable(
            "Cost by shared object (self time)",
            aggregateByDso(report),
            "selfPct",
            opts,
            report,
            "shared object",
        );
    }
}

function printJson(report: Report, opts: Options): void {
    const symbols = aggregateBySymbol(report, opts.filter);
    const toRows = (key: "selfPct" | "childrenPct", rows: Row[]) =>
        [...rows]
            .sort((a, b) => b[key] - a[key])
            .slice(0, opts.top)
            .map((r) => ({
                name: r.name,
                selfPercent: Number(r.selfPct.toFixed(4)),
                totalPercent: Number(r.childrenPct.toFixed(4)),
            }));

    const out = {
        input: opts.input,
        event: report.event,
        samples: report.sampleCount,
        totalEvents: report.totalEvents,
        uniqueSymbols: symbols.length,
        filter: opts.filter?.source ?? null,
        collapseGenerics: opts.collapseGenerics,
        inline: opts.inline,
        self: toRows("selfPct", symbols),
        total: toRows("childrenPct", symbols),
        byDso: toRows("selfPct", aggregateByDso(report)).map((r) => ({
            dso: r.name,
            selfPercent: r.selfPercent,
            totalPercent: r.totalPercent,
        })),
    };

    console.log(JSON.stringify(out, null, 2));
}

async function main(): Promise<void> {
    const opts = parseArgs(process.argv.slice(2));

    if (!existsSync(opts.input)) fail(`input not found: ${opts.input}`);

    const report = await runPerfReport(opts);

    if (opts.json) {
        printJson(report, opts);
    } else {
        printReports(report, opts);
    }
}

main().catch((e) => fail(e instanceof Error ? e.message : String(e)));
