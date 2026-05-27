import * as z from "zod";

/**
 * A unique identifier that combines process ID with an incrementing counter.
 * Format: high 32 bits = PID, low 32 bits = counter
 */
export class Id {
    private readonly value: bigint;
    private constructor(value: bigint | number | string) {
        if (typeof value === "bigint") {
            this.value = value;
        } else {
            this.value = BigInt(value);
        }
    }

    /**
     * Generate a new unique ID for this process
     */
    static create(): Id {
        return new Id(uniqueU64ForPid());
    }

    /**
     * Create an Id from a string representation
     */
    static fromString(str: string): Id {
        return new Id(str);
    }

    static fromBigInt(value: bigint): Id {
        return new Id(value);
    }

    static fromNumber(value: number): Id {
        return new Id(value);
    }

    /**
     * Get the underlying u64 value as a bigint
     */
    public getValue(): bigint {
        return this.value;
    }

    public toNumber(): number {
        return Number(this.value);
    }

    public valueOf(): bigint {
        return this.value;
    }

    /**
     * Convert to string representation
     */
    public toString(): string {
        return this.value.toString();
    }

    /**
     * Convert to JSON (as number)
     */
    public toJSON(): number {
        return Number(this.value);
    }

    /**
     * Check equality with another Id
     */
    public equals(other: Id): boolean {
        return this.value === other.value;
    }
}

// Zod schema for Id
export const IdSchema = z.bigint().transform((b) => Id.fromBigInt(b));

// Global counter state
class Counter {
    private value: number;

    constructor() {
        // Seed counter with random start to reduce chance of collision after PID reuse
        this.value = Math.floor(Math.random() * 0xffffffff);
    }

    /**
     * Atomically increment and return the previous value
     * Note: JavaScript is single-threaded, so this is naturally atomic
     */
    fetchAdd(): number {
        const current = this.value;
        // Wrap around at 32-bit boundary
        this.value = (this.value + 1) >>> 0;
        return current;
    }
}

// Lazy-initialized global counter
let counter: Counter | null = null;

function getCounter(): Counter {
    if (counter === null) {
        counter = new Counter();
    }
    return counter;
}

/**
 * Returns a u64 (as bigint) that is unique per-call within the process
 * and unique across concurrently running processes on the same machine
 * (PID in high 32 bits, counter in low 32 bits).
 */
export function uniqueU64ForPid(): bigint {
    const pid = process.pid; // number
    const low = getCounter().fetchAdd();

    // Combine: (pid << 32) | low
    const pidBig = BigInt(pid >>> 0); // Ensure unsigned 32-bit
    const lowBig = BigInt(low >>> 0); // Ensure unsigned 32-bit

    return (pidBig << 32n) | lowBig;
}

/**
 * Reset the counter (useful for testing)
 * @internal
 */
export function resetCounter(): void {
    counter = null;
}
