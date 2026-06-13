use crate::db;
use crate::helpers::get_guild_settings;
use crate::tts::{SPOILER_REGEX, apply_kanalizer, format_message, play_voicevox, sanitize_text};
use crate::types::{Data, Error};
use poise::serenity_prelude as serenity;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

pub async fn on_message(
    ctx: &serenity::Context,
    new_message: &serenity::Message,
    data: &Data,
) -> Result<(), Error> {
    let Some(guild_id) = new_message.guild_id else {
        return Ok(());
    };

    let is_target = {
        let map = data.voice_to_text_map.read().await;
        map.values()
            .any(|info| info.text_channels.contains(&new_message.channel_id))
    };

    if !is_target {
        return Ok(());
    }

    if new_message.author.bot {
        let is_whitelisted = db::bot_whitelist::Entity::find()
            .filter(db::bot_whitelist::Column::GuildId.eq(guild_id.get() as i64))
            .filter(db::bot_whitelist::Column::BotId.eq(new_message.author.id.get() as i64))
            .one(&data.db)
            .await
            .ok()
            .flatten()
            .is_some();
        if !is_whitelisted {
            return Ok(());
        }
    }

    let guild_settings = get_guild_settings(data, guild_id).await;

    if !guild_settings.read_silent {
        let is_silent = new_message
            .flags
            .map(|f| f.contains(serenity::MessageFlags::SUPPRESS_NOTIFICATIONS))
            .unwrap_or(false);
        if is_silent {
            return Ok(());
        }
    }

    if !guild_settings.read_non_vc_user {
        let is_in_vc = ctx
            .cache
            .guild(guild_id)
            .map(|g| g.voice_states.contains_key(&new_message.author.id))
            .unwrap_or(false);
        if !is_in_vc {
            return Ok(());
        }
    }

    if !guild_settings.read_server_muted {
        let is_server_muted = ctx
            .cache
            .guild(guild_id)
            .and_then(|g| g.voice_states.get(&new_message.author.id).map(|vs| vs.mute))
            .unwrap_or(false);
        if is_server_muted {
            return Ok(());
        }
    }

    if new_message.content == "s" {
        let manager = songbird::get(ctx)
            .await
            .expect("failed to initialize songbird");

        if let Some(call_lock) = manager.get(guild_id) {
            let call = call_lock.lock().await;
            let queue = call.queue();

            if queue.current().is_some() {
                let _ = queue.skip();

                let reaction = serenity::ReactionType::Unicode("⏭️".to_string());
                if let Err(why) = new_message.react(&ctx.http, reaction).await {
                    tracing::error!(?why, "failed to add reaction");
                }
            }
        }
        return Ok(());
    }

    let mut text_to_read = format_message(new_message, ctx, guild_settings.reply_prefix_type);

    if guild_settings.read_spoiler {
        text_to_read = SPOILER_REGEX.replace_all(&text_to_read, "$1").into_owned();
    }

    text_to_read = sanitize_text(&text_to_read);

    text_to_read = apply_kanalizer(&text_to_read, &data.kanalizer);

    if guild_settings.read_embed {
        text_to_read = {
            let embed_text = new_message
                .embeds
                .iter()
                .map(embed_to_text)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            if !embed_text.is_empty() {
                if text_to_read.is_empty() {
                    embed_text
                } else {
                    format!("{} {}", text_to_read, embed_text)
                }
            } else {
                text_to_read
            }
        };
    }

    if guild_settings.read_username && !text_to_read.is_empty() {
        let display_name = ctx
            .cache
            .guild(guild_id)
            .and_then(|g| {
                g.members
                    .get(&new_message.author.id)
                    .map(|m| m.display_name().to_string())
            })
            .unwrap_or_else(|| new_message.author.name.clone());

        text_to_read = format!("{}のメッセージ {}", display_name, text_to_read);
    }

    if !text_to_read.is_empty() {
        play_voicevox(
            ctx,
            guild_id,
            &text_to_read,
            data,
            Some(new_message.author.id),
        )
        .await?;
    }

    Ok(())
}

fn embed_to_text(embed: &serenity::Embed) -> String {
    let mut parts: Vec<String> = Vec::new();

    if let Some(author) = &embed.author {
        let text = sanitize_text(&author.name);
        if !text.is_empty() {
            parts.push(text);
        }
    }

    if let Some(title) = &embed.title {
        let text = sanitize_text(title);
        if !text.is_empty() {
            parts.push(text);
        }
    }

    for field in &embed.fields {
        let name = sanitize_text(&field.name);
        let value = sanitize_text(&field.value);
        match (name.is_empty(), value.is_empty()) {
            (false, false) => parts.push(format!("{} {}", name, value)),
            (false, true) => parts.push(name),
            (true, false) => parts.push(value),
            (true, true) => {}
        }
    }

    if let Some(footer) = &embed.footer {
        let text = sanitize_text(&footer.text);
        if !text.is_empty() {
            parts.push(text);
        }
    }

    if let Some(ts) = &embed.timestamp {
        parts.push(format!("{}", ts.timestamp()))
    }

    parts.join(" ")
}
