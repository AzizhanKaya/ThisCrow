CREATE TABLE group_bans (
    group_id  INT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    user_id   INT NOT NULL REFERENCES users(id)  ON DELETE CASCADE,
    banned_by INT     NULL REFERENCES users(id)  ON DELETE NO ACTION,
    banned_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (group_id, user_id)
);

CREATE INDEX idx_group_bans_group_id ON group_bans(group_id);
