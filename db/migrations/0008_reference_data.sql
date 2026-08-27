-- ============================================================================
-- Reference data: permissions, roles, leave types, chart of accounts, fiscal
-- periods for the current and next year. Idempotent (on conflict do nothing).
-- ============================================================================

insert into permissions (key, description) values
  ('org:read',                    'View the org chart (names, titles, reporting lines)'),
  ('employees:read:self',         'View own employee record'),
  ('employees:read:subtree',      'View employees who report up to me'),
  ('employees:read:department',   'View employees in my department tree'),
  ('employees:read:all',          'View every employee'),
  ('employees:write:subtree',     'Edit employees who report up to me'),
  ('employees:write:all',         'Edit any employee (HR)'),
  ('leave:request',               'Request leave for myself'),
  ('leave:approve:subtree',       'Approve or reject leave for my reports'),
  ('leave:manage:all',            'Create, approve or cancel leave for anyone (HR)'),
  ('shifts:manage:subtree',       'Schedule shifts for my reports'),
  ('attendance:record:self',      'Clock in and out'),
  ('documents:read:self',         'View own documents'),
  ('documents:manage:all',        'Upload and view any employee document (HR)'),
  ('shipments:read',              'View shipments'),
  ('shipments:write',             'Create and update shipments'),
  ('shipments:assign',            'Assign legs, drivers, vehicles and work orders'),
  ('customers:read',              'View customers'),
  ('customers:manage',            'Create and update customers'),
  ('fleet:manage',                'Manage vehicles, carriers and sites'),
  ('tasks:read:self',             'View my work orders'),
  ('tasks:update:self',           'Update my work orders'),
  ('tasks:manage:subtree',        'Create and assign work orders for my reports'),
  ('ledger:read',                 'View the ledger and financial reports'),
  ('ledger:post',                 'Post manual journal entries'),
  ('periods:close',               'Close a fiscal period'),
  ('invoices:draft',              'Create and edit draft invoices'),
  ('invoices:issue',              'Issue invoices below the approval threshold'),
  ('invoices:approve',            'Approve invoices at or above the threshold'),
  ('payments:record',             'Record customer payments'),
  ('vendors:manage',              'Manage vendors and vendor bills'),
  ('expenses:submit',             'Submit expense claims'),
  ('expenses:approve:subtree',    'Manager approval of expense claims'),
  ('expenses:approve:finance',    'Finance approval and payment of expense claims'),
  ('payroll:prepare',             'Prepare payroll runs'),
  ('payroll:approve',             'Approve and post payroll runs'),
  ('payroll:read:all',            'View payroll for everyone'),
  ('reports:read:department',     'View operational reports for my department'),
  ('reports:read:all',            'View all reports'),
  ('messages:send:chain',         'Message my manager and my direct reports'),
  ('messages:send:department',    'Message anyone in my department'),
  ('messages:send:subtree',       'Message anyone who reports up to me'),
  ('messages:send:any',           'Message anyone in the company'),
  ('messages:broadcast:subtree',  'Announce to everyone who reports up to me'),
  ('messages:broadcast:company',  'Announce to the whole company'),
  ('tickets:create',              'Open support tickets'),
  ('tickets:manage',              'Triage, assign and resolve tickets'),
  ('tickets:read:all',            'View every ticket'),
  ('users:manage',                'Create users, reset passwords, lock accounts'),
  ('roles:manage',                'Assign roles'),
  ('audit:read',                  'Read the audit log'),
  ('system:admin',                'Reopen periods and other break-glass actions')
on conflict (key) do nothing;

insert into roles (key, name, description) values
  ('baseline',      'Baseline',        'Granted to every active user'),
  ('field_worker',  'Field worker',    'Drivers, dock crews, handlers'),
  ('staff',         'Staff',           'Coordinators and specialists'),
  ('supervisor',    'Supervisor',      'Level 5: leads a team'),
  ('manager',       'Manager',         'Level 4: runs a unit'),
  ('director',      'Director',        'Level 3: runs a department'),
  ('executive',     'Executive',       'Levels 1 and 2'),
  ('hr_admin',      'HR administrator','People Operations'),
  ('accountant',    'Accountant',      'Accounting, billing and payroll specialists'),
  ('finance_admin', 'Finance admin',   'CFO, Director of Finance, Billing Manager'),
  ('dispatcher',    'Dispatcher',      'Dispatch and freight coordination'),
  ('support_agent', 'Support agent',   'Service Desk'),
  ('it_admin',      'IT administrator','Platform Engineering'),
  ('auditor',       'Auditor',         'Read-only access everywhere')
on conflict (key) do nothing;

-- role -> permissions
with rp(role_key, perm) as (values
  ('baseline','org:read'),('baseline','employees:read:self'),('baseline','leave:request'),
  ('baseline','attendance:record:self'),('baseline','documents:read:self'),('baseline','tasks:read:self'),
  ('baseline','tasks:update:self'),('baseline','expenses:submit'),('baseline','messages:send:chain'),
  ('baseline','messages:send:department'),('baseline','tickets:create'),('baseline','shipments:read'),

  ('staff','shipments:write'),('staff','customers:read'),

  ('supervisor','employees:read:subtree'),('supervisor','tasks:manage:subtree'),('supervisor','shifts:manage:subtree'),
  ('supervisor','leave:approve:subtree'),('supervisor','expenses:approve:subtree'),('supervisor','messages:send:subtree'),

  ('manager','employees:read:subtree'),('manager','tasks:manage:subtree'),('manager','shifts:manage:subtree'),
  ('manager','leave:approve:subtree'),('manager','expenses:approve:subtree'),('manager','messages:send:subtree'),
  ('manager','employees:write:subtree'),('manager','messages:broadcast:subtree'),('manager','shipments:assign'),

  ('director','employees:read:subtree'),('director','tasks:manage:subtree'),('director','shifts:manage:subtree'),
  ('director','leave:approve:subtree'),('director','expenses:approve:subtree'),('director','messages:send:subtree'),
  ('director','employees:write:subtree'),('director','messages:broadcast:subtree'),('director','shipments:assign'),
  ('director','employees:read:department'),('director','reports:read:department'),

  ('executive','employees:read:subtree'),('executive','tasks:manage:subtree'),('executive','shifts:manage:subtree'),
  ('executive','leave:approve:subtree'),('executive','expenses:approve:subtree'),('executive','messages:send:subtree'),
  ('executive','employees:write:subtree'),('executive','messages:broadcast:subtree'),('executive','shipments:assign'),
  ('executive','employees:read:department'),('executive','reports:read:department'),
  ('executive','employees:read:all'),('executive','messages:broadcast:company'),('executive','messages:send:any'),
  ('executive','reports:read:all'),('executive','audit:read'),('executive','payroll:read:all'),('executive','ledger:read'),
  ('executive','tickets:read:all'),

  ('hr_admin','employees:read:all'),('hr_admin','employees:write:all'),('hr_admin','leave:manage:all'),
  ('hr_admin','documents:manage:all'),('hr_admin','payroll:prepare'),('hr_admin','users:manage'),('hr_admin','payroll:read:all'),

  ('accountant','ledger:read'),('accountant','ledger:post'),('accountant','invoices:draft'),('accountant','invoices:issue'),
  ('accountant','payments:record'),('accountant','vendors:manage'),('accountant','payroll:prepare'),('accountant','reports:read:all'),
  ('accountant','customers:read'),

  ('finance_admin','ledger:read'),('finance_admin','ledger:post'),('finance_admin','invoices:draft'),('finance_admin','invoices:issue'),
  ('finance_admin','payments:record'),('finance_admin','vendors:manage'),('finance_admin','payroll:prepare'),('finance_admin','reports:read:all'),
  ('finance_admin','customers:read'),('finance_admin','invoices:approve'),('finance_admin','expenses:approve:finance'),
  ('finance_admin','payroll:approve'),('finance_admin','periods:close'),('finance_admin','payroll:read:all'),

  ('dispatcher','shipments:write'),('dispatcher','shipments:assign'),('dispatcher','fleet:manage'),
  ('dispatcher','tasks:manage:subtree'),('dispatcher','customers:read'),('dispatcher','customers:manage'),

  ('support_agent','tickets:manage'),('support_agent','tickets:read:all'),('support_agent','messages:send:any'),

  ('it_admin','users:manage'),('it_admin','roles:manage'),('it_admin','audit:read'),('it_admin','system:admin'),

  ('auditor','ledger:read'),('auditor','audit:read'),('auditor','employees:read:all'),('auditor','reports:read:all'),
  ('auditor','tickets:read:all'),('auditor','payroll:read:all')
)
insert into role_permissions (role_id, permission_key)
select r.id, rp.perm from rp join roles r on r.key = rp.role_key
on conflict do nothing;

insert into leave_types (key, name, paid, annual_quota_days) values
  ('annual',   'Annual leave',   true,  20),
  ('sick',     'Sick leave',     true,  10),
  ('unpaid',   'Unpaid leave',   false, 0),
  ('parental', 'Parental leave', true,  90)
on conflict (key) do nothing;

insert into accounts (code, name, type) values
  ('1000','Cash and Bank','asset'),
  ('1100','Accounts Receivable','asset'),
  ('1200','Prepaid Expenses','asset'),
  ('1500','Vehicles and Equipment','asset'),
  ('2000','Accounts Payable','liability'),
  ('2100','Salaries Payable','liability'),
  ('2200','Taxes Payable','liability'),
  ('3000','Share Capital','equity'),
  ('3100','Retained Earnings','equity'),
  ('4000','Freight Revenue','revenue'),
  ('4100','Warehousing Revenue','revenue'),
  ('4200','Customs Brokerage Revenue','revenue'),
  ('5000','Carrier Costs','expense'),
  ('5100','Salaries and Wages','expense'),
  ('5200','Fuel','expense'),
  ('5300','Warehouse Operations','expense'),
  ('5400','Office and Administration','expense'),
  ('5500','Depreciation','expense'),
  ('5600','Bad Debt','expense'),
  ('5700','Travel and Meals','expense')
on conflict (code) do nothing;

-- Fiscal periods: previous, current and next calendar year, all open.
insert into fiscal_periods (year, month, starts_on, ends_on)
select extract(year from d)::smallint, extract(month from d)::smallint, d::date, (d + interval '1 month - 1 day')::date
  from generate_series(date_trunc('year', now() - interval '1 year'),
                       date_trunc('year', now() + interval '1 year') + interval '11 months',
                       interval '1 month') d
on conflict (year, month) do nothing;
