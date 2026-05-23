CREATE TABLE reactions (
    message_id BIGINT NOT NULL,
    user_id    INT    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    reaction   CHAR(1) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (message_id, user_id, reaction)
);
CREATE INDEX idx_reactions_message_id ON reactions(message_id);
