# Changelog
All notable changes to this project will be documented in this file. See [conventional commits](https://www.conventionalcommits.org/) for commit guidelines.

- - -
## omni_configurations-v0.12.0 - 2026-08-28
#### Features
- (**omni_configurations**) add projection configuration types and validation - (b72ad6c) - Clarence Manuel
#### Bug Fixes
- emit closed projection strategy arms so routes validate - (1c58a96) - Clarence Manuel
- add missing projection project manifests and dependency edges - (d900ea0) - Clarence Manuel
- inherit extends on omission instead of clearing it during merge - (978dc0d) - Clarence Manuel
#### Tests
- (**omni_configurations**) guard workspace schema for projection sources - (2d0707a) - Clarence Manuel
- (**omni_configurations**) pin workspace schema snapshot after source unification - (80abf17) - Clarence Manuel
#### Refactoring
- (**omni_configurations**) adopt strategy-tagged projection config; retire runtime strategy validators - (adffc20) - Clarence Manuel
- (**omni_configurations**) collapse source validators into generic SourcesValidator - (ec48788) - Clarence Manuel
- (**omni_configurations**) unify source configs into generic SourceConfig<E> - (e7200c1) - Clarence Manuel
- extract projection config types into omni_projection_configurations crate - (3750255) - Clarence Manuel
#### Miscellaneous Chores
- fix clippy clint errors - (92c5d0a) - Clarence Manuel

- - -

## omni_configurations-v0.11.0 - 2026-08-21
#### Features
- <span style="background-color: #d73a49; color: white; padding: 2px 6px; border-radius: 3px; font-weight: bold; font-size: 0.85em;">BREAKING</span>(**capabilities**) broker-authoritative Windows fs sandbox with import-closure grants and an experimental enforcement gate - (f3e15a6) - Clarence Manuel
- (**omni_configurations**) accept single-or-list project extends - (6585d64) - Clarence Manuel
- (**omni_configurations**) add task-level extension (base / extends) - (61bf142) - Clarence Manuel
- add reusable imperative tool subsystem - (0f195cb) - Clarence Manuel
- implement capability sandboxing - (eb3e5f9) - Clarence Manuel
#### Refactoring
- (**omni_configurations**) lower tasks via a canonical long-form expansion - (f0119a7) - Clarence Manuel

- - -

## omni_configurations-v0.10.0 - 2026-07-11
#### Features
- support array style commands - (32c32aa) - Clarence Manuel

- - -

## omni_configurations-v0.9.0 - 2026-07-04
#### Features
- implement progress ui for task execution and add  --output-logs, --output-cached-logs flags - (9e7e679) - Clarence Manuel
#### Bug Fixes
- (**omni_configurations**) improve error messages in load_config - (3f54c90) - Clarence Manuel
- skip serializing null values - (e25f20a) - Clarence Manuel
- move output configuration under cache field - (ed633c0) - Clarence Manuel
- apply deny_unknown_fields for configuration types - (3acf7b6) - Clarence Manuel
- swap serde_norway for noyalib and improve serialization-related error reporting - (7c65947) - Clarence Manuel

- - -

## omni_configurations-v0.8.1 - 2026-05-24
#### Bug Fixes
- support multiple paths for local generator source - (8757b00) - Clarence Manuel

- - -

## omni_configurations-v0.8.0 - 2026-05-18
#### Features
- support remote git generator sources - (c83d835) - Clarence Manuel
#### Bug Fixes
- rename command to exec - (7384f6e) - Clarence Manuel
#### Miscellaneous Chores
- update omni configs json schema links [skip ci] - (d484be7) - Clarence Manuel

- - -

## omni_configurations-v0.7.0 - 2026-03-21
#### Features
- implement retry_command - (d2f1b3a) - Clarence Manuel

- - -

## omni_configurations-v0.6.0 - 2026-02-19
#### Features
- support auto tui mode - (db6e135) - Clarence Manuel

- - -

## omni_configurations-v0.5.0 - 2026-02-15
#### Features
- add task args - (199f686) - Clarence Manuel

- - -

## omni_configurations-v0.4.1 - 2026-02-08
#### Bug Fixes
- (**omni_cofigurations**) enable field default value - (db6ff61) - Clarence Manuel

- - -

## omni_configurations-v0.4.0 - 2026-02-07
#### Features
- (**omni_configurations**) rename if to enabled in TaskLongFormConfiguration - (22df638) - Clarence Manuel
#### Bug Fixes
- rename fields - (efc8cb2) - Clarence Manuel

- - -

## omni_configurations-v0.3.0 - 2026-02-04
#### Features
- support tera template in task command - (7830096) - Clarence Manuel
#### Bug Fixes
- improve error message when extended config is missing - (820c1ae) - Clarence Manuel

- - -

## omni_configurations-v0.2.0 - 2026-02-03
#### Features
- support if expressions for task condition - (cb1c87d) - Clarence Manuel

- - -

## omni_configurations-v0.1.0 - 2026-01-31
#### Features
- (**omni_tracing_subscriber**) support for custom ad-hoc writers - (f584f1f) - Clarence Manuel
- initial implementation of run-javascript - (dd9d040) - Clarence Manuel
- update omni_configurations dependencies - (89758a1) - Clarence Manuel
- implement retry interval - (209a039) - Clarence Manuel
- implement task retry - (9b5232b) - Clarence Manuel
- add initial omni_generator_configurations crate implementation - (f80884b) - Clarence Manuel
- implement setup command for remote cache - (e2ae4d0) - Clarence Manuel
- implement loading of remote-cache config - (09b6361) - Clarence Manuel
- experimental tui mode - (a44bbad) - Clarence Manuel
- add omni_term_ui stream implementation - (57505b5) - Clarence Manuel
- allow env:vars in workspace configuration - (c2b03e0) - Clarence Manuel
#### Bug Fixes
- task_configuration.retry_interval json schema and serialization - (5d6df02) - Clarence Manuel
- rename siblings field to with in project_configuration - (48fd996) - Clarence Manuel
- default values for CacheConfiguration and TaskConfiguration - (ac8329f) - Clarence Manuel
- merging behavior for CacheConfiguration - (43c3313) - Clarence Manuel
- default value for persistent and interactive form TaskConfigurationLongForm - (bd8bd28) - Clarence Manuel
- merging behavior for enabled, persistent, interactive in TaskConfigurationLongForm - (ce1a976) - Clarence Manuel
- env:files config should only be available in workspace config - (7111d3e) - Clarence Manuel
#### Refactoring
- omni presets and logs - (13dc57c) - Clarence Manuel
- rename hash fields to digest - (90ad6c9) - Clarence Manuel
- major refactor for omni_context and related crates - (28f69e7) - Clarence Manuel
- create omni_context crate [skip ci] - (4fefab0) - Clarence Manuel
- create omni_configurations crate - (4b3a7b1) - Clarence Manuel
#### Miscellaneous Chores
- set versions and update cog.toml [skip ci] - (c6efdcf) - Clarence Manuel

- - -

Changelog generated by [cocogitto](https://github.com/cocogitto/cocogitto).