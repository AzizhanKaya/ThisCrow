use crate::id::id;
use crate::message::snowflake::snowflake_id;
use crate::message::{Ack, Message};
use crate::state::group::Permissions;
use crate::{State, db};
use anyhow::Result;
use serde::Serialize;
use sqlx::PgPool;

#[derive(Serialize, sqlx::FromRow)]
pub struct Reaction {
    pub user_id: id,
    pub reaction: String,
}

pub async fn insert(
    pool: &PgPool,
    message_id: snowflake_id,
    user_id: id,
    reaction: char,
) -> Result<bool> {
    let row = sqlx::query!(
        "INSERT INTO reactions (message_id, user_id, reaction) VALUES ($1, $2, $3) \
         ON CONFLICT (message_id, user_id, reaction) DO NOTHING RETURNING 1 AS inserted",
        *message_id as i64,
        *user_id,
        reaction.to_string(),
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some())
}

pub async fn delete(
    pool: &PgPool,
    message_id: snowflake_id,
    user_id: id,
    reaction: char,
) -> Result<()> {
    sqlx::query!(
        "DELETE FROM reactions WHERE message_id = $1 AND user_id = $2 AND reaction = $3",
        *message_id as i64,
        *user_id,
        reaction.to_string(),
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list(pool: &PgPool, message_id: snowflake_id) -> Result<Vec<Reaction>> {
    let rows = sqlx::query_as!(
        Reaction,
        r#"SELECT user_id AS "user_id: id", reaction FROM reactions
           WHERE message_id = $1 ORDER BY created_at"#,
        *message_id as i64,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn delete_for_message(pool: &PgPool, message_id: snowflake_id) -> Result<()> {
    sqlx::query!(
        "DELETE FROM reactions WHERE message_id = $1",
        *message_id as i64,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn handle_reaction(
    state: &State,
    event_id: snowflake_id,
    user_id: id,
    target_message_id: snowflake_id,
    reaction: char,
    add: bool,
) -> Result<()> {
    let mut stored = state.messages.get(target_message_id).await?;

    if let Some(gid) = stored.group_id {
        let group = state
            .groups
            .get(&gid)
            .ok_or_else(|| anyhow::anyhow!("Group not found"))?;

        if !group
            .compute_permissions(user_id, Some(stored.to))
            .contains(Permissions::VIEW_MESSAGES)
        {
            anyhow::bail!("You don't have permission to react to this message");
        }
    } else if user_id != stored.from && user_id != stored.to {
        anyhow::bail!("You don't have permission to react to this message");
    }

    if add {
        let inserted =
            db::reaction::insert(&state.pool, target_message_id, user_id, reaction).await?;
        if inserted && stored.reacted != Some(true) {
            stored.reacted = Some(true);
            state.messages.overwrite(stored.clone()).await?;
        }
    } else {
        db::reaction::delete(&state.pool, target_message_id, user_id, reaction).await?;
    }

    let ack_data = if add {
        Ack::Reacted {
            message: target_message_id,
            reaction,
        }
    } else {
        Ack::RemovedReaction {
            message: target_message_id,
            reaction,
        }
    };

    let ack = Message {
        id: event_id,
        from: user_id,
        to: stored.to,
        data: ack_data,
        ..Message::default()
    };

    if let Some(gid) = stored.group_id {
        if let Some(group) = state.groups.get(&gid) {
            group.notify(ack, state);
        }
    } else {
        if let Some(u) = state.users.get(&stored.from) {
            u.send_message(ack.clone());
        }
        if stored.to != stored.from {
            if let Some(u) = state.users.get(&stored.to) {
                u.send_message(ack);
            }
        }
    }

    Ok(())
}
