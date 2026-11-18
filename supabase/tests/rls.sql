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
select pg_temp.count_is($$select 1 from public.room_members where login='alice' and is_admin and review and merge and deploy$$, 1);

-- 表に直接 入れて部屋を作ることはできない
select pg_temp.fails($$insert into public.rooms (name) values ('raw')$$, '42501');

-- 管理者は repo を足し、名前で人を招ける
insert into public.room_repos select room, 'acme/app' from pg_temp.ids;
insert into public.room_members (room_id, github_id, login) select room, 102, 'bob' from pg_temp.ids;
select pg_temp.fails($$insert into public.room_repos select room, 'not a repo' from pg_temp.ids$$, '23514');

-- 招かれた bob には部屋が見える。変えることも、自分の部屋を作ることもできない
select pg_temp.act('00000000-0000-0000-0000-00000000000b');
select pg_temp.fails($$select public.create_room('mine')$$, '42501');
select pg_temp.count_is('select 1 from public.rooms', 1);
select pg_temp.count_is('select 1 from public.room_repos', 1);
select pg_temp.count_is('select 1 from public.room_members', 2);
select pg_temp.fails($$insert into public.room_repos select room, 'acme/other' from pg_temp.ids$$, '42501');
select pg_temp.fails($$insert into public.room_members (room_id, github_id, login) select room, 103, 'carol' from pg_temp.ids$$, '42501');
update public.rooms set name = 'hacked';
update public.room_members set is_admin = true where github_id = 102;
delete from public.room_members where github_id = 101;
select pg_temp.count_is($$select 1 from public.rooms where name='team'$$, 1);
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
\echo RLS OK
