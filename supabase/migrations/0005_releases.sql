-- 配るアプリの版。アプリは、ここを見て新しい版に入れ替える(crates/asis/src/update.rs)。
-- ここに書ける人は、入れている人全員の PC で動くものを差し替えられる。
-- だから、書けるのは DB を動かしている人(管理用の鍵)だけ。API の役には、読むことしか渡さない。
create table public.app_releases (
  app         text not null check (app ~ '^[a-z0-9-]{1,40}$'),       -- 配るものの名前
  version     text not null check (version ~ '^\d+\.\d+\.\d+$'),
  min_version text not null check (min_version ~ '^\d+\.\d+\.\d+$'), -- これより古い版は、もう使えない
  files       jsonb not null,  -- [{"name": 入れる先での名前, "path": 置き場の中の場所, "sha256": 中身の照合値}]
  created_at  timestamptz not null default now(),
  primary key (app, version)
);
alter table public.app_releases enable row level security;
revoke all on public.app_releases from public, anon, authenticated;
grant select on public.app_releases to authenticated;

-- どこかの部屋に入っている人。配るものを受け取れるのは、この人たちだけ。
create function public.is_any_member() returns boolean
language sql stable security definer set search_path = '' as $$
  select exists (select 1 from public.room_members m where m.github_id = public.github_id())
$$;
revoke execute on function public.is_any_member() from public, anon;
grant execute on function public.is_any_member() to authenticated;

create policy releases_read on public.app_releases for select to authenticated
  using (public.is_any_member());

-- ファイルの置き場。公開しない。読めるのは、どこかの部屋に入っている人だけ。
-- 書く方針は作らない(方針が無ければ、API の役は書けない)。
insert into storage.buckets (id, name, public) values ('releases', 'releases', false)
  on conflict (id) do nothing;
create policy releases_files_read on storage.objects for select to authenticated
  using (bucket_id = 'releases' and public.is_any_member());

-- どの SQL まで流したかの記録。API からは触れない。
create table public.schema_migrations (
  version    text primary key,
  applied_at timestamptz not null default now()
);
alter table public.schema_migrations enable row level security;
revoke all on public.schema_migrations from public, anon, authenticated;
insert into public.schema_migrations (version) values ('0001'), ('0002'), ('0003'), ('0004'), ('0005');
