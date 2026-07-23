# ADR 0020: PDF v3 Recursive Identity Stream Rendering

Date: 2026-07-17

Status: Accepted

## Context

ADR 0016 selected a Rosetta-owned content-stream renderer and proved identity
rewrites for top-level page content. ADR 0019 added invocation-aware recursive
Form mapping, but the renderer still ignored Form streams.

Renderer identity and mapping identity have different ownership:

- mapping sees each visual Form invocation and its geometry;
- the PDF stores one underlying Form stream that may be invoked many times;
- rewriting the same shared stream once affects every visual invocation;
- rewriting it once with identical bytes is safe;
- applying different translated text to one invocation is not safe without
  copy-on-write.

## Decision

The PDF v3 identity renderer uses a two-pass recursive stream model.

### Discovery pass

The renderer first walks selected page content and every indirect Form `Do`
invocation without mutating the document. It records:

- unique stream object IDs in deterministic first-seen order;
- full Form invocation paths;
- total Form invocation count;
- unique and shared Form stream counts;
- direct-stream, cycle and 32-level depth fallbacks.

Form-owned resources take priority and missing categories fall back to the
parent invocation context.

### Rewrite pass

After discovery is complete, each unique stream object is decoded, inspected,
identity-rewritten, encoded and written back exactly once. A shared Form with
multiple invocation paths still receives one stream rewrite.

Content-stream inspection distinguishes:

- top-level page streams;
- Form streams;
- cross-page sharing;
- sharing across Form invocations.

Identity rewriting does not authorize translated patches against shared Form
invocations. ADR 0019 preservation remains in force until copy-on-write or a
validated whole-stream translation policy exists.

## Evidence

On page 1 of `2305.13048v2.pdf`, recursive identity rewriting processes:

- 2 top-level page streams;
- 5 unique Form streams;
- 27 Form invocations;
- 4 shared Form streams;
- 1,360 operations;
- 258 text-show operators;
- 800 text operands;
- 800 identity-rewritten operands;
- 0 malformed text-show operators.

The output preserves:

- PDFium text: exact, 3,909 / 3,909 characters;
- PDFium pixels: 0 changed;
- Poppler page PNG: byte-identical SHA-256 to the source render;
- page count: exact.

One Windows debug probe measured about 74 ms for recursive parse/rewrite and
about 725 ms total including save and PDFium validation. Output size was
1,505,764 bytes, 94.69% of the source.

Poppler reported the same missing local display-font warnings for source and
output. The rendered PNGs remained byte-identical.

## Consequences

### Positive

- Identity coverage now includes nested Form content.
- Shared Form streams cannot be rewritten repeatedly by accident.
- Renderer statistics describe visual invocation sharing separately from
  underlying stream ownership.
- PDFium and independent Poppler validation agree on zero visual change.
- The renderer has the discovery graph required for future copy-on-write.

### Costs

- Recursive discovery parses Form content before the rewrite pass parses each
  unique stream again.
- The current probe still loads and saves the complete `lopdf::Document`.
- Form sharing across pages is not yet fully indexed without scanning other
  pages' content; current cross-page reporting remains exact for direct page
  content streams and selected-page Form invocations.
- Identity does not prove translated Unicode encoding, fitting or font reuse.

## Rejected Alternatives

- Continue validating only top-level page streams.
- Rewrite a shared Form once per visual invocation.
- Flatten Form operations into the page content stream.
- Treat identity success as permission for invocation-local translated patches.
- Mutate streams while still discovering the Form graph.
