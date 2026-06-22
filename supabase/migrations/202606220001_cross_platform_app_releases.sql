alter table public.app_releases
  drop constraint if exists app_releases_rosetta_macos_arm64_only;

alter table public.app_releases
  add column if not exists installer_storage_path text,
  add column if not exists installer_sha256 text,
  add column if not exists installer_size_bytes bigint;

update public.app_releases
set installer_storage_path = dmg_storage_path
where installer_storage_path is null
  and dmg_storage_path is not null;

alter table public.app_releases
  add constraint app_releases_rosetta_supported_platforms check (
    app = 'rosetta'
    and storage_bucket = 'rosetta-releases'
    and (
      (target = 'darwin' and arch = 'aarch64')
      or (target = 'windows' and arch = 'x86_64')
    )
  );
