use crate::config::{ActionDistributions, SimulationConfig};
use crate::state::{AppState, Group, User};
use futures::{StreamExt, stream};
use log::{debug, info};
use rand::prelude::*;
use rand_distr::Distribution;
use std::sync::Arc;

// Placeholder for API URL - assuming localhost for now
const API_BASE_URL: &str = "http://localhost:3000/api";

pub async fn run(
    state: Arc<AppState>,
    config: &SimulationConfig,
    dist: &ActionDistributions,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. User Registration
    info!("Registering {} users...", config.user_count);

    let client = reqwest::Client::new();

    // Create a stream of indices 0..user_count
    let mut stream = stream::iter(0..config.user_count)
        .map(|i| {
            let client = client.clone();
            let state = state.clone();
            async move {
                let username = format!("user_{}", i);
                let password = "password123";

                // TODO: Replace with actual HTTP call
                // For now, simulating success and token generation
                // let res = client.post(format!("{}/register", API_BASE_URL)).json(&...).send().await?;
                // let cookie = res.cookies()...;

                let cookie = format!("session_id=mock_cookie_{}", i);
                let user_id = i as u64; // Mock ID

                let user = User {
                    id: user_id,
                    username,
                    cookie,
                };

                state.users.insert(user_id, user);
                if i % 1000 == 0 {
                    info!("Registered {} users", i);
                }
                Ok::<_, anyhow::Error>(())
            }
        })
        .buffer_unordered(100); // Concurrency limit

    while let Some(res) = stream.next().await {
        if let Err(e) = res {
            // Log error but maybe continue?
            // prioritizing bulk creation
            debug!("Failed to register user: {}", e);
        }
    }

    info!("User registration complete. Total: {}", state.users.len());

    // 2. Group Creation & Membership
    info!("Generating group memberships...");
    let mut rng = rand::rng();

    // We'll gather all memberships and then create the groups.
    // Map: GroupID -> Vec<UserID>
    // Since we don't know GroupIDs yet, we'll just create a list of groups.

    // To satisfy "User group membership count" distribution first:
    // We calculate how many groups each user WANTS to be in.
    let user_desired_groups: Vec<usize> = (0..config.user_count)
        .map(|_| {
            let count = dist.groups_per_user.sample(&mut rng).round() as usize;
            count.max(0)
        })
        .collect();

    let total_memberships: usize = user_desired_groups.iter().sum();
    info!("Total desired group memberships: {}", total_memberships);

    // Average group size to estimate total groups needed
    let avg_group_size = dist.group_size.sample(&mut rng).max(2.0); // Sample once? No, mean.
    // Actually we should just fill groups until memberships are exhausted.

    // Algorithm:
    // Create a pool of users who need groups (weighted by how many they need?).
    // Or simpler: Just iterate.

    let mut current_memberships = 0;
    let mut group_id_counter = 0u64;

    // We need a list of users available for more groups.
    // (UserID, RemainingSlots)
    let mut available_users: Vec<(u64, usize)> = user_desired_groups
        .iter()
        .enumerate()
        .filter(|&(_, &count)| count > 0)
        .map(|(i, &count)| (i as u64, count))
        .collect();

    // Shuffle for randomness
    available_users.shuffle(&mut rng);

    // Wait, the pop loop above has a logic gap: I need to handle the re-insertion properly.
    // Let's restart the loop logic slightly.

    // Correct loop for group filling:
    // Since we want to respect the distributions, we can't just close a group if we run out of users.
    // But we also can't force users into groups if they don't want to be.
    // Just fill as much as possible.

    // ----------------------------------------------------
    // REVISED GROUP GENERATION
    // ----------------------------------------------------
    let mut group_defs: Vec<Group> = Vec::new();

    // Use a flat list of "slots" (UserID) needed. Shuffle it.
    // Memory heavy? 100k users * 5 groups = 500k u64s = 4MB. Negligible.
    let mut user_slots: Vec<u64> = Vec::with_capacity(total_memberships);
    for (i, &count) in user_desired_groups.iter().enumerate() {
        for _ in 0..count {
            user_slots.push(i as u64);
        }
    }
    user_slots.shuffle(&mut rng);

    let mut current_slot_idx = 0;
    let mut gid = 0u64;

    while current_slot_idx < user_slots.len() {
        gid += 1;
        let size = (dist.group_size.sample(&mut rng).round() as usize).max(2);

        // Take slice
        let end = (current_slot_idx + size).min(user_slots.len());
        let members = &user_slots[current_slot_idx..end];

        // Avoid duplicate users in same group?
        // With random shuffle, collision is rare but possible.
        // We can dedup.
        let mut members_dedup = members.to_vec();
        members_dedup.sort();
        members_dedup.dedup();

        // If dedup reduced size significantly, we might want to fill up?
        // Ignore for now for performance, simulated behavior is "close enough".

        // Generate Channels
        let channel_count = (dist.channels_per_group.sample(&mut rng).round() as usize).max(1);
        let channel_ids: Vec<u64> = (0..channel_count)
            .map(|c| (gid * 1000 + c as u64))
            .collect(); // Mock Channel IDs

        let group = Group {
            id: gid,
            channel_ids,
        };

        // Store Group
        state.groups.insert(gid, group.clone());

        // Update User memberships
        for &uid in &members_dedup {
            // DashMap Entry API or just push?
            // "entry" might lock.
            // Since we are single-threaded here (in async task but sequential logic), we can batch?
            // But state.user_groups is DashMap.
            // Let's just push.
            // Since multiple groups might include same user, and we process groups sequentially,
            // but we might want to batch this to avoid lock contention if we parallelize.
            // Here we are sequential.
            state.user_groups.entry(uid).or_default().push(gid);
        }

        current_slot_idx = end;
    }

    info!("Generated {} groups.", gid);

    // 3. Friends Generation
    info!("Generating friendships...");

    // Algorithm:
    // Each user wants F friends.
    // Sum of all F must be even (Handshaking lemma).
    // Construct a "stub" list again: [User, User, User...] where User appears F times.
    // Shuffle and pair them up.
    // If pair is (A, A) or (A, B) already exists -> retry or ignore.

    let mut friend_slots: Vec<u64> = Vec::new();
    for i in 0..config.user_count {
        let count = (dist.friend_count.sample(&mut rng).round() as usize).max(0);
        for _ in 0..count {
            friend_slots.push(i as u64);
        }
    }

    if friend_slots.len() % 2 != 0 {
        friend_slots.pop(); // Make even
    }

    friend_slots.shuffle(&mut rng);

    let mut pairs = Vec::with_capacity(friend_slots.len() / 2);
    for chunk in friend_slots.chunks(2) {
        if chunk.len() == 2 {
            let u1 = chunk[0];
            let u2 = chunk[1];
            if u1 != u2 {
                pairs.push((u1, u2));
            }
        }
    }

    // Add to state with Affinity
    for (u1, u2) in pairs {
        // Sample affinity
        // "Mutuality" is improved here because we generate ONE affinity for the pair.
        let affinity = dist.friend_affinity.sample(&mut rng).clamp(0.0, 1.0);

        // Check if exists?
        // DashMap operations.

        // Insert for U1
        state.friends.entry(u1).or_default().push((u2, affinity));
        // Insert for U2
        state.friends.entry(u2).or_default().push((u1, affinity));
    }

    info!("Friendship generation complete.");

    Ok(())
}
