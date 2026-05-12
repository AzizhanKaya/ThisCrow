-- Allow channel permission overrides targeted at @everyone, which is not a real
-- row in the roles table. @everyone overrides are stored with both role_id and
-- user_id set to NULL.

ALTER TABLE permission_overrides DROP CONSTRAINT IF EXISTS permission_overrides_check;

ALTER TABLE permission_overrides
    ADD CONSTRAINT permission_overrides_target_check CHECK (
        NOT (role_id IS NOT NULL AND user_id IS NOT NULL)
    );
