# macOS Supabase Updater Design

## Status

Approved for implementation planning.

## Date

2026-05-22

## Scope

Rosetta will support in-app updates for macOS Apple Silicon only.

The first updater release targets:

- `target`: `darwin`
- `arch`: `aarch64`
- Tauri updater platform key: `darwin-aarch64`

Out of scope:

- Windows updates
- Intel macOS updates
- Linux updates
- GitHub Actions packaging
- GitHub Releases as the updater source
- automatic cloud publishing from CI
- login, sync, telemetry, collaboration, or user document upload

Supabase is only used for app release metadata and updater artifact storage. Rosetta must not upload source documents, translated text, job cache data, model prompts, or local runtime diagnostics as part of this feature.

## Goals

- Let users manually check for and install Rosetta app updates from inside Settings.
- Keep local packaging under the developer's control.
- Store built updater artifacts in Supabase Storage.
- Use a Supabase Edge Function as the Tauri updater endpoint.
- Keep release metadata in Supabase so versions can be published, hidden, rolled back, or superseded without rebuilding the app.
- Preserve Tauri updater signature verification.

## Current State

Rosetta already has the Tauri updater plugin wired:

- `tauri-plugin-updater = "2"` in `rosetta-app/src-tauri/Cargo.toml`
- `@tauri-apps/plugin-updater` in `rosetta-app/package.json`
- updater/process plugins registered in `rosetta-app/src-tauri/src/lib.rs`
- updater permission in `rosetta-app/src-tauri/capabilities/default.json`
- manual update UI in `rosetta-app/src/features/settings/SettingsPage.tsx`
- `bundle.createUpdaterArtifacts = true` in `rosetta-app/src-tauri/tauri.conf.json`

The existing updater endpoint points at GitHub Releases and the Settings UI still describes a Windows beta release path. Existing engineering notes also record that the historical updater public key does not have an accessible matching private key. The macOS release script currently creates a notarized public DMG but does not publish Tauri updater artifacts.

## Architecture

```txt
Local release machine
  -> build Rosetta.app for macOS arm64
  -> sign and notarize app
  -> produce Tauri updater artifact and .sig
  -> upload updater artifact to Supabase Storage
  -> write release row to Supabase Postgres

Rosetta app
  -> Tauri updater check()
  -> Supabase Edge Function endpoint
  -> release row lookup
  -> short-lived signed Storage URL
  -> Tauri signature verification
  -> download, install, relaunch
```

## Tauri Configuration

`rosetta-app/src-tauri/tauri.conf.json` should keep:

```json
{
  "bundle": {
    "createUpdaterArtifacts": true
  }
}
```

The updater endpoint should change from GitHub Releases to the Supabase Edge Function:

```json
{
  "plugins": {
    "updater": {
      "pubkey": "<new-tauri-updater-public-key>",
      "endpoints": [
        "https://<project-ref>.supabase.co/functions/v1/rosetta-update?target={{target}}&arch={{arch}}&current_version={{current_version}}"
      ]
    }
  }
}
```

The updater keypair must be regenerated before implementation because the existing configured public key is historical and its matching private key is unavailable. The new private key must remain outside the repository. The new public key is safe to commit in `tauri.conf.json`.

## Supabase Storage

Create a private bucket:

```txt
rosetta-releases
```

Recommended object layout:

```txt
macos/aarch64/<version>/
  <tauri-updater-artifact>
```

The exact artifact filename should be copied from Tauri's build output rather than inferred by string templates. The release script should store the final object path in the database row.

The public DMG can remain a separate manual artifact. The updater URL must point to the Tauri updater artifact, not the public DMG, unless Tauri's macOS updater output for the selected build flow explicitly requires that artifact.

## Supabase Database

Create a release table such as:

```sql
create table public.app_releases (
  id uuid primary key default gen_random_uuid(),
  app text not null default 'rosetta',
  version text not null,
  target text not null,
  arch text not null,
  platform text generated always as (target || '-' || arch) stored,
  storage_bucket text not null default 'rosetta-releases',
  storage_path text not null,
  signature text not null,
  notes text not null default '',
  pub_date timestamptz not null default now(),
  is_published boolean not null default false,
  minimum_version text,
  created_at timestamptz not null default now(),
  unique (app, version, target, arch)
);
```

The first implementation should only publish rows where:

```txt
app = rosetta
target = darwin
arch = aarch64
is_published = true
```

`minimum_version` is reserved for future forced migration rules. It does not need app UI in the first implementation.

## Edge Function

Create a Supabase Edge Function:

```txt
rosetta-update
```

Inputs:

- `target`
- `arch`
- `current_version`

Behavior:

- Reject unsupported platforms with `204 No Content`.
- Only accept `target=darwin` and `arch=aarch64` in the first release.
- Look up the newest published semver greater than `current_version`.
- Return `204 No Content` when there is no newer published release.
- Create a short-lived signed Storage URL for the selected artifact.
- Return Tauri's dynamic updater response:

```json
{
  "version": "0.1.0-beta.3",
  "pub_date": "2026-05-22T00:00:00Z",
  "url": "https://...",
  "signature": "<contents-of-sig-file>",
  "notes": "Release notes"
}
```

The function should use a Supabase service role secret stored in Supabase secrets, not in the app or repository. The app should not need a Supabase anon key for update checks because the Tauri updater only calls the public Edge Function URL.

## Release Script

Extend the local macOS release path with a separate publish step rather than replacing the existing notarized DMG workflow.

Recommended scripts:

```txt
rosetta-app/src-tauri/scripts/release-macos.sh
rosetta-app/src-tauri/scripts/publish-macos-updater.sh
```

The publish script should:

1. Read the version from `rosetta-app/package.json`.
2. Confirm the matching version in `rosetta-app/src-tauri/Cargo.toml` and `tauri.conf.json`.
3. Build or locate the macOS `darwin-aarch64` updater artifact.
4. Read the matching `.sig` file content.
5. Upload the artifact to Supabase Storage under `macos/aarch64/<version>/`.
6. Upsert the `app_releases` row with `is_published = false`.
7. Print a final confirmation command or SQL statement to mark the row as published.

Publishing should be a deliberate final step. This prevents a partially uploaded or untested artifact from becoming visible to existing users.

## Settings UI

The existing Settings update card should remain a manual check flow.

Change the visible copy from Windows/GitHub beta wording to macOS/Supabase wording:

- Badge: `macOS Apple Silicon`
- Description: `手动检查 Rosetta 更新。`
- Card description: `更新包通过 Rosetta 的 Supabase 发布通道分发，并使用 Tauri updater 签名校验。`

The UI should not claim automatic background updates unless that behavior is implemented later.

## Security And Privacy

- Tauri updater signature verification remains mandatory.
- The updater private key must never be committed.
- Supabase service role keys must only live in Supabase secrets or local developer environment variables.
- Supabase Storage bucket should be private; downloads use signed URLs created by the Edge Function.
- The Edge Function must not log request bodies containing document content. The updater endpoint only receives app version, target, and arch.
- This feature must not add telemetry or user identity.
- This feature must not broaden frontend filesystem permissions.

## Rollback And Release Control

Rollback means selecting which published release row the Edge Function returns. The first implementation can keep this simple:

- set `is_published = false` on a bad release;
- publish a newer fixed release;
- optionally let the Edge Function return the latest published version greater than the current app version.

Installing an older version than the current version is out of scope. If future rollback-to-older-version behavior is needed, it should be designed explicitly because it changes updater version comparison semantics.

## Verification

Implementation should verify:

- `cd rosetta-app && pnpm typecheck`
- `cd rosetta-app/src-tauri && cargo check`
- local Edge Function returns `204` for unsupported target/arch;
- local or deployed Edge Function returns Tauri updater JSON for `darwin/aarch64` when a newer published row exists;
- returned `signature` exactly matches the updater artifact `.sig` content;
- Settings manual check finds the update from a test build;
- update download, signature verification, install, and relaunch works on macOS Apple Silicon;
- notarized app still passes Gatekeeper checks after update.

Do not run dev servers or production builds unless explicitly requested for runtime verification or release packaging.

## Implementation Plan Boundary

The implementation plan should be split into small steps:

1. Generate and configure the new updater public key.
2. Change updater endpoint and Settings copy.
3. Add Supabase schema and Edge Function files.
4. Add local publish script.
5. Update macOS release documentation.
6. Run typecheck and Rust checks.
7. Perform a manual updater smoke test when the user is ready to build and publish test artifacts.
