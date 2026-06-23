use crate::db;
use crate::helpers::{get_guild_settings, count_members_in_vc};
use crate::tts::{apply_kanalizer, play_voicevox};
use crate::types::{Data, Error, VoiceContextInfo, colors};
use poise::serenity_prelude as serenity;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use std::collections::HashSet;
use std::time::Duration;

pub async fn on_ready(
    ctx: &serenity::Context,
    data_about_bot: &serenity::Ready,
    data: &Data,
) -> Result<(), Error> {
    tracing::info!("ready, logged in as {}", data_about_bot.user.name);

    if let Some(entries) = crate::helpers::load_and_clear_restart_state() {
        let manager = songbird::get(ctx)
            .await
            .expect("failed to initialize songbird");

        for entry in entries {
            let member_count = count_members_in_vc(ctx, entry.guild_id, entry.voice_channel_id);
            let notify_channel_id = entry.context.command_channel;

            if member_count == 0 {
                continue;
            }

            match manager.join(entry.guild_id, entry.voice_channel_id).await {
                Ok(_) => {
                    let reading_list = entry
                        .context
                        .text_channels
                        .iter()
                        .map(|id| format!("<#{}>", id))
                        .collect::<Vec<_>>()
                        .join(" ");

                    let _ = notify_channel_id
                        .send_message(
                            &ctx.http,
                            serenity::CreateMessage::new().embed(
                                serenity::CreateEmbed::new()
                                    .title("自動接続")
                                    .description(format!(
                                        "<#{}>に再接続しました。",
                                        entry.voice_channel_id
                                    ))
                                    .field(
                                        "通知送信チャンネル",
                                        format!("<#{}>", notify_channel_id),
                                        false,
                                    )
                                    .field("読み上げ対象", reading_list, false)
                                    .color(colors::SUCCEED),
                            ),
                        )
                        .await;

                    data.voice_to_text_map
                        .write()
                        .await
                        .insert(entry.voice_channel_id, entry.context);

                    let mut bot_name = ctx.cache.current_user().name.clone();
                    let current_user_id = ctx.cache.current_user().id;
                    if let Ok(member) = entry.guild_id.member(&ctx.http, current_user_id).await {
                        if let Some(nick) = member.nick {
                            bot_name = nick;
                        }
                    }

                    let _ = play_voicevox(
                        ctx,
                        entry.guild_id,
                        &format!("{}が接続しました", bot_name),
                        data,
                        Some(current_user_id),
                    )
                    .await;
                }
                Err(e) => {
                    let _ = notify_channel_id
                        .send_message(
                            &ctx.http,
                            serenity::CreateMessage::new().embed(
                                serenity::CreateEmbed::new()
                                    .title("自動接続失敗")
                                    .description(format!(
                                        "bot再起動後の自動での再接続に失敗しました。: {} \n/vc connectコマンドを使用してbotをVCに参加させてください。",
                                        e
                                    ))
                                    .color(colors::ERROR),
                            ),
                        )
                        .await;
                    tracing::error!(
                        error = ?e,
                        guild_id = %entry.guild_id,
                        channel_id = %entry.voice_channel_id,
                        "failed to reconnect"
                    )
                }
            }
        }
    }

    if let Ok(channel_id_str) = std::env::var("NOTIFY_CHANNEL_ID") {
        if let Ok(channel_id_u64) = channel_id_str.parse::<u64>() {
            let channel_id = serenity::ChannelId::new(channel_id_u64);
            let _ = channel_id
                .send_message(
                    &ctx.http,
                    serenity::CreateMessage::new()
                        .content(format!("起動完了: {}", &data_about_bot.user.name)),
                )
                .await;
        } else {
            tracing::error!("NOTIFY_CHANNEL_ID not a valid u64");
        }
    }

    Ok(())
}

pub async fn on_voice_state_update(
    ctx: &serenity::Context,
    old: &Option<serenity::VoiceState>,
    new: &serenity::VoiceState,
    data: &Data,
) -> Result<(), Error> {
    if new.user_id == ctx.cache.current_user().id {
        if new.channel_id.is_none() {
            if let Some(guild_id) = new.guild_id {
                if let Some(old_vc) = old.as_ref().and_then(|v| v.channel_id) {
                    let manager = songbird::get(ctx)
                        .await
                        .expect("failed to initialize songbird");
                    manager.remove(guild_id).await.ok();
                    data.voice_to_text_map.write().await.remove(&old_vc);
                }
            }
        }
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

    if manager.get(guild_id).is_none() {
        if let Some(new_ch) = new_channel_id {
            let auto_connect_config = db::auto_connections::Entity::find_by_id(new_ch.get() as i64)
                .one(&data.db)
                .await
                .ok()
                .flatten();

            if let Some(config) = auto_connect_config {
                let member_count = {
                    ctx.cache
                        .guild(guild_id)
                        .map(|g| {
                            g.voice_states
                                .values()
                                .filter(|vs| vs.channel_id == Some(new_ch))
                                .filter(|vs| {
                                    !g.members
                                        .get(&vs.user_id)
                                        .map(|m| m.user.bot)
                                        .unwrap_or(false)
                                })
                                .count()
                        })
                        .unwrap_or(0)
                };

                if member_count == 1 {
                    let notify_id = serenity::ChannelId::new(config.notify_channel_id as u64);
                    let reading_channels: HashSet<serenity::ChannelId> =
                        db::reading_targets::Entity::find()
                            .filter(
                                db::reading_targets::Column::VoiceChannelId
                                    .eq(config.voice_channel_id),
                            )
                            .all(&data.db)
                            .await
                            .unwrap_or_default()
                            .into_iter()
                            .map(|r| serenity::ChannelId::new(r.text_channel_id as u64))
                            .collect();

                    let _ = manager.join(guild_id, new_ch).await;

                    let reading_list = reading_channels
                        .iter()
                        .map(|id| format!("<#{}>", id))
                        .collect::<Vec<_>>()
                        .join(" ");

                    data.voice_to_text_map.write().await.insert(
                        new_ch,
                        VoiceContextInfo {
                            command_channel: notify_id,
                            text_channels: reading_channels,
                        },
                    );

                    let _ = notify_id
                        .send_message(
                            &ctx.http,
                            serenity::CreateMessage::new().embed(
                                serenity::CreateEmbed::new()
                                    .title("自動接続")
                                    .description(format!("<#{}>に接続しました", new_ch))
                                    .field("通知送信チャンネル", format!("<#{}>", notify_id), false)
                                    .field("読み上げ対象", reading_list, false)
                                    .color(colors::SUCCEED),
                            ),
                        )
                        .await;

                    let mut bot_name = ctx.cache.current_user().name.clone();

                    if let Some(guild_id) = new.guild_id {
                        let current_user_id = ctx.cache.current_user().id;

                        if let Ok(member) = guild_id.member(&ctx.http, current_user_id).await {
                            if let Some(nick) = member.nick {
                                bot_name = nick;
                            }
                        }
                    }

                    let _ = play_voicevox(
                        ctx,
                        guild_id,
                        &format!("{}が参加しました", bot_name),
                        data,
                        None,
                    )
                    .await;
                }
            }
        }

        return Ok(());
    }

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

    let guild_name = ctx
        .cache
        .guild(guild_id)
        .map(|g| g.name.clone())
        .unwrap_or_else(|| "unknown guild".to_string());

    let member_name = member.display_name();
    let user_id = member.user.id.get();
    let mut should_check_auto_disconnect = false;

    let text_to_dm = match (old_channel_id, new_channel_id) {
        (None, Some(new_id)) => Some(format!(
            "{} (@{}) joined {} in {}",
            member_name,
            user_id,
            get_channel_name(new_id),
            guild_name
        )),
        (Some(old_id), None) => Some(format!(
            "{} (@{}) left {} in {}",
            member_name,
            user_id,
            get_channel_name(old_id),
            guild_name
        )),
        _ => None,
    };

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
                if new_id.get() == bot_channel_id.get() {
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

    if let Some(text) = text_to_dm {
        let user =
            serenity::UserId::new(std::env::var("DEVELOPPER_ID").expect("").parse().expect(""));
        user.dm(&ctx.http, serenity::CreateMessage::new().content(&text))
            .await?;
    }

    if let Some(text) = text_to_read {
        play_voicevox(
            ctx,
            guild_id,
            &apply_kanalizer(&text, &data.kanalizer),
            data,
            Some(new.user_id),
        )
        .await?;
    }

    if new_channel_id.map(|id| id.get()) == Some(bot_channel_id.get()) {
        cancel_auto_disconnect(data, serenity::ChannelId::new(bot_channel_id.into())).await;
    }

    if !should_check_auto_disconnect {
        return Ok(());
    }

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
                schedule_auto_disconnect(ctx, data, guild_id, channel_id).await;
            }
        }
    }

    Ok(())
}

async fn cancel_auto_disconnect(data: &Data, channel_id: serenity::ChannelId) {
    if let Some(handle) = data.pending_disconnects.write().await.remove(&channel_id) {
        handle.abort();
        tracing::info!(channel_id = %channel_id, "auto disconnect cancelled");
    }
}

async fn schedule_auto_disconnect(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
    channel_id: serenity::ChannelId,
) {
    {
        let pending = data.pending_disconnects.read().await;
        if pending.contains_key(&channel_id) {
            return;
        }
    }

    let ctx_clone = ctx.clone();
    let voice_map = data.voice_to_text_map.clone();
    let pending_map = data.pending_disconnects.clone();

    let handle = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;

        let member_count = ctx_clone
            .cache
            .guild(guild_id)
            .map(|guild| {
                guild
                    .voice_states
                    .values()
                    .filter(|vs| vs.channel_id == Some(channel_id))
                    .filter(|vs| {
                        !guild
                            .members
                            .get(&vs.user_id)
                            .map(|m| m.user.bot)
                            .unwrap_or(false)
                    })
                    .count()
            })
            .unwrap_or(0);

        if member_count == 0 {
            if let Some(manager) = songbird::get(&ctx_clone).await {
                manager.remove(guild_id).await.ok();
            }
            voice_map.write().await.remove(&channel_id);
            tracing::info!(channel_id = %channel_id, "automatically disconnected");
        }

        pending_map.write().await.remove(&channel_id);
    });

    data.pending_disconnects
        .write()
        .await
        .insert(channel_id, handle);
}
