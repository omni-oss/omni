import { describe, expect, it } from "vitest";
import type { Process } from "./proc-interfaces";

export type ProcessTestDeclarationsArgs = {
    name: string;
    proc: Process;
    currentDir: string;
    newCurrentDir: string;
    args: string[];
    env: Record<string, string | undefined>;
    skip?: boolean;
};

export function declareProcTests(args: ProcessTestDeclarationsArgs): void {
    describe.skipIf(args.skip ?? false)(`Process ${args.name}`, () => {
        it("currentDir should be equivalent to process.cwd()", () => {
            expect(args.proc.currentDir()).toBe(args.currentDir);
        });

        it("setCurrentDir should set cwd", () => {
            args.proc.setCurrentDir(args.newCurrentDir);
            expect(args.proc.currentDir()).toBe(args.newCurrentDir);
        });

        it("args should match the provided args", () => {
            expect(args.proc.args()).toEqual(args.args);
        });

        it("env should match the provided env", () => {
            expect(args.proc.env()).toEqual(args.env);
        });
    });
}
