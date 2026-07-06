/** Base environment applied to every benchmarked process (timed or measured). */
export const BASE_ENV = {
    FORCE_COLOR: "0",
    TURBO_TELEMETRY_DISABLED: "1",
    DO_NOT_TRACK: "1",
    NX_TUI: "false",
} as const;
