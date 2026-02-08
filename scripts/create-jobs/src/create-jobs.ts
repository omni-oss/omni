import type { Meta, TaskResult, TaskResultArray } from "./schemas";
import path from "node:path"

export type Job = {
    task_name: string;
    project_name: string;
    output_files: string[];
    project_dir: string;
    meta: Meta;
};

export type Jobs = {
    test: TestJobs;
    build: BuildJobs;
    publish: PublishJobs;
};

export type TestJobs = {
    rust: Job[];
    typescript: Job[];
};

export type BuildJobs = {
    rust: Job[];
    typescript: Job[];
};

export type PublishJobs = {
    npm: Job[];
    rust_github: Job[];
};

export function createJobs(results: TaskResultArray): Jobs {
    const jobs: Jobs = {
        test: {
            rust: [],
            typescript: [],
        },
        build: {
            rust: [],
            typescript: [],
        },
        publish: {
            npm: [],
            rust_github: [],
        },
    };

    for (const result of results) {
        if (result.status === "skipped") {
            continue;
        }

        const task = result.task;
        if (task.task_name === "test") {
            if (result.details.meta?.language === "rust") {
                jobs.test.rust.push(jobFromResult(result));
            }

            if (result.details.meta?.language === "typescript") {
                jobs.test.typescript.push(jobFromResult(result));
            }
        }

        if (task.task_name === "build") {
            if (result.details.meta?.language === "rust") {
                jobs.build.rust.push(jobFromResult(result));
            }

            if (result.details.meta?.language === "typescript") {
                jobs.build.typescript.push(jobFromResult(result));
            }
        }

        if (task.task_name === "publish" && result.details.meta?.release?.npm) {
            jobs.publish.npm.push(jobFromResult(result));
        }

        if (
            result.details.meta?.release?.github &&
            result.details.meta?.language === "rust"
        ) {
            jobs.publish.rust_github.push(jobFromResult(result));
        }
    }

    return jobs;
}

function jobFromResult(result: TaskResult): Job {
    return {
        task_name: result.task.task_name,
        project_name: result.task.project_name,
        output_files: result.details.output_files?.map((file) => path.resolve(result.task.project_dir, file)) ?? [],
        project_dir: result.task.project_dir,
        meta: result.details.meta ?? {},
    };
}
