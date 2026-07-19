# ADR 0071: PDF v3 Bounded Workbench Preview

Date: 2026-07-19

Status: Accepted

Refines ADR 0055, ADR 0069 and ADR 0070.

## Context

The native v3 scheduler, bounded run enumeration and lazy translated-page PNG
command existed, but the workbench still derived preview readiness from legacy
PDF page state and translated PDF paths. Loading every v3 page record would
reintroduce document-sized frontend state for hundreds-page PDFs, while a new
run dashboard would duplicate the existing document workbench.

The UI needs one reconstructible projection of durable native authority. It
must distinguish sparse non-requested pages from requested pages whose status
has not been fetched, avoid rendering a stale legacy artifact during run
discovery, and keep old beta jobs usable when no v3 run exists.

## Decision

Integrate v3 into the existing virtualized `PdfDocumentPreview`. For the active
target language, request only the newest validated run (`limit: 1`) and do not
persist that selection. Re-run discovery while a translation is starting so a
new revision can replace the previous projection without a frontend run index.

Map the first virtual row to a physical 64-page window and query the existing
bounded run-control command with an exclusive `startAfter` cursor. Retain at
most four windows. Sparse PageSets may make returned record windows overlap, so
merge overlapping page records by the most recent fetch rather than map
insertion order. Poll only the visible window while the run is nonterminal.
When the run becomes completed or cancelled, refresh any previously active
window once before treating its cached terminal projection as final.

Parse the scheduler's canonical PageSet into compact ranges for membership
checks without expanding it into a page array. A requested page with no cached
record remains a loading placeholder; a non-requested page is explicitly
outside this run.

Render exact `completed` pages with the v3 binary PNG command. Reuse the source
PNG for `preserved` pages. Keep pending, extracted, leased and failed pages as
typed placeholders. Include run ID, patch ID, translation revision and page
update identity in the frontend image render version. Do not consult legacy
translated PDF paths for a selected v3 run.

During initial v3 discovery, suppress legacy translated-page rendering. If no
v3 run exists after successful enumeration, retain the existing v1/v2 preview
path for beta job continuity; this fallback is not a migration or shared
authority. Enumeration failure stays visible and must not be interpreted as an
empty v3 run list.

## Consequences

### Positive

- Workbench memory for v3 status is bounded to at most 256 page records.
- Scrolling and polling do not read the complete status of a long PDF.
- Completed and preserved pages follow native durable authority without
  requiring a complete translated PDF.
- Existing virtual scrolling, page selection and legacy beta jobs remain in
  one workbench instead of gaining a parallel dashboard.

### Costs

- Entering a new 64-page region performs a status command before requested
  pages can render.
- Active visible windows poll once per second; run discovery also polls briefly
  while the primary translation state reports an active start.
- Pause, resume, cancel, retry and recovery controls are still a later
  workbench phase.

## Rejected Alternatives

- Fetch every requested page record when opening a PDF.
- Persist a frontend current-run pointer or duplicate scheduler page state.
- Generate a complete translated PDF before showing any completed page.
- Add a separate run-management dashboard for basic document preview.
- Treat missing page records as non-requested without consulting PageSet.
