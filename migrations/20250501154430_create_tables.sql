-- Create users table
CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    avatar TEXT,
    username TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    email TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    public_key BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen TIMESTAMPTZ
);
-- Create friends table
CREATE TABLE friends (
    user_1 INT NOT NULL REFERENCES users(id),
    user_2 INT NOT NULL REFERENCES users(id),
    PRIMARY KEY (user_1, user_2),
    CHECK (user_1 < user_2)
);
-- Create friend_requests table
CREATE TABLE friend_requests (
    "from" INT NOT NULL REFERENCES users(id),
    "to" INT NOT NULL REFERENCES users(id),
    requested_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY ("from", "to")
);
-- Create groups table
CREATE TABLE groups (
    id SERIAL PRIMARY KEY,
    icon TEXT,
    name TEXT NOT NULL,
    description TEXT,
    created_by INT NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- Create group_users table
CREATE TABLE group_users (
    group_id INT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    user_id INT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    position SMALLINT NOT NULL,
    name TEXT,
    PRIMARY KEY (group_id, user_id)
);
-- Create channels table
CREATE TABLE channels (
    id SERIAL PRIMARY KEY,
    group_id INT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    channel_type BOOLEAN NOT NULL DEFAULT false,
    position SMALLINT NOT NULL,
    title TEXT,
    name TEXT NOT NULL
);
-- Create roles table
CREATE TABLE roles (
    id SERIAL PRIMARY KEY,
    group_id INT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    color TEXT,
    permissions BIGINT NOT NULL DEFAULT 0,
    position SMALLINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (group_id, id)
);
-- Create group_user_roles table
CREATE TABLE group_user_roles (
    group_id INT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    user_id INT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id INT NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    PRIMARY KEY (group_id, user_id, role_id),
    FOREIGN KEY (group_id, user_id) REFERENCES group_users(group_id, user_id) ON DELETE CASCADE,
    FOREIGN KEY (group_id, role_id) REFERENCES roles(group_id, id) ON DELETE CASCADE
);
-- Create permission_overrides table
CREATE TABLE permission_overrides (
    id SERIAL PRIMARY KEY,
    group_id INT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    channel_id INT NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    role_id INT REFERENCES roles(id) ON DELETE CASCADE,
    user_id INT REFERENCES users(id) ON DELETE CASCADE,
    allow INT NOT NULL DEFAULT 0,
    deny INT NOT NULL DEFAULT 0,
    CHECK (
        (role_id IS NOT NULL AND user_id IS NULL)
        OR 
        (role_id IS NULL AND user_id IS NOT NULL)
    ),
    UNIQUE (channel_id, role_id, user_id)
);