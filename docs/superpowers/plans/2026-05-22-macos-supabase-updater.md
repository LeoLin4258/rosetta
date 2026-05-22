# macOS Supabase Updater Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a macOS Apple Silicon in-app update path backed by Supabase Storage and a Supabase Edge Function.

**Architecture:** Keep Tauri's updater plugin as the installer and signature verifier. Point the compiled app at a Supabase Edge Function, store updater artifacts in a private Supabase Storage bucket, and publish release metadata through a local script that writes a Postgres row only after upload succeeds.

**Tech Stack:** Tauri v2 updater, React/TypeScript Settings UI, Bash release scripts, Supabase Storage, Supabase Postgres, Supabase Edge Functions on Deno.

---

## File Structure

- Modify `rosetta-app/src-tauri/tauri.conf.json`: replace the GitHub updater endpoint with the Supabase Edge Function endpoint and update the updater public key after generating a new keypair.
- Modify `rosetta-app/src/features/settings/SettingsPage.tsx`: change update card copy from Windows/GitHub beta wording to macOS Apple Silicon/Supabase wording.
- Create `supabase/migrations/202605220001_rosetta_app_releases.sql`: schema for app release metadata and the private Storage bucket.
- Create `supabase/functions/rosetta-update/index.ts`: dynamic updater endpoint for Tauri.
- Create `rosetta-app/src-tauri/scripts/publish-macos-updater.sh`: local publish script that uploads a Tauri updater artifact and writes an unpublished release row.
- Modify `docs/engineering/release/macos-release.md`: document the new updater publish flow.
- Add `docs/engineering/change-log/2026-05-22-macos-supabase-updater.md`: record the release-channel architecture change after implementation.

---

### Task 1: Generate And Configure A New Tauri Updater Key

**Files:**
- Modify: `rosetta-app/src-tauri/tauri.conf.json`

- [ ] **Step 1: Generate a new updater keypair outside the repository**

Run from any directory:

```bash
mkdir -p "$HOME/.tauri/rosetta"
cd /Users/leolin/Documents/GitHub/rosetta/rosetta-app
pnpm tauri signer generate -- -w "$HOME/.tauri/rosetta/updater.key"
```

Expected: the command prints a public key and writes the private key to `$HOME/.tauri/rosetta/updater.key`. Do not move the private key into the repository.

- [ ] **Step 2: Save the private key password in your local shell only**

If the signer command created an encrypted private key, export the password only in your local terminal session before release builds:

```bash
read -rsp 'Tauri updater private key password: ' TAURI_SIGNING_PRIVATE_KEY_PASSWORD
printf '\n'
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD
```

Expected: this environment variable exists only in your shell or local secret manager. It is not committed.

- [ ] **Step 3: Replace the updater block in `tauri.conf.json`**

Run this command and paste the public key printed by Step 1 at the prompt:

```bash
cd /Users/leolin/Documents/GitHub/rosetta
read -r -p 'Tauri updater public key: ' ROSETTA_UPDATER_PUBKEY
export ROSETTA_UPDATER_PUBKEY
node <<'NODE'
const fs = require("fs");
const path = "rosetta-app/src-tauri/tauri.conf.json";
const config = JSON.parse(fs.readFileSync(path, "utf8"));
config.plugins ??= {};
config.plugins.updater ??= {};
config.plugins.updater.pubkey = process.env.ROSETTA_UPDATER_PUBKEY;
config.plugins.updater.endpoints = [
  "https://bdujdewqopcgwijhfbcz.supabase.co/functions/v1/rosetta-update?target={{target}}&arch={{arch}}&current_version={{current_version}}",
];
config.plugins.updater.windows = {
  installMode: "passive",
};
fs.writeFileSync(path, `${JSON.stringify(config, null, 2)}\n`);
NODE
```

Keep the existing Windows `installMode` field even though this phase only supports macOS; it is harmless and avoids unrelated config churn.

- [ ] **Step 4: Validate JSON**

Run:

```bash
cd /Users/leolin/Documents/GitHub/rosetta
node -e "JSON.parse(require('fs').readFileSync('rosetta-app/src-tauri/tauri.conf.json', 'utf8')); console.log('ok')"
```

Expected: prints `ok`.

- [ ] **Step 5: Commit**

```bash
cd /Users/leolin/Documents/GitHub/rosetta
git add rosetta-app/src-tauri/tauri.conf.json
git commit -m "Configure Supabase updater endpoint"
```

---

### Task 2: Update Settings UI Copy

**Files:**
- Modify: `rosetta-app/src/features/settings/SettingsPage.tsx`

- [ ] **Step 1: Edit the update section copy**

In `rosetta-app/src/features/settings/SettingsPage.tsx`, change the update section from:

```tsx
<SettingsSectionHeader
  description="手动检查内部测试版更新。"
  icon={<Rocket />}
  title="应用更新"
>
  <Badge variant="outline">Windows Beta</Badge>
</SettingsSectionHeader>
```

to:

```tsx
<SettingsSectionHeader
  description="手动检查 Rosetta 更新。"
  icon={<Rocket />}
  title="应用更新"
>
  <Badge variant="outline">macOS Apple Silicon</Badge>
</SettingsSectionHeader>
```

Change the card description from:

```tsx
<CardDescription>
  更新包会通过 GitHub Release 分发，并使用 Tauri updater
  签名校验。
</CardDescription>
```

to:

```tsx
<CardDescription>
  更新包通过 Rosetta 的 Supabase 发布通道分发，并使用 Tauri updater
  签名校验。
</CardDescription>
```

- [ ] **Step 2: Run typecheck**

Run:

```bash
cd /Users/leolin/Documents/GitHub/rosetta/rosetta-app
pnpm typecheck
```

Expected: passes.

- [ ] **Step 3: Commit**

```bash
cd /Users/leolin/Documents/GitHub/rosetta
git add rosetta-app/src/features/settings/SettingsPage.tsx
git commit -m "Update macOS updater settings copy"
```

---

### Task 3: Add Supabase Release Schema

**Files:**
- Create: `supabase/migrations/202605220001_rosetta_app_releases.sql`

- [ ] **Step 1: Create the Supabase migration**

Create `supabase/migrations/202605220001_rosetta_app_releases.sql`:

```sql
create extension if not exists pgcrypto;

insert into storage.buckets (id, name, public)
values ('rosetta-releases', 'rosetta-releases', false)
on conflict (id) do update set public = excluded.public;

create table if not exists public.app_releases (
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
  updated_at timestamptz not null default now(),
  constraint app_releases_unique_version unique (app, version, target, arch),
  constraint app_releases_rosetta_macos_arm64_only check (
    app = 'rosetta'
    and target = 'darwin'
    and arch = 'aarch64'
    and storage_bucket = 'rosetta-releases'
  )
);

create index if not exists app_releases_lookup_idx
  on public.app_releases (app, target, arch, is_published, pub_date desc);

create or replace function public.set_app_releases_updated_at()
returns trigger
language plpgsql
as $$
begin
  new.updated_at = now();
  return new;
end;
$$;

drop trigger if exists app_releases_set_updated_at on public.app_releases;

create trigger app_releases_set_updated_at
before update on public.app_releases
for each row
execute function public.set_app_releases_updated_at();

alter table public.app_releases enable row level security;

drop policy if exists "No public app release reads" on public.app_releases;
create policy "No public app release reads"
on public.app_releases
for select
using (false);

drop policy if exists "No public app release writes" on public.app_releases;
create policy "No public app release writes"
on public.app_releases
for all
using (false)
with check (false);
```

- [ ] **Step 2: Apply the migration to the linked Supabase project**

Run:

```bash
cd /Users/leolin/Documents/GitHub/rosetta
supabase link --project-ref bdujdewqopcgwijhfbcz
supabase db push
```

Expected: migration applies successfully. If the project is already linked, `supabase link` should report the existing link or complete without changing code.

- [ ] **Step 3: Commit**

```bash
cd /Users/leolin/Documents/GitHub/rosetta
git add supabase/migrations/202605220001_rosetta_app_releases.sql
git commit -m "Add Supabase release metadata schema"
```

---

### Task 4: Add Supabase Edge Function

**Files:**
- Create: `supabase/functions/rosetta-update/index.ts`

- [ ] **Step 1: Create the Edge Function**

Create `supabase/functions/rosetta-update/index.ts`:

```ts
import { createClient } from "npm:@supabase/supabase-js@2";

type ReleaseRow = {
  version: string;
  storage_bucket: string;
  storage_path: string;
  signature: string;
  notes: string;
  pub_date: string;
};

const corsHeaders = {
  "access-control-allow-origin": "*",
  "access-control-allow-methods": "GET, OPTIONS",
  "access-control-allow-headers": "authorization, x-client-info, apikey, content-type",
};

function noContent(): Response {
  return new Response(null, {
    status: 204,
    headers: corsHeaders,
  });
}

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      ...corsHeaders,
      "content-type": "application/json; charset=utf-8",
      "cache-control": "no-store",
    },
  });
}

function parseSemver(input: string): [number, number, number, string] | null {
  const match = input.match(/^v?(\\d+)\\.(\\d+)\\.(\\d+)(?:-([0-9A-Za-z.-]+))?$/);
  if (!match) {
    return null;
  }

  return [
    Number.parseInt(match[1], 10),
    Number.parseInt(match[2], 10),
    Number.parseInt(match[3], 10),
    match[4] ?? "",
  ];
}

function comparePrerelease(left: string, right: string): number {
  if (left === right) {
    return 0;
  }

  if (left === "") {
    return 1;
  }

  if (right === "") {
    return -1;
  }

  const leftParts = left.split(".");
  const rightParts = right.split(".");
  const max = Math.max(leftParts.length, rightParts.length);

  for (let index = 0; index < max; index += 1) {
    const leftPart = leftParts[index];
    const rightPart = rightParts[index];

    if (leftPart === undefined) {
      return -1;
    }

    if (rightPart === undefined) {
      return 1;
    }

    const leftNumber = /^\\d+$/.test(leftPart) ? Number.parseInt(leftPart, 10) : null;
    const rightNumber = /^\\d+$/.test(rightPart) ? Number.parseInt(rightPart, 10) : null;

    if (leftNumber !== null && rightNumber !== null) {
      if (leftNumber !== rightNumber) {
        return leftNumber > rightNumber ? 1 : -1;
      }
      continue;
    }

    if (leftNumber !== null) {
      return -1;
    }

    if (rightNumber !== null) {
      return 1;
    }

    if (leftPart !== rightPart) {
      return leftPart > rightPart ? 1 : -1;
    }
  }

  return 0;
}

function compareSemver(left: string, right: string): number {
  const leftParsed = parseSemver(left);
  const rightParsed = parseSemver(right);

  if (!leftParsed || !rightParsed) {
    return left.localeCompare(right);
  }

  for (let index = 0; index < 3; index += 1) {
    if (leftParsed[index] !== rightParsed[index]) {
      return leftParsed[index] > rightParsed[index] ? 1 : -1;
    }
  }

  return comparePrerelease(leftParsed[3], rightParsed[3]);
}

Deno.serve(async (request) => {
  if (request.method === "OPTIONS") {
    return new Response(null, {
      status: 204,
      headers: corsHeaders,
    });
  }

  if (request.method !== "GET") {
    return jsonResponse({ error: "Method not allowed" }, 405);
  }

  const url = new URL(request.url);
  const target = url.searchParams.get("target") ?? "";
  const arch = url.searchParams.get("arch") ?? "";
  const currentVersion = url.searchParams.get("current_version") ?? "0.0.0";

  if (target !== "darwin" || arch !== "aarch64") {
    return noContent();
  }

  const supabaseUrl = Deno.env.get("SUPABASE_URL");
  const serviceRoleKey = Deno.env.get("SUPABASE_SERVICE_ROLE_KEY");

  if (!supabaseUrl || !serviceRoleKey) {
    return jsonResponse({ error: "Updater service is not configured" }, 500);
  }

  const supabase = createClient(supabaseUrl, serviceRoleKey, {
    auth: {
      persistSession: false,
      autoRefreshToken: false,
    },
  });

  const { data, error } = await supabase
    .from("app_releases")
    .select("version, storage_bucket, storage_path, signature, notes, pub_date")
    .eq("app", "rosetta")
    .eq("target", "darwin")
    .eq("arch", "aarch64")
    .eq("is_published", true);

  if (error) {
    return jsonResponse({ error: "Could not load release metadata" }, 500);
  }

  const candidates = ((data ?? []) as ReleaseRow[])
    .filter((release) => compareSemver(release.version, currentVersion) > 0)
    .sort((left, right) => compareSemver(right.version, left.version));

  const release = candidates[0];

  if (!release) {
    return noContent();
  }

  const { data: signedUrl, error: signedUrlError } = await supabase.storage
    .from(release.storage_bucket)
    .createSignedUrl(release.storage_path, 60 * 30);

  if (signedUrlError || !signedUrl?.signedUrl) {
    return jsonResponse({ error: "Could not create update download URL" }, 500);
  }

  return jsonResponse({
    version: release.version,
    pub_date: release.pub_date,
    url: signedUrl.signedUrl,
    signature: release.signature,
    notes: release.notes,
  });
});
```

- [ ] **Step 2: Serve the function locally for syntax/runtime checks**

Run:

```bash
cd /Users/leolin/Documents/GitHub/rosetta
supabase functions serve rosetta-update --env-file <(printf 'SUPABASE_URL=https://bdujdewqopcgwijhfbcz.supabase.co\nSUPABASE_SERVICE_ROLE_KEY=%s\n' "$SUPABASE_SERVICE_ROLE_KEY")
```

Expected: Supabase CLI starts the local function server. Leave it running for Step 3, then stop it with `Ctrl-C`.

- [ ] **Step 3: Verify unsupported platforms return 204**

In a second terminal, run:

```bash
curl -i 'http://127.0.0.1:54321/functions/v1/rosetta-update?target=windows&arch=x86_64&current_version=0.1.0-beta.2'
```

Expected: response status is `204 No Content`.

- [ ] **Step 4: Deploy the function**

Run:

```bash
cd /Users/leolin/Documents/GitHub/rosetta
supabase functions deploy rosetta-update --no-verify-jwt
```

Expected: function deploys successfully. `--no-verify-jwt` is required because Tauri updater does not send a Supabase user JWT.

- [ ] **Step 5: Verify deployed unsupported platforms return 204**

Run:

```bash
curl -i 'https://bdujdewqopcgwijhfbcz.supabase.co/functions/v1/rosetta-update?target=windows&arch=x86_64&current_version=0.1.0-beta.2'
```

Expected: response status is `204 No Content`.

- [ ] **Step 6: Commit**

```bash
cd /Users/leolin/Documents/GitHub/rosetta
git add supabase/functions/rosetta-update/index.ts
git commit -m "Add Supabase updater function"
```

---

### Task 5: Add Local macOS Updater Publish Script

**Files:**
- Create: `rosetta-app/src-tauri/scripts/publish-macos-updater.sh`

- [ ] **Step 1: Create the publish script**

Create `rosetta-app/src-tauri/scripts/publish-macos-updater.sh`:

```bash
#!/usr/bin/env bash
# Upload the macOS Apple Silicon Tauri updater artifact to Supabase and create
# an unpublished release row. Run this only after producing a signed/notarized
# macOS release artifact and verifying the updater artifact.

set -euo pipefail

APP_NAME="${APP_NAME:-rosetta}"
SUPABASE_PROJECT_URL="${SUPABASE_PROJECT_URL:-https://bdujdewqopcgwijhfbcz.supabase.co}"
SUPABASE_BUCKET="${SUPABASE_BUCKET:-rosetta-releases}"
TARGET="${TARGET:-darwin}"
ARCH="${ARCH:-aarch64}"
NOTES_FILE="${NOTES_FILE:-}"
PUBLISH="${PUBLISH:-false}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TAURI_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
APP_DIR="$(cd "$TAURI_DIR/.." && pwd)"
REPO_ROOT="$(cd "$APP_DIR/.." && pwd)"

log() {
  printf '[publish-macos-updater] %s\n' "$*" >&2
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    log "missing required command: $1"
    exit 2
  fi
}

require_env() {
  if [[ -z "${!1:-}" ]]; then
    log "missing required environment variable: $1"
    exit 2
  fi
}

json_escape() {
  node -e 'process.stdout.write(JSON.stringify(process.argv[1]))' "$1"
}

version() {
  cd "$APP_DIR"
  node -p "require('./package.json').version"
}

config_version() {
  cd "$APP_DIR"
  node -p "require('./src-tauri/tauri.conf.json').version"
}

cargo_version() {
  cd "$TAURI_DIR"
  cargo metadata --no-deps --format-version 1 | node -e 'const fs = require("fs"); const data = JSON.parse(fs.readFileSync(0, "utf8")); console.log(data.packages.find((pkg) => pkg.name === "rosetta-app").version)'
}

find_artifact() {
  local app_version="$1"
  local bundle_dir="$TAURI_DIR/target/release/bundle"

  find "$bundle_dir" -type f \
    \( -name "*.app.tar.gz" -o -name "*.app.tar.gz.sig" -o -name "*.tar.gz" -o -name "*.tar.gz.sig" \) \
    | grep -v '\.sig$' \
    | grep "$app_version" \
    | sort \
    | head -n 1
}

main() {
  require_command node
  require_command cargo
  require_command curl
  require_env SUPABASE_SERVICE_ROLE_KEY

  local app_version tauri_version rust_version
  app_version="$(version)"
  tauri_version="$(config_version)"
  rust_version="$(cargo_version)"

  if [[ "$app_version" != "$tauri_version" || "$app_version" != "$rust_version" ]]; then
    log "version mismatch: package.json=$app_version tauri.conf.json=$tauri_version Cargo.toml=$rust_version"
    exit 2
  fi

  local artifact
  artifact="${UPDATER_ARTIFACT:-$(find_artifact "$app_version")}"

  if [[ -z "$artifact" || ! -f "$artifact" ]]; then
    log "could not find updater artifact for version $app_version under $TAURI_DIR/target/release/bundle"
    log "set UPDATER_ARTIFACT=/absolute/path/to/artifact to publish a specific file"
    exit 2
  fi

  local sig_file="$artifact.sig"
  if [[ ! -f "$sig_file" ]]; then
    log "missing signature file: $sig_file"
    exit 2
  fi

  local signature notes storage_path artifact_name artifact_size
  signature="$(tr -d '\n' < "$sig_file")"
  artifact_name="$(basename "$artifact")"
  artifact_size="$(wc -c < "$artifact" | tr -d ' ')"
  storage_path="macos/aarch64/$app_version/$artifact_name"

  if [[ -n "$NOTES_FILE" ]]; then
    notes="$(cat "$NOTES_FILE")"
  else
    notes="Rosetta $app_version"
  fi

  log "uploading $artifact_name ($artifact_size bytes) to $SUPABASE_BUCKET/$storage_path"
  curl --fail-with-body \
    --request POST \
    --header "Authorization: Bearer $SUPABASE_SERVICE_ROLE_KEY" \
    --header "apikey: $SUPABASE_SERVICE_ROLE_KEY" \
    --header "Content-Type: application/octet-stream" \
    --header "x-upsert: true" \
    --data-binary "@$artifact" \
    "$SUPABASE_PROJECT_URL/storage/v1/object/$SUPABASE_BUCKET/$storage_path" >/dev/null

  local payload
  payload="$(
    printf '{"app":"rosetta","version":%s,"target":"darwin","arch":"aarch64","storage_bucket":"rosetta-releases","storage_path":%s,"signature":%s,"notes":%s,"is_published":%s}' \
      "$(json_escape "$app_version")" \
      "$(json_escape "$storage_path")" \
      "$(json_escape "$signature")" \
      "$(json_escape "$notes")" \
      "$(if [[ "$PUBLISH" == "true" ]]; then printf true; else printf false; fi)"
  )"

  log "upserting release metadata with is_published=$PUBLISH"
  curl --fail-with-body \
    --request POST \
    --header "Authorization: Bearer $SUPABASE_SERVICE_ROLE_KEY" \
    --header "apikey: $SUPABASE_SERVICE_ROLE_KEY" \
    --header "Content-Type: application/json" \
    --header "Prefer: resolution=merge-duplicates" \
    --data "$payload" \
    "$SUPABASE_PROJECT_URL/rest/v1/app_releases?on_conflict=app,version,target,arch" >/dev/null

  log "uploaded updater artifact:"
  printf '  version: %s\n' "$app_version"
  printf '  platform: %s-%s\n' "$TARGET" "$ARCH"
  printf '  storage: %s/%s\n' "$SUPABASE_BUCKET" "$storage_path"
  printf '  published: %s\n' "$PUBLISH"

  if [[ "$PUBLISH" != "true" ]]; then
    cat <<EOF

Release row is unpublished. After smoke testing, publish it with:

curl --fail-with-body \\
  --request PATCH \\
  --header "Authorization: Bearer \$SUPABASE_SERVICE_ROLE_KEY" \\
  --header "apikey: \$SUPABASE_SERVICE_ROLE_KEY" \\
  --header "Content-Type: application/json" \\
  --data '{"is_published":true}' \\
  "$SUPABASE_PROJECT_URL/rest/v1/app_releases?app=eq.rosetta&version=eq.$app_version&target=eq.darwin&arch=eq.aarch64"
EOF
  fi
}

main "$@"
```

- [ ] **Step 2: Make the script executable**

Run:

```bash
cd /Users/leolin/Documents/GitHub/rosetta
chmod +x rosetta-app/src-tauri/scripts/publish-macos-updater.sh
```

- [ ] **Step 3: Run a no-artifact failure check**

Run:

```bash
cd /Users/leolin/Documents/GitHub/rosetta
SUPABASE_SERVICE_ROLE_KEY=test rosetta-app/src-tauri/scripts/publish-macos-updater.sh
```

Expected: if no updater artifact exists for the current version, the script fails before upload with a message that it could not find an updater artifact. If an artifact does exist, stop and do not continue with the test service key.

- [ ] **Step 4: Commit**

```bash
cd /Users/leolin/Documents/GitHub/rosetta
git add rosetta-app/src-tauri/scripts/publish-macos-updater.sh
git commit -m "Add macOS Supabase updater publish script"
```

---

### Task 6: Document Release Procedure

**Files:**
- Modify: `docs/engineering/release/macos-release.md`
- Create: `docs/engineering/change-log/2026-05-22-macos-supabase-updater.md`

- [ ] **Step 1: Replace the old updater note in macOS release docs**

In `docs/engineering/release/macos-release.md`, replace the `### Tauri updater artifacts` section with this text:

### Tauri updater artifacts

The public DMG and the Tauri updater artifact are separate release outputs.
The DMG is for manual installation. The updater artifact is for the in-app
Tauri updater and is distributed through Supabase.

The updater endpoint is:

~~~txt
https://bdujdewqopcgwijhfbcz.supabase.co/functions/v1/rosetta-update?target={{target}}&arch={{arch}}&current_version={{current_version}}
~~~

The first updater channel supports only:

~~~txt
darwin-aarch64
~~~

Before publishing an updater release, make sure:

~~~bash
read -rsp 'Supabase service role key: ' SUPABASE_SERVICE_ROLE_KEY
printf '\n'
export SUPABASE_SERVICE_ROLE_KEY
export TAURI_SIGNING_PRIVATE_KEY="$HOME/.tauri/rosetta/updater.key"
read -rsp 'Tauri updater private key password: ' TAURI_SIGNING_PRIVATE_KEY_PASSWORD
printf '\n'
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD
~~~

Then build the macOS release and publish the updater artifact:

~~~bash
bash rosetta-app/src-tauri/scripts/release-macos.sh
bash rosetta-app/src-tauri/scripts/publish-macos-updater.sh
~~~

The publish script uploads the updater artifact to the private
`rosetta-releases` bucket and writes an unpublished `app_releases` row.
After testing, publish the row by running the PATCH command printed by the
script.

To hide a bad release:

~~~bash
ROSETTA_RELEASE_VERSION="0.1.0-beta.3"
curl --fail-with-body \
  --request PATCH \
  --header "Authorization: Bearer $SUPABASE_SERVICE_ROLE_KEY" \
  --header "apikey: $SUPABASE_SERVICE_ROLE_KEY" \
  --header "Content-Type: application/json" \
  --data '{"is_published":false}' \
  "https://bdujdewqopcgwijhfbcz.supabase.co/rest/v1/app_releases?app=eq.rosetta&version=eq.$ROSETTA_RELEASE_VERSION&target=eq.darwin&arch=eq.aarch64"
~~~

Do not upload user documents, translated text, job caches, model prompts, or
runtime logs to Supabase as part of the updater flow.

- [ ] **Step 2: Add the engineering change log**

Create `docs/engineering/change-log/2026-05-22-macos-supabase-updater.md`:

```md
# macOS Supabase Updater Channel

## Date

2026-05-22

## Scope

Added the planned macOS Apple Silicon updater channel for Rosetta.

## Changes

- Pointed the Tauri updater configuration at the Supabase Edge Function release endpoint.
- Added Supabase release metadata schema for `darwin-aarch64` app releases.
- Added a Supabase Edge Function that returns Tauri dynamic updater responses.
- Added a local publish script for uploading updater artifacts to private Supabase Storage.
- Updated Settings copy from the old Windows/GitHub beta wording to macOS Apple Silicon/Supabase wording.
- Documented the release and rollback procedure.

## Privacy Boundary

Supabase is only used for app release artifacts and version metadata. The updater flow does not upload source documents, translated text, job cache data, prompts, runtime logs, telemetry, or user identity.

## Validation

Implementation validation:

- `cd rosetta-app && pnpm typecheck`
- `cd rosetta-app/src-tauri && cargo check`
- Supabase Edge Function returns `204 No Content` for unsupported platforms.
- Supabase Edge Function returns Tauri updater JSON for a newer published `darwin-aarch64` release.
- Manual macOS Apple Silicon updater smoke test before public distribution.
```

- [ ] **Step 3: Commit**

```bash
cd /Users/leolin/Documents/GitHub/rosetta
git add docs/engineering/release/macos-release.md docs/engineering/change-log/2026-05-22-macos-supabase-updater.md
git commit -m "Document Supabase updater release flow"
```

---

### Task 7: Final Validation

**Files:**
- Verify all files changed in Tasks 1-6.

- [ ] **Step 1: Run frontend typecheck**

Run:

```bash
cd /Users/leolin/Documents/GitHub/rosetta/rosetta-app
pnpm typecheck
```

Expected: passes.

- [ ] **Step 2: Run Rust check**

Run:

```bash
cd /Users/leolin/Documents/GitHub/rosetta/rosetta-app/src-tauri
cargo check
```

Expected: passes.

- [ ] **Step 3: Verify deployed unsupported update check**

Run:

```bash
curl -i 'https://bdujdewqopcgwijhfbcz.supabase.co/functions/v1/rosetta-update?target=windows&arch=x86_64&current_version=0.1.0-beta.2'
```

Expected: response status is `204 No Content`.

- [ ] **Step 4: Verify deployed no-update response for current macOS version**

Run before publishing any newer row:

```bash
curl -i 'https://bdujdewqopcgwijhfbcz.supabase.co/functions/v1/rosetta-update?target=darwin&arch=aarch64&current_version=999.999.999'
```

Expected: response status is `204 No Content`.

- [ ] **Step 5: Inspect git status**

Run:

```bash
cd /Users/leolin/Documents/GitHub/rosetta
git status --short
```

Expected: clean worktree after all task commits.

---

### Task 8: Manual Updater Smoke Test

**Files:**
- No code changes expected.

- [ ] **Step 1: Build a lower-version app and install it**

Use an already installed Rosetta build with a version lower than the test release row. If needed, temporarily set the installed test app version to `0.1.0-beta.2`, build it, and install it locally.

Expected: the installed app reports the lower version in Settings.

- [ ] **Step 2: Build and publish a higher-version updater artifact**

Run on macOS Apple Silicon with real local secrets:

```bash
cd /Users/leolin/Documents/GitHub/rosetta
read -rsp 'Supabase service role key: ' SUPABASE_SERVICE_ROLE_KEY
printf '\n'
export SUPABASE_SERVICE_ROLE_KEY
export TAURI_SIGNING_PRIVATE_KEY="$HOME/.tauri/rosetta/updater.key"
read -rsp 'Tauri updater private key password: ' TAURI_SIGNING_PRIVATE_KEY_PASSWORD
printf '\n'
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD
bash rosetta-app/src-tauri/scripts/release-macos.sh
bash rosetta-app/src-tauri/scripts/publish-macos-updater.sh
```

Expected: script uploads the artifact and prints a PATCH command. Run the printed PATCH command to set `is_published=true`.

- [ ] **Step 3: Verify the updater endpoint returns JSON**

Run:

```bash
curl -s 'https://bdujdewqopcgwijhfbcz.supabase.co/functions/v1/rosetta-update?target=darwin&arch=aarch64&current_version=0.1.0-beta.2' | node -e 'const fs=require("fs"); const data=JSON.parse(fs.readFileSync(0,"utf8")); console.log(data.version); console.log(Boolean(data.url)); console.log(Boolean(data.signature));'
```

Expected: prints the newer version, `true`, `true`.

- [ ] **Step 4: Check and install from Rosetta Settings**

Open the lower-version Rosetta app, go to Settings, click `检查更新`, then `下载并安装`, then `重启完成更新`.

Expected: the app relaunches and Settings reports the newer version.

- [ ] **Step 5: Verify Gatekeeper acceptance after update**

Run:

```bash
spctl --assess --type execute --verbose=4 /Applications/Rosetta.app
```

Expected: output includes `accepted`.
