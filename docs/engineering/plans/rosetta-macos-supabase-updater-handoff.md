# Rosetta macOS Supabase Updater Handoff

## Context

User asked to stop because token balance is low, and requested a progress/context document for another AI to continue.

Repository:

```txt
/Users/leolin/Documents/GitHub/rosetta
```

Current branch:

```txt
codex/macos-supabase-updater
```

The user wants communication in Chinese.

## Suggested Skills

- `handoff` was used to create this document.
- Next agent should use `superpowers:verification-before-completion` before claiming completion.
- If continuing implementation, use `superpowers:subagent-driven-development` or direct review discipline.
- If debugging release/update failures, use `diagnose` or `superpowers:systematic-debugging`.

## Goal

Implement app-internal updates for Rosetta, first release only:

```txt
macOS Apple Silicon / darwin-aarch64
```

The chosen architecture is:

```txt
local macOS build/sign/notarize
  -> signed Tauri updater artifact
  -> Supabase Storage private bucket
  -> Supabase app_releases metadata table
  -> Supabase Edge Function dynamic updater endpoint
  -> Tauri updater check/download/install
```

Supabase must only store app release artifacts and metadata. Do not upload user documents, translations, job caches, prompts, runtime logs, telemetry, or identity.

## Key Artifacts

Design spec:

```txt
docs/superpowers/specs/2026-05-22-macos-supabase-updater-design.md
```

Implementation plan:

```txt
docs/superpowers/plans/2026-05-22-macos-supabase-updater.md
```

Important implementation files:

```txt
rosetta-app/src-tauri/tauri.conf.json
rosetta-app/src/features/settings/SettingsPage.tsx
supabase/migrations/202605220001_rosetta_app_releases.sql
supabase/functions/rosetta-update/index.ts
rosetta-app/src-tauri/scripts/release-macos.sh
rosetta-app/src-tauri/scripts/publish-macos-updater.sh
docs/engineering/release/macos-release.md
docs/engineering/change-log/2026-05-22-macos-supabase-updater.md
```

Supabase project URL used in config/docs:

```txt
https://bdujdewqopcgwijhfbcz.supabase.co/
```

No Supabase service role key or Apple secrets were written here.

## Current Commit State

Recent commits on `codex/macos-supabase-updater`:

```txt
a55b7f4 Produce signed macOS updater artifacts
fc3067a Record pending Supabase endpoint validation
2aad5a5 Mark macOS updater validation complete
38b9d88 Fix updater rollback documentation
92a5b33 Add updater change log date
b51e1c4 Document Supabase updater release flow
24624a7 Improve macOS updater artifact discovery
2445cdf Honor publish script release variables
d183463 Add macOS Supabase updater publish script
b77a374 Add Supabase updater function
83433a0 Add Supabase release metadata schema
eef35db Update macOS updater settings copy
a7da45c Configure Supabase updater endpoint
6f5af7e Plan macOS Supabase updater implementation
```

`main` has:

```txt
74a5802 Document macOS Supabase updater design
```

Last known `git status --short --branch` before this handoff showed a clean branch except after final worker commit status was not rechecked by the main agent. Next agent should run:

```bash
cd /Users/leolin/Documents/GitHub/rosetta
git status --short --branch
git log --oneline --decorate -15
```

## What Was Done

1. Generated a new Tauri updater keypair outside the repo:

```txt
$HOME/.tauri/rosetta/updater.key
$HOME/.tauri/rosetta/updater.key.pub
```

Only the public key was committed into `tauri.conf.json`.

2. Updated Tauri updater endpoint:

```txt
https://bdujdewqopcgwijhfbcz.supabase.co/functions/v1/rosetta-update?target={{target}}&arch={{arch}}&current_version={{current_version}}
```

3. Updated Settings UI copy to macOS Apple Silicon/Supabase wording.

4. Added Supabase migration:

```txt
supabase/migrations/202605220001_rosetta_app_releases.sql
```

It creates private `rosetta-releases` bucket, `public.app_releases`, RLS deny policies, `darwin/aarch64` scope constraint, and updated-at trigger.

5. Added Edge Function:

```txt
supabase/functions/rosetta-update/index.ts
```

It returns 204 for unsupported platforms/no update, uses service role env inside Edge Function only, creates signed Storage URLs, and returns Tauri updater JSON. Final worker changed semver parsing to accept `+build.metadata` and ignore build metadata in comparisons.

6. Added/iterated publish script:

```txt
rosetta-app/src-tauri/scripts/publish-macos-updater.sh
```

Final worker changed it so it:

- only publishes versioned signed updater artifacts from `dist/release`;
- rejects unsupported overrides before upload;
- moves Supabase service role key out of curl argv into stdin `--config -`;
- uploads artifact then upserts release metadata as unpublished by default.

7. Final worker changed release script:

```txt
rosetta-app/src-tauri/scripts/release-macos.sh
```

It now creates updater artifact from the signed/stapled/Gatekeeper-verified app:

```txt
dist/release/Rosetta-<version>-macos-arm64.app.tar.gz
dist/release/Rosetta-<version>-macos-arm64.app.tar.gz.sig
```

This fixed the major review issue where publish might have uploaded unsigned Tauri build output.

8. Updated release docs and change log.

## Verification Already Run

Before final review:

```bash
cd rosetta-app && pnpm typecheck
cd rosetta-app/src-tauri && cargo check
```

Both passed. `cargo check` had existing warning about unused `PdfError::Encrypted`.

Supabase deployed endpoint checks returned:

```txt
HTTP/2 404 NOT_FOUND
Requested function was not found
```

This is recorded as pending in the implementation plan. It means `rosetta-update` was not deployed/reachable yet.

Final worker for commit `a55b7f4` reported these checks:

```txt
bash -n rosetta-app/src-tauri/scripts/release-macos.sh -> pass
bash -n rosetta-app/src-tauri/scripts/publish-macos-updater.sh -> pass
SUPABASE_SERVICE_ROLE_KEY=dummy rosetta-app/src-tauri/scripts/publish-macos-updater.sh -> expected exit 2 before upload, missing signed dist/release artifact
temporary artifact without .sig + publish script -> expected exit 2 before upload, missing signature
APP_NAME=bad SUPABASE_SERVICE_ROLE_KEY=dummy rosetta-app/src-tauri/scripts/publish-macos-updater.sh -> expected exit 2 at scope validation
deno check supabase/functions/rosetta-update/index.ts -> skipped, deno not installed
git diff --check -> pass
```

## Important Unfinished Work

User explicitly said stop. Do not keep implementing unless asked.

Next agent should first review commit `a55b7f4`, because it was not yet reviewed by a fresh reviewer after completion. Focus on:

- `release-macos.sh` signer invocation. Worker used `pnpm --silent "${signer_args[@]}" "$artifact_path"`. Confirm this actually invokes `pnpm tauri signer sign` correctly with array expansion.
- Confirm `.sig` content is exactly what Tauri updater expects. Earlier `pnpm tauri signer sign ...` printed a verbose success message plus signature; it also writes a `.sig` file. The worker may be capturing stdout into `signature` and writing it to `.sig`, which could be wrong if stdout includes non-signature text. This is the highest-priority thing to inspect.
- Confirm release script does not require updater signing key when user only wants DMG. Current worker intended updater artifact as required.
- Confirm publish script’s curl `--config -` placement works as intended and does not leak key in argv.
- Confirm docs now match scripts after `a55b7f4`.
- Consider adding `supabase/config.toml` with `verify_jwt = false` for `rosetta-update`, because deployment currently relies on remembering `supabase functions deploy rosetta-update --no-verify-jwt`.

## Known External Dependencies Missing Locally

These were not installed/available:

```txt
supabase CLI
deno
shellcheck
psql
Docker
```

Because of that:

- Supabase migration was not pushed.
- Edge Function was not locally served/deployed.
- Edge Function was not Deno-checked.
- Full updater smoke test was not run.

## Next Recommended Steps

1. Run status:

```bash
cd /Users/leolin/Documents/GitHub/rosetta
git status --short --branch
```

2. Review `a55b7f4` carefully:

```bash
git show --stat --patch a55b7f4
```

3. Specifically test Tauri signer behavior in release script. A safe local scratch test:

```bash
tmp=$(mktemp)
echo hi > "$tmp"
cd /Users/leolin/Documents/GitHub/rosetta/rosetta-app
pnpm tauri signer sign -f "$HOME/.tauri/rosetta/updater.key" -p '' "$tmp"
cat "$tmp.sig"
rm -f "$tmp" "$tmp.sig"
```

4. If the release script overwrites `.sig` with verbose stdout, fix it to let Tauri signer write the `.sig` file itself, then check `-s "$artifact_path.sig"`.

5. Run validation again:

```bash
cd /Users/leolin/Documents/GitHub/rosetta/rosetta-app
pnpm typecheck
cd src-tauri
cargo check
bash -n scripts/release-macos.sh
bash -n scripts/publish-macos-updater.sh
```

6. When Supabase CLI and secrets are available:

```bash
supabase link --project-ref bdujdewqopcgwijhfbcz
supabase db push
supabase functions deploy rosetta-update --no-verify-jwt
```

Then re-run endpoint checks from the plan.

7. Manual smoke test remains pending:

- build lower-version app;
- build/publish higher-version signed updater artifact;
- publish metadata row;
- use Settings update flow;
- verify relaunch and Gatekeeper.

## Caution

Do not claim the update channel is production-ready until:

- `rosetta-update` is deployed and returns 204 for no update;
- migration is applied;
- a signed updater artifact and `.sig` are published;
- Tauri updater successfully downloads, verifies, installs, and relaunches on macOS Apple Silicon.

