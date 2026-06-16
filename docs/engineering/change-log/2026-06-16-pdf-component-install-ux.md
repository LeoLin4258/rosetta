# 2026-06-16 PDF Component Install UX

## Context

After the PDF pack was changed to bundle DocLayout-YOLO, users without the updated PDF component could still discover the problem only after entering the PDF translation flow. The settings page also hid the primary install action behind "管理 PDF 组件" and mixed multiple names for the same module.

## Change

- Header PDF status now keeps missing or stale PDF components visible and routes the user to Settings.
- PDF translation no longer installs the component from the workspace. If the component is missing, the normal translate action is disabled and explains that the PDF component must be installed from Settings.
- PDF component download and local archive import are now Settings-only flows. Onboarding no longer downloads the PDF component; users can install it from Settings after entering the app.
- The Settings PDF section is always visible, consistently named "PDF 组件", and shows install/import/proxy controls before technical details.
- The macOS PDF pack profile now lists the GitHub release URL first and the `githubdog.com` mirror second for mainland users.

## Compatibility

Existing users with an installed, current PDF component continue without action. Existing users with an older beta.9/beta.11 component are detected as needing update and are guided to reinstall the PDF component from Settings.

## Validation

- `pnpm typecheck`
- `cargo check`
- `cargo test managed_pdf2zh::profile::tests --lib`
- `cargo test managed_pdf2zh::status::tests --lib`
- `cargo test rosetta_jobs`
