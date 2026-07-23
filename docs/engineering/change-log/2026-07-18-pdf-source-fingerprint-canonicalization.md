# PDF Source Fingerprint Canonicalization

Date: 2026-07-18

## Summary

Aligned imported PDF source metadata with the native PDF v3 source identity
contract.

## Implementation

- changed `pdf_source.json.sourceFingerprint` generation from bare lowercase
  SHA-256 hex to canonical `sha256:<64 lowercase hex>`;
- added a real Windows/PDFium fixture test proving source metadata and
  `DocumentHandle` produce the exact same identity;
- kept source hashing streaming with the existing fixed 64 KiB buffer.

## Compatibility

Existing beta PDF source metadata with a bare digest compares as a changed
source during repair. Its derived PDF state is discarded and rebuilt from the
immutable cached `source.pdf`; it is not migrated into PDF v3 authority.

## Validation

- focused source-state identity test;
- complete native/job validation is recorded with the lifecycle stage that
  consumes this identity.
