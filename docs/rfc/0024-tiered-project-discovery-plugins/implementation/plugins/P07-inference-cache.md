# P07: Inference cache

**Size:** L · **Depends on:** R08, P06

## Goal

Cache hook results so unchanged discovery work is not rerun, on top of the R08 storage
primitives, kept separate from the task execution cache.

## RFC references

- [§8.2](../../008.01-inference-cache-and-storage.md#82-shared-storage-separate-task-and-inference-caches)
- [§17.5 batch-independent results](../../017.00-review-findings-and-prerequisites.md#175-batch-independent-results-versus-resource-dependent-outcomes)

## Work

1. **Physical layout and accounting design** first (short doc in the PR): per-root
   logical records for root-oriented hooks, singleton records for workspace-wide hooks,
   bulk physical storage allowed, bounded lookup/dependency indexes.
2. **Identity:** plugin artifact/module fingerprint, options, negotiated versions,
   claim-compatibility mapping identity, consumed producer-claim view, grant set, and
   tracked dependencies (including negative reads and membership). Not registration or
   completion order.
3. **Validation on hit:** revalidate tracked inputs from the captured view (F10);
   identical logical limits for hits and live roots; cache I/O/decode costs bounded but
   unexecuted plugin fuel/time not charged.
4. **Publication:** only after stage success and final pass consistency (P08); rejected
   passes publish nothing; retain existing valid entries.
5. **Best-effort writes:** full-disk/read-only failures produce bounded warnings, keep
   accepted graphs usable, never expose incomplete records; discovery correctness
   failures stay fatal.
6. **Dispositions and claims stored together**; cached results restore identical
   scheduling consequences.

## Acceptance

- Mixed cache-hit/live-root batches; changed partitions; shared evidence; independent
  root invalidation; negative-query invalidation when a matching claim appears.
- Cached results under reduced limits behave per §17.5.
- Corruption, concurrent prune, crash recovery on the inference consumer.
- Task cache formats and remote protocol untouched.
