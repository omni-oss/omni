import path from "node:path";
import type { Meta, TaskResult, TaskResultArray } from "./schemas";

export type Job = {
    task_name: string;
    project_name: string;
    project_dir: string;
    artifacts: {
        workspace: {
            files: string[];
        };
        project: {
            files: string[];
        };
    };
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
    const workspaceArtifacts = [] as string[];
    const projectArtifacts = [] as string[];

    if (result.details.output_files && result.details.output_files.length > 0) {
        for (const file of result.details.output_files) {
            const fullPath = path.resolve(result.task.project_dir, file);

            if (isPathInside(result.task.project_dir, fullPath)) {
                projectArtifacts.push(fullPath);
            } else {
                workspaceArtifacts.push(fullPath);
            }
        }
    }

    return {
        task_name: result.task.task_name,
        project_name: result.task.project_name,
        artifacts: {
            project: {
                files: projectArtifacts,
            },
            workspace: {
                files: workspaceArtifacts,
            },
        },
        project_dir: result.task.project_dir,
        meta: result.details.meta ?? {},
    };
}

/**
 * Checks if 'child' is physically inside 'parent'
 * @param {string} parent - The potential parent directory
 * @param {string} child - The path to check
 */
function isPathInside(parent: string, child: string) {
    const relative = path.relative(parent, child);

    // path.relative returns an empty string if paths are the same.
    // If the path starts with '..' (or the platform equivalent),
    // it means the child is outside the parent.
    return relative && !relative.startsWith("..") && !path.isAbsolute(relative);
}
