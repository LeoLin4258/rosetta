# PDF v3 PageGraph Store and Extraction Worker

Date: 2026-07-18

## Summary

Connected native page extraction to the durable scheduler through a compressed,
source-validated PageGraph authority.

## Implementation

- added deterministic gzip PageGraph artifacts with independent 64 MiB
  compressed and uncompressed limits;
- bound each store to source fingerprint/page count, engine version and
  PageGraph schema;
- added 64-page bounded metadata shards rebuilt from validated artifact files;
- added atomic artifact/index writes, corruption removal and idempotent repair;
- added validated extraction inventory generation for scheduler recovery;
- exposed a read-only scheduler extraction binding;
- added a sequential extraction worker that reuses one exact `PageSet` mapping
  index and commits artifact authority before scheduler state;
- added stable retryable store failure and non-retryable reconciliation failure
  transitions;
- added claim, reconciliation, storage and scheduler commit timing counters;
- reused the already-present `flate2` dependency offline, adding no new lockfile
  package.

## Windows AMD Evidence

On real-paper pages 1-10, the complete debug worker batch took 3,432 ms. Native
reconciliation used 723 ms and durable PageGraph storage used 2,357 ms.

The ten PageGraphs represented 37,855,795 bytes of JSON and occupied 3,327,910
bytes in the store. Compressed artifact payloads were 3,323,703 bytes, or 8.78%
of raw JSON. Scheduler metadata occupied 4,034 bytes.

## Current Boundary

The native extraction worker is connected to durable authority and scheduler
state inside the isolated PDF v3 module. Translation workers, patch-inventory
assembly, Tauri lifecycle commands, UI status and real 500/1,000-page complex
end-to-end translation/export remain pending.
