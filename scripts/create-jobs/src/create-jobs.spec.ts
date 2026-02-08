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

        const jobs = createJobs(results);

        expect(jobs.test.rust[0]).toMatchObject({
            project_name: "rust-app",
            task_name: "test",
            artifacts: {
                project: {
                    files: ["/mnt/c/Users/user/project/binary"],
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
                details: { meta: { release: { npm: true } } },
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
                    },
                },
            },
        ];

        const jobs = createJobs(results);

        // Checks NPM logic: task_name must be "publish" AND meta.release.npm must be true
        expect(jobs.publish.npm).toHaveLength(1);
        expect(jobs.publish.npm[0]?.project_name).toBe("js-pkg");

        // Checks GitHub logic: meta.release.github must be true AND language must be "rust"
        // Note: your code doesn't strictly check if task_name === "publish" for rust_github!
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

        const jobs = createJobs(results);

        expect(jobs.test.rust[0]).toMatchObject({
            project_name: "minimal",
            task_name: "test",
            artifacts: {
                project: {
                    files: ["/mnt/c/Users/user/project/target/debug/minimal"],
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

        const jobs = createJobs(results);

        expect(jobs.test.rust[0]).toMatchObject({
            project_name: "minimal",
            task_name: "test",
            artifacts: {
                workspace: {
                    files: ["/mnt/c/Users/user/target/debug/minimal"],
                },
            },
        });
    });
});
