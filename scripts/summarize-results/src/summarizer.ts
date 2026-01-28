import type { Meta, TaskResult, TaskResultArray } from "./schemas";

export type SummaryData = {
    total: number;
    skipped: number;
    errored: number;
    completed: number;
    completed_with_error: number;
    completed_with_success: number;
    completed_with_cache_hit: number;
    completed_with_cache_hit_error: number;
    completed_with_cache_hit_success: number;
};

export type Summary = SummaryData & {
    aggregated_by_metadata: {
        [key: string]: SummaryData;
    };
    aggregated_by_project: {
        [key: string]: SummaryData;
    };
    aggregated_by_task: {
        [key: string]: SummaryData;
    };
    projects: {
        [key: string]: {
            tasks: {
                [key: string]: {
                    execute: boolean;
                    meta: Meta;
                };
            };
        };
    };
};

function initSummaryData(total: number): SummaryData {
    return {
        total,
        skipped: 0,
        errored: 0,
        completed: 0,
        completed_with_error: 0,
        completed_with_success: 0,
        completed_with_cache_hit: 0,
        completed_with_cache_hit_error: 0,
        completed_with_cache_hit_success: 0,
    };
}

export function summarize(results: TaskResultArray): Summary {
    const summary: Summary = {
        ...initSummaryData(results.length),
        aggregated_by_metadata: {},
        aggregated_by_project: {},
        aggregated_by_task: {},
        projects: {},
    };

    for (const result of results) {
        if (!summary.projects[result.task.project_name]) {
            summary.projects[result.task.project_name] = {
                tasks: {},
            };
        }

        // biome-ignore lint/style/noNonNullAssertion: expected
        summary.projects[result.task.project_name]!.tasks[
            result.task.task_name
        ] = {
            execute: result.status === "completed" && !result.cache_hit,
            meta: result.details.meta,
        };

        applyResultToSummary(result, summary);

        const metas = flattenMetadata(result.details.meta);

        for (const metadata of metas) {
            if (!summary.aggregated_by_metadata[metadata]) {
                summary.aggregated_by_metadata[metadata] = initSummaryData(0);
            }

            applyResultToSummary(
                result,
                summary.aggregated_by_metadata[metadata],
            );
        }

        if (!summary.aggregated_by_project[result.task.project_name]) {
            summary.aggregated_by_project[result.task.project_name] =
                initSummaryData(0);
        }

        applyResultToSummary(
            result,
            // biome-ignore lint/style/noNonNullAssertion: should be assigned at this point
            summary.aggregated_by_project[result.task.project_name]!,
        );

        if (!summary.aggregated_by_task[result.task.task_name]) {
            summary.aggregated_by_task[result.task.task_name] =
                initSummaryData(0);
        }

        applyResultToSummary(
            result,
            // biome-ignore lint/style/noNonNullAssertion: should be assigned at this point
            summary.aggregated_by_task[result.task.task_name]!,
        );
    }

    for (const k in summary.aggregated_by_metadata) {
        const value = summary.aggregated_by_metadata[k];
        if (value) {
            value.total = value.skipped + value.errored + value.completed;
        }
    }

    for (const k in summary.aggregated_by_project) {
        const value = summary.aggregated_by_project[k];
        if (value) {
            value.total = value.skipped + value.errored + value.completed;
        }
    }

    for (const k in summary.aggregated_by_task) {
        const value = summary.aggregated_by_task[k];
        if (value) {
            value.total = value.skipped + value.errored + value.completed;
        }
    }

    return summary;
}

function flattenMetadata(meta: Meta): string[] {
    const metadatas: string[] = [];

    if (meta.type) {
        metadatas.push(`type:${meta.type}`);
    }

    if (meta.language) {
        metadatas.push(`language:${meta.language}`);
    }

    if (meta.publish) {
        metadatas.push(`publish:${meta.publish}`);
    }

    return metadatas;
}

function applyResultToSummary(result: TaskResult, summaryData: SummaryData) {
    switch (result.status) {
        case "skipped":
            summaryData.skipped++;
            break;
        case "errored":
            summaryData.errored++;
            break;
        case "completed":
            summaryData.completed++;

            if (result.exit_code === 0) {
                summaryData.completed_with_success++;
            } else {
                summaryData.completed_with_error++;
            }

            if (result.cache_hit) {
                summaryData.completed_with_cache_hit++;

                if (result.exit_code === 0) {
                    summaryData.completed_with_cache_hit_success++;
                } else {
                    summaryData.completed_with_cache_hit_error++;
                }
            }

            break;
    }
}
