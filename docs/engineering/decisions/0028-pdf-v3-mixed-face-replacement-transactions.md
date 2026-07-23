# ADR 0028: PDF v3 Mixed-Face Replacement Transactions

Date: 2026-07-17

Status: Accepted

Amends ADR 0023 and ADR 0027. Form/shared-stream target restrictions are
amended by ADR 0029.

## Context

ADR 0027 made several anchored text shows atomic, but required every show to
use one prepared translation face. Real PDF text objects can switch between
Regular and Bold while retaining valid positioning anchors. Splitting those
shows into separate commits would permit partial output and would not preserve
the source style boundary as one renderer transaction.

Staging two fonts independently against the same unmodified `Document.max_id`
would allocate overlapping PDF object IDs. Materializing page resources once
per face would also let the second cloned dictionary overwrite the first. Both
resources and both six-object font subsets therefore need one allocation and
commit plan.

## Decision

A text-show replacement transaction accepts a set of prepared translation
faces keyed by `TranslationFontWeight`. Each request still derives its face
only from its reconciled PageGraph style and validated source paint state. A
missing or duplicate required face is a typed transaction failure.

The renderer plans every replacement against the unchanged source stream. It
then collects the used face set in deterministic Regular/Bold order and:

- allocates each six-object Type0/CIDFont subset after the previously reserved
  object number, without mutating the document;
- materializes the page resource dictionary once and attaches all required
  font resources together;
- encodes each replacement with the prepared face selected by its style;
- commits all font objects, the rewritten stream, the page dictionary and
  `max_id` only after every validation and staging step succeeds.

Single-show replacement remains a one-request transaction. The transaction
diagnostic reports `translationFontWeights` rather than one weight. Per-show
diagnostics advance to `rosetta-pdf-v3-text-show-replacement/5`; transaction
diagnostics advance to
`rosetta-pdf-v3-text-show-replacement-transaction/2`.

## Evidence

The Windows real-paper fixture contains one anchored `BT`/`ET` with eligible
Regular and Bold shows. The automated transaction test proves:

- both replacements are searchable and retain their selected Arial
  Regular/Bold face;
- two distinct page font resources reference distinct Type0 objects;
- 12 staged font objects increase `max_id` by exactly 12;
- omitting the required Bold face leaves every document object and `max_id`
  unchanged.

The Source Han Sans CN Regular/Bold probe replaced one body show and one Bold
heading show at fit scale 1.0:

- transaction time: about 13 ms in the Windows debug probe;
- source PDF: 1,590,242 bytes;
- output PDF: 1,511,382 bytes, 78,860 bytes smaller than source;
- PDFium searchable text and distinct production font faces: passed;
- Poppler page-1 changes: 1,373 of 2,005,644 pixels, confined to the two target
  regions;
- Poppler page 2: pixel-exact.

## Consequences

### Positive

- One source text object can preserve validated Regular/Bold boundaries in one
  atomic renderer transaction.
- Font object IDs and page resource updates cannot collide or overwrite each
  other.
- Bold is still loaded only when at least one planned show requires it.
- The output embeds one subset per used face, not one subset per show.

### Costs

- Italic and other translation faces remain unsupported.
- A single source show that resolves to mixed PageGraph styles remains a typed
  preservation fallback.
- ADR 0029 extends the transaction to one validated Form invocation or one
  shared top-level page stream. Paragraph layout remains disconnected.
- Document-wide reuse across several page transactions still needs the Phase 4
  patch/export resource registry.

## Rejected Alternatives

- Split Regular and Bold shows into separate PDF mutations.
- Let the caller choose a face independently of PageGraph style.
- Stage both faces from the same `Document.max_id`.
- Clone and commit the page resource dictionary once per face.
- Synthesize Bold by reusing or transforming the Regular face.
