/**
 * Vitest setup file for the omni e2e suite.
 *
 * Registered via `test.setupFiles` in `vitest.config.e2e.ts`. Importing the
 * matchers module runs its `expect.extend(...)` call and pulls in the matcher
 * type augmentation for every test file.
 */

import "./matchers";
