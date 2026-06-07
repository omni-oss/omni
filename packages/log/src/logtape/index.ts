import { type Config, configure, configureSync } from "@logtape/logtape";
import type { CategoryParam } from "@/core";
import { Log } from "@/log";
import { LogTapeLoggerFactory } from "./logtape-logger";

export * from "@logtape/logtape";

export async function withLogTapeRoot<
    TReturn,
    TSinkId extends string,
    TFilterId extends string,
>(
    rootCategory: CategoryParam,
    config: Config<TSinkId, TFilterId>,
    fn: () => Promise<TReturn>,
): Promise<TReturn> {
    await configure(config);
    const factory = new LogTapeLoggerFactory();
    return await Log.withRoot(factory, rootCategory, fn);
}

export function withLogTapeRootSync<
    TReturn,
    TSinkId extends string,
    TFilterId extends string,
>(
    rootCategory: CategoryParam,
    config: Config<TSinkId, TFilterId>,
    fn: () => TReturn,
): TReturn {
    configureSync(config);
    const factory = new LogTapeLoggerFactory();
    return Log.withRoot(factory, rootCategory, fn);
}
