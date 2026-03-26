-- Create invitations table
CREATE TABLE invitations (
    id SERIAL PRIMARY KEY,
    code TEXT NOT NULL UNIQUE,
    group_id INT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    created_by INT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    max_uses INT,
    uses INT NOT NULL DEFAULT 0,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_invitations_code ON invitations(code);
CREATE INDEX idx_invitations_group_id ON invitations(group_id);
