use rand_distr::{Exp, LogNormal, Normal};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationConfig {
    /// Number of users to simulate
    pub user_count: usize,
    /// Mean latency for network requests (ms)
    pub mean_latency_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ActionDistributions {
    /// Time between direct messages (seconds) - Exponential distribution often fits "time between events"
    pub dm_interval: Exp<f64>,
    /// Time between group messages (seconds)
    pub group_msg_interval: Exp<f64>,
    /// Number of friends per user - LogNormal or Normal
    pub friend_count: LogNormal<f64>,
    /// Affinity strength between friends (0.0 to 1.0) - Beta distribution is good for probabilities,
    /// but for simplicity we might use Normal clamped to 0-1 or just Uniform if not specified.
    /// User asked for "mutual affinity", so we'll generate a weight for each friendship edge.
    /// Let's use Normal for the weight distribution.
    pub friend_affinity: Normal<f64>,
    /// Number of groups a user is member of
    pub groups_per_user: LogNormal<f64>,
    /// Number of users in a group
    pub group_size: LogNormal<f64>,
    /// Number of channels in a group
    pub channels_per_group: LogNormal<f64>,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            user_count: 100_000,
            mean_latency_ms: 50,
        }
    }
}

impl Default for ActionDistributions {
    fn default() -> Self {
        Self {
            // Expected mean ~ 60s
            dm_interval: Exp::new(1.0 / 60.0).unwrap(),
            // Expected mean ~ 120s
            group_msg_interval: Exp::new(1.0 / 120.0).unwrap(),
            // Mean ~ 50 friends
            friend_count: LogNormal::new(3.9, 0.5).unwrap(),
            // Affinity mean 0.5, std_dev 0.2
            friend_affinity: Normal::new(0.5, 0.2).unwrap(),
            // Mean ~ 5 groups
            groups_per_user: LogNormal::new(1.6, 0.5).unwrap(),
            // Mean ~ 50 users in group
            group_size: LogNormal::new(3.9, 0.8).unwrap(),
            // Mean ~ 5 channels
            channels_per_group: LogNormal::new(1.6, 0.4).unwrap(),
        }
    }
}
