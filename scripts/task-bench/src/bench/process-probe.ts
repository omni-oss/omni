import { type ChildProcess, execFile, spawn } from "node:child_process";
import { readdir, readFile } from "node:fs/promises";
import { platform } from "node:os";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

/** Resident memory + cumulative CPU for a single process. */
export interface ProcStat {
    /** Resident set size (working set) in bytes. */
    rssBytes: number;
    /** Cumulative CPU time (user + kernel) in milliseconds. */
    cpuMs: number;
}

/**
 * A reusable, platform-specific process sampler with two operations:
 *   - `sample(pids)` — fast RSS + cumulative CPU for a specific set of PIDs.
 *     Called every tick, so it must be cheap (Windows uses `Get-Process`, not
 *     the ~800ms `Get-CimInstance`).
 *   - `parents()` — a pid → ppid map of every process, used to discover the
 *     descendants of the invocation. This is slower on Windows (it needs CIM),
 *     so callers run it on a coarse cadence and off the critical path.
 */
export interface ProcessProbe {
    sample(pids: number[]): Promise<Map<number, ProcStat>>;
    parents(): Promise<Map<number, number>>;
    /** Start any backing helper so the first real `sample` is fast (idempotent). */
    warmup(): Promise<void>;
    dispose(): Promise<void>;
}

const currentPlatform = platform();

export function createProcessProbe(): ProcessProbe {
    if (currentPlatform === "win32") return new WindowsProbe();
    if (currentPlatform === "linux") return new LinuxProbe();
    return new PosixProbe();
}

/**
 * Every PID reachable from `roots` (inclusive) by following parent → child
 * links in a `pid → ppid` map. Roots absent from the map still appear in the
 * result (so the caller keeps sampling them even after the map misses them).
 */
export function descendantPids(
    parents: Map<number, number>,
    roots: Iterable<number>,
): Set<number> {
    const children = new Map<number, number[]>();
    for (const [pid, ppid] of parents) {
        const siblings = children.get(ppid);
        if (siblings) siblings.push(pid);
        else children.set(ppid, [pid]);
    }

    const out = new Set<number>();
    const stack = [...roots];
    while (stack.length > 0) {
        const pid = stack.pop();
        if (pid === undefined || out.has(pid)) continue;
        out.add(pid);
        for (const child of children.get(pid) ?? []) {
            if (!out.has(child)) stack.push(child);
        }
    }
    return out;
}

// --- Windows ----------------------------------------------------------------

const WIN_EOF = "<<TASKBENCH_PROBE_EOF>>";

// Persistent read-loop driver for the fast per-PID sampler. For every stdin
// line (comma-separated PIDs) it prints "pid rss cpuMs" per live process, then
// a marker line, flushing explicitly. Passed via -EncodedCommand (base64
// UTF-16LE) to avoid quoting, and using an explicit ReadLine/Flush loop to
// avoid PowerShell's `-Command -` stdin/stdout buffering.
const WIN_SAMPLE_DRIVER = [
    "$ErrorActionPreference='SilentlyContinue'",
    "while ($true) {",
    "  $line = [Console]::In.ReadLine()",
    "  if ($null -eq $line -or $line -eq 'q') { break }",
    "  $ids = $line.Split(',') | Where-Object { $_ -ne '' }",
    "  if ($ids.Count -gt 0) {",
    "    Get-Process -Id $ids | ForEach-Object {",
    '      [Console]::Out.Write("$($_.Id) $([long]$_.WorkingSet64) $([long]$_.TotalProcessorTime.TotalMilliseconds)`n")',
    "    }",
    "  }",
    `  [Console]::Out.Write("${WIN_EOF}\`n")`,
    "  [Console]::Out.Flush()",
    "}",
].join("\n");

/** Parse the fast sampler's "pid rss cpuMs" lines into stats. */
export function parseWindowsSample(text: string): Map<number, ProcStat> {
    const out = new Map<number, ProcStat>();
    for (const line of text.split("\n")) {
        const trimmed = line.trim();
        if (!trimmed) continue;
        const parts = trimmed.split(/\s+/);
        if (parts.length < 3) continue;
        const pid = Number(parts[0]);
        if (!Number.isFinite(pid)) continue;
        out.set(pid, {
            rssBytes: Number(parts[1]) || 0,
            cpuMs: Number(parts[2]) || 0,
        });
    }
    return out;
}

/** Parse the `Get-CimInstance ... | ConvertTo-Json` output into pid → ppid. */
export function parseWindowsParents(json: string): Map<number, number> {
    const out = new Map<number, number>();
    const trimmed = json.trim();
    if (!trimmed) return out;
    let parsed: unknown;
    try {
        parsed = JSON.parse(trimmed);
    } catch {
        return out;
    }
    const rows = Array.isArray(parsed) ? parsed : [parsed];
    for (const row of rows) {
        if (typeof row !== "object" || row === null) continue;
        const r = row as Record<string, unknown>;
        const pid = Number(r.ProcessId);
        if (!Number.isFinite(pid)) continue;
        out.set(pid, Number(r.ParentProcessId) || 0);
    }
    return out;
}

class WindowsProbe implements ProcessProbe {
    private proc: ChildProcess | null = null;
    private buffer = "";
    private queue: Array<(payload: string) => void> = [];
    private failed = false;
    private warmed = false;

    async warmup(): Promise<void> {
        if (this.warmed) return;
        this.warmed = true;
        // Force the persistent PowerShell to spawn and JIT its first query now,
        // so the first sample of a (possibly short-lived) run isn't paid then.
        await this.sample([process.pid]);
    }

    private ensure(): ChildProcess | null {
        if (this.failed) return null;
        if (this.proc) return this.proc;
        try {
            const encoded = Buffer.from(WIN_SAMPLE_DRIVER, "utf16le").toString(
                "base64",
            );
            const proc = spawn(
                "powershell.exe",
                [
                    "-NoProfile",
                    "-NonInteractive",
                    "-NoLogo",
                    "-EncodedCommand",
                    encoded,
                ],
                { windowsHide: true, stdio: ["pipe", "pipe", "ignore"] },
            );
            proc.stdout?.setEncoding("utf8");
            proc.stdout?.on("data", (chunk: string) => this.onData(chunk));
            proc.once("error", () => this.fail());
            proc.once("exit", () => this.fail());
            this.proc = proc;
            return proc;
        } catch {
            this.failed = true;
            return null;
        }
    }

    private fail(): void {
        this.proc = null;
        for (const resolve of this.queue.splice(0)) resolve("");
    }

    private onData(chunk: string): void {
        this.buffer += chunk;
        let idx = this.buffer.indexOf(WIN_EOF);
        while (idx !== -1) {
            const payload = this.buffer.slice(0, idx);
            this.buffer = this.buffer.slice(idx + WIN_EOF.length);
            this.queue.shift()?.(payload);
            idx = this.buffer.indexOf(WIN_EOF);
        }
    }

    async sample(pids: number[]): Promise<Map<number, ProcStat>> {
        if (pids.length === 0) return new Map();
        const proc = this.ensure();
        if (!proc?.stdin) return new Map();
        const payload = await new Promise<string>((resolve) => {
            this.queue.push(resolve);
            proc.stdin?.write(`${pids.join(",")}\n`, (err) => {
                if (err) {
                    const i = this.queue.indexOf(resolve);
                    if (i !== -1) this.queue.splice(i, 1);
                    resolve("");
                }
            });
        });
        return parseWindowsSample(payload);
    }

    async parents(): Promise<Map<number, number>> {
        // One-shot CIM query (slow, ~1s incl. startup) on a *separate* process
        // so it never blocks the persistent fast sampler above.
        try {
            const { stdout } = await execFileAsync(
                "powershell.exe",
                [
                    "-NoProfile",
                    "-NonInteractive",
                    "-NoLogo",
                    "-Command",
                    "Get-CimInstance Win32_Process | Select-Object " +
                        "ProcessId,ParentProcessId | ConvertTo-Json -Compress",
                ],
                { windowsHide: true, maxBuffer: 16 * 1024 * 1024 },
            );
            return parseWindowsParents(stdout);
        } catch {
            return new Map();
        }
    }

    async dispose(): Promise<void> {
        const proc = this.proc;
        this.proc = null;
        this.failed = true;
        if (!proc) return;
        try {
            proc.stdin?.end("q\n");
        } catch {
            // ignore
        }
        proc.kill();
    }
}

// --- Linux ------------------------------------------------------------------

// Clock ticks per second (_SC_CLK_TCK); effectively always 100 on Linux.
const CLK_TCK = 100;
// Page size in bytes; 4 KiB on the platforms this harness targets.
const PAGE_SIZE = 4096;

interface ProcStatFull extends ProcStat {
    ppid: number;
}

/** Parse one `/proc/<pid>/stat` line into ppid + resource stats. */
export function parseProcStat(content: string): ProcStatFull | undefined {
    // The `comm` field (2nd) is wrapped in parens and may itself contain
    // spaces or parens, so split on the *last* ')' to find the stable tail.
    const rparen = content.lastIndexOf(")");
    if (rparen === -1) return undefined;
    const rest = content
        .slice(rparen + 1)
        .trim()
        .split(/\s+/);
    // rest[0] is field 3 (state). Field 4 (ppid) => rest[1],
    // field 14 (utime) => rest[11], field 15 (stime) => rest[12],
    // field 24 (rss, in pages) => rest[21].
    const ppid = Number(rest[1]);
    const utime = Number(rest[11]);
    const stime = Number(rest[12]);
    const rssPages = Number(rest[21]);
    if (!Number.isFinite(utime) || !Number.isFinite(stime)) return undefined;
    return {
        ppid: Number.isFinite(ppid) ? ppid : 0,
        rssBytes: (Number.isFinite(rssPages) ? rssPages : 0) * PAGE_SIZE,
        cpuMs: ((utime + stime) / CLK_TCK) * 1000,
    };
}

class LinuxProbe implements ProcessProbe {
    async warmup(): Promise<void> {}

    async sample(pids: number[]): Promise<Map<number, ProcStat>> {
        const out = new Map<number, ProcStat>();
        await Promise.all(
            pids.map(async (pid) => {
                try {
                    const content = await readFile(`/proc/${pid}/stat`, "utf8");
                    const info = parseProcStat(content);
                    if (info) {
                        out.set(pid, {
                            rssBytes: info.rssBytes,
                            cpuMs: info.cpuMs,
                        });
                    }
                } catch {
                    // PID gone; skip.
                }
            }),
        );
        return out;
    }

    async parents(): Promise<Map<number, number>> {
        const out = new Map<number, number>();
        let entries: string[];
        try {
            entries = await readdir("/proc");
        } catch {
            return out;
        }
        await Promise.all(
            entries.map(async (name) => {
                if (!/^\d+$/.test(name)) return;
                try {
                    const content = await readFile(
                        `/proc/${name}/stat`,
                        "utf8",
                    );
                    const info = parseProcStat(content);
                    if (info) out.set(Number(name), info.ppid);
                } catch {
                    // PID gone; skip.
                }
            }),
        );
        return out;
    }

    async dispose(): Promise<void> {}
}

// --- macOS / other POSIX ----------------------------------------------------

/** Parse a `ps` cumulative CPU time string (`[[dd-]hh:]mm:ss[.frac]`) to ms. */
export function parsePosixCpuTime(value: string): number {
    let rest = value.trim();
    if (!rest) return 0;
    let days = 0;
    const dash = rest.indexOf("-");
    if (dash !== -1) {
        days = Number(rest.slice(0, dash));
        rest = rest.slice(dash + 1);
    }
    // Fold the colon-separated components (ss, mm:ss, or hh:mm:ss) into seconds.
    let seconds = 0;
    for (const part of rest.split(":")) {
        seconds = seconds * 60 + (Number(part) || 0);
    }
    return ((Number.isFinite(days) ? days : 0) * 86_400 + seconds) * 1000;
}

/** Parse `ps -o pid=,rss=,time=` output into stats. */
export function parsePosixSample(stdout: string): Map<number, ProcStat> {
    const out = new Map<number, ProcStat>();
    for (const line of stdout.split("\n")) {
        const trimmed = line.trim();
        if (!trimmed) continue;
        const parts = trimmed.split(/\s+/);
        if (parts.length < 3) continue;
        const pid = Number(parts[0]);
        if (!Number.isFinite(pid)) continue;
        out.set(pid, {
            // `rss` is reported in kilobytes.
            rssBytes: (Number(parts[1]) || 0) * 1024,
            cpuMs: parsePosixCpuTime(parts[2] ?? ""),
        });
    }
    return out;
}

/** Parse `ps -axo pid=,ppid=` output into a pid → ppid map. */
export function parsePosixParents(stdout: string): Map<number, number> {
    const out = new Map<number, number>();
    for (const line of stdout.split("\n")) {
        const trimmed = line.trim();
        if (!trimmed) continue;
        const parts = trimmed.split(/\s+/);
        if (parts.length < 2) continue;
        const pid = Number(parts[0]);
        if (!Number.isFinite(pid)) continue;
        out.set(pid, Number(parts[1]) || 0);
    }
    return out;
}

class PosixProbe implements ProcessProbe {
    async warmup(): Promise<void> {}

    async sample(pids: number[]): Promise<Map<number, ProcStat>> {
        if (pids.length === 0) return new Map();
        try {
            const { stdout } = await execFileAsync("ps", [
                "-o",
                "pid=,rss=,time=",
                "-p",
                pids.join(","),
            ]);
            return parsePosixSample(stdout);
        } catch {
            return new Map();
        }
    }

    async parents(): Promise<Map<number, number>> {
        try {
            const { stdout } = await execFileAsync(
                "ps",
                ["-axo", "pid=,ppid="],
                { maxBuffer: 16 * 1024 * 1024 },
            );
            return parsePosixParents(stdout);
        } catch {
            return new Map();
        }
    }

    async dispose(): Promise<void> {}
}
