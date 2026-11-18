-- 部屋:repo とメンバーと役割をまとめる、共有の定義。
-- ここに入るのは定義だけ。issue・PR・コードは入れない(各自の PC が GitHub から読む)。
-- 部屋に入っても、GitHub で見られない repo は見えない。見る権限を決めるのは GitHub だけ。

create table public.rooms (
  id               uuid primary key default gen_random_uuid(),
  name             text not null check (char_length(name) between 1 and 80),
  merge_by_author  boolean not null default false,  -- 取り込むのは PR を出した本人
  deploy_by_author boolean not null default false,  -- 本番に出すのは PR を出した本人
  ignore_checks    text[] not null default '{}',    -- 数えないチェックの名前
  created_at       timestamptz not null default now()
);

create table public.room_repos (
  room_id uuid not null references public.rooms(id) on delete cascade,
  repo    text not null check (repo ~ '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$'),
  primary key (room_id, repo)
);

-- メンバーは GitHub の番号で持つ。名前は変えられるが、番号は変わらない。
create table public.room_members (
  room_id    uuid not null references public.rooms(id) on delete cascade,
  github_id  bigint not null,
  login      text not null check (login ~ '^[A-Za-z0-9-]{1,39}$'),
  is_admin   boolean not null default false,
  review     boolean not null default false,
  merge      boolean not null default false,
  deploy     boolean not null default false,
  created_at timestamptz not null default now(),
  primary key (room_id, github_id)
);
create index room_members_github_id on public.room_members (github_id);

-- ログインしている人の GitHub の番号。
-- 本人が書き換えられる user_metadata ではなく、認証が持つ identities から読む。
create function public.github_id() returns bigint
language sql stable security definer set search_path = '' as $$
  select i.provider_id::bigint from auth.identities i
  where i.user_id = auth.uid() and i.provider = 'github'
  limit 1
$$;

-- 方針の中で room_members を直接 読むと、方針が自分を呼び続ける。関数に出す。
create function public.is_member(room uuid) returns boolean
language sql stable security definer set search_path = '' as $$
  select exists (select 1 from public.room_members m
                 where m.room_id = room and m.github_id = public.github_id())
$$;

create function public.is_admin(room uuid) returns boolean
language sql stable security definer set search_path = '' as $$
  select exists (select 1 from public.room_members m
                 where m.room_id = room and m.github_id = public.github_id() and m.is_admin)
$$;

-- 部屋を作る。作った人が最初の管理者になる。2 つの行を 1 回で入れる。
create function public.create_room(room_name text, my_login text) returns uuid
language plpgsql security definer set search_path = '' as $$
declare
  me bigint := public.github_id();
  new_id uuid;
begin
  if me is null then
    raise exception 'GitHub でログインしていません' using errcode = '28000';
  end if;
  insert into public.rooms (name) values (room_name) returning id into new_id;
  insert into public.room_members (room_id, github_id, login, is_admin, review, merge, deploy)
  values (new_id, me, my_login, true, true, true, true);
  return new_id;
end
$$;

-- 管理者が 1 人もいない部屋を作らせない(部屋ごと消すときは通す)。
create function public.keep_one_admin() returns trigger
language plpgsql security definer set search_path = '' as $$
begin
  if exists (select 1 from public.rooms r where r.id = old.room_id)
     and not exists (select 1 from public.room_members m
                     where m.room_id = old.room_id and m.is_admin) then
    raise exception '管理者が 1 人もいなくなります' using errcode = '23514';
  end if;
  return null;
end
$$;

create trigger room_members_keep_admin
  after update or delete on public.room_members
  for each row execute function public.keep_one_admin();

alter table public.rooms        enable row level security;
alter table public.room_repos   enable row level security;
alter table public.room_members enable row level security;

revoke all on public.rooms, public.room_repos, public.room_members from anon;
-- 関数は、作っただけで誰でも呼べる。ログインしていない人から外す。
revoke execute on function public.create_room(text, text) from public, anon;
revoke execute on function public.github_id(), public.is_member(uuid), public.is_admin(uuid) from public, anon;
revoke execute on function public.keep_one_admin() from public, anon, authenticated;
grant execute on function public.github_id(), public.is_member(uuid), public.is_admin(uuid) to authenticated;
grant select, update, delete         on public.rooms        to authenticated;
grant select, insert, delete         on public.room_repos   to authenticated;
grant select, insert, update, delete on public.room_members to authenticated;
grant execute on function public.create_room(text, text) to authenticated;

-- 読めるのはメンバーだけ。変えられるのは管理者だけ。部屋は create_room でしか作れない。
create policy rooms_read   on public.rooms for select to authenticated using (public.is_member(id));
create policy rooms_change on public.rooms for update to authenticated
  using (public.is_admin(id)) with check (public.is_admin(id));
create policy rooms_remove on public.rooms for delete to authenticated using (public.is_admin(id));

create policy repos_read   on public.room_repos for select to authenticated using (public.is_member(room_id));
create policy repos_add    on public.room_repos for insert to authenticated with check (public.is_admin(room_id));
create policy repos_remove on public.room_repos for delete to authenticated using (public.is_admin(room_id));

create policy members_read   on public.room_members for select to authenticated using (public.is_member(room_id));
create policy members_invite on public.room_members for insert to authenticated with check (public.is_admin(room_id));
create policy members_change on public.room_members for update to authenticated
  using (public.is_admin(room_id)) with check (public.is_admin(room_id));
-- 管理者は外せる。自分は自分で出られる。
create policy members_remove on public.room_members for delete to authenticated
  using (public.is_admin(room_id) or github_id = public.github_id());
