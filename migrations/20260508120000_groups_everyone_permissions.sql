ALTER TABLE groups
    ADD COLUMN everyone_permissions BIGINT NOT NULL DEFAULT 0;
