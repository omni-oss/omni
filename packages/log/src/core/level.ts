export const LOG_LEVELS = ["error", "warn", "info", "debug", "trace"] as const;

export type LogLevel = (typeof LOG_LEVELS)[number];
