-- 部屋の権限の試験。期待と違えば例外で止まる。
\set ON_ERROR_STOP on
insert into auth.identities values
  ('00000000-0000-0000-0000-00000000000a', 'github', '101', '{"user_name":"alice"}'),
  ('00000000-0000-0000-0000-00000000000b', 'github', '102', '{"user_name":"bob"}'),
  ('00000000-0000-0000-0000-00000000000c', 'github', '103', '{"user_name":"carol"}');  -- 部外者

create function pg_temp.act(u text) returns void language sql as $$
  select set_config('request.jwt.claim.sub', u, false); $$;
create function pg_temp.fails(q text, want text) returns void language plpgsql as $$
begin
  begin execute q; exception when others then
    if sqlstate = want then return; end if;
    raise exception '違う失敗: % (%), 期待 %', sqlerrm, sqlstate, want;
  end;
  raise exception '通ってはいけないものが通った: %', q;
end $$;
create function pg_temp.count_is(q text, want bigint) returns void language plpgsql as $$
declare n bigint; begin
  execute 'select count(*) from (' || q || ') t' into n;
  if n <> want then raise exception '件数が違う: % → % (期待 %)', q, n, want; end if;
end $$;
create table pg_temp.ids (room uuid); grant all on pg_temp.ids to authenticated;

set role authenticated;

-- ログインしていない人は部屋を作れない
select pg_temp.act('');
select pg_temp.fails($$select public.create_room('x')$$, '28000');

-- 許可の一覧に載っていない人は部屋を作れない。一覧は API の役からは読めも書けもしない
select pg_temp.act('00000000-0000-0000-0000-00000000000a');
select pg_temp.fails($$select public.create_room('team')$$, '42501');
select pg_temp.fails($$insert into public.room_creators (github_id) values (101)$$, '42501');
select pg_temp.fails($$select 1 from public.room_creators$$, '42501');
reset role; insert into public.room_creators (github_id) values (101); set role authenticated;

-- alice が部屋を作る。本人が管理者で、全部の役を持つ
insert into pg_temp.ids select public.create_room('team');
select pg_temp.count_is($$select 1 from public.room_members where login='alice' and is_admin and plan and implement and review and merge and deploy$$, 1);

-- 表に直接 入れて部屋を作ることはできない
select pg_temp.fails($$insert into public.rooms (name) values ('raw')$$, '42501');

-- 管理者は repo を足し、名前で人を招ける
insert into public.room_repos select room, 'acme/app' from pg_temp.ids;
insert into public.room_members (room_id, github_id, login) select room, 102, 'bob' from pg_temp.ids;
select pg_temp.fails($$insert into public.room_repos select room, 'not a repo' from pg_temp.ids$$, '23514');
update public.room_repos set everyone = true where repo = 'acme/app';
select pg_temp.count_is($$select 1 from public.room_repos where everyone$$, 1);

-- 招かれた bob には部屋が見える。変えることも、自分の部屋を作ることもできない
select pg_temp.act('00000000-0000-0000-0000-00000000000b');
select pg_temp.fails($$select public.create_room('mine')$$, '42501');
select pg_temp.count_is('select 1 from public.rooms', 1);
select pg_temp.count_is('select 1 from public.room_repos', 1);
select pg_temp.count_is('select 1 from public.room_members', 2);
select pg_temp.fails($$insert into public.room_repos select room, 'acme/other' from pg_temp.ids$$, '42501');
select pg_temp.fails($$insert into public.room_members (room_id, github_id, login) select room, 103, 'carol' from pg_temp.ids$$, '42501');
update public.rooms set name = 'hacked';
update public.room_repos set everyone = false;
update public.room_members set is_admin = true where github_id = 102;
delete from public.room_members where github_id = 101;
select pg_temp.count_is($$select 1 from public.rooms where name='team'$$, 1);
select pg_temp.count_is($$select 1 from public.room_repos where everyone$$, 1);
select pg_temp.count_is($$select 1 from public.room_members where github_id=102 and not is_admin$$, 1);
select pg_temp.count_is('select 1 from public.room_members', 2);

-- 部外者の carol には何も見えない。自分を足すこともできない
select pg_temp.act('00000000-0000-0000-0000-00000000000c');
select pg_temp.count_is('select 1 from public.rooms', 0);
select pg_temp.count_is('select 1 from public.room_repos', 0);
select pg_temp.count_is('select 1 from public.room_members', 0);
select pg_temp.fails($$insert into public.room_members (room_id, github_id, login) select room, 103, 'carol' from pg_temp.ids$$, '42501');

-- ログインしていない役(anon)は表に触れない
reset role; set role anon;
select pg_temp.fails('select 1 from public.rooms', '42501');
reset role; set role authenticated;

-- 計画。読むのも送るのも、その部屋のメンバーだけ(役は問わない)。誰が送ったかは、送る側に書かせない
select pg_temp.act('00000000-0000-0000-0000-00000000000b');  -- bob(役なしのメンバー)
insert into public.plans (room_id, key, data, sent_by) select room, 'T-1', '{"題":"a"}', 999 from pg_temp.ids;
select pg_temp.count_is($$select 1 from public.plans where key = 'T-1' and sent_by = 102$$, 1);
insert into public.plans (room_id, key, data) select room, 'T-1', '{"題":"b"}' from pg_temp.ids
  on conflict (room_id, key) do update set data = excluded.data;
select pg_temp.count_is($$select 1 from public.plans where data->>'題' = 'b'$$, 1);
select pg_temp.fails($$insert into public.plans (room_id, key, data) select room, 'T-2', '[1]' from pg_temp.ids$$, '23514');
select pg_temp.fails($$insert into public.plans (room_id, key, data) select room, '', '{}' from pg_temp.ids$$, '23514');
select pg_temp.act('00000000-0000-0000-0000-00000000000a');  -- alice(管理者)にも見える
select pg_temp.count_is('select 1 from public.plans', 1);
select pg_temp.act('00000000-0000-0000-0000-00000000000c');  -- 部外者には見えず、送れず、変えられない
select pg_temp.count_is('select 1 from public.plans', 0);
select pg_temp.fails($$insert into public.plans (room_id, key, data) select room, 'T-9', '{}' from pg_temp.ids$$, '42501');
update public.plans set data = '{"題":"evil"}';
delete from public.plans;
reset role; set role anon;
select pg_temp.fails('select 1 from public.plans', '42501');
reset role;
select pg_temp.count_is($$select 1 from public.plans where data->>'題' = 'b'$$, 1);
set role authenticated;

-- 配るアプリの版。書けるのは DB を動かしている人だけ。読めるのは、どこかの部屋に入っている人だけ
reset role;
insert into public.app_releases (app, version, min_version, files) values ('demo', '0.2.0', '0.1.0', '[]');
insert into storage.objects (bucket_id, name) values ('releases', 'demo/0.2.0/a.exe'), ('other', 'x');
select pg_temp.count_is($$select 1 from storage.buckets where id = 'releases' and not public$$, 1);
set role authenticated;
select pg_temp.act('00000000-0000-0000-0000-00000000000a');  -- メンバー
select pg_temp.count_is('select 1 from public.app_releases', 1);
select pg_temp.count_is($$select 1 from storage.objects where bucket_id = 'releases'$$, 1);
select pg_temp.count_is($$select 1 from storage.objects where bucket_id = 'other'$$, 0);
select pg_temp.fails($$insert into public.app_releases (app, version, min_version, files) values ('demo', '9.9.9', '9.9.9', '[]')$$, '42501');
select pg_temp.fails($$update public.app_releases set version = '9.9.9'$$, '42501');
select pg_temp.fails($$delete from public.app_releases$$, '42501');
select pg_temp.fails($$insert into storage.objects (bucket_id, name) values ('releases', 'demo/9.9.9/a.exe')$$, '42501');
update storage.objects set name = 'evil' where bucket_id = 'releases';
delete from storage.objects where bucket_id = 'releases';
select pg_temp.fails($$select 1 from public.schema_migrations$$, '42501');
select pg_temp.act('00000000-0000-0000-0000-00000000000c');  -- 部外者
select pg_temp.count_is('select 1 from public.app_releases', 0);
select pg_temp.count_is('select 1 from storage.objects', 0);
select pg_temp.act('');                                      -- ログインしていない
select pg_temp.count_is('select 1 from public.app_releases', 0);
reset role; set role anon;
select pg_temp.fails('select 1 from public.app_releases', '42501');
reset role;
select pg_temp.count_is($$select 1 from storage.objects where bucket_id = 'releases' and name = 'demo/0.2.0/a.exe'$$, 1);
select pg_temp.count_is($$select 1 from public.schema_migrations$$, 7);
-- 招かれた人は、役なしで始まる(計画・実装も)
select pg_temp.count_is($$select 1 from public.room_members where github_id = 102 and not plan and not implement$$, 1);
set role authenticated;

-- 最後の管理者は出られない・降りられない
select pg_temp.act('00000000-0000-0000-0000-00000000000a');
select pg_temp.fails($$delete from public.room_members where github_id = 101$$, '23514');
select pg_temp.fails($$update public.room_members set is_admin = false where github_id = 101$$, '23514');

-- bob は自分で出られる
select pg_temp.act('00000000-0000-0000-0000-00000000000b');
delete from public.room_members where github_id = 102;
select pg_temp.count_is('select 1 from public.rooms', 0);

-- 管理者は部屋ごと消せる。repo とメンバーも消える
select pg_temp.act('00000000-0000-0000-0000-00000000000a');
delete from public.rooms;
reset role;
select pg_temp.count_is('select 1 from public.rooms', 0);
select pg_temp.count_is('select 1 from public.room_members', 0);
select pg_temp.count_is('select 1 from public.room_repos', 0);
select pg_temp.count_is('select 1 from public.plans', 0);  -- 部屋を消すと、計画も消える
\echo RLS OK
