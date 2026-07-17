# ADR 0031: PDF v3 Multi-Target Translated Replacement

Date: 2026-07-17

Status: Accepted

Amends ADR 0027, ADR 0028, ADR 0029 and ADR 0030.

## Context

The translated replacement planner previously accepted one logical target: one
page, underlying stream, complete Form invocation path and `BT`/`ET` text
object. ADR 0030 proved that the low-level executor could atomically merge
multiple Form paths and page-content roots, but translated replacement did not
yet use that capability.

Calling the single-target transaction repeatedly would duplicate unified fonts,
rebuild shared clone ancestors and allow a later failure to expose a partially
translated page. A mixed batch can also contain a Form target and an otherwise
unique top-level stream. Once any target requires page rewiring, mutating that
top-level stream in place would violate the batch ownership boundary.

## Decision

Translated replacement exposes one page-level batch containing one or more
logical target transactions.

Each target retains the existing transaction gates: all of its shows must use
the same page, stream, complete `FormInvocationStep[]` path and `BT`/`ET` text
object, with unique operation indices. A batch must target one selected page
and may contain each `stream + invocation path` key only once.

The planner validates every replacement target against the unchanged source
document before object ID reservation. This includes operand identity, content
state, PageGraph geometry and style, fit policy and font-face coverage. Targets
are sorted deterministically by stream and structured path. The clone stage
then validates every invocation path against that same unchanged document
before any object mutation.

Required translation faces are unioned across the full batch. Exactly one
document-level subset is staged per weight in deterministic order, regardless
of target count. Font object IDs are reserved before clone IDs.

If no target requires copy-on-write, all unique top-level streams are staged in
place and the selected page resources are updated once. If any target requires
copy-on-write, every target enters the ADR 0030 clone forest. This includes
otherwise unique top-level roots, preventing source mutation outside the atomic
page rewrite. Form leaves receive effective local resources; top-level targets
receive the shared staged page resources. All `/Contents` root replacements are
applied to one staged page dictionary.

Fonts, rewritten/cloned streams, the page dictionary and `Document.max_id` are
committed only after the complete batch stages successfully. Any target failure
leaves the document object table and `max_id` unchanged.

The page-level diagnostic schemas are:

- `rosetta-pdf-v3-text-show-replacement-batch/1`;
- `rosetta-pdf-v3-text-show-replacement-batch-target/1`.

The batch reports page, target/replacement counts, the union of font weights,
font object count, clone count, page rewiring and elapsed time. Each target
reports stream, invocation depth, replacement count, weights and per-show
diagnostics. Neither schema contains source or translated text.

The existing transaction API is a one-target wrapper over the batch planner and
keeps `rosetta-pdf-v3-text-show-replacement-transaction/3` for current internal
callers.

## Evidence

Automated Windows tests prove:

- two invocations of one shared Form receive different translated text while
  sharing one six-object Regular font subset;
- their clone forest contains one common root and two independent leaves;
- corrupting the second `Do` path rejects the full batch with zero mutation;
- one Form target plus one independent top-level target clones the Form root,
  Form leaf and top-level root, then rewires both page `/Contents` references in
  one commit;
- source root, Form and top-level streams remain byte-unchanged;
- the cloned Form owns the Rosetta font, the selected page owns the top-level
  Rosetta font binding, and the unselected sibling Form text remains present;
- serialized diagnostics contain neither source nor translated text.

The Source Han two-invocation probe completed two replacements and three clones
in about 4 ms. The source was 13,129 bytes and the searchable output was 17,044
bytes, a 3,915-byte increase. PDFium and `pypdf` extracted both distinct CJK
translations. Poppler rendered both translations on the original baselines;
pixel differences were confined to the two source text rows with no clipping,
overlap or unrelated page changes.

## Consequences

### Positive

- Page translation can commit multiple independent streams and Form paths as
  one atomic renderer operation.
- Shared clone ancestors and unified font subsets are created once per batch.
- Mixed ownership cannot leak in-place mutations when another target requires
  page rewiring.
- The compatibility API and per-show diagnostics remain stable.
- Output growth follows the union of clone paths and font faces, not the number
  of translated targets.

### Costs

- A batch still targets one page.
- Separate targets with the same stream and invocation path are rejected. Shows
  in one `BT`/`ET` must use one target transaction; multiple text objects in the
  same stream/path need a future grouping model.
- Unanchored consecutive shows, paragraph reflow, protected spans, durable
  `TranslationPatch` persistence and bounded-memory export remain pending.
- The current implementation still stages an in-memory `lopdf::Document`.

## Rejected Alternatives

- Call the single-target transaction once per translated unit.
- Embed one unified font subset per target.
- Mutate unique top-level targets in place while cloning sibling Form targets.
- Commit completed targets before validating the rest of the page batch.
- Merge targets by source stream object ID without the invocation path.
