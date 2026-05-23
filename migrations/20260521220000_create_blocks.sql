CREATE TABLE blocks (
    "from" INT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    "to"   INT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY ("from", "to"),
    CHECK ("from" <> "to")
);

CREATE INDEX idx_blocks_to ON blocks("to");
