# 2026-05-08 Task Workbench UX, File Delete, and ScrollArea

## What changed

- Refined the Jobs workbench header so it clearly shows:
  - the current project
  - the current file
  - file-level progress
  - available actions
- Changed the delete action on the Jobs page from deleting the whole project to deleting the currently selected file.
  - When the current file is the last file in the project, the project is removed.
- Replaced the hand-rolled preview overflow container with shadcn `ScrollArea`.
- Tightened shared visual details:
  - more consistent rounded corners
  - less ad hoc styling in the workbench
  - removed the temporary `.xxx` CSS helper

## Notes

- This update keeps the Rosetta workbench aligned with the current single-file-at-a-time preview flow.
- The delete-file flow updates the local job cache and preserves the remaining files in the project.
