# ADR 0032: PDF v3 Same-Stream Multi-Text-Object Staging

Date: 2026-07-17

Status: Accepted

Amends ADR 0027, ADR 0029 and ADR 0031.

## Context

ADR 0031 allowed one translated replacement batch to span multiple streams and
Form invocation paths, but rejected two logical targets with the same
`stream + invocation path`. That prevented a common PDF layout: one content
stream contains many independent `BT`/`ET` text objects.

Treating those text objects as one logical target would weaken the existing
transaction invariant because each target must remain inside one `BT`/`ET`.
Staging each target independently would also be incorrect: two staged versions
of the same physical stream would overwrite one another at commit, and a Form
copy-on-write node accepts only one completed leaf stream.

## Decision

Logical validation and physical stream staging are separate renderer layers.

Each logical target retains the existing safety boundary: one page, underlying
stream, complete invocation path and one source `BT`/`ET` bounds pair. A batch
may contain multiple targets with the same `stream + invocation path` when
their source text-object bounds differ. Repeating the same physical key and
text-object bounds is a typed `DuplicateBatchTarget` failure.

Every logical target is planned independently against the unchanged source
content. Its operation hashes, source text state, anchors, PageGraph geometry,
style, font coverage and fit must pass before physical grouping.

After logical planning, targets are grouped by physical
`stream + invocation path`. The renderer:

1. decodes the source stream once for the physical group;
2. unions all already validated replacement operations;
3. rejects duplicate operation indices defensively;
4. splices replacements in descending source operation order;
5. encodes and compresses one completed staged stream.

The resulting physical stream enters the existing ownership path exactly once.
A unique top-level stream is updated once. A Form invocation contributes one
leaf to the clone tree, regardless of how many logical text objects it contains.
If another target requires copy-on-write, the physical stream participates in
the same atomic page-level clone forest defined by ADR 0031.

Logical target diagnostics remain separate and source-ordered. The existing
batch `/1`, batch-target `/1`, transaction `/3` and per-show `/6` diagnostic
schemas do not change because their fields already identify each replacement
operation without serializing source text.

## Evidence

Automated Windows tests prove:

- two independent `BT`/`ET` targets in one unique top-level stream produce one
  physical stream rewrite, one six-object font subset and no clone;
- the selected page still references the same top-level stream object;
- two `BT`/`ET` targets in one selected Form invocation produce one leaf clone
  plus one root clone, not two independent leaf paths;
- the unselected sibling Form invocation retains the source text;
- a stale second-target hash leaves every object and `max_id` unchanged;
- repeating the same text-object target is rejected with zero mutation;
- diagnostics contain neither source nor translated text.

The Source Han top-level probe completed both logical targets in about 4 ms.
The source was 13,473 bytes and the searchable output was 16,488 bytes, a
3,015-byte increase. PDFium and `pypdf` extracted `甲OBJECT` and `乙OBJECT`.
Poppler differences were confined to the two original text rows with no
clipping, overlap or unrelated page changes.

## Consequences

### Positive

- Normal content streams with many positioned text objects no longer require
  one artificial mega-transaction or source preservation.
- Validation remains local to one `BT`/`ET`, while encoding and ownership are
  deduplicated at the physical stream boundary.
- Form clone count follows invocation paths rather than logical text-object
  count.
- All targets still plan against stable source operation indices.

### Costs

- A logical target still cannot cross `BT`/`ET` boundaries.
- Unanchored consecutive shows inside one text object remain preserved.
- The renderer still decodes the same source stream once per logical target
  during validation, then once for physical staging. Shared read-only planning
  context can remove that repeated decode later without changing the contract.
- Paragraph reflow, protected spans, durable patches and bounded-memory export
  remain pending.

## Rejected Alternatives

- Flatten all same-stream requests into one cross-`BT`/`ET` transaction.
- Commit one staged stream per logical target and keep the last result.
- Allow duplicate text-object targets and resolve them by request order.
- Recalculate operation indices after applying each target.
