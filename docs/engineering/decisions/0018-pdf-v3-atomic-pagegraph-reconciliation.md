# ADR 0018: PDF v3 Atomic PageGraph Reconciliation

Date: 2026-07-17

Status: Accepted

Form XObject preservation boundary superseded by ADR 0019.

## Context

ADR 0017 separated PDFium geometry from ToUnicode source text and established
object-to-text-show candidate mapping. Object-level hashes and counts are not
enough for durable patches: a translated span must ultimately identify the
exact encoded operand bytes that produced each source atom.

The fixture corpus exposes four different relationships:

- PDFium and ToUnicode agree on the Unicode character;
- ToUnicode corrects a PDFium Unicode defect;
- PDFium inserts visual whitespace that has no encoded operand;
- ToUnicode contains whitespace for which PDFium exposes no geometry atom.

An object can also fail because of a missing font decoder, incomplete atom
coverage, an empty decoded source unit, missing page atoms, or Form XObject
resource nesting. Applying successful atom changes before discovering a later
failure would create an internally inconsistent PageGraph.

## Decision

PDF v3 adopts PageGraph schema v2 and an atomic object reconciliation stage.

Each atom has one explicit source state:

- `pdfium-unverified` before reconciliation;
- `pdfium-verified` when PDFium and the source decoder agree;
- `to-unicode-corrected` when validated source decoding replaces PDFium text;
- `pdfium-synthetic-whitespace` when PDFium generated spacing without an
  encoded source unit;
- `preserved-unmapped` when the object is not safely reconcilable.

Mapped atoms reference stable provenance containing:

- mapping ID;
- text-show ID and ordinal index;
- operand ID and operand index;
- optional `TJ` array index;
- encoded byte start and length within that operand;
- character index and count within the decoded source unit.

The IR stores locations and hashes, not copies of encoded source bytes. A
ligature or other multi-character ToUnicode destination shares one byte range
across its atoms and distinguishes them with source-unit character indexes.

Reconciliation first builds a complete update plan for a text object. It only
mutates PageGraph after count, font, atom coverage, decoder and character
alignment checks all pass. Any failure preserves the whole object.

Source whitespace without a PDFium atom is counted but receives no fabricated
geometry. Its encoded bytes remain untouched. PDFium synthetic whitespace is
kept for reading order but receives no fabricated operand provenance and is not
translated.

Page reconciliation is explicitly `unreconciled`, `complete`, `partial` or
`preserved`. A Form XObject gap keeps a page `partial` even when all top-level
text objects map successfully.

## Evidence

The simple one-page fixture reconciles completely:

- 86 PageGraph atoms;
- 80 PDFium/source-verified atoms;
- 6 synthetic whitespace atoms;
- 0 preserved atoms;
- 4 / 4 text objects mapped.

Page 1 of `2305.13048v2.pdf` produces:

- 3,911 PageGraph atoms;
- 3,238 verified atoms;
- 15 ToUnicode-corrected atoms;
- 602 PDFium synthetic whitespace atoms;
- 2 source whitespace characters without PDFium atoms;
- 56 preserved Form XObject atoms;
- 242 / 242 top-level text objects mapped;
- `partial` page status because Form XObject recursion is still unsupported;
- about 606 ms in the unoptimized spike, including repeated PDFium and `lopdf`
  whole-file loads.

The first-page corpus also demonstrates conservative behavior:

- `pdflatex-image.pdf`: complete, 0 preserved atoms;
- `multicolumn.pdf`: partial, 49 / 3,512 atoms preserved because one decoded
  source unit is empty;
- `google-doc-document.pdf`: partial, 16 / 1,139 atoms preserved, with font and
  missing-page-atom reasons;
- `GeoTopo.pdf`: preserved, 82 / 86 non-synthetic atoms preserved because its
  relevant content requires Form XObject recursion.

## Consequences

### Positive

- Translation patches can target deterministic encoded byte ranges instead of
  searching reconstructed Unicode strings.
- PDFium Unicode defects can be corrected without discarding its geometry.
- Ligatures retain an explicit many-Unicode-to-one-encoded-unit relationship.
- Partial support is inspectable at page, object and atom levels.
- Unsupported content remains unchanged and cannot be silently treated as a
  successful extraction.

### Costs

- Reconciliation adds a second representation and an explicit alignment pass.
- Source whitespace without geometry cannot yet become a standalone visual atom.
- Form XObjects require recursive content and resource traversal.
- The spike loads the source repeatedly and is not the production DocumentHandle
  or bounded-memory long-document implementation.

## Rejected Alternatives

- Store only object-level text hashes in PageGraph.
- Assign every atom in an ordinally matched object to the whole text show.
- Give PDFium synthetic whitespace fake operand byte ranges.
- Mutate atoms incrementally and preserve only the suffix after an error.
- Drop unsupported atoms from PageGraph and report the page as complete.
