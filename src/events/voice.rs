use crate::types::{Data, Error};
use crate::tts::play_voicevox;
use crate::helpers::get_guild_settings;
use crate::db;
use poise::serenity_prelude as serenity;
use sea_orm::EntityTrait;

pub async fn on_ready(
    _ctx: &serenity::Context,
    data_about_bot: &serenity::Ready,
    data: &Data,
) -> Result<(), Error> {
    let all_settings = db::guild_settings::Entity::find().all(&data.db).await?;

    let mut cache = data.guild_settings_cache.write().await;
    for settings in all_settings {
        let guild_id = serenity::GuildId::new(settings.guild_id as u64);
        cache.insert(guild_id, settings);
    }

    tracing::info!("ready, logged in as {}", data_about_bot.user.name);
    Ok(())
}

pub async fn on_voice_state_update(
    ctx: &serenity::Context,
    old: &Option<serenity::VoiceState>,
    new: &serenity::VoiceState,
    data: &Data,
) -> Result<(), Error> {
    if new.user_id == ctx.cache.current_user().id {
        return Ok(());
    }

    let guild_id = match new.guild_id {
        Some(id) => id,
        None => return Ok(()),
    };
    let member = guild_id.member(&ctx.http, new.user_id).await?;

    let old_channel_id = old.as_ref().and_then(|v| v.channel_id);
    let new_channel_id = new.channel_id;

    let manager = songbird::get(ctx)
        .await
        .expect("failed to initialize songbird");

    let bot_channel_id = if let Some(call_lock) = manager.get(guild_id) {
        let call = call_lock.lock().await;
        call.current_channel().map(|c| c.0)
    } else {
        return Ok(());
    };

    let Some(bot_channel_id) = bot_channel_id else {
        return Ok(());
    };

    let guild_settings = get_guild_settings(&data, guild_id).await;

    let get_channel_name = |chan_id: serenity::ChannelId| -> String {
        ctx.cache
            .guild(guild_id)
            .and_then(|g| g.channels.get(&chan_id).map(|c| c.name.clone()))
            .unwrap_or_else(|| "不明なチャンネル".to_string())
    };

    let member_name = member.display_name();
    let mut should_check_auto_disconnect = false;

    let text_to_read = match (old_channel_id, new_channel_id) {
        (None, Some(new_id)) => {
            if new_id.get() == bot_channel_id.get() {
                guild_settings
                    .read_vc_join
                    .then(|| format!("{}が参加しました", member_name))
            } else {
                guild_settings.read_vc_move.then(|| {
                    format!(
                        "{}が{}に参加しました",
                        member_name,
                        get_channel_name(new_id),
                    )
                })
            }
        }
        (Some(old_id), None) => {
            if old_id.get() == bot_channel_id.get() {
                should_check_auto_disconnect = true;
                guild_settings
                    .read_vc_leave
                    .then(|| format!("{}が退出しました", member_name))
            } else {
                guild_settings.read_vc_move.then(|| {
                    format!(
                        "{}が{}から退出しました",
                        member_name,
                        get_channel_name(old_id),
                    )
                })
            }
        }
        (Some(old_id), Some(new_id)) => {
            if old_id == new_id {
                let old_stream = old.as_ref().and_then(|s| s.self_stream).unwrap_or(false);
                let new_stream = new.self_stream.unwrap_or(false);

                let old_video = old.as_ref().map(|s| s.self_video).unwrap_or(false);
                let new_video = new.self_video;

                if !old_stream && new_stream {
                    guild_settings
                        .read_vc_stream_start
                        .then(|| format!("{}が配信を開始しました", member_name))
                } else if old_stream && !new_stream {
                    guild_settings
                        .read_vc_stream_stop
                        .then(|| format!("{}が配信を終了しました", member_name))
                } else if !old_video && new_video {
                    guild_settings
                        .read_vc_camera_on
                        .then(|| format!("{}がカメラをオンにしました", member_name))
                } else if old_video && !new_video {
                    guild_settings
                        .read_vc_camera_off
                        .then(|| format!("{}がカメラをオフにしました", member_name))
                } else {
                    None
                }
            } else {
                if new_id.get() == bot_channel_id.get() {
                    guild_settings
                        .read_vc_join
                        .then(|| format!("{}が参加しました", member_name))
                } else {
                    should_check_auto_disconnect = true;
                    if guild_settings.read_vc_move {
                        Some(format!(
                            "{}が{}に参加しました",
                            member_name,
                            get_channel_name(new_id)
                        ))
                    } else {
                        guild_settings
                            .read_vc_leave
                            .then(|| format!("{}が退出しました", member_name))
                    }
                }
            }
        }
        _ => None,
    };

    if let Some(text) = text_to_read {
        play_voicevox(ctx, guild_id, &text, data, Some(new.user_id)).await?;
    }

    if !should_check_auto_disconnect {
        return Ok(());
    }

    let voice_to_text_map = data.voice_to_text_map.read().await;
    if let Some(call_lock) = manager.get(guild_id) {
        let call = call_lock.lock().await;
        if let Some(current_channel) = call.current_channel() {
            let channel_id = serenity::ChannelId::new(current_channel.0.get());

            let member_count = {
                ctx.cache
                    .guild(guild_id)
                    .map(|guild| {
                        guild
                            .voice_states
                            .values()
                            .filter(|vs| {
                                vs.channel_id.map(|c| c.get()) == Some(current_channel.0.get())
                            })
                            .filter(|vs| {
                                !guild
                                    .members
                                    .get(&vs.user_id)
                                    .map(|m| m.user.bot)
                                    .unwrap_or(false)
                            })
                            .count()
                    })
                    .unwrap_or(0)
            };

            if member_count == 0 {
                drop(call);
                drop(voice_to_text_map);
                manager.remove(guild_id).await.ok();
                let mut map = data.voice_to_text_map.write().await;
                map.remove(&channel_id);
            }
        }
    }

    Ok(())
}
