# PDF v3 Bounded Lazy Extraction Mapping

Date: 2026-07-18

## Summary

Removed the remaining complete `lopdf::Document` requirement from PDF v3
selected-page extraction, content mapping and PageGraph reconciliation.

## Changes

- changed `DocumentHandle` to own the bounded lazy source-object store and
  PDFium document instead of a complete lopdf object graph;
- resolved exact page identity, inherited resources, content streams, fonts
  and recursive Form XObjects through existing lazy page contracts;
- retained ToUnicode-first source decoding and the conservative lopdf fallback
  without giving that fallback the source document;
- expanded indirectly referenced top-level content arrays lazily;
- made raw xref-stream trailer parsing use the initialized source resolver so
  indirect stream lengths remain supported;
- added cache residency to the ten-page diagnostic and regression coverage for
  the newly exercised PDF structures.

## Evidence

- PDF v3: 147 passed, 14 ignored;
- three ten-page Windows AMD debug runs: 742-807 ms total, 404-434 ms mapping;
- 39,783 atoms and 219 page-local parsed-stream cache hits in every run;
- final lazy source cache: 167 entries / 524,541 estimated bytes, with 167
  source loads and 998 cache hits;
- no serialized schema, PageGraph identity, TranslationPatch identity or PDF
  output contract changed.
