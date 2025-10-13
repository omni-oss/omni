import { z } from "zod";

// --- Nested Schemas ---

/**
 * Defines the structure for the duration object.
 */
const ElapsedSchema = z.object({
    secs: z
        .number()
        .int()
        .nonnegative()
        .describe("The number of whole seconds elapsed."),
    nanos: z
        .number()
        .int()
        .nonnegative()
        .describe("The number of nanoseconds elapsed."),
});

/**
 * Defines the metadata for the project/task details.
 */
const MetaSchema = z.object({
    type: z
        .enum(["lib", "service", "script", "app"])
        .or(z.string())
        .describe("The type of project (e.g., library, service, application)."),
    language: z
        .enum(["rust", "typescript"])
        .or(z.string())
        .describe("The primary language of the project."),
});

/**
 * Defines the details of the task that was run or skipped.
 */
const TaskSchema = z.object({
    task_name: z
        .string()
        .describe("The short name of the task (e.g., 'test', 'build')."),
    task_command: z.string().describe("The command executed for the task."),
    project_name: z.string().describe("The name of the project."),
    project_dir: z
        .string()
        .describe("The absolute directory path of the project."),
    full_task_name: z
        .string()
        .describe("The fully qualified task name (e.g., 'omni_utils#test')."),
    dependencies: z
        .array(z.string())
        .describe("A list of dependent task names."),
    enabled: z
        .boolean()
        .describe("Whether the task is enabled by configuration."),
    interactive: z.boolean().describe("Whether the task is interactive."),
    persistent: z.boolean().describe("Whether the task is persistent."),
});

// --- Discriminant Schemas (Union Members) ---

/**
 * Schema for a task that successfully completed.
 * Note: 'elapsed', 'exit_code', 'hash', and 'cache_hit' are required here.
 */
const CompletedTaskSchema = z.object({
    status: z.literal("completed"),
    hash: z
        .string()
        .describe("The task's content hash (Base64 encoded string). Used for caching."),
    task: TaskSchema,
    exit_code: z
        .number()
        .int()
        .describe(
            "The exit code of the executed command (typically 0 for success).",
        ),
    elapsed: ElapsedSchema.describe("The duration the task took to execute."),
    cache_hit: z
        .boolean()
        .describe("Indicates if the result was pulled from cache."),
    details: z
        .object({
            meta: MetaSchema,
        })
        .describe("Additional run details."),
});

const ErroredTaskSchema = z.object({
    status: z.literal("errored"),
    task: TaskSchema,
    error: z.string().describe("The error message."),
    details: z
        .object({
            meta: MetaSchema,
        })
        .describe("Additional run details."),
});

/**
 * Schema for a task that was skipped.
 * Note: 'skip_reason' is required here, and fields like 'hash' or 'elapsed' are omitted.
 */
const SkippedTaskSchema = z.object({
    status: z.literal("skipped"),
    task: TaskSchema,
    skip_reason: z
        .string()
        .describe("The reason the task was skipped (e.g., 'disabled')."),
    details: z
        .object({
            meta: MetaSchema,
        })
        .describe("Additional run details."),
});

// --- Root Schema ---

/**
 * The primary schema for a single task result, using a discriminated union
 * based on the 'status' field to correctly type the required fields.
 */
export const TaskResultSchema = z
    .discriminatedUnion("status", [
        CompletedTaskSchema,
        SkippedTaskSchema,
        ErroredTaskSchema,
    ])
    .describe(
        "Schema for a single task execution result (completed or skipped).",
    );

/**
 * The final schema for the root array of task results.
 */
export const TaskResultArraySchema = z
    .array(TaskResultSchema)
    .describe("An array of task execution results.");

// --- TypeScript Types (Inferred) ---

export type Elapsed = z.infer<typeof ElapsedSchema>;
export type Meta = z.infer<typeof MetaSchema>;
export type Task = z.infer<typeof TaskSchema>;
export type CompletedTaskResult = z.infer<typeof CompletedTaskSchema>;
export type SkippedTaskResult = z.infer<typeof SkippedTaskSchema>;
export type TaskResult = z.infer<typeof TaskResultSchema>;
export type TaskResultArray = z.infer<typeof TaskResultArraySchema>;
