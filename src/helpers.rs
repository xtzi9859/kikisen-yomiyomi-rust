use crate::db;
use crate::types::{Context, DEFAULT_PREFIX, Data, Error, colors, PersistedVoiceEntry};
use std::path::Path;
use poise::serenity_prelude as serenity;
use sea_orm::{ActiveModelTrait, ColumnTrait, Condition,EntityTrait, QueryFilter};
pub use crate::pager::Pager;

const RESTART_STATE_PATH: &str = "./voice_state.json";

pub fn save_voice_state(entries: &[PersistedVoiceEntry]) -> Result<(), Error> {
    let json = serde_json::to_string_pretty(entries)?;
    std::fs::write(RESTART_STATE_PATH, json)?;
    tracing::info!("save voice states for restart");
    Ok(())
}

pub fn load_and_clear_restart_state() -> Option<Vec<PersistedVoiceEntry>> {
    if !Path::new(RESTART_STATE_PATH).exists() {
        return None;
    }

    let result = std::fs::read_to_string(RESTART_STATE_PATH)
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<PersistedVoiceEntry>>(&s).ok());

    let _ = std::fs::remove_file(RESTART_STATE_PATH);

    result
}

pub async fn get_guild_settings(
    data: &Data,
    guild_id: serenity::GuildId,
) -> db::guild_settings::Model {
    db::guild_settings::Entity::find()
        .filter(db::guild_settings::Column::GuildId.eq(guild_id.get() as i64))
        .one(&data.db)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| db::guild_settings::Model::default_for_guild(guild_id.get() as i64))
}

pub async fn upsert_guild_setting<F>(
    data: &Data,
    guild_id: serenity::GuildId,
    update_fn: F,
) -> Result<(), Error>
where
    F: FnOnce(&mut db::guild_settings::ActiveModel),
{
    let existing = db::guild_settings::Entity::find()
        .filter(db::guild_settings::Column::GuildId.eq(guild_id.get() as i64))
        .one(&data.db)
        .await?;

    if let Some(model) = existing {
        let mut active: db::guild_settings::ActiveModel = model.into();
        update_fn(&mut active);
        active.update(&data.db).await?
    } else {
        let mut active: db::guild_settings::ActiveModel =
            db::guild_settings::Model::default_for_guild(guild_id.get() as i64).into();
        update_fn(&mut active);
        active.insert(&data.db).await?
    };

    Ok(())
}

pub async fn check_admin_permission(ctx: &Context<'_>) -> Result<bool, Error> {
    let guild_id = ctx.guild_id().ok_or("このコマンドはサーバー内でのみ実行できます。")?.get() as i64;
    let user_id = ctx.author().id.get() as i64;
    let mut role_ids = Vec::new();

    if let Some(member) = ctx.author_member().await {
        #[allow(deprecated)]
        let permissions = member.permissions(ctx.cache()).unwrap_or_default();

        if permissions.administrator() {
            return Ok(true);
        }

        role_ids = member.roles.iter().map(|role_id| role_id.get() as i64).collect();
    }

    let db = &ctx.data().db;

    let mut permission_conditions = Condition::any()
        .add(
            Condition::all()
                .add(db::server_manager::Column::ManagerId.eq(user_id))
                .add(db::server_manager::Column::IsRole.eq(false)),
        );

    if !role_ids.is_empty() {
        permission_conditions = permission_conditions.add(
            Condition::all()
                .add(db::server_manager::Column::ManagerId.is_in(role_ids))
                .add(db::server_manager::Column::IsRole.eq(true)),
        );
    }

    let count = db::server_manager::Entity::find()
        .filter(db::server_manager::Column::GuildId.eq(guild_id))
        .filter(permission_conditions)
        .one(db)
        .await?;

    Ok(count.is_some())
}

pub async fn reply_no_permission(ctx: &Context<'_>) -> Result<(), Error> {
    ctx.send(
        poise::CreateReply::default().embed(
            serenity::CreateEmbed::new()
                .description("このコマンドを使用する権限がありません。")
                .color(colors::ERROR),
        ),
    )
    .await?;

    Ok(())
}

pub fn get_command_prefix<'a>(
    ctx: poise::PartialContext<'a, Data, Error>,
) -> poise::BoxFuture<'a, Result<Option<String>, Error>> {
    Box::pin(async move {
        let prefix = match ctx.guild_id {
            Some(gid) => get_guild_settings(ctx.data, gid).await.command_prefix,
            None => DEFAULT_PREFIX.to_string(),
        };
        Ok(Some(prefix))
    })
}

/// botを除外してVCの現在員を数える
pub fn count_members_in_vc(
    ctx: &serenity::Context,
    guild_id: serenity::GuildId,
    voice_channel_id: serenity::ChannelId,
) -> usize {
    ctx.cache
    .guild(guild_id)
    .map(|g| {
        g.voice_states
            .values()
            .filter(|vs| vs.channel_id == Some(voice_channel_id))
            .filter(|vs| {
                !g.members
                    .get(&vs.user_id)
                    .map(|m| m.user.bot)
                    .unwrap_or(false)
            })
            .count()
    })
    .unwrap_or(0)
}
