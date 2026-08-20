/**
 * Process-global registry of async log-sink drains.
 *
 * LogTape dispatches a {@link https://logtape.org LogRecord} into each sink
 * synchronously and **never awaits an async sink** — a sink that forwards
 * records over a transport (e.g. the bridge-service `/log` RPC sink) is
 * therefore inherently fire-and-forget. Any record still in flight when the
 * process or its transport tears down is silently lost, which surfaces as
 * intermittently-missing log lines.
 *
 * To close that gap deterministically, a sink that owns pending async work
 * registers a *drain* here (via {@link registerLogDrain}); call sites that
 * must guarantee delivery await {@link flushLogs} at a point where the
 * underlying transport is still alive (e.g. just before a service writes its
 * response, or before the process exits).
 *
 * The registry is intentionally process-global, mirroring LogTape's own
 * process-global configuration: a drain registered when the logging pipeline
 * is configured stays valid for every subsequent flush.
 */

/**
 * Drains a sink's outstanding async work. Resolving means every record
 * dispatched to that sink *before the call* has been fully delivered. It must
 * be safe to invoke repeatedly, and it must not reject (a drain reports its
 * own delivery failures through its own channel).
 */
export type LogDrain = () => Promise<void>;

const drains = new Set<LogDrain>();

/**
 * Register a {@link LogDrain} to be awaited by {@link flushLogs}.
 *
 * @returns A disposer that removes the drain from the registry. Call it when
 *   the owning sink is torn down so a stale drain is not awaited forever.
 */
export function registerLogDrain(drain: LogDrain): () => void {
    drains.add(drain);
    return () => {
        drains.delete(drain);
    };
}

/**
 * Await every registered {@link LogDrain}, guaranteeing that all records
 * dispatched so far have been delivered.
 *
 * Individual drain rejections are swallowed: a failure to flush must never
 * turn into an unhandled rejection that takes down the caller (which is
 * typically mid-way through serving a request or shutting down).
 */
export async function flushLogs(): Promise<void> {
    await Promise.all([...drains].map((drain) => drain().catch(() => {})));
}
