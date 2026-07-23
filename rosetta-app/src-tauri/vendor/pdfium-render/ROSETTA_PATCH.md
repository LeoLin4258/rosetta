# Rosetta pdfium-render Patch

Upstream: `pdfium-render` 0.9.1 (`076dd8f3a6c7da9298ddffbcc0d5a109f89caf967fa4871c9a172d5b3498b35b`)

Rosetta vendors the minimum source required by the desktop build and the
PDFium 7763 bindings. The local patch adds two read-only methods:

- `PdfPageTextObject::object_identity()`;
- `PdfPageTextChar::text_object_identity()`.

Both return the same opaque, process-local PDFium object identity. Rosetta uses
that identity only while the page is open to associate every page character
with its exact text object in one pass. The value is never persisted and is
not treated as a PDF object number.

When updating upstream, replace the vendored source, retain the selected
binding version, reapply these two methods, and run the PDF v3 extraction
equivalence test before accepting the update.
