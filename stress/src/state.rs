use dashmap::DashMap;

#[derive(Debug, Clone)]
pub struct User {
    pub id: u64,
    pub username: String,
    pub cookie: String,
    // Add other fields as needed
}

#[derive(Debug, Clone)]
pub struct Group {
    pub id: u64,
    pub channel_ids: Vec<u64>,
}

#[derive(Debug)]
pub struct AppState {
    /// Map of UserID -> User
    pub users: DashMap<u64, User>,
    /// Map of GroupID -> Group
    pub groups: DashMap<u64, Group>,
    /// Map of UserID -> List of (FriendID, AffinityWeight)
    /// This represents the adjacency list of the friendship graph.
    /// Since affinity is mutual, `friends.get(A).contains(B)` implies `friends.get(B).contains(A)`
    /// and the weights should be identical.
    pub friends: DashMap<u64, Vec<(u64, f64)>>,
    /// Map of UserID -> List of GroupIDs they belong to
    pub user_groups: DashMap<u64, Vec<u64>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            users: DashMap::new(),
            groups: DashMap::new(),
            friends: DashMap::new(),
            user_groups: DashMap::new(),
        }
    }
}
