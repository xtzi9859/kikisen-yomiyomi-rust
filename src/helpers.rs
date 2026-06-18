use crate::db;
use crate::types::{Context, DEFAULT_PREFIX, Data, Error, colors, PersistedVoiceEntry};
use std::path::Path;
use poise::serenity_prelude as serenity;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter};

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
    {
        let cache = data.guild_settings_cache.read().await;
        if let Some(settings) = cache.get(&guild_id) {
            return settings.clone();
        }
    }

    let settings = db::guild_settings::Entity::find()
        .filter(db::guild_settings::Column::GuildId.eq(guild_id.get() as i64))
        .one(&data.db)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| db::guild_settings::Model::default_for_guild(guild_id.get() as i64));

    data.guild_settings_cache
        .write()
        .await
        .insert(guild_id, settings.clone());

    settings
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

    let updated = if let Some(model) = existing {
        let mut active: db::guild_settings::ActiveModel = model.into();
        update_fn(&mut active);
        active.update(&data.db).await?
    } else {
        let mut active: db::guild_settings::ActiveModel =
            db::guild_settings::Model::default_for_guild(guild_id.get() as i64).into();
        update_fn(&mut active);
        active.insert(&data.db).await?
    };

    data.guild_settings_cache
        .write()
        .await
        .insert(guild_id, updated);
    Ok(())
}

pub fn permission_from_str(s: &str) -> serenity::Permissions {
    match s {
        "manage_messages" => serenity::Permissions::MANAGE_MESSAGES,
        "manage_channels" => serenity::Permissions::MANAGE_CHANNELS,
        "moderate_members" => serenity::Permissions::MODERATE_MEMBERS,
        "manage_guild" => serenity::Permissions::MANAGE_GUILD,
        "administrator" => serenity::Permissions::ADMINISTRATOR,
        _ => serenity::Permissions::ADMINISTRATOR,
    }
}

pub async fn check_admin_permission(ctx: &Context<'_>) -> Result<bool, Error> {
    let guild_id = ctx.guild_id().ok_or("サーバー内でのみ実行可能です。")?;
    let settings = get_guild_settings(ctx.data(), guild_id).await;
    let required = permission_from_str(&settings.admin_permission);

    let Some(member) = ctx.author_member().await else {
        return Ok(false);
    };

    let permissions = ctx
        .guild()
        .map(|g| g.member_permissions(&*member))
        .unwrap_or(serenity::Permissions::empty());

    Ok(permissions.contains(required))
}

pub async fn reply_no_permission(ctx: &Context<'_>) -> Result<(), Error> {
    ctx.send(
        poise::CreateReply::default().ephemeral(true).embed(
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
