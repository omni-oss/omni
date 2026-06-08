import { describe, expect, it } from "vitest";
import { RequestErrorCode, ResponseErrorCode } from "./error-code";

describe("ResponseErrorCode", () => {
    describe("predefined codes", () => {
        it("should expose UNEXPECTED_FRAME with value 0", () => {
            expect(ResponseErrorCode.UNEXPECTED_FRAME.valueOf()).toBe(0);
        });
    });

    describe("from", () => {
        it("should return the UNEXPECTED_FRAME singleton for value 0", () => {
            expect(ResponseErrorCode.from(0)).toBe(
                ResponseErrorCode.UNEXPECTED_FRAME,
            );
        });

        it("should create a custom code for unknown values", () => {
            const code = ResponseErrorCode.from(123);
            expect(code.valueOf()).toBe(123);
        });

        it("should cache custom codes so the same value yields the same instance", () => {
            const a = ResponseErrorCode.from(999);
            const b = ResponseErrorCode.from(999);
            expect(a).toBe(b);
        });

        it("should produce different instances for different custom values", () => {
            const a = ResponseErrorCode.from(10);
            const b = ResponseErrorCode.from(11);
            expect(a).not.toBe(b);
            expect(a.valueOf()).toBe(10);
            expect(b.valueOf()).toBe(11);
        });

        it("should reject negative values", () => {
            expect(() => ResponseErrorCode.from(-1)).toThrow();
        });

        it("should reject values greater than or equal to 65536", () => {
            expect(() => ResponseErrorCode.from(65_536)).toThrow();
        });

        it("should reject non-integer values", () => {
            expect(() => ResponseErrorCode.from(2.5)).toThrow();
        });

        it("should accept the maximum allowed value (65535)", () => {
            const code = ResponseErrorCode.from(65_535);
            expect(code.valueOf()).toBe(65_535);
        });
    });

    describe("toString", () => {
        it("should return a string representation of the underlying value", () => {
            expect(ResponseErrorCode.UNEXPECTED_FRAME.toString()).toBe("0");
            expect(ResponseErrorCode.from(42).toString()).toBe("42");
        });
    });

    describe("toJSON", () => {
        it("should return the numeric value", () => {
            expect(ResponseErrorCode.UNEXPECTED_FRAME.toJSON()).toBe(0);
            expect(ResponseErrorCode.from(42).toJSON()).toBe(42);
        });

        it("should serialize as a number when used with JSON.stringify", () => {
            expect(JSON.stringify(ResponseErrorCode.from(42))).toBe("42");
        });
    });

    describe("valueOf", () => {
        it("should return the numeric value", () => {
            expect(ResponseErrorCode.UNEXPECTED_FRAME.valueOf()).toBe(0);
            expect(ResponseErrorCode.from(7).valueOf()).toBe(7);
        });
    });

    describe("equals", () => {
        it("should return true for the same singleton instance", () => {
            expect(
                ResponseErrorCode.UNEXPECTED_FRAME.equals(
                    ResponseErrorCode.UNEXPECTED_FRAME,
                ),
            ).toBe(true);
        });

        it("should return true when comparing two custom codes with the same value", () => {
            const a = ResponseErrorCode.from(321);
            const b = ResponseErrorCode.from(321);
            expect(a.equals(b)).toBe(true);
        });

        it("should return false when comparing codes with different values", () => {
            expect(
                ResponseErrorCode.UNEXPECTED_FRAME.equals(
                    ResponseErrorCode.from(1),
                ),
            ).toBe(false);
        });

        it("should treat from(0) as equal to UNEXPECTED_FRAME", () => {
            expect(
                ResponseErrorCode.from(0).equals(
                    ResponseErrorCode.UNEXPECTED_FRAME,
                ),
            ).toBe(true);
        });
    });
});

describe("RequestErrorCode", () => {
    describe("predefined codes", () => {
        it("should expose UNEXPECTED_FRAME with value 0", () => {
            expect(RequestErrorCode.UNEXPECTED_FRAME.valueOf()).toBe(0);
        });

        it("should expose TIMED_OUT with value 1", () => {
            expect(RequestErrorCode.TIMED_OUT.valueOf()).toBe(1);
        });
    });

    describe("from", () => {
        it("should return the UNEXPECTED_FRAME singleton for value 0", () => {
            expect(RequestErrorCode.from(0)).toBe(
                RequestErrorCode.UNEXPECTED_FRAME,
            );
        });

        it("should return the TIMED_OUT singleton for value 1", () => {
            expect(RequestErrorCode.from(1)).toBe(RequestErrorCode.TIMED_OUT);
        });

        it("should create a custom code for unknown values", () => {
            const code = RequestErrorCode.from(456);
            expect(code.valueOf()).toBe(456);
        });

        it("should cache custom codes so the same value yields the same instance", () => {
            const a = RequestErrorCode.from(2024);
            const b = RequestErrorCode.from(2024);
            expect(a).toBe(b);
        });

        it("should produce different instances for different custom values", () => {
            const a = RequestErrorCode.from(20);
            const b = RequestErrorCode.from(21);
            expect(a).not.toBe(b);
            expect(a.valueOf()).toBe(20);
            expect(b.valueOf()).toBe(21);
        });

        it("should reject negative values", () => {
            expect(() => RequestErrorCode.from(-1)).toThrow();
        });

        it("should reject values greater than or equal to 65536", () => {
            expect(() => RequestErrorCode.from(65_536)).toThrow();
        });

        it("should reject non-integer values", () => {
            expect(() => RequestErrorCode.from(3.14)).toThrow();
        });

        it("should accept the maximum allowed value (65535)", () => {
            const code = RequestErrorCode.from(65_535);
            expect(code.valueOf()).toBe(65_535);
        });
    });

    describe("toString", () => {
        it("should return a string representation of the underlying value", () => {
            expect(RequestErrorCode.UNEXPECTED_FRAME.toString()).toBe("0");
            expect(RequestErrorCode.TIMED_OUT.toString()).toBe("1");
            expect(RequestErrorCode.from(99).toString()).toBe("99");
        });
    });

    describe("toJSON", () => {
        it("should return the numeric value", () => {
            expect(RequestErrorCode.UNEXPECTED_FRAME.toJSON()).toBe(0);
            expect(RequestErrorCode.TIMED_OUT.toJSON()).toBe(1);
            expect(RequestErrorCode.from(99).toJSON()).toBe(99);
        });

        it("should serialize as a number when used with JSON.stringify", () => {
            expect(JSON.stringify(RequestErrorCode.TIMED_OUT)).toBe("1");
        });
    });

    describe("valueOf", () => {
        it("should return the numeric value", () => {
            expect(RequestErrorCode.UNEXPECTED_FRAME.valueOf()).toBe(0);
            expect(RequestErrorCode.TIMED_OUT.valueOf()).toBe(1);
            expect(RequestErrorCode.from(7).valueOf()).toBe(7);
        });
    });

    describe("equals", () => {
        it("should return true for the same singleton instance", () => {
            expect(
                RequestErrorCode.TIMED_OUT.equals(RequestErrorCode.TIMED_OUT),
            ).toBe(true);
        });

        it("should return true when comparing two custom codes with the same value", () => {
            const a = RequestErrorCode.from(321);
            const b = RequestErrorCode.from(321);
            expect(a.equals(b)).toBe(true);
        });

        it("should return false when comparing different predefined codes", () => {
            expect(
                RequestErrorCode.UNEXPECTED_FRAME.equals(
                    RequestErrorCode.TIMED_OUT,
                ),
            ).toBe(false);
        });

        it("should treat from(1) as equal to TIMED_OUT", () => {
            expect(
                RequestErrorCode.from(1).equals(RequestErrorCode.TIMED_OUT),
            ).toBe(true);
        });
    });
});

describe("RequestErrorCode and ResponseErrorCode interop", () => {
    it("should not share instances even when values are equal", () => {
        const requestCode = RequestErrorCode.from(0);
        const responseCode = ResponseErrorCode.from(0);

        expect(requestCode).not.toBe(
            responseCode as unknown as RequestErrorCode,
        );
        expect(requestCode.valueOf()).toBe(responseCode.valueOf());
    });
});
