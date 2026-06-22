use crate::db;
use crate::helpers::{check_admin_permission, reply_no_permission};
use crate::types::{Context, Error, colors};
use poise::serenity_prelude as serenity;
use poise::serenity_prelude::Mentionable;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};

#[poise::command(slash_command)]
pub async fn auto_connect(
    ctx: Context<'_>,
    #[channel_types("Voice", "Stage")]
    #[description = "自動接続対象になるボイスチャンネル"]
    channel: serenity::GuildChannel,
) -> Result<(), Error> {
    if !check_admin_permission(&ctx).await? {
        return reply_no_permission(&ctx).await;
    }

    let guild_id = ctx.guild_id().ok_or("このコマンドはサーバー内でのみ実行できます。")?;
    let vc_id = channel.id.get() as i64;
    let ctx_id = ctx.id();
    let serenity_ctx = ctx.serenity_context();
    let author_id = ctx.author().id;
    let channel_mention = channel.mention().to_string();
    let id = |s: &str| format!("{}{}", ctx_id, s);

    let existing = db::auto_connections::Entity::find_by_id(vc_id)
        .one(&ctx.data().db)
        .await?;

    if existing.is_some() {
        let reply = ctx
            .send(
                poise::CreateReply::default()
                    .ephemeral(true)
                    .embed(
                        serenity::CreateEmbed::new()
                            .description(format!(
                                "{}は既に自動接続の対象です。操作を選択してください。",
                                channel_mention
                            ))
                            .color(colors::WARN),
                    )
                    .components(vec![serenity::CreateActionRow::Buttons(vec![
                        serenity::CreateButton::new(id("edit"))
                            .label("編集")
                            .style(serenity::ButtonStyle::Primary),
                        serenity::CreateButton::new(id("delete"))
                            .label("削除")
                            .style(serenity::ButtonStyle::Danger),
                    ])]),
            )
            .await?;

        let msg = reply.message().await?;
        let prefix = format!("{}", ctx_id);

        let Some(press) = msg
            .await_component_interaction(serenity_ctx)
            .author_id(author_id)
            .timeout(std::time::Duration::from_secs(60))
            .filter(move |m| m.data.custom_id.starts_with(&prefix))
            .await
        else {
            let _ = reply
                .edit(
                    ctx,
                    poise::CreateReply::default()
                        .embed(
                            serenity::CreateEmbed::new()
                                .description("タイムアウトしました。")
                                .color(colors::WARN),
                        )
                        .components(vec![]),
                )
                .await;
            return Ok(());
        };

        if press.data.custom_id == id("delete") {
            db::reading_targets::Entity::delete_many()
                .filter(db::reading_targets::Column::VoiceChannelId.eq(vc_id))
                .exec(&ctx.data().db)
                .await?;
            db::auto_connections::Entity::delete_by_id(vc_id)
                .exec(&ctx.data().db)
                .await
                .ok();

            press
                .create_response(
                    serenity_ctx,
                    serenity::CreateInteractionResponse::UpdateMessage(
                        serenity::CreateInteractionResponseMessage::new()
                            .embed(
                                serenity::CreateEmbed::new()
                                    .description(format!(
                                        "{}を自動接続対象から削除しました。",
                                        channel_mention
                                    ))
                                    .color(colors::SUCCEED),
                            )
                            .components(vec![]),
                    ),
                )
                .await?;
        } else {
            press
                .create_response(
                    serenity_ctx,
                    serenity::CreateInteractionResponse::UpdateMessage(
                        serenity::CreateInteractionResponseMessage::new()
                            .embed(
                                serenity::CreateEmbed::new()
                                    .description(format!(
                                        "{}の設定を編集します。設定を入力してください。",
                                        channel_mention,
                                    ))
                                    .color(colors::INFO),
                            )
                            .components(config_ui_components(
                                &id("notify"),
                                &id("reading"),
                                &id("save"),
                                &id("cancel"),
                            )),
                    ),
                )
                .await?;

            config_ui_loop(
                serenity_ctx,
                &msg,
                author_id,
                &ctx.data().db,
                guild_id,
                vc_id,
                &channel_mention,
                ctx_id,
            )
            .await?;
        }
    } else {
        let reply = ctx
            .send(
                poise::CreateReply::default()
                    .ephemeral(true)
                    .embed(
                        serenity::CreateEmbed::new()
                            .description(format!(
                                "{}を自動接続の対象に追加します。設定を入力してください。",
                                channel_mention
                            ))
                            .color(colors::INFO),
                    )
                    .components(config_ui_components(
                        &id("notify"),
                        &id("reading"),
                        &id("save"),
                        &id("cancel"),
                    )),
            )
            .await?;

        let msg = reply.message().await?;
        config_ui_loop(
            serenity_ctx,
            &msg,
            author_id,
            &ctx.data().db,
            guild_id,
            vc_id,
            &channel_mention,
            ctx_id,
        )
        .await?;
    }

    Ok(())
}

fn config_ui_components(
    notify_id: &str,
    reading_id: &str,
    save_id: &str,
    cancel_id: &str,
) -> Vec<serenity::CreateActionRow> {
    vec![
        serenity::CreateActionRow::SelectMenu(
            serenity::CreateSelectMenu::new(
                notify_id,
                serenity::CreateSelectMenuKind::Channel {
                    channel_types: Some(vec![
                        serenity::ChannelType::Text,
                        serenity::ChannelType::Voice,
                    ]),
                    default_channels: None,
                },
            )
            .placeholder("通知送信チャンネルを選択（読み上げ対象には含まれません）")
            .min_values(1)
            .max_values(1),
        ),
        serenity::CreateActionRow::SelectMenu(
            serenity::CreateSelectMenu::new(
                reading_id,
                serenity::CreateSelectMenuKind::Channel {
                    channel_types: Some(vec![serenity::ChannelType::Text]),
                    default_channels: None,
                },
            )
            .placeholder("読み上げ対象チャンネルを選択（複数選択可）")
            .min_values(0)
            .max_values(25),
        ),
        serenity::CreateActionRow::Buttons(vec![
            serenity::CreateButton::new(save_id)
                .label("保存")
                .style(serenity::ButtonStyle::Success),
            serenity::CreateButton::new(cancel_id)
                .label("キャンセル")
                .style(serenity::ButtonStyle::Danger),
        ]),
    ]
}

async fn config_ui_loop(
    ctx: &serenity::Context,
    msg: &serenity::Message,
    author_id: serenity::UserId,
    db: &sea_orm::DatabaseConnection,
    guild_id: serenity::GuildId,
    vc_id: i64,
    channel_mention: &str,
    ctx_id: u64,
) -> Result<(), Error> {
    let notify_id = format!("{}notify", ctx_id);
    let reading_id = format!("{}reading", ctx_id);
    let save_id = format!("{}save", ctx_id);
    let cancel_id = format!("{}cancel", ctx_id);

    let mut notify_channel: Option<serenity::ChannelId> = None;
    let mut reading_channels: Vec<serenity::ChannelId> =
        vec![serenity::ChannelId::new(vc_id as u64)];

    loop {
        let n = notify_id.clone();
        let r = reading_id.clone();
        let s = save_id.clone();
        let c = cancel_id.clone();

        let Some(press) = msg
            .await_component_interaction(ctx)
            .author_id(author_id)
            .timeout(std::time::Duration::from_secs(180))
            .filter(move |m| {
                m.data.custom_id == n
                    || m.data.custom_id == r
                    || m.data.custom_id == s
                    || m.data.custom_id == c
            })
            .await
        else {
            return Ok(());
        };

        if press.data.custom_id == notify_id {
            if let serenity::ComponentInteractionDataKind::ChannelSelect { values } =
                &press.data.kind
            {
                notify_channel = values.first().copied();
            }
            press
                .create_response(ctx, serenity::CreateInteractionResponse::Acknowledge)
                .await?;
        } else if press.data.custom_id == reading_id {
            if let serenity::ComponentInteractionDataKind::ChannelSelect { values } =
                &press.data.kind
            {
                let mut ids: Vec<serenity::ChannelId> = values.clone();
                let vc_chan = serenity::ChannelId::new(vc_id as u64);
                if !ids.contains(&vc_chan) {
                    ids.push(vc_chan);
                }
                reading_channels = ids;
            }
            press
                .create_response(ctx, serenity::CreateInteractionResponse::Acknowledge)
                .await?;
        } else if press.data.custom_id == cancel_id {
            press
                .create_response(
                    ctx,
                    serenity::CreateInteractionResponse::UpdateMessage(
                        serenity::CreateInteractionResponseMessage::new()
                            .embed(
                                serenity::CreateEmbed::new()
                                    .description("設定を中断しました。")
                                    .color(colors::WARN),
                            )
                            .components(vec![]),
                    ),
                )
                .await?;
            return Ok(());
        } else if press.data.custom_id == save_id {
            let Some(notify_ch) = notify_channel else {
                press
                    .create_response(
                        ctx,
                        serenity::CreateInteractionResponse::Message(
                            serenity::CreateInteractionResponseMessage::new()
                                .ephemeral(true)
                                .embed(
                                    serenity::CreateEmbed::new()
                                        .description("通知送信チャンネルを選択してください。")
                                        .color(colors::WARN),
                                ),
                        ),
                    )
                    .await?;
                continue;
            };

            db::reading_targets::Entity::delete_many()
                .filter(db::reading_targets::Column::VoiceChannelId.eq(vc_id))
                .exec(db)
                .await?;
            db::auto_connections::Entity::delete_by_id(vc_id)
                .exec(db)
                .await
                .ok();

            db::auto_connections::ActiveModel {
                voice_channel_id: Set(vc_id),
                guild_id: Set(guild_id.get() as i64),
                notify_channel_id: Set(notify_ch.get() as i64),
            }
            .insert(db)
            .await?;

            for chan_id in &reading_channels {
                db::reading_targets::ActiveModel {
                    id: sea_orm::ActiveValue::NotSet,
                    voice_channel_id: Set(vc_id),
                    text_channel_id: Set(chan_id.get() as i64),
                    guild_id: Set(guild_id.get() as i64),
                }
                .insert(db)
                .await?;
            }

            press
                .create_response(
                    ctx,
                    serenity::CreateInteractionResponse::UpdateMessage(
                        serenity::CreateInteractionResponseMessage::new()
                            .embed(
                                serenity::CreateEmbed::new()
                                    .description(format!(
                                        "{}の設定を保存しました。",
                                        channel_mention,
                                    ))
                                    .color(colors::SUCCEED),
                            )
                            .components(vec![]),
                    ),
                )
                .await?;
            return Ok(());
        }
    }
}
