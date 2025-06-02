-- Enable UUID extension
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- Create users table
CREATE TABLE IF NOT EXISTS users (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    avatar TEXT,
    username TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    email TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Create groups table
CREATE TABLE IF NOT EXISTS groups (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    avatar TEXT,
    name TEXT NOT NULL,
    users UUID[] NOT NULL DEFAULT '{}',
    voicechats UUID[] NOT NULL DEFAULT '{}',
    chats UUID[] NOT NULL DEFAULT '{}',
    admin UUID[] NOT NULL DEFAULT '{}',
    description TEXT,
    created_by UUID NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Create messages table
CREATE TABLE IF NOT EXISTS messages (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    "from" UUID NOT NULL REFERENCES users(id),
    "to" UUID NOT NULL REFERENCES users(id),
    data JSONB NOT NULL,
    time TIMESTAMPTZ NOT NULL DEFAULT now(),
    type TEXT NOT NULL
);

-- Create voicechats table
CREATE TABLE IF NOT EXISTS voicechats (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name TEXT NOT NULL
);

-- Create chats table
CREATE TABLE IF NOT EXISTS chats (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name TEXT NOT NULL
); 

-- Create friends table
CREATE TABLE IF NOT EXISTS friends (
    user_1 UUID NOT NULL REFERENCES users(id),
    user_2 UUID NOT NULL REFERENCES users(id),
    requested BOOLEAN DEFAULT true,
    PRIMARY KEY (user_1, user_2),
    CHECK (user_1 < user_2)
);

-- Create friend_requests table
CREATE TABLE IF NOT EXISTS friend_requests (
    "from" UUID NOT NULL REFERENCES users(id),
    "to" UUID NOT NULL REFERENCES users(id),
    PRIMARY KEY ("from", "to")
);