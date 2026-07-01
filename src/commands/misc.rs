use crate::types::{Context, Error, PersistedVoiceEntry, colors};
use poise::serenity_prelude as serenity;

/// botを再起動する
#[poise::command(slash_command)]
pub async fn restart(ctx: Context<'_>) -> Result<(), Error> {
    ctx.send(
        poise::CreateReply::default().embed(
            serenity::CreateEmbed::new()
                .color(colors::SUCCEED)
                .description("再起動します…"),
        ),
    )
    .await?;

    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("failed to initialize songbird");

    let map = ctx.data().voice_to_text_map.read().await;
    let mut entries = Vec::new();

    for (guild_id, call_lock) in manager.iter() {
        let call = call_lock.lock().await;
        if let Some(current_channel) = call.current_channel() {
            let channel_id = serenity::ChannelId::new(current_channel.0.get());
            if let Some(context) = map.get(&channel_id) {
                entries.push(PersistedVoiceEntry {
                    guild_id: serenity::GuildId::new(guild_id.0.get()),
                    voice_channel_id: channel_id,
                    context: crate::types::VoiceContextInfo {
                        command_channel: context.command_channel,
                        text_channels: context.text_channels.clone(),
                    },
                });
            }
        }
    }

    let mut notify_channel = std::collections::HashSet::new();
    for entry in &entries {
        if notify_channel.insert(entry.context.command_channel) {
            let _ = entry
                .context
                .command_channel
                .send_message(
                    &ctx.serenity_context().http,
                    serenity::CreateMessage::new().embed(
                        serenity::CreateEmbed::new()
                            .color(colors::SUCCEED)
                            .description("restartコマンドが実行されたのでbotを再起動します。\n再起動後に自動でボイスチャンネルに再接続します。"),
                    ),
                )
                .await;
        }
    }

    drop(map);

    if let Err(e) = crate::helpers::save_voice_state(&entries) {
        tracing::error!(?e, "failed to save restart state");
    }

    tracing::info!("restart command executed; restarting...");

    ctx.framework().shard_manager().shutdown_all().await;
    std::process::exit(0);
}

/// botの招待リンクを表示する
#[poise::command(slash_command)]
pub async fn invite(ctx: Context<'_>) -> Result<(), Error> {
    ctx.send(poise::CreateReply::default().embed(
        serenity::CreateEmbed::new()
            .title("聞き専読み読みくんの招待リンク")
            .description("https://discord.com/oauth2/authorize?client_id=1413693235506839644&permissions=3148800&integration_type=0&scope=bot")
            .color(colors::INFO)
    ))
    .await?;

    Ok(())
}

/// show age of user executed this command or specified.
#[poise::command(slash_command)]
pub async fn age(
    ctx: Context<'_>,
    #[description = "Selected user"] user: Option<serenity::User>,
) -> Result<(), Error> {
    let u = user.as_ref().unwrap_or_else(|| ctx.author());
    let response = format!("{} account was created at {}", u.name, u.created_at());
    ctx.say(response).await?;

    Ok(())
}
