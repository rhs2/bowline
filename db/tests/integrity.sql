-- Database integrity tests. Runs inside one transaction that is always rolled
-- back, so it is safe against any database that has the migrations applied.
-- Every expectation is asserted in PL/pgSQL; the script fails loudly on the
-- first broken rule.
begin;

create temporary table t (k text primary key, v uuid);
insert into t values
  ('d1', gen_random_uuid()), ('p1', gen_random_uuid()), ('p2', gen_random_uuid()), ('p7', gen_random_uuid()),
  ('e1', gen_random_uuid()), ('e2', gen_random_uuid()), ('e3', gen_random_uuid()), ('e4', gen_random_uuid()), ('e5', gen_random_uuid());

create or replace function tv(text) returns uuid language sql as $$ select v from t where k = $1 $$;

insert into departments (id, code, name) values (tv('d1'), 'T_EXEC', 'Test Executive Office');
insert into positions (id, code, title, level) values
  (tv('p1'), 'T_CEO', 'Chief Executive Officer', 1),
  (tv('p2'), 'T_COO', 'Chief Operating Officer', 2),
  (tv('p7'), 'T_DRV', 'Driver', 7);
-- The fixture hangs off the existing CEO when the database already holds a
-- company, and provides its own root when it does not, so this file can be run
-- against an empty database in CI and against a seeded one by hand.
insert into employees (id, employee_no, first_name, last_name, email, position_id, department_id, manager_id, hire_date) values
  (tv('e1'), 'T-1', 'Ada', 'Chief', 't1@test.invalid', tv('p1'), tv('d1'),
   (select id from employees where manager_id is null and status <> 'terminated' limit 1), '2020-01-01'),
  (tv('e2'), 'T-2', 'Bo',  'Ops',   't2@test.invalid', tv('p2'), tv('d1'), tv('e1'), '2020-01-01'),
  (tv('e3'), 'T-3', 'Cy',  'Drive', 't3@test.invalid', tv('p7'), tv('d1'), tv('e2'), '2020-01-01'),
  (tv('e4'), 'T-4', 'Di',  'Dock',  't4@test.invalid', tv('p7'), tv('d1'), tv('e3'), '2020-01-01'),
  (tv('e5'), 'T-5', 'Ed',  'Fork',  't5@test.invalid', tv('p7'), tv('d1'), tv('e4'), '2020-01-01');

create function pg_temp.expect_error(sql text, label text) returns void
language plpgsql as $f$
begin
  begin
    execute sql;
  exception when check_violation or exclusion_violation or raise_exception then
    return;
  end;
  raise exception 'INTEGRITY TEST FAILED: % was accepted but must be rejected', label;
end $f$;

set constraints all immediate;

do $$
declare
  n int; s numeric;
begin
  -- 1. paths are derived from the manager chain. Measured relative to the
  -- fixture root, since that root may itself sit under a real CEO.
  if (select nlevel(path) from employees where id = tv('e5'))
     <> (select nlevel(path) from employees where id = tv('e1')) + 4 then
    raise exception 'INTEGRITY TEST FAILED: e5 should sit four levels below the fixture root';
  end if;

  -- 2. only one CEO
  perform pg_temp.expect_error(format($q$insert into employees (employee_no, first_name, last_name, email, position_id, department_id, manager_id, hire_date)
    values ('T-X','X','X','tx@test.invalid',%L,%L,null,'2020-01-01')$q$, tv('p1'), tv('d1')), 'second CEO');

  -- 3. cycles are rejected (CEO under a grandchild)
  perform pg_temp.expect_error(format('update employees set manager_id = %L where id = %L', tv('e5'), tv('e1')), 'reporting cycle');

  -- 4. re-parenting cascades to the whole subtree
  update employees set manager_id = tv('e1') where id = tv('e3');
  if (select path from employees where id = tv('e5')) <> (select path from employees where id = tv('e1')) || employee_label(tv('e3')) || employee_label(tv('e4')) || employee_label(tv('e5')) then
    raise exception 'INTEGRITY TEST FAILED: cascade did not rewrite e5';
  end if;
  select count(*) into n from employees where path <@ (select path from employees where id = tv('e2')) and id <> tv('e2');
  if n <> 0 then raise exception 'INTEGRITY TEST FAILED: COO subtree should be empty, got %', n; end if;
  select count(*) into n from employees where path <@ (select path from employees where id = tv('e3')) and id <> tv('e3');
  if n <> 2 then raise exception 'INTEGRITY TEST FAILED: e3 subtree should be 2, got %', n; end if;

  -- 5. path cannot be written by hand
  perform pg_temp.expect_error(format('update employees set path = %L where id = %L', 'a.b', tv('e5')), 'manual path write');

  -- 6. unbalanced journal entry
  insert into journal_entries (id, period_id, entry_date, memo, source_type)
    values ('11111111-1111-1111-1111-111111111111', (select id from fiscal_periods where starts_on <= current_date and ends_on >= current_date), current_date, 'unbalanced', 'manual');
  perform pg_temp.expect_error($q$insert into journal_lines (entry_id, account_id, debit) values ('11111111-1111-1111-1111-111111111111', (select id from accounts where code = '1000'), 100)$q$, 'unbalanced entry');

  -- 7. balanced entry is accepted and lines are then immutable
  insert into journal_entries (id, period_id, entry_date, memo, source_type)
    values ('22222222-2222-2222-2222-222222222222', (select id from fiscal_periods where starts_on <= current_date and ends_on >= current_date), current_date, 'balanced', 'manual');
  insert into journal_lines (entry_id, account_id, debit, credit) values
    ('22222222-2222-2222-2222-222222222222', (select id from accounts where code = '1100'), 100, 0),
    ('22222222-2222-2222-2222-222222222222', (select id from accounts where code = '4000'), 0, 100);
  perform pg_temp.expect_error($q$update journal_lines set debit = 1 where entry_id = '22222222-2222-2222-2222-222222222222'$q$, 'line edit');
  perform pg_temp.expect_error($q$delete from journal_lines where entry_id = '22222222-2222-2222-2222-222222222222'$q$, 'line delete');
  perform pg_temp.expect_error($q$delete from journal_entries where id = '22222222-2222-2222-2222-222222222222'$q$, 'entry delete');

  -- 8. closed period
  update fiscal_periods set status = 'closed' where starts_on = (select min(starts_on) from fiscal_periods);
  perform pg_temp.expect_error($q$insert into journal_entries (period_id, entry_date, memo, source_type)
    values ((select id from fiscal_periods where status = 'closed' limit 1), (select starts_on from fiscal_periods where status = 'closed' limit 1), 'late', 'manual')$q$, 'closed period post');

  -- 9. trial balance sums to zero
  select sum(balance) into s from trial_balance;
  if s <> 0 then raise exception 'INTEGRITY TEST FAILED: trial balance is %', s; end if;

  -- 10. overlapping leave
  insert into leave_requests (employee_id, type_key, start_date, end_date, days) values (tv('e5'), 'annual', '2030-09-01', '2030-09-05', 5);
  perform pg_temp.expect_error(format($q$insert into leave_requests (employee_id, type_key, start_date, end_date, days) values (%L, 'sick', '2030-09-05', '2030-09-06', 2)$q$, tv('e5')), 'overlapping leave');

  -- 11. audit log is append-only
  insert into audit_log (action, entity_type) values ('test', 'x');
  perform pg_temp.expect_error('delete from audit_log where action = ''test''', 'audit delete');
  perform pg_temp.expect_error('update audit_log set action = ''y'' where action = ''test''', 'audit update');

  raise notice 'integrity: all 11 rules hold';
end $$;

rollback;
