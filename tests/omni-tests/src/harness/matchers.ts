/**
 * Custom Vitest matchers for asserting on {@link OmniResult} values.
 *
 * Registered globally via the harness setup file (see `setup.ts`). They keep
 * e2e assertions terse and produce readable diffs that include the failing
 * command plus its stdout/stderr.
 *
 * @example
 * const result = await runOmni(["project", "list"], { cwd: ws.cwd });
 * expect(result).toSucceed();
 * expect(result).toOutputContaining("app");
 */

import { expect } from "vitest";
import type { OmniResult } from "./run-omni";

function isOmniResult(value: unknown): value is OmniResult {
    return (
        typeof value === "object" &&
        value !== null &&
        "exitCode" in value &&
        "stdout" in value &&
        "stderr" in value
    );
}

function describeResult(result: OmniResult): string {
    return [
        `command: ${result.command}`,
        `exitCode: ${result.exitCode}`,
        `stdout:\n${result.stdout}`,
        `stderr:\n${result.stderr}`,
    ].join("\n");
}

function guard(received: unknown): OmniResult {
    if (!isOmniResult(received)) {
        throw new TypeError(
            "Expected an OmniResult (the value returned by runOmni).",
        );
    }
    return received;
}

expect.extend({
    toHaveSucceeded(received: unknown) {
        const result = guard(received);
        return {
            pass: result.exitCode === 0,
            message: () =>
                `expected omni to exit 0, but got ${result.exitCode}\n${describeResult(result)}`,
        };
    },

    toHaveFailed(received: unknown) {
        const result = guard(received);
        return {
            pass: result.exitCode !== 0,
            message: () =>
                `expected omni to exit non-zero, but it succeeded\n${describeResult(result)}`,
        };
    },

    toHaveExitCode(received: unknown, expected: number) {
        const result = guard(received);
        return {
            pass: result.exitCode === expected,
            message: () =>
                `expected exit code ${expected}, but got ${result.exitCode}\n${describeResult(result)}`,
        };
    },

    toOutput(received: unknown, expected: string) {
        const result = guard(received);
        const expectedNorm = expected
            .replace(/\r\n?/g, "\n")
            .replace(/\n+$/, "");
        return {
            pass: result.out === expectedNorm,
            actual: result.out,
            expected: expectedNorm,
            message: () =>
                `expected normalized stdout to equal the expected text\n${describeResult(result)}`,
        };
    },

    toOutputContaining(received: unknown, substring: string) {
        const result = guard(received);
        return {
            pass: result.stdout.includes(substring),
            message: () =>
                `expected stdout to contain ${JSON.stringify(substring)}\n${describeResult(result)}`,
        };
    },

    toMatchOutput(received: unknown, pattern: RegExp) {
        const result = guard(received);
        return {
            pass: pattern.test(result.stdout),
            message: () =>
                `expected stdout to match ${pattern}\n${describeResult(result)}`,
        };
    },

    toHaveStderrContaining(received: unknown, substring: string) {
        const result = guard(received);
        return {
            pass: result.stderr.includes(substring),
            message: () =>
                `expected stderr to contain ${JSON.stringify(substring)}\n${describeResult(result)}`,
        };
    },
});

interface OmniMatchers<R = unknown> {
    /** Asserts the process exited 0. */
    toHaveSucceeded(): R;
    /** Asserts the process exited non-zero. */
    toHaveFailed(): R;
    /** Asserts a specific exit code. */
    toHaveExitCode(code: number): R;
    /** Asserts normalized stdout equals the (normalized) expected text. */
    toOutput(expected: string): R;
    /** Asserts raw stdout contains the given substring. */
    toOutputContaining(substring: string): R;
    /** Asserts raw stdout matches the given pattern. */
    toMatchOutput(pattern: RegExp): R;
    /** Asserts raw stderr contains the given substring. */
    toHaveStderrContaining(substring: string): R;
}

declare module "vitest" {
    // biome-ignore lint/suspicious/noExplicitAny: matches Vitest's own signature
    interface Matchers<T = any> extends OmniMatchers<T> {}
}
