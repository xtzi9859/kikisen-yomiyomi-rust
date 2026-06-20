use crate::types::{Context, Error, colors};
use poise::serenity_prelude as serenity;
use poise::serenity_prelude::Mentionable;

#[poise::command(slash_command, subcommands("add", "remove"))]
pub async fn tc(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// テキストチャンネルを読み上げ対象に一時的に加える
#[poise::command(slash_command)]
pub async fn add(
    ctx: Context<'_>,
    #[channel_types("Text")] channel: Option<serenity::GuildChannel>,
) -> Result<(), Error> {
    let _ = ctx.guild_id().ok_or("このコマンドはサーバー内でのみ実行できます。");

    let bot_vc_channel_id = {
        let bot_id = ctx.cache().current_user().id;
        ctx.guild()
            .and_then(|g| g.voice_states.get(&bot_id).and_then(|vs| vs.channel_id))
    };

    let Some(vc_channel_id) = bot_vc_channel_id else {
        ctx.send(
            poise::CreateReply::default().embed(
                serenity::CreateEmbed::new()
                    .description("botがVCに参加していません。")
                    .color(colors::WARN),
            ),
        )
        .await?;
        return Ok(());
    };

    let (channel_id, channel_mention) = match channel {
        Some(ref ch) => (ch.id, ch.mention().to_string()),
        None => {
            let id = ctx.channel_id();
            (id, id.mention().to_string())
        }
    };

    let mut map = ctx.data().voice_to_text_map.write().await;
    let Some(info) = map.get_mut(&vc_channel_id) else {
        ctx.send(
            poise::CreateReply::default().embed(
                serenity::CreateEmbed::new()
                    .description("botの接続情報が見付かりませんでした。")
                    .color(colors::ERROR),
            ),
        )
        .await?;
        return Ok(());
    };

    if info.text_channels.contains(&channel_id) {
        ctx.send(
            poise::CreateReply::default().embed(
                serenity::CreateEmbed::new()
                    .description(format!("{}は既に読み上げ対象です。", channel_mention,))
                    .color(colors::WARN),
            ),
        )
        .await?;
        return Ok(());
    }

    info.text_channels.insert(channel_id);

    let reading_targets = info
        .text_channels
        .iter()
        .map(|id| format!("<#{}>", id))
        .collect::<Vec<_>>()
        .join(" ");

    ctx.send(
        poise::CreateReply::default().embed(
            serenity::CreateEmbed::new()
                .title(format!("{}を読み上げ対象に追加しました。", channel_mention,))
                .field(
                    "通知送信チャンネル",
                    info.command_channel.mention().to_string(),
                    false,
                )
                .field("読み上げ対象", reading_targets, false)
                .color(colors::SUCCEED),
        ),
    )
    .await?;

    Ok(())
}

/// テキストチャンネルを読み上げ対象から一時的に除く
#[poise::command(slash_command)]
pub async fn remove(
    ctx: Context<'_>,
    #[channel_types("Text")] channel: Option<serenity::GuildChannel>,
) -> Result<(), Error> {
    let _ = ctx.guild_id().ok_or("このコマンドはサーバー内でのみ実行できます。");

    let bot_vc_channel_id = {
        let bot_id = ctx.cache().current_user().id;
        ctx.guild()
            .and_then(|g| g.voice_states.get(&bot_id).and_then(|vs| vs.channel_id))
    };

    let Some(vc_channel_id) = bot_vc_channel_id else {
        ctx.send(
            poise::CreateReply::default().embed(
                serenity::CreateEmbed::new()
                    .description("botがVCに参加していません。")
                    .color(colors::WARN),
            ),
        )
        .await?;
        return Ok(());
    };

    let (channel_id, channel_mention) = match channel {
        Some(ref ch) => (ch.id, ch.mention().to_string()),
        None => {
            let id = ctx.channel_id();
            (id, id.mention().to_string())
        }
    };

    let mut map = ctx.data().voice_to_text_map.write().await;
    let Some(info) = map.get_mut(&vc_channel_id) else {
        ctx.send(
            poise::CreateReply::default().embed(
                serenity::CreateEmbed::new()
                    .description("botの接続情報が見付かりませんでした。")
                    .color(colors::WARN),
            ),
        )
        .await?;
        return Ok(());
    };

    if !info.text_channels.contains(&channel_id) {
        ctx.send(
            poise::CreateReply::default().embed(
                serenity::CreateEmbed::new()
                    .description(format!("{}は読み上げ対象ではありません。", channel_mention,))
                    .color(colors::WARN),
            ),
        )
        .await?;
        return Ok(());
    }

    info.text_channels.remove(&channel_id);

    let reading_targets = info
        .text_channels
        .iter()
        .map(|id| format!("<#{}>", id))
        .collect::<Vec<_>>()
        .join(" ");

    ctx.send(
        poise::CreateReply::default().embed(
            serenity::CreateEmbed::new()
                .title(format!(
                    "{}を読み上げ対象から削除しました。",
                    channel_mention,
                ))
                .field(
                    "通知送信チャンネル",
                    info.command_channel.mention().to_string(),
                    false,
                )
                .field("読み上げ対象", reading_targets, false)
                .color(colors::SUCCEED),
        ),
    )
    .await?;

    Ok(())
}
