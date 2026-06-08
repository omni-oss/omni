import { describe, expect, it } from "vitest";
import { ResponseStatusCode } from "./status-code";

describe("ResponseStatusCode", () => {
    describe("predefined codes", () => {
        it("should expose SUCCESS with value 0", () => {
            expect(ResponseStatusCode.SUCCESS.valueOf()).toBe(0);
        });

        it("should expose NO_HANDLER_FOR_PATH with value 100", () => {
            expect(ResponseStatusCode.NO_HANDLER_FOR_PATH.valueOf()).toBe(100);
        });
    });

    describe("from", () => {
        it("should return the SUCCESS singleton for value 0", () => {
            expect(ResponseStatusCode.from(0)).toBe(ResponseStatusCode.SUCCESS);
        });

        it("should return the NO_HANDLER_FOR_PATH singleton for value 100", () => {
            expect(ResponseStatusCode.from(100)).toBe(
                ResponseStatusCode.NO_HANDLER_FOR_PATH,
            );
        });

        it("should create a custom code for unknown values", () => {
            const code = ResponseStatusCode.from(500);
            expect(code.valueOf()).toBe(500);
        });

        it("should cache custom codes so the same value yields the same instance", () => {
            const a = ResponseStatusCode.from(1234);
            const b = ResponseStatusCode.from(1234);
            expect(a).toBe(b);
        });

        it("should produce different instances for different custom values", () => {
            const a = ResponseStatusCode.from(200);
            const b = ResponseStatusCode.from(201);
            expect(a).not.toBe(b);
            expect(a.valueOf()).toBe(200);
            expect(b.valueOf()).toBe(201);
        });

        it("should reject negative values", () => {
            expect(() => ResponseStatusCode.from(-1)).toThrow();
        });

        it("should reject values greater than or equal to 65536", () => {
            expect(() => ResponseStatusCode.from(65_536)).toThrow();
        });

        it("should reject non-integer values", () => {
            expect(() => ResponseStatusCode.from(1.5)).toThrow();
        });

        it("should accept the maximum allowed value (65535)", () => {
            const code = ResponseStatusCode.from(65_535);
            expect(code.valueOf()).toBe(65_535);
        });
    });

    describe("toString", () => {
        it("should return a string representation of the underlying value", () => {
            expect(ResponseStatusCode.SUCCESS.toString()).toBe("0");
            expect(ResponseStatusCode.NO_HANDLER_FOR_PATH.toString()).toBe(
                "100",
            );
            expect(ResponseStatusCode.from(404).toString()).toBe("404");
        });
    });

    describe("toJSON", () => {
        it("should return the numeric value", () => {
            expect(ResponseStatusCode.SUCCESS.toJSON()).toBe(0);
            expect(ResponseStatusCode.from(404).toJSON()).toBe(404);
        });

        it("should serialize as a number when used with JSON.stringify", () => {
            expect(JSON.stringify(ResponseStatusCode.from(404))).toBe("404");
        });
    });

    describe("valueOf", () => {
        it("should return the numeric value", () => {
            expect(ResponseStatusCode.SUCCESS.valueOf()).toBe(0);
            expect(ResponseStatusCode.from(42).valueOf()).toBe(42);
        });
    });

    describe("equals", () => {
        it("should return true for the same singleton instance", () => {
            expect(
                ResponseStatusCode.SUCCESS.equals(ResponseStatusCode.SUCCESS),
            ).toBe(true);
        });

        it("should return true when comparing two custom codes with the same value", () => {
            const a = ResponseStatusCode.from(321);
            const b = ResponseStatusCode.from(321);
            expect(a.equals(b)).toBe(true);
        });

        it("should return false when comparing codes with different values", () => {
            expect(
                ResponseStatusCode.SUCCESS.equals(
                    ResponseStatusCode.NO_HANDLER_FOR_PATH,
                ),
            ).toBe(false);
        });

        it("should treat a from(0) as equal to SUCCESS", () => {
            expect(
                ResponseStatusCode.from(0).equals(ResponseStatusCode.SUCCESS),
            ).toBe(true);
        });
    });
});
