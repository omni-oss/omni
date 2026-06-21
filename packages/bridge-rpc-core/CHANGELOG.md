# Changelog
All notable changes to this project will be documented in this file. See [conventional commits](https://www.conventionalcommits.org/) for commit guidelines.

- - -
## @omni-oss/bridge-rpc-core-v0.1.0 - 2026-06-14
#### Features
- implement bridge-rpc utils and services - (121791b) - Clarence Manuel
#### Bug Fixes
- (**@omni-oss/bridge-rpc**) implement async-context propagation in AbstractTransport.onReceive - (5a4422d) - Clarence Manuel
- (**@omni-oss/bridge-rpc-core**) improve error handling - (181bac2) - Clarence Manuel
- (**@omni-oss/bridge-rpc-core**) fix transport-read-framer bug and improve error handling and propagation - (9ec7d72) - Clarence Manuel
- (**@omni-oss/bridge-rpc-core**) implement frame tuple wire format - (84ec442) - Clarence Manuel
- (**@omni-oss/bridge-rpc-core**) improve error handling on BridgeRpc.stop - (fe924d4) - Clarence Manuel
- (**@omni-oss/bridge-rpc-core**) status code visibility issue - (b54edc4) - Clarence Manuel
- (**@omni-oss/bridge-rpc-core**) implement instance caching for status code classes - (53c2aef) - Clarence Manuel
- (**@omni-oss/bridge-rpc-core**) add asyncDispose cleanup support for request and response objects - (403f47e) - Clarence Manuel
- (**@omni-oss/bridge-rpc-core**) use interface instead of type for Service type - (3751cc1) - Clarence Manuel
- (**@omni-oss/bridge-rpc-core**) add equals method to status code classes - (f0cb3d1) - Clarence Manuel
- (**@omni-oss/bridge-rpc-core**) allow method chaining on writeBodyChunk methods - (42f75d0) - Clarence Manuel
- (**@omni-oss/bridge-rpc-core**) add state check assertions for request and response objects - (e45545c) - Clarence Manuel
- (**@omni-oss/bridge-rpc-core**) expose ClientHandle to services - (842cce1) - Clarence Manuel
- add validation for status code classes - (99ea27d) - Clarence Manuel
#### Miscellaneous Chores
- (**@omni-oss/bridge-rpc-core**) update integration tests - (f33d4b3) - Clarence Manuel
- fix ts build errors - (45a6b32) - Clarence Manuel
- add retries - (b2d4616) - Clarence Manuel
- update concurrent request amount in integration tests - (a38c527) - Clarence Manuel
- update project configs - (fee5066) - Clarence Manuel
- update packages versions and configurations - (bb47909) - Clarence Manuel

- - -

## @omni-oss/bridge-rpc-v0.1.2 - 2026-05-22
#### Bug Fixes
- path error on windows - (8c75e42) - Clarence Manuel
#### Miscellaneous Chores
- update omni configs json schema links [skip ci] - (d484be7) - Clarence Manuel
- update vite configs - (e326f27) - Clarence Manuel
- utilize bun catalogs - (40fe122) - Clarence Manuel
- bump node version [skip ci] - (04067be) - Clarence Manuel
- use ^ for workspace package version [skip ci] - (89ba03b) - Clarence Manuel
- add dependencies to project.omni.yaml [skip ci] - (de71dd7) - Clarence Manuel
- add publishConfig to existing packages [skip ci] - (5a48ec0) - Clarence Manuel
- update npm packages [skip ci] - (8fba262) - Clarence Manuel

- - -

## @omni-oss/bridge-rpc-v0.1.1 - 2026-02-03
#### Bug Fixes
- (**@omni-oss/bridge-rpc**) remove console logs - (0ded265) - Clarence Manuel

- - -

## @omni-oss/bridge-rpc-v0.1.0 - 2026-01-31
#### Features
- (**bridge-rpc**) update implementation to v2 - (f7f38b8) - Clarence Manuel
- (**bridge-rpc-router**) implement router - (89bce93) - Clarence Manuel
- (**omni_term_ui**) support cursor movements - (f81a428) - Clarence Manuel
- (**omni_tracing_subscriber**) support for custom ad-hoc writers - (f584f1f) - Clarence Manuel
- implement initial system-interface package - (d579bf6) - Clarence Manuel
- experimental tui mode - (a44bbad) - Clarence Manuel
- support sibling task, disabling task - (2f9ff3a) - Clarence Manuel
- add apply-version script - (092372c) - Clarence Manuel
- support empty commands - (211b260) - Clarence Manuel
- new on_failure handling - (2f42dc9) - Clarence Manuel
- support more characters in task dependency syntax - (39ed73e) - Clarence Manuel
- add BridgeRpc integration tests - (5710a86) - Clarence Manuel
- add TransportReadFramer and TransportWriteFramer to bridge_rpc crate - (85faf60) - Clarence Manuel
- bridge-rpc-ts integration test add probe - (a8c5af1) - Clarence Manuel
- bridge-rpc-ts integration test - (a392aed) - Clarence Manuel
- unify TcpTransport and StdioTransport to StreamTransport - (585c730) - Clarence Manuel
- add unit tests for StdioTransport - (0a7920c) - Clarence Manuel
- read and write framers in typescript - (b5acf32) - Clarence Manuel
- add probe to bridge-rpc-ts - (118bb26) - Clarence Manuel
- bridge rpc ts package initial implementation - (c1f61f5) - Clarence Manuel
- add typescript projects - (028e56c) - Clarence Manuel
#### Bug Fixes
- (**bridge-rpc**) background-processor should clear all errors after awaitAll - (1ef5cf9) - Clarence Manuel
- (**bridge-rpc**) background-processor should return the collected errors in awaitAll - (28a03dd) - Clarence Manuel
- (**bridge-rpc**) cleanup wrong imports - (24de4fe) - Clarence Manuel
- (**bridge-rpc**) compile error - (4d28512) - Clarence Manuel
- test error on ts packages - (5c49879) - Clarence Manuel
- persistent task should not be cached and it should keep stdin open - (416b76e) - Clarence Manuel
#### Documentation
- fix build error - (894dcbd) - Clarence Manuel
- add startlight astro docs - (00d29de) - Clarence Manuel
#### Refactoring
- (**@omni-oss/bridge-rpc**) update implementations and fix tests - (343a06d) - Clarence Manuel
- (**bridge-rpc**) add utility channel factory - (bb533d5) - Clarence Manuel
- (**bridge-rpc**) utilities and tests - (5baaa3a) - Clarence Manuel
- create packages for channels and async-utils - (797e64a) - Clarence Manuel
#### Miscellaneous Chores
- (**bridge-rpc**) add ClientHandle - (956697c) - Clarence Manuel
- update package.json version [skip ci] - (5f549ba) - Clarence Manuel
- update tsconfig.types.json - (4fc5146) - Clarence Manuel
- fix declaration types output - (5f09c39) - Clarence Manuel
- update ts build - (5c64054) - Clarence Manuel
- update build for ts packages - (5b11205) - Clarence Manuel
- refactor vitest configs - (5943ba0) - Clarence Manuel
- update packages - (eec46c9) - Clarence Manuel
- update packages - (17b4708) - Clarence Manuel
- update npm dependency versions - (996f24f) - Clarence Manuel
- update dependencies [skip ci] - (280976f) - Clarence Manuel
- update dependencies [skip ci] - (d31a4ea) - Clarence Manuel
- update packages [skip ci] - (513b425) - Clarence Manuel
- add publish-json-schemas workflow [skip ci] - (8144c86) - Clarence Manuel
- update yaml-language-server schema on omni config files [skip ci] - (554ea4d) - Clarence Manuel
- refactor - (90efb7b) - Clarence Manuel
- sync updates - (c494e10) - Clarence Manuel

- - -

Changelog generated by [cocogitto](https://github.com/cocogitto/cocogitto).