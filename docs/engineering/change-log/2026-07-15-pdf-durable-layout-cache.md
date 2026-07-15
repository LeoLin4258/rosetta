# PDF durable layout prepare cache

Date: 2026-07-15

## Summary

PDF background preparse now persists compressed ONNX layout masks inside each
job. A matching PDF window can skip layout inference after the app or PDF
worker restarts, while the existing in-memory LRU still provides immediate
same-session reuse.

## Behavior

- Cache identity covers source metadata and fingerprint, selected pages,
  language direction, thread count, engine version, and layout model signature.
- Disk writes are atomic and corrupt or incompatible entries become normal
  cache misses.
- Each job retains at most 12 disk entries and 256MB, using LRU eviction.
- Worker diagnostics distinguish `memory`, `disk`, and `miss` cache tiers.
- Worker startup validates durable manifests and restores prepared indicators
  for every matching PDF job.
- The cache stores numeric layout masks only. It does not store source text,
  translations, prompts, or provider responses.

## Validation

- A real one-page engine probe restored the disk cache and completed identity
  rendering after disposing all process-local prepared state.
- Repeated real 10-page, two-worker probes measured about 8.1-9.1 seconds for
  the first prepare and 4.9-5.2 seconds after a full worker restart, with
  `layout=0`, 105 matching units, `cacheTier="disk"`, and the owner restored in
  the second process's ready event.
- Python worker and PDF pack patch tests cover cache-tier protocol behavior.
