alter table public.app_releases
  add column if not exists updater_url text,
  add column if not exists installer_url text;

alter table public.app_releases
  alter column storage_path drop not null;

alter table public.app_releases
  drop constraint if exists app_releases_update_artifact_location;

alter table public.app_releases
  add constraint app_releases_update_artifact_location check (
    (storage_path is not null and btrim(storage_path) <> '')
    or (updater_url is not null and btrim(updater_url) <> '')
  );

alter table public.app_releases
  drop constraint if exists app_releases_github_urls;

alter table public.app_releases
  add constraint app_releases_github_urls check (
    (
      updater_url is null
      or updater_url ~ '^https://github[.]com/LeoLin4258/rosetta/releases/download/[^/]+/[^/]+$'
    )
    and (
      installer_url is null
      or installer_url ~ '^https://github[.]com/LeoLin4258/rosetta/releases/download/[^/]+/[^/]+$'
    )
  );
