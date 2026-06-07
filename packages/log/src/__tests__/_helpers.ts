import {
    type Config,
    configure,
    type LogRecord,
    type LogLevel as LogTapeLogLevel,
} from "@logtape/logtape";
import { LogTapeLoggerFactory } from "../logtape/logtape-logger";

// ---------------------------------------------------------------------------
// Capturer
// ---------------------------------------------------------------------------

/**
 * A simple in-memory sink that records every `LogRecord` it receives.
 * Use with `configure` (or the `setupCapture` helper below) to assert on
 * what the integration ultimately produces.
 */
export interface Capturer {
    records: LogRecord[];
    sink: (record: LogRecord) => void;
    clear: () => void;
}

export function createCapturer(): Capturer {
    const records: LogRecord[] = [];
    return {
        records,
        sink: (record) => {
            records.push(record);
        },
        clear: () => {
            records.length = 0;
        },
    };
}

// ---------------------------------------------------------------------------
// Record assertions
// ---------------------------------------------------------------------------

/**
 * Pulls a record at the given index from a capturer, throwing if it is
 * missing. Used to keep test bodies free of non-null assertions while still
 * giving TypeScript a narrowed `LogRecord` to work with.
 */
export function recordAt(cap: Capturer, index = 0): LogRecord {
    const r = cap.records[index];
    if (!r) {
        throw new Error(
            `Expected a record at index ${index}, but only ${cap.records.length} were captured`,
        );
    }
    return r;
}

// ---------------------------------------------------------------------------
// Shared factory
// ---------------------------------------------------------------------------

export const factory = new LogTapeLoggerFactory();

// ---------------------------------------------------------------------------
// Quick logtape config presets
// ---------------------------------------------------------------------------

/**
 * Build a `Config` that drains a single `["app"]` category into the given
 * capturer. Silences logtape's internal meta logger so its noise doesn't
 * land in tests.
 *
 * Returning the config (rather than applying it) lets tests pass it
 * straight into helpers like {@link withLogTapeRoot} or
 * {@link withLogTapeRootSync}, while {@link setupCapture} keeps the
 * apply-it-now convenience for tests that just want a configured logtape.
 */
export function captureConfig(
    cap: Capturer,
    lowestLevel: LogTapeLogLevel = "trace",
): Config<"capture", never> {
    return {
        reset: true,
        sinks: { capture: cap.sink },
        loggers: [
            { category: ["app"], sinks: ["capture"], lowestLevel },
            {
                category: ["logtape", "meta"],
                sinks: [],
                lowestLevel: "warning",
            },
        ],
    };
}

/**
 * Wires up a single `["app"]` category logger that drains into the given
 * capturer. Silences logtape's internal meta logger to avoid noise.
 *
 * For more elaborate setups (multiple categories, parentSinks: 'override',
 * etc.), call `configure` directly in the test.
 */
export async function setupCapture(
    cap: Capturer,
    lowestLevel: LogTapeLogLevel = "trace",
): Promise<void> {
    await configure(captureConfig(cap, lowestLevel));
}
