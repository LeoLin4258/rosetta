# ADR 0016: PDF v3 Hybrid PDFium Extraction and Content-Stream Rendering

Date: 2026-07-17

Status: Accepted

Form traversal and recursive identity-render boundaries refined by ADR 0019 and
ADR 0020. Extraction adapter ownership and single-pass identity are refined by
ADR 0050. Page-local content mapping reuse is refined by ADR 0051. Bounded lazy
selected-page extraction mapping is refined by ADR 0052.

## Context

ADR 0015 established Rosetta-owned `PageGraph` and patch-first contracts but
left the native PDF engine boundary open pending identity-render experiments.

The experiments produced three distinct results:

- PDFium provides fast selected-page character extraction, geometry, style,
  object-order provenance, preview rendering, and validation;
- PDFium `set_text()` does not preserve original encoding, glyph placement, or
  extracted text on a real paper page;
- PyMuPDF/MuPDF high-level redact-and-reinsert cannot replay every embedded font
  and causes large visual and extraction-order changes.

Reconstructing a page from extracted Unicode spans therefore cannot satisfy the
v3 visual-fidelity contract. Rosetta needs a renderer boundary below Unicode
layout reconstruction.

## Decision

PDF v3 will use a hybrid native architecture:

- PDFium is the extraction, random-page inspection, preview, and validation
  engine;
- a Rosetta-owned Rust patch layer operates on PDF content-stream operators and
  encoded string operands;
- stable patch provenance uses the source fingerprint, page number, content
  stream object/generation, operation index, operand index, and optional `TJ`
  array index;
- original text matrices, `TJ` advances, font resource references, graphics
  order, and untouched operators remain authoritative;
- unsupported or ambiguous operators remain unchanged and produce typed
  preservation reasons.

The current spike uses `lopdf` for parsing and writing, but the durable patch
contract will not expose `lopdf` types. The library remains replaceable if its
memory, malformed-file, incremental-write, or performance behavior fails the
fixture and stress corpus.

## Identity Evidence

On page 1 of `2305.13048v2.pdf`, the Rust operator spike processed:

- 2 page content streams;
- 752 total operators;
- 242 text-show operators;
- 779 literal or hexadecimal string operands, including strings inside `TJ`
  arrays;
- 0 malformed text-show operators;
- 0 content streams shared with another page.

Every encoded text operand was replaced with an identical byte vector and the
streams were re-encoded and saved. Results:

- PDFium text: exact, 3,909 / 3,909 characters;
- PDFium pixels: 0 changed;
- independent Poppler PNGs: byte-identical and 0 changed pixels;
- content-stream parse and rewrite: about 21 ms;
- complete save and PDFium validation: about 462 ms;
- output: 1,506,085 bytes, 94.71% of the 1,590,242-byte source.

The simple LibreOffice fixture also passes exact text and pixel identity.

## Safety Boundary

Identity success proves that Rosetta can locate and rewrite encoded operands
without reconstructing page layout. It does not yet prove that arbitrary
translated Unicode can be inserted safely.

Production rendering remains blocked on explicit solutions for:

- source font encoding, `Encoding`, `ToUnicode`, and composite `CMap` handling;
- document-level CJK font embedding and subsetting without per-page duplication;
- deterministic line fitting, overflow detection, and safe source preservation;
- `Form` XObject content streams and nested resource inheritance;
- content streams or form resources shared by multiple pages, which require
  copy-on-write before patching;
- direct `/Contents` streams, encrypted PDFs, damaged streams, Type3 text, RTL,
  vertical writing, and other unsupported cases;
- incremental or random-access writing with bounded memory for very long PDFs.

The original spike loaded the complete PDF into `lopdf::Document`. ADR 0052
removes that requirement from selected-page extraction and mapping; production
rendering and incremental export already use the same bounded lazy source view.

## Consequences

### Positive

- Extraction and rendering can evolve independently behind stable Rosetta
  contracts.
- The normal path retains exact encoded glyph data, matrices, spacing, colors,
  and graphics ordering until a patch explicitly changes an operand.
- Citations and mixed-style spans can reference deterministic low-level source
  boundaries instead of relying on string search.
- Identity validation can detect renderer regressions before translation is
  involved.
- Reusing document resources avoids the per-page PDF artifact model that caused
  excessive disk usage.

### Costs

- Unicode-to-PDF encoding and font management become explicit Rosetta-owned
  engineering responsibilities.
- PageGraph atoms must be mapped to low-level operands without leaking library
  types into persistent data.
- Nested forms and shared resources require graph-aware copy-on-write behavior.
- A bounded-memory writer or incremental export strategy may require work below
  the current `lopdf` API.

## Rejected Alternatives

- Use PDFium `set_text()` as the production replacement renderer.
- Use PyMuPDF redaction plus Unicode text reinsertion.
- Rasterize translated pages and discard searchable text or vector content.
- Treat complete translated page PDFs as durable translation state.
- Expose content-stream library objects directly through the frontend or job
  persistence contracts.
