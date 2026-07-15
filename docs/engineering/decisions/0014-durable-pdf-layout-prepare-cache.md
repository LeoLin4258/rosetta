# ADR 0014: Durable PDF Layout Prepare Cache

Date: 2026-07-15

## Status

Accepted.

## Context

Rosetta background-prepares the first selected PDF window and keeps prepared
runs in the persistent worker's bounded memory LRU. This makes translation
immediate while the worker remains alive, but all preprocessing was previously
lost when the app or worker exited.

The PDF engine's complete prepared state cannot be safely serialized. Real
probes showed that pdfminer `LTPage`, font maps, and font IDs retain parser and
open-file handles. Pickling the object graph fails on `BufferedReader`, and a
pickle would also be an unstable, unsafe persistence contract. Persisting the
prepared PDF is technically possible, but font embedding expanded a 135KB
one-page fixture to 13-20MB and saved only about 0.6 seconds in the measured
prepare path.

The ONNX layout masks are stable numeric arrays, contain no extracted document
text, compress well, and account for about 2.8-3.6 seconds of a measured
10-page prepare. They are the useful durable boundary.

## Decision

PDF prepare uses two cache tiers:

- A six-entry process-local LRU retains complete prepared runs for immediate
  reuse. It reports `cacheTier="memory"`.
- A job-local, versioned disk cache retains compressed ONNX layout masks across
  app and worker restarts. Restoring it reports `cacheTier="disk"`.

Disk entries live at:

```txt
<jobDir>/pdf-prepare-cache/v1/<prepare-key-sha256>/
  manifest.json
  layout.npz
```

The prepare identity includes the source path, byte length, modification time,
source fingerprint, selected pages, language direction, thread count, and
cache schema. The manifest additionally records the PDF engine version and
layout-model file signature. Any mismatch or malformed/missing file is a cache
miss; the engine performs normal layout inference and replaces the entry.

The engine writes `layout.npz` first and `manifest.json` last using same-volume
temporary files plus atomic replace. Readers never treat a partial entry as
valid. Each job keeps at most 12 page-window entries and 256MB of layout cache,
evicting least-recently-used entries. Deleting the job deletes its cache with
the rest of the job directory.

On worker startup, Rosetta scans job-local manifests and validates cache schema,
source fingerprint, engine version, model signature, and required files. Valid
owners are reported through the existing prepare-cache status event, so the
sidebar can restore its restrained prepared indicator without opening every
PDF first. An invalid entry is never reported as prepared.

Restoring layout masks does not restore pdfminer replay objects. Background
preparse still rebuilds page interpretation and translation units before the
in-memory cache becomes ready. This is deliberate: the remaining objects do
not have a safe or stable serialization boundary.

## Compatibility

There is no migration from an older cache because no durable prepare cache
existed. `v1` is a derived-data format and may be discarded at any time. A
future format change uses a new directory version or schema version; source
PDFs and translated page artifacts remain untouched.

Old PDF component packs ignore the new options and continue to work without a
disk hit. New packs expose `persistentLayoutCacheHit` in `PreparedRun`; Rust
ignores unknown engine fields for backward compatibility.

## Consequences

Positive:

- Reopening a PDF no longer reruns ONNX layout inference for a matching window.
- Cache entries are small enough to retain many documents.
- Corruption, source changes, engine updates, and model updates fail closed to
  a normal prepare.
- No source text, translation, prompt, or model response is written to this
  cache.

Costs:

- A restart still pays pdfminer unit collection in the background.
- The PDF component patch and Rosetta worker protocol must evolve together to
  report cache tier accurately.
- Multiple page selections can create multiple job-local entries, bounded by
  the per-job limits.

## Rejected Alternatives

- Pickle the complete prepared state. It fails on live file handles, is unsafe
  for durable loading, and couples data to internal Python class layouts.
- Persist the prepared PDF for every window. The measured disk cost was
  13-20MB per one-page window for only about 0.6 seconds of additional savings.
- Persist only a boolean "prepared" marker. That would make the UI claim a
  performance benefit without preserving any reusable computation.
- Share one cache across duplicate jobs. Jobs remain independent per ADR 0008;
  implicit cross-job sharing would complicate deletion and ownership.
