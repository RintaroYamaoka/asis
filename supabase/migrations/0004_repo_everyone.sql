-- repo ごとに、誰の分を出すかを選べるようにする。
-- 既定はメンバーの分だけ。everyone を立てた repo は、メンバーでない人の PR も出す。
alter table public.room_repos add column everyone boolean not null default false;

grant update on public.room_repos to authenticated;
create policy repos_change on public.room_repos for update to authenticated
  using (public.is_admin(room_id)) with check (public.is_admin(room_id));
