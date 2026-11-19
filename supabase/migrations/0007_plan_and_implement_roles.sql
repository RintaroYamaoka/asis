-- 役を 2 つ足す。計画する(issue を書く・本番に出たものを確かめて閉じる)と、実装する。
-- 今までの役(確認・取り込み・本番)は、PR が出てからのことしか言えなかった。
alter table public.room_members
  add column plan      boolean not null default false,
  add column implement boolean not null default false;

-- 部屋を作った人は、全部の役を持って始まる(今までと同じ決まり)。今ある部屋も、その形にそろえる。
update public.room_members set plan = true, implement = true
  where is_admin and review and merge and deploy;

-- create or replace は、呼べる人の決まり(0003 の revoke / grant)をそのまま保つ
create or replace function public.create_room(room_name text) returns uuid
language plpgsql security definer set search_path = '' as $$
declare
  me bigint := public.github_id();
  my_login text := public.github_login();
  new_id uuid;
begin
  if me is null or my_login is null then
    raise exception 'GitHub でログインしていません' using errcode = '28000';
  end if;
  if not exists (select 1 from public.room_creators c where c.github_id = me) then
    raise exception 'この DB では、部屋を作る許可がありません' using errcode = '42501';
  end if;
  insert into public.rooms (name) values (room_name) returning id into new_id;
  insert into public.room_members (room_id, github_id, login, is_admin, plan, implement, review, merge, deploy)
  values (new_id, me, my_login, true, true, true, true, true, true);
  return new_id;
end
$$;

insert into public.schema_migrations (version) values ('0007');
