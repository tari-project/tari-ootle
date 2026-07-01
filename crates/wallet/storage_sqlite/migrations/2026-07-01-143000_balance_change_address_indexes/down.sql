DROP INDEX IF EXISTS account_balance_changes_account_created_idx;
DROP INDEX IF EXISTS account_balance_changes_account_resource_created_idx;

CREATE INDEX account_balance_changes_account_created_idx
    ON account_balance_changes (account_id, created_at DESC, id DESC);
CREATE INDEX account_balance_changes_account_resource_created_idx
    ON account_balance_changes (account_id, resource_id, created_at DESC, id DESC);
