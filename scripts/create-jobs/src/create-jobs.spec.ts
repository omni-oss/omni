/** biome-ignore-all lint/suspicious/noExplicitAny: test file */
import { describe, expect, it } from "vitest";
import { createJobs } from "./create-jobs"; // Update with actual path

describe("createJobs", () => {
    it("should return an empty structure when given an empty array", () => {
        const results: any[] = [];
        const jobs = createJobs(results);

        expect(jobs.test.rust).toHaveLength(0);
        expect(jobs.build.typescript).toHaveLength(0);
        expect(jobs.publish.npm).toHaveLength(0);
    });

    it('should skip tasks with status "skipped"', () => {
        const results: any[] = [
            {
                status: "skipped",
                task: {
                    task_name: "test",
                    project_name: "p1",
                    project_dir: "/mnt/c/Users/user/project",
                },
                details: { meta: { language: "rust" } },
            },
        ];
        const jobs = createJobs(results);
        expect(jobs.test.rust).toHaveLength(0);
    });

    it("should categorize test and build tasks by language", () => {
        const results: any[] = [
            {
                status: "success",
                task: {
                    task_name: "test",
                    project_name: "rust-app",
                    project_dir: "/mnt/c/Users/user/project",
                },
                details: {
                    meta: { language: "rust" },
                    output_files: ["binary"],
                },
            },
            {
                status: "success",
                task: {
                    task_name: "build",
                    project_name: "ts-lib",
                    project_dir: "/mnt/c/Users/user/project",
                },
                details: { meta: { language: "typescript" } },
            },
        ];

        const jobs = createJobs(results, "/mnt/c/Users/user");

        expect(jobs.test.rust[0]).toMatchObject({
            project_name: "rust-app",
            task_name: "test",
            artifacts: {
                project: {
                    files: ["binary"],
                },
            },
        });
        expect(jobs.build.typescript[0]?.project_name).toBe("ts-lib");
        expect(jobs.test.typescript).toHaveLength(0);
    });

    it("should handle publish tasks correctly (npm vs rust_github)", () => {
        const results: any[] = [
            {
                status: "success",
                task: {
                    task_name: "publish",
                    project_name: "js-pkg",
                    project_dir: "/mnt/c/Users/user/project",
                },
                details: {
                    meta: {
                        language: "typescript",
                        release: { npm: true },
                    },
                },
            },
            {
                status: "success",
                task: {
                    task_name: "publish",
                    project_name: "rust-pkg",
                    project_dir: "/mnt/c/Users/user/project",
                },
                details: {
                    meta: {
                        language: "rust",
                        release: { github: true },
                    },
                },
            },
        ];

        const jobs = createJobs(results);

        // Checks NPM logic: task_name must be "publish" AND meta.release.npm must be true
        expect(jobs.publish.npm).toHaveLength(1);
        expect(jobs.publish.npm[0]?.project_name).toBe("js-pkg");

        // Checks GitHub logic: meta.release.github must be true AND language must be "rust" AND task_name must be "publish"
        expect(jobs.publish.rust_github).toHaveLength(1);
        expect(jobs.publish.rust_github[0]?.project_name).toBe("rust-pkg");
    });

    it("should provide default values for missing meta or output_files", () => {
        const results: any[] = [
            {
                status: "success",
                task: {
                    task_name: "test",
                    project_name: "minimal",
                    project_dir: "/mnt/c/Users/user/project",
                },
                details: {}, // Missing meta and output_files
            },
        ];

        // This won't be added to test.rust because meta.language is missing
        const jobs = createJobs(results);
        expect(jobs.test.rust).toHaveLength(0);
    });

    it("should resolve the relative output files to the project directory", () => {
        const results: any[] = [
            {
                status: "success",
                task: {
                    task_name: "test",
                    project_name: "minimal",
                    project_dir: "/mnt/c/Users/user/project",
                },
                details: {
                    meta: { language: "rust" },
                    output_files: ["target/debug/minimal"],
                },
            },
        ];

        const jobs = createJobs(results, "/mnt/c/Users/usert");

        expect(jobs.test.rust[0]).toMatchObject({
            project_name: "minimal",
            task_name: "test",
            artifacts: {
                project: {
                    files: ["target/debug/minimal"],
                },
            },
        });
    });

    it("should assign output files not in the project directory to the workspace artifacts", () => {
        const results: any[] = [
            {
                status: "success",
                task: {
                    task_name: "test",
                    project_name: "minimal",
                    project_dir: "/mnt/c/Users/user/project",
                },
                details: {
                    meta: { language: "rust" },
                    output_files: ["/mnt/c/Users/user/target/debug/minimal"],
                },
            },
        ];

        const jobs = createJobs(results, "/mnt/c/Users/user");

        expect(jobs.test.rust[0]).toMatchObject({
            project_name: "minimal",
            task_name: "test",
            artifacts: {
                workspace: {
                    files: ["target/debug/minimal"],
                },
            },
        });
    });

    it("should sanitize the project name and task name in artifact names", () => {
        const results: any[] = [
            {
                status: "success",
                task: {
                    task_name: "test",
                    project_name: "@project/name",
                    project_dir: "/mnt/c/Users/user/project",
                },
                details: {
                    meta: { language: "rust" },
                    output_files: [],
                },
            },
        ];

        const jobs = createJobs(results);

        expect(jobs.test.rust[0]).toMatchObject({
            project_name: "@project/name",
            task_name: "test",
            artifacts: {
                project: {
                    name: "project-%40project%2Fname__test",
                    files: [],
                },
                workspace: {
                    name: "workspace-%40project%2Fname__test",
                    files: [],
                },
            },
        });
    });

    it("should handle is_publish_task correctly", () => {
        const results: any[] = [
            {
                status: "success",
                task: {
                    task_name: "publish",
                    project_name: "js-pkg",
                    project_dir: "/mnt/c/Users/user/project",
                },
                details: {
                    meta: {
                        language: "typescript",
                        release: { npm: true },
                        is_publish_task: true,
                    },
                },
            },
            {
                status: "success",
                task: {
                    task_name: "any-task",
                    project_name: "rust-pkg",
                    project_dir: "/mnt/c/Users/user/project",
                },
                details: {
                    meta: {
                        language: "rust",
                        release: { github: true },
                        is_publish_task: true,
                    },
                },
            },
        ];

        const jobs = createJobs(results);

        // Checks NPM logic: task_name must be "publish" AND meta.release.npm must be true
        expect(jobs.publish.npm).toHaveLength(1);
        expect(jobs.publish.npm[0]?.project_name).toBe("js-pkg");

        // Checks GitHub logic: meta.release.github must be true AND language must be "rust"
        // Note that your code doesn't strictly check if task_name === "publish" for rust_github!
        expect(jobs.publish.rust_github).toHaveLength(1);
        expect(jobs.publish.rust_github[0]?.project_name).toBe("rust-pkg");
    });

    it("should handle is_build_task correctly", () => {
        const results: any[] = [
            {
                status: "success",
                task: {
                    task_name: "build",
                    project_name: "ts-lib",
                    project_dir: "/mnt/c/Users/user/project",
                },
                details: {
                    meta: { language: "typescript", is_build_task: true },
                },
            },
            {
                status: "success",
                task: {
                    task_name: "any-task",
                    project_name: "rust-pkg",
                    project_dir: "/mnt/c/Users/user/project",
                },
                details: {
                    meta: {
                        language: "rust",
                        is_build_task: true,
                    },
                },
            },
        ];

        const jobs = createJobs(results);

        // Checks build logic: task_name must be "build" AND meta.language must be "typescript"
        expect(jobs.build.typescript).toHaveLength(1);
        expect(jobs.build.typescript[0]?.project_name).toBe("ts-lib");

        // Checks build logic: task_name must be "any-task" AND meta.language must be "rust"
        expect(jobs.build.rust).toHaveLength(1);
        expect(jobs.build.rust[0]?.project_name).toBe("rust-pkg");
    });

    it("should handle is_test_task correctly", () => {
        const results: any[] = [
            {
                status: "success",
                task: {
                    task_name: "test",
                    project_name: "rust-app",
                    project_dir: "/mnt/c/Users/user/project",
                },
                details: {
                    meta: { language: "rust", is_test_task: true },
                },
            },
            {
                status: "success",
                task: {
                    task_name: "test",
                    project_name: "rust-pkg",
                    project_dir: "/mnt/c/Users/user/project",
                },
                details: {
                    meta: {
                        language: "rust",
                        is_test_task: true,
                    },
                },
            },
        ];

        const jobs = createJobs(results);

        // Checks test logic: task_name must be "test" AND meta.language must be "rust"
        expect(jobs.test.rust).toHaveLength(2);
        expect(jobs.test.rust[0]?.project_name).toBe("rust-app");
        expect(jobs.test.rust[1]?.project_name).toBe("rust-pkg");
    });

    it("should handle generic publish tasks correctly", () => {
        const results: any[] = [
            {
                status: "success",
                task: {
                    task_name: "publish",
                    project_name: "rust-pkg",
                    project_dir: "/mnt/c/Users/user/project",
                },
                details: {
                    meta: {
                        release: { github: true },
                        is_publish_task: true,
                    },
                },
            },
        ];

        const jobs = createJobs(results);

        // Checks generic logic: task_name must be "publish" AND meta.release.github must be true
        expect(jobs.publish.generic).toHaveLength(1);
        expect(jobs.publish.generic[0]?.project_name).toBe("rust-pkg");
    });
});
