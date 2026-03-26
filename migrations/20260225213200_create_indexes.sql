CREATE INDEX idx_friends_reverse ON friends (user_2, user_1);
CREATE INDEX idx_friend_requests_to ON friend_requests("to");
CREATE INDEX idx_group_users_user_id ON group_users(user_id);
CREATE INDEX idx_channels_group_id ON channels(group_id);
CREATE INDEX idx_roles_group_id ON roles(group_id);
CREATE INDEX idx_permission_overrides_group_id ON permission_overrides(group_id);
