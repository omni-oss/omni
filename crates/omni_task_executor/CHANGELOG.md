# Changelog
All notable changes to this project will be documented in this file. See [conventional commits](https://www.conventionalcommits.org/) for commit guidelines.

- - -
## omni_task_executor-v0.6.2 - 2026-02-17
#### Bug Fixes
- (**omni_task_executor**) error when trace folder doesn't exist - (a8de21d) - Clarence Manuel

- - -

## omni_task_executor-v0.6.1 - 2026-02-16
#### Bug Fixes
- error when cache is disabled for a task - (bb21a9b) - Clarence Manuel

- - -

## omni_task_executor-v0.6.0 - 2026-02-15
#### Features
- add task args - (199f686) - Clarence Manuel

- - -

## omni_task_executor-v0.5.3 - 2026-02-14
#### Bug Fixes
- task meta not discovered - (ec2412b) - Clarence Manuel

- - -

## omni_task_executor-v0.5.2 - 2026-02-10
#### Bug Fixes
- (**omni_task_executor**) update verbose info trace to trace level - (84e9471) - Clarence Manuel
#### Miscellaneous Chores
- add traces - (58e5715) - Clarence Manuel

- - -

## omni_task_executor-v0.5.1 - 2026-02-09
#### Bug Fixes
- (**omni_task_executor**) expanding output paths for tera removes the OmniPath root - (15e3b82) - Clarence Manuel

- - -

## omni_task_executor-v0.5.0 - 2026-02-08
#### Features
- add meta to template context - (bd1c6d0) - Clarence Manuel
#### Bug Fixes
- (**omni_task_executor**) bug where persistent tasks causes panic on exit - (8de6991) - Clarence Manuel

- - -

## omni_task_executor-v0.4.0 - 2026-02-07
#### Features
- (**omni_task_executor**) support output files and cache input key files in task result details - (212fe99) - Clarence Manuel
#### Bug Fixes
- (**omni_task_executor**) resolve workspace scoped paths - (45bb047) - Clarence Manuel
- (**omni_task_executor**) update task results details - (4daa71a) - Clarence Manuel
- rename fields - (efc8cb2) - Clarence Manuel

- - -

## omni_task_executor-v0.3.0 - 2026-02-04
#### Features
- support tera template in cache input files and output files - (c0b305b) - Clarence Manuel
- support tera template in task command - (7830096) - Clarence Manuel

- - -

## omni_task_executor-v0.2.0 - 2026-02-03
#### Features
- support if expressions for task condition - (cb1c87d) - Clarence Manuel

- - -

## omni_task_executor-v0.1.0 - 2026-01-31
#### Features
- (**omni_task_executor**) utilize omni_tracing_subscriber ad-hoc writer for TUI mode - (413a3f1) - Clarence Manuel
- (**omni_task_executor**) add batch_executor module - (dfe27a2) - Clarence Manuel
- (**omni_task_executor**) add cache_manager module - (310e4b2) - Clarence Manuel
- (**omni_task_executor**) add task_context_provider module - (2db2b7d) - Clarence Manuel
- (**omni_term_ui**) support cursor movements - (f81a428) - Clarence Manuel
- (**omni_term_ui**) implement key event handling in TUI mode - (123ccb0) - Clarence Manuel
- (**omni_tracing_subscriber**) support for custom ad-hoc writers - (f584f1f) - Clarence Manuel
- implement run-command action - (a1bf24f) - Clarence Manuel
- add log for retry interval - (268912e) - Clarence Manuel
- implement retry interval - (209a039) - Clarence Manuel
- implement task retry - (9b5232b) - Clarence Manuel
- implement --with-dependents - (f0421a4) - Clarence Manuel
- implement --scm-affected flag - (5bf7c28) - Clarence Manuel
- implement --dir filter - (30449d6) - Clarence Manuel
- add support for multiple flags for --project and --task - (6b4532c) - Clarence Manuel
- implement --force=failed flag - (b3a04b8) - Clarence Manuel
- update path for remote caching configuration - (a911520) - Clarence Manuel
- implement setup command for remote cache - (e2ae4d0) - Clarence Manuel
- implement loading of remote-cache config - (09b6361) - Clarence Manuel
- utilize RemoteCacheClient in omni_cache - (5d04312) - Clarence Manuel
- implement experimental --stale-only flag - (1d6029b) - Clarence Manuel
- experimental tui mode - (a44bbad) - Clarence Manuel
- add omni_term_ui stream implementation - (57505b5) - Clarence Manuel
- utilize command expansion - (04d38d1) - Clarence Manuel
#### Bug Fixes
- (**omni_cli_core**) broken log colors due to tracing-subscriber 0.3.20 - (91ceb27) - Clarence Manuel
- (**omni_task_executor**) fix Cargo.toml - (75e5480) - Clarence Manuel
- (**omni_task_executor**) no cache hits bug - (a0f2b01) - Clarence Manuel
- (**omni_task_executor**) compile error - (0eb44cf) - Clarence Manuel
- retry flag precedence - (b60924f) - Clarence Manuel
- panic when running exec - (19ce74f) - Clarence Manuel
- invert precedence of --retry and --retry-interval precedence for CLI and file config - (1866d88) - Clarence Manuel
- no hash when running dry run - (f4495c9) - Clarence Manuel
- merging behavior for enabled, persistent, interactive in TaskConfigurationLongForm - (ce1a976) - Clarence Manuel
- term ui scrolling not working - (7284f62) - Clarence Manuel
- not skipping task when there are dependency failures - (c5348cc) - Clarence Manuel
#### Performance Improvements
- add omni_context benchmarks - (55628d7) - Clarence Manuel
#### Refactoring
- (**clap_utils**) create ValueEnumAdapter - (e0fc922) - Clarence Manuel
- (**omni_context**) utilize ProjectQuery - (836fc52) - Clarence Manuel
- (**omni_execution_plan**) create reusable omni_execution_plan crate - (c583161) - Clarence Manuel
- (**omni_task_context**) create reusable omni_task_context crate - (f154cf4) - Clarence Manuel
- (**omni_task_executor**) add result, error, sys types - (4a547a0) - Clarence Manuel
- rename error result status to errored - (adad5d7) - Clarence Manuel
- rename hash fields to digest - (90ad6c9) - Clarence Manuel
- start creating omni_task_executor crate [skip ci] - (5e4899e) - Clarence Manuel
#### Miscellaneous Chores
- set versions and update cog.toml [skip ci] - (c6efdcf) - Clarence Manuel
- update crates [skip ci] - (8b9022a) - Clarence Manuel
- update error implementations - (ad81dc0) - Clarence Manuel
- cleanup unneeded files [skip ci] - (04d1c09) - Clarence Manuel

- - -

Changelog generated by [cocogitto](https://github.com/cocogitto/cocogitto).