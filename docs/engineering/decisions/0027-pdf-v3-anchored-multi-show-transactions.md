# ADR 0027: PDF v3 Anchored Multi-Show Transactions

Date: 2026-07-17

Status: Accepted

Amends ADR 0024, ADR 0025 and ADR 0026.

Mixed-face transaction restrictions are amended by ADR 0028.
Shared-stream and Form target restrictions are amended by ADR 0029.

## Context

The initial replacement path required its target to be the final text show in a
`BT`/`ET` object. This prevented a translated show's changed advance from moving
later source text, but it rejected many safe real documents. The paper and
Google Docs fixtures commonly emit one show per positioned fragment and place a
`Td` before the next show.

A PDF text show advances the current text matrix but does not advance the text
line matrix. `Tm`, `Td`, `TD` and `T*` establish the next text matrix from an
explicit matrix or the text line matrix. The quote operators also perform an
implicit `T*`. A valid operator of this kind therefore cuts the dependency on
the previous show's advance. Consecutive `Tj`/`TJ` operations without such an
anchor remain dependent and cannot be replaced safely without exact source-font
advance compensation.

Replacing several independently anchored shows one at a time would still
permit partial output if a later hash, style or fit check failed. They need one
transaction boundary.

## Decision

PDF v3 supports an anchored multi-show replacement transaction. Every request
in one transaction must:

- target the same 1-based page and unique top-level page content stream;
- belong to the same `BT`/`ET` text object;
- have a distinct operation index and unchanged operator/operand hash;
- pass the existing source state, PageGraph geometry, style, paint, glyph and
  fit gates;
- select the same prepared Regular or Bold translation face;
- either be the final show in the text object or have a validated text-position
  anchor before the next show.

Accepted anchors are finite, correctly shaped `Tm`, `Td`, `TD` and `T*`
operations, plus well-formed quote text-show operators. An unanchored later
`Tj`/`TJ`, malformed positioning operator, nested/missing text-object boundary,
cross-stream request or duplicate operation is a typed preservation failure.

The renderer plans every replacement against the original content stream. It
does not mutate the operation list while validating later provenance. After all
requests pass, replacement sequences are spliced in descending operation-index
order. One rewritten stream, one cloned page resource dictionary and one staged
font subset are then committed together. Any failure before commit leaves the
document object table and `max_id` unchanged.

The per-show diagnostic schema advances to
`rosetta-pdf-v3-text-show-replacement/4`. The transaction result uses
`rosetta-pdf-v3-text-show-replacement-transaction/1` and reports page, stream,
face, replacement count, staged object count and timing without source or
translated text.

## Evidence

Automated Windows tests use two real-paper Bold shows in one `BT`/`ET`:

- both translated shows are searchable after one transaction;
- one six-object Arial Bold subset is staged and reused;
- operation results remain in source order after descending splices;
- a stale hash on the second request rejects the transaction;
- rejection leaves every document object and `max_id` unchanged;
- consecutive unanchored shows and malformed `Td` are typed failures.

The Source Han Sans CN Bold probe replaced two real-paper shows with `甲` and
`乙`:

- both fit scales: 1.0;
- replacement transaction: about 15 ms in the Windows debug probe;
- PDFium text, Bold face and black fill validation: passed;
- source PDF: 1,590,242 bytes;
- output PDF: 1,508,982 bytes, 81,260 bytes smaller;
- Poppler page-1 differences were confined to the two target glyph regions in
  one author line;
- the complete page 2 Poppler render was pixel-exact.

## Consequences

### Positive

- Common LaTeX and Google Docs positioned-show sequences no longer fail merely
  because another show exists later in the text object.
- Multiple source identities, styles and fits share one atomic commit boundary.
- Later source positioning remains independent from translated glyph advance.
- One font subset and page resource update serve all transaction entries.

### Costs

- Consecutive unanchored `Tj`/`TJ` still preserve the source until exact source
  advance compensation exists.
- ADR 0028 now permits validated Regular/Bold shows in one transaction with
  atomic multi-face staging.
- Every entry still fits to its own PageGraph source-object region. Paragraph
  reflow and multi-object shared layout are not implemented.
- ADR 0029 connects shared streams and Form targets through invocation-local
  copy-on-write while retaining the same anchored-show transaction gates.

## Rejected Alternatives

- Remove the final-show gate without proving a position anchor.
- Assume every `Td`/`Tm` token is valid without checking its operands.
- Apply each request as an independent PDF mutation.
- Recalculate later provenance after inserting earlier replacement operations.
- Parse every source font's widths as a shortcut before the anchored path is
  exhausted.
- Lower the readability floor to make an oversized probe pass.
