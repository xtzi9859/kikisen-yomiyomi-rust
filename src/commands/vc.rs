use crate::tts::play_voicevox;
use crate::types::{Context, Error, VoiceContextInfo, colors};
use poise::serenity_prelude as serenity;

#[poise::command(slash_command, subcommands("connect", "disconnect"))]
pub async fn vc(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

#[poise::command(slash_command)]
pub async fn disconnect(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?;

    let user_voice_state = ctx
        .guild()
        .and_then(|g| g.voice_states.get(&ctx.author().id).cloned());
    let user_voice_channel_id = match user_voice_state.and_then(|v| v.channel_id) {
        Some(id) => id,
        None => {
            ctx.send(
                poise::CreateReply::default().embed(
                    serenity::CreateEmbed::new()
                        .description("botと同じVCに参加していないので、使用できません。")
                        .color(colors::WARN),
                ),
            )
            .await?;

            return Ok(());
        }
    };

    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("failed to initialize songbird")
        .clone();

    if let Some(call_lock) = manager.get(guild_id) {
        let current_channel = {
            let call = call_lock.lock().await;
            call.current_channel()
        };

        if current_channel.is_none() {
            ctx.send(
                poise::CreateReply::default().embed(
                    serenity::CreateEmbed::new()
                        .description("botがVCに参加していません。")
                        .color(colors::WARN),
                ),
            )
            .await?;

            return Ok(());
        } else if current_channel.unwrap() != user_voice_channel_id.into() {
            ctx.send(
                poise::CreateReply::default().embed(
                    serenity::CreateEmbed::new()
                        .description("botと同じVCに参加していないので、使用できません。")
                        .color(colors::WARN),
                ),
            )
            .await?;

            return Ok(());
        }

        manager.remove(guild_id).await.ok();
    }

    Ok(())
}

/// botをVCに参加させ、コマンドが実行されたチャンネルを通知送信先と読み上げ対象に設定する
#[poise::command(slash_command)]
pub async fn connect(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?;

    let user_voice_state = ctx
        .guild()
        .and_then(|g| g.voice_states.get(&ctx.author().id).cloned());
    let connect_channel_id = match user_voice_state.and_then(|v| v.channel_id) {
        Some(id) => id,
        None => {
            ctx.send(
                poise::CreateReply::default().embed(
                    serenity::CreateEmbed::new()
                        .description("このコマンドを使用するには先にVCに参加してください。")
                        .color(colors::WARN),
                ),
            )
            .await?;

            return Ok(());
        }
    };

    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("failed to initialize songbird")
        .clone();

    if let Some(call_lock) = manager.get(guild_id) {
        let current_channel = {
            let call = call_lock.lock().await;
            call.current_channel()
        };

        if current_channel.is_some() {
            let ctx_id = ctx.id();
            let move_button_id = format!("move{}", ctx_id);

            let reply = ctx
                .send(
                    poise::CreateReply::default()
                        .embed(
                            serenity::CreateEmbed::new()
                                .description(
                                    "別のボイスチャンネルに既に参加しています。移動しますか？",
                                )
                                .color(colors::WARN),
                        )
                        .components(vec![serenity::CreateActionRow::Buttons(vec![
                            serenity::CreateButton::new(&move_button_id)
                                .label("移動する")
                                .style(serenity::ButtonStyle::Primary),
                        ])]),
                )
                .await?;

            let interaction = reply
                .message()
                .await?
                .await_component_interaction(ctx.serenity_context())
                .author_id(ctx.author().id)
                .timeout(std::time::Duration::from_secs(30))
                .filter(move |m| m.data.custom_id == move_button_id)
                .await;

            if let Some(mci) = interaction {
                join_vc(ctx, guild_id, connect_channel_id).await?;

                mci.create_response(
                    &ctx.serenity_context(),
                    serenity::CreateInteractionResponse::UpdateMessage(
                        (serenity::CreateInteractionResponseMessage::new().embed(
                            serenity::CreateEmbed::new()
                                .description("ボイスチャンネルを移動しました。")
                                .color(colors::SUCCEED),
                        ))
                        .components(vec![]),
                    ),
                )
                .await?;
            } else {
                reply
                    .edit(
                        ctx,
                        poise::CreateReply::default()
                            .embed(
                                serenity::CreateEmbed::new()
                                    .description("タイムアウトしました。")
                                    .color(colors::INFO),
                            )
                            .components(vec![]),
                    )
                    .await?;
            }
            return Ok(());
        }
    }

    join_vc(ctx, guild_id, connect_channel_id).await?;
    let embed = serenity::CreateEmbed::new()
        .title(format!("<#{}>に接続しました。", connect_channel_id.get()))
        .color(colors::SUCCEED)
        .field(
            "通知送信先",
            format!("<#{}>", ctx.channel_id().get()),
            false,
        )
        .field(
            "読み上げ対象",
            format!(
                "<#{}> <#{}>",
                ctx.channel_id().get(),
                connect_channel_id.get()
            ),
            false,
        );

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}

pub async fn join_vc(
    ctx: Context<'_>,
    guild_id: serenity::GuildId,
    channel_id: serenity::ChannelId,
) -> Result<(), Error> {
    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("failed to initialize songbird");

    if let Some(call_lock) = manager.get(guild_id) {
        let call = call_lock.lock().await;
        if let Some(old_channel) = call.current_channel() {
            let old_channel_id = serenity::ChannelId::new(old_channel.0.get());
            let mut map = ctx.data().voice_to_text_map.write().await;
            map.remove(&old_channel_id);
        }
    }

    let _handler = manager.join(guild_id, channel_id).await;

    let mut map = ctx.data().voice_to_text_map.write().await;
    map.insert(
        channel_id,
        VoiceContextInfo {
            command_channel: ctx.channel_id(),
            text_channels: std::collections::HashSet::from([ctx.channel_id(), channel_id]),
        },
    );
    ctx.data()
        .last_clear_executed
        .write()
        .await
        .insert(guild_id, std::time::Instant::now());

    let bot_name = ctx.cache().current_user().name.clone();
    play_voicevox(
        ctx.serenity_context(),
        guild_id,
        &[format!("{}が参加しました", bot_name)],
        ctx.data(),
        None,
    )
    .await?;

    Ok(())
}
