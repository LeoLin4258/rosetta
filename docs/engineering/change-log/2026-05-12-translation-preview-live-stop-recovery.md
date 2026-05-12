# Translation Preview Live Status, Stop, and Recovery

## What changed

- Independent translation preview windows now refresh the opened translation file while it is visible, so source/translation comparison updates during an active translation run.
- Translation preview windows show a top status bar with file status, completed segment count, failed segment count, remaining count during translation, and progress percent.
- The main workbench translation row now exposes a Stop action for the currently running translation file.
- Preview-window block retranslation also exposes Stop while a local retranslation is running.
- Before sending a batch to the RWKV API, Rosetta persists the affected translation segments as `translating`, so other windows can reflect in-flight progress.
- When the user stops a run, the current in-flight batch is restored from `translating` to `pending`; completed segments remain completed.
- When a project is loaded, any persisted `translating` translation segments from a previous app session are recovered to `pending`, then translation file status is rebuilt from the recovered segments.

## Why

The RWKV backend can occasionally hang or take longer than the user is willing to wait. Previously, the UI had no way to stop the current translation loop, and persisted `translating` segments could survive app restart, leaving a translation file stuck in an impossible running state.

`translating` is now treated as an active frontend-run marker, not as proof that a durable background worker exists after restart.

## Notes

- Stopping a run does not currently abort the underlying Tauri `invoke` / HTTP request already sent to the model. It stops Rosetta's frontend translation loop and prevents that run from advancing subsequent batches.
- The current batch is made retryable by restoring it to `pending`.
- Full request cancellation can be added later by introducing an explicit cancellation-aware Tauri command/API boundary.

## Validation

- Ran `tsc --noEmit`.
- No dev server or production build was run.
