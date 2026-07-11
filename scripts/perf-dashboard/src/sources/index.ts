export type { GitHubConfig } from "./github";
export { GitHubDataSource } from "./github";
export type { LocalFsConfig } from "./local-fs";
export { LocalFsDataSource } from "./local-fs";
export {
    isKnownTarget,
    KNOWN_TARGETS,
    osArchToTarget,
    targetToOs,
} from "./target";
export type {
    DataSource,
    DataSourceDescriptor,
    RunQuery,
    RunRef,
    TargetId,
} from "./types";
