import { describe, expect, it } from "vitest";
import {
    descendantPids,
    parsePosixCpuTime,
    parsePosixParents,
    parsePosixSample,
    parseProcStat,
    parseWindowsParents,
    parseWindowsSample,
} from "./process-probe";

describe("parseWindowsSample", () => {
    it("parses 'pid rss cpuMs' lines", () => {
        const text = "1234 2097152 4000\n5678 100 1\n";
        const map = parseWindowsSample(text);
        expect(map.get(1234)).toEqual({ rssBytes: 2_097_152, cpuMs: 4000 });
        expect(map.get(5678)).toEqual({ rssBytes: 100, cpuMs: 1 });
    });

    it("ignores blank/short lines", () => {
        expect(parseWindowsSample("\n  \n42 100\n").size).toBe(0);
    });
});

describe("parseWindowsParents", () => {
    it("parses a JSON array into pid → ppid", () => {
        const json = JSON.stringify([
            { ProcessId: 2, ParentProcessId: 1 },
            { ProcessId: 3, ParentProcessId: 2 },
        ]);
        const map = parseWindowsParents(json);
        expect(map.get(2)).toBe(1);
        expect(map.get(3)).toBe(2);
    });

    it("accepts a single (non-array) object", () => {
        const map = parseWindowsParents(
            JSON.stringify({ ProcessId: 7, ParentProcessId: 3 }),
        );
        expect(map.get(7)).toBe(3);
    });

    it("returns empty for blank or invalid input", () => {
        expect(parseWindowsParents("  ").size).toBe(0);
        expect(parseWindowsParents("nope").size).toBe(0);
    });
});

describe("parseProcStat", () => {
    it("extracts ppid/utime/stime/cutime/cstime/rss even when comm has spaces and parens", () => {
        // Fields: pid (comm) state ppid pgrp ... utime(14) stime(15) cutime(16) cstime(17) ... rss(24)
        const fields = [
            "1000",
            "(weird )( name)",
            "R",
            "42", // ppid (field 4)
            "1000",
            "1000",
            "0",
            "-1",
            "0",
            "0",
            "0",
            "0",
            "0",
            "150", // utime (field 14)
            "50", // stime (field 15)
            "30", // cutime (field 16) — reaped children's user time
            "20", // cstime (field 17) — reaped children's kernel time
            "20",
            "0",
            "1",
            "0",
            "12345",
            "0",
            "512", // rss in pages (field 24)
        ];
        const info = parseProcStat(fields.join(" "));
        // (150 + 50 + 30 + 20) ticks / 100 * 1000 = 2500ms; 512 pages * 4096 = 2_097_152.
        expect(info).toEqual({ ppid: 42, rssBytes: 2_097_152, cpuMs: 2500 });
    });

    it("treats missing cutime/cstime as zero", () => {
        // Minimal stat with only fields up through stime present.
        const fields = [
            "1",
            "(init)",
            "S",
            "0", // ppid
            "1",
            "1",
            "0",
            "0",
            "0",
            "0",
            "0",
            "0",
            "0",
            "100", // utime
            "40", // stime
            // cutime / cstime absent — rest[13] and rest[14] will be undefined
        ];
        const info = parseProcStat(fields.join(" "));
        // (100 + 40 + 0 + 0) / 100 * 1000 = 1400ms
        expect(info?.cpuMs).toBe(1400);
    });

    it("returns undefined when there is no closing paren", () => {
        expect(parseProcStat("garbage without paren")).toBeUndefined();
    });
});

describe("parsePosixCpuTime", () => {
    it("parses mm:ss", () => {
        expect(parsePosixCpuTime("01:30")).toBe(90_000);
    });

    it("parses hh:mm:ss", () => {
        expect(parsePosixCpuTime("01:00:00")).toBe(3_600_000);
    });

    it("parses dd-hh:mm:ss", () => {
        expect(parsePosixCpuTime("1-00:00:00")).toBe(86_400_000);
    });

    it("parses fractional seconds", () => {
        expect(parsePosixCpuTime("00:02.50")).toBe(2500);
    });
});

describe("parsePosixSample", () => {
    it("parses pid/rss(KB)/time rows into byte + ms stats", () => {
        const stdout = "  100 2048 00:01\n  200 4096 00:00:02\n";
        const map = parsePosixSample(stdout);
        expect(map.get(100)).toEqual({ rssBytes: 2_097_152, cpuMs: 1000 });
        expect(map.get(200)).toEqual({ rssBytes: 4_194_304, cpuMs: 2000 });
    });

    it("ignores blank lines", () => {
        expect(parsePosixSample("\n\n").size).toBe(0);
    });
});

describe("parsePosixParents", () => {
    it("parses pid/ppid rows", () => {
        const map = parsePosixParents("100 1\n200 100\n");
        expect(map.get(100)).toBe(1);
        expect(map.get(200)).toBe(100);
    });
});

describe("descendantPids", () => {
    const parents = new Map<number, number>([
        [1, 0],
        [2, 1], // child of 1
        [3, 2], // grandchild of 1
        [4, 1], // child of 1
        [5, 99], // unrelated
        [10, 0], // detached daemon
        [11, 10], // daemon worker
    ]);

    it("collects a root and all its descendants", () => {
        expect([...descendantPids(parents, [1])].sort((a, b) => a - b)).toEqual(
            [1, 2, 3, 4],
        );
    });

    it("collects from multiple roots (e.g. CLI + detached daemon)", () => {
        expect(
            [...descendantPids(parents, [1, 10])].sort((a, b) => a - b),
        ).toEqual([1, 2, 3, 4, 10, 11]);
    });

    it("keeps roots that are absent from the map", () => {
        expect([...descendantPids(parents, [1234])]).toEqual([1234]);
    });
});
