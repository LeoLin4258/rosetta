# ADR 0040: PDF v3 Incremental Delta Export Commit

Date: 2026-07-18

Status: Accepted

Amends ADR 0016 and ADR 0039.

## Context

PDF v3 can render consecutive translated pages through one document-wide font
registry, but complete export still used `lopdf::Document::save_to()`. That
rewrites every reachable source object and requires a complete parsed source
graph. `lopdf::IncrementalDocument` avoids the rewrite, but its loader retains
both the complete source bytes and the complete parsed previous document.

The final export writer needs a narrower ownership boundary. It must preserve
the original catalog, navigation, annotations and metadata, append only objects
changed by page patches, and commit a complete file without exposing a partial
destination after cancellation or a crash.

## Decision

PDF v3 introduces an isolated incremental export committer with three inputs:

- an immutable source PDF path;
- `IncrementalExportBase`, containing source SHA-256, source byte count, latest
  xref offset, maximum object number and source trailer;
- a sorted map of changed or newly allocated indirect objects.

The source is copied to a same-directory temporary file through one fixed 64
KiB buffer. Copying recomputes SHA-256 and byte count; either mismatch aborts
before destination replacement. The writer then serializes only delta objects,
emits contiguous classic xref subsections and writes a trailer whose `/Prev`
points to the source's latest xref. Source `/Root`, `/Info`, `/ID` and other
ordinary trailer identity are retained. Xref-stream-only keys are removed from
the new classic trailer.

The serializer covers every `lopdf::Object` variant and recalculates direct
stream `/Length` from the bytes being emitted. It does not parse or modify
objects with string search. Delta validation rejects object zero, reserved free
generation 65535, duplicate object numbers and empty commits. The current
classic xref implementation explicitly rejects offsets above `u32::MAX`, which
matches the current `lopdf` reader boundary.

Cancellation is checked before work, between source chunks, between delta
objects and immediately before commit. The temporary file is flushed and
`sync_all()` is called before replacement. Unix uses same-directory `rename`;
Windows uses `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` and
`MOVEFILE_WRITE_THROUGH`. Any failure before replacement removes the temporary
file and leaves an existing destination untouched.

Every export starts from the immutable job source. A previous translated export
must not become the next revision's base, so incremental sections do not grow
with translation revisions.

This decision establishes a bounded-memory write side, not complete streaming
export. The current renderer still loads and mutates the complete source object
graph. Production integration requires a lazy source-object reader and renderer
APIs that return explicit staged delta objects instead of mutating a full
`Document`.

## Evidence

Automated tests cover:

- replacement of an existing destination with a valid incremental update;
- source SHA-256 mismatch and pre-cancellation preserving the old destination;
- temporary-file cleanup on failure;
- reopening the merged object graph with `lopdf`;
- a 30-page real-paper export containing two translated pages and one shared
  Arial Type0 subset;
- reopening that output with PDFium and extracting both translations.

The Windows AMD real-paper test measured:

- source: 1,590,242 bytes;
- output: 1,617,258 bytes;
- appended delta: 27,016 bytes (1.70% of source size);
- delta objects: 10;
- shared font subsets: one;
- retained pages: 30;
- Poppler page 1 changed 2,559 pixels (0.1176%), confined to the target footer;
- Poppler page 2 changed 2,059 pixels (0.0946%), confined to the target footer;
- Poppler page 3 remained pixel-exact;
- `pypdf` retained metadata and page 1-3 annotation counts `26, 31, 7`.

Independent extraction found each translation only on its target page. Visual
inspection found no clipping, overlap or unrelated movement.

## Consequences

### Positive

- Source bytes are never retained in a second in-memory `Vec` by the writer.
- Output work scales with a fixed copy buffer plus the explicit delta object set.
- Untouched source objects stay byte-identical in the copied prefix.
- Existing exports survive cancellation, source changes and pre-commit errors.
- Shared document fonts add one compact payload instead of one payload per page.

### Costs

- Incremental output cannot be smaller than the immutable source PDF.
- The current xref writer has a deliberate 4 GiB offset ceiling.
- A small PDF object serializer is now Rosetta-owned and needs malformed-object
  and fixture coverage.
- End-to-end export memory remains unbounded until object loading and renderer
  mutation move to page-local working sets and explicit deltas.

## Rejected Alternatives

- Use `lopdf::IncrementalDocument` while ignoring its retained source bytes and
  previous object graph.
- Rewrite the complete PDF for every export.
- Persist one complete translated PDF per page and merge those files.
- Append to the previous translated export for every revision.
- Replace the destination before source identity and file sync complete.
