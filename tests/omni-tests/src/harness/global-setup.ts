/**
 * Vitest global setup for the omni e2e suite.
 *
 * Runs once before any test worker starts. Ensures an `omni` binary exists
 * (building it with cargo when necessary) so individual tests can spawn it
 * without each racing to build. See {@link ensureOmniBinary} for the env vars
 * that control this behavior (`OMNI_TEST_BIN`, `OMNI_TEST_PROFILE`,
 * `OMNI_TEST_BUILD`).
 */

import { ensureOmniBinary } from "./binary";

export default function setup(): void {
    const bin = ensureOmniBinary();
    // Make resolution cheap and consistent across worker processes.
    process.env.OMNI_TEST_BIN = bin;
}
