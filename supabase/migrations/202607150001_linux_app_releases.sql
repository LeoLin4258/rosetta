alter table public.app_releases
  drop constraint if exists app_releases_rosetta_supported_platforms;

alter table public.app_releases
  add constraint app_releases_rosetta_supported_platforms check (
    app = 'rosetta'
    and storage_bucket = 'rosetta-releases'
    and (
      (target = 'darwin' and arch = 'aarch64')
      or (target = 'windows' and arch = 'x86_64')
      or (target = 'linux' and arch = 'x86_64')
    )
  );
