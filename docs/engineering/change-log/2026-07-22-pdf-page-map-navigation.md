# PDF Page Map and Navigation

## Summary

The PDF workbench now separates page navigation from translation selection.
The source preview has a fixed page map, while the top workbench controls expose
the current viewport page and a direct translation-range input.

## Changes

- Replaced the checkbox beside each virtualized PDF row with a fixed page map on
  the left of the source preview.
- Added independent visual channels for translated state, selected state,
  current viewport page, and hover inspection.
- Added a compressed global map for long PDFs. Hovering or focusing the rail
  expands it into a stable page workspace instead of placing controls in a
  pointer-following popover.
- The expanded workspace uses a virtualized page list beside a fixed source-page
  preview, so checkboxes, page navigation and Shift range selection remain
  usable without rendering every page.
- The collapsed rail uses a narrow document-status spine with a distinct blue
  current-page marker. The expanded preview constrains the raster to a stable
  paper aspect ratio and uses contain fitting to avoid image distortion.
- The source-page inspector now uses the available vertical space for a
  previous/current/next filmstrip. The hovered page remains visually primary,
  while either adjacent page can be clicked to navigate without destabilizing
  the list hover state.
- Reduced nested borders, filled labels, saturated status marks, and the visual
  weight of the collapsed rail so navigation remains legible without competing
  with the document itself.
- Removed the expanded panel's repeated page totals, selection summary, preview
  heading, status copy, and thumbnail page captions. The page list now carries
  the visible text while the filmstrip remains a direct visual target.
- Removed thumbnail transition animation so inspected-page changes are
  immediate. The collapsed rail now uses the same surface token as the adjacent
  PDF canvas and omits its right border until expanded.
- Refined the collapsed page map from a segmented progress bar into a one-pixel
  neutral document spine. Only active translation states add muted color, while
  the viewport position uses a compact blue point.
- The current-page point now eases between viewport positions. The expanded
  source filmstrip no longer scrolls: thumbnails use the full preview width,
  the current page stays centered, and adjacent pages extend beyond a masked
  top and bottom edge to communicate intentional clipping.
- Main source and translated page frames now derive their aspect ratio from the
  rendered PNG instead of assuming portrait A4. The ratio is cached with the
  raster so landscape and non-standard pages fill their frame without
  letterboxing or repeated layout correction.
- Added viewport-center current-page tracking and virtualized page navigation.
- Added topbar previous/next controls, direct page entry, and page-range
  selection such as `21-30` or `21-30,42`.
- Preserved the existing automatic next-untranslated-page selection policy.

## Boundaries

- This is a frontend interaction change only. It does not alter PDF job data,
  scheduler authority, persistence, translation output, or Tauri permissions.
- PDF page content remains virtualized through `@tanstack/react-virtual`.
- Translation selection remains disabled while a PDF run is active.

## Validation

- `cd rosetta-app && pnpm typecheck`
- `git diff --check --` for the changed frontend and change-log files
