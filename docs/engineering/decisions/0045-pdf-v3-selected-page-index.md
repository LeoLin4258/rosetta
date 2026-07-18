# ADR 0045: PDF v3 Selected-Page Index

Date: 2026-07-18

Status: Accepted

Amends ADR 0042, ADR 0043 and ADR 0044.

## Context

The lazy source store and accumulated overlay removed complete source loading
from font staging and export writing, but page replacement still called
`Document::get_pages()` for every preflight and batch. It also asked the
complete document to enumerate the selected page's content streams.

This made page identity an implicit renderer side effect, repeated page-tree
work across targets, and prevented a caller from proving that only an explicit
page selection was navigated.

## Decision

PDF v3 adds a transient `PdfPageIndex` resolved from immutable
`PdfObjectView`. It starts at trailer `/Root`, resolves catalog `/Pages`, and
records only an explicit `PageSet`:

- 1-based page number and exact page object ID;
- ancestor `/Pages` object IDs;
- direct `/Contents` stream references.

The traversal uses `/Count` to skip subtrees that do not intersect the
selection and stops after the greatest selected page. Along encountered
selected paths it rejects malformed root/catalog/page-tree structure, cycles,
repeated ownership, depth beyond 64, invalid counts or contents, overflow and
missing selected pages.

TranslationPatch and replacement staging now receive the index separately
from both the temporary complete traversal document and accumulated object
view. The index owns selected-page identity. The accumulated overlay remains
the authority for font lookup, maximum object number and clone allocation.

Compatibility mutation and single-page artifact APIs construct a one-page
index from their owned `Document`. Final multi-page staging constructs one
index from the lazy source for the complete explicit selection and reuses it
across pages.

## Evidence

Automated Windows AMD tests prove:

- sparse nested-tree selections resolve the same page and content object IDs
  as `lopdf`;
- selecting only page 1 of the real 30-page paper requires at most four lazy
  source-object loads;
- out-of-bounds pages, cycles and repeated page ownership are rejected;
- the real two-page renderer reuses one lazy index while leaving the complete
  source traversal document unchanged;
- output remains 1,617,258 bytes from a 1,590,242-byte source, with 27,016
  appended bytes and 10 delta objects, unchanged from ADR 0044.

## Consequences

### Positive

- Page selection is explicit, typed and independently testable.
- Unselected later page-tree subtrees need not be loaded.
- Replacement preflight and staging no longer enumerate all pages to resolve
  the selected page or its top-level content roots.
- The index records the ancestor identity needed for later inherited-resource
  migration without retaining page content bytes.

### Costs

- `/Count` is trusted for subtree skipping; malformed count values produce a
  typed failure or missing-page result rather than a best-effort scan.
- Inherited resource lookup, content decoding and global cross-page ownership
  checks still use a complete `lopdf::Document`.
- The current index is transient and is rebuilt after process restart.
- Callers must build the index and temporary traversal document from the same
  immutable source identity; cross-source negotiation is not part of this
  transient adapter.
- End-to-end renderer memory is not yet bounded by source object count.

## Rejected Alternatives

- Continue calling `Document::get_pages()` inside every renderer operation.
- Build and retain an unconditional all-pages map for every PDF.
- Let the accumulated delta overlay define source page-tree identity.
- Combine page indexing with resource and content migration in one unbounded
  architectural change.
