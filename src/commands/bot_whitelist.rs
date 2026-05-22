use crate::types::{Error, Context, colors};
use crate::db;
use crate::helpers::{check_admin_permission, reply_no_permission};
use poise::serenity_prelude as serenity;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, ModelTrait as _, QueryFilter};

#[poise::command(slash_command, subcommands("bw_add", "bw_remove", "bw_list"))]
pub async fn bot_whitelist(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

#[poise::command(slash_command, rename = "add")]
async fn bw_add(ctx: Context<'_>, bot: serenity::User) -> Result<(), Error> {
    if !check_admin_permission(&ctx).await? {
        return reply_no_permission(&ctx).await;
    }

    if !bot.bot {
        ctx.send(
            poise::CreateReply::default().ephemeral(true).embed(
                serenity::CreateEmbed::new()
                    .description("指定されたユーザーはbotではありません。")
                    .color(colors::ERROR),
            ),
        )
        .await?;
        return Ok(());
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("サーバー内でのみ実行可能です。")?
        .get() as i64;
    let bot_id = bot.id.get() as i64;

    let exists = db::bot_whitelist::Entity::find()
        .filter(db::bot_whitelist::Column::GuildId.eq(guild_id))
        .filter(db::bot_whitelist::Column::BotId.eq(bot_id))
        .one(&ctx.data().db)
        .await?
        .is_some();

    if exists {
        ctx.send(
            poise::CreateReply::default().ephemeral(true).embed(
                serenity::CreateEmbed::new()
                    .description(format!("`{}`は既に登録されています。", bot.name))
                    .color(colors::WARN),
            ),
        )
        .await?;
        return Ok(());
    }

    db::bot_whitelist::ActiveModel {
        guild_id: Set(guild_id),
        bot_id: Set(bot_id),
    }
    .insert(&ctx.data().db)
    .await?;

    ctx.send(
        poise::CreateReply::default().ephemeral(true).embed(
            serenity::CreateEmbed::new()
                .description(format!("`{}`をホワイトリストに登録しました。", bot.name,))
                .color(colors::SUCCEED),
        ),
    )
    .await?;

    Ok(())
}

#[poise::command(slash_command, rename = "remove")]
async fn bw_remove(ctx: Context<'_>, bot: serenity::User) -> Result<(), Error> {
    if !check_admin_permission(&ctx).await? {
        return reply_no_permission(&ctx).await;
    }

    let guild_id = ctx.guild_id().ok_or("サーバー内でのみ実行可能です。")?.get() as i64;
    let bot_id = bot.id.get() as i64;

    let record = db::bot_whitelist::Entity::find()
        .filter(db::bot_whitelist::Column::GuildId.eq(guild_id))
        .filter(db::bot_whitelist::Column::BotId.eq(bot_id))
        .one(&ctx.data().db)
        .await?;

    match record {
        None => {
            ctx.send(
                poise::CreateReply::default()
                    .ephemeral(true)
                    .embed(serenity::CreateEmbed::new()
                        .description(format!(
                            "`{}`は登録されていません。",
                            bot.name,
                        ))
                        .color(colors::WARN),
                ),
            )
            .await?;
        }
        Some(model) => {
            model.delete(&ctx.data().db).await?;
            ctx.send(
                poise::CreateReply::default()
                    .ephemeral(true)
                    .embed(serenity::CreateEmbed::new()
                        .description(format!(
                            "`{}`をホワイトリストから削除しました。",
                            bot.name
                        ))
                        .color(colors::SUCCEED),
                ),
            )
            .await?;
        }
    }

    Ok(())
}

#[poise::command(slash_command, rename = "list",)]
async fn bw_list(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("サーバー内でのみ実行可能です。")?.get() as i64;
    let entries = db::bot_whitelist::Entity::find()
        .filter(db::bot_whitelist::Column::GuildId.eq(guild_id))
        .all(&ctx.data().db)
        .await?;

    let description = if entries.is_empty() {
        "登録されているbotはありません。".to_string()
    } else {
        entries
            .iter()
            .map(|e| format!("- <@{}>", e.bot_id))
            .collect::<Vec<_>>()
            .join("\n")
    };

    ctx.send(
        poise::CreateReply::default()
            .ephemeral(true)
            .embed(serenity::CreateEmbed::new()
                .title("botホワイトリスト")
                .description(description)
                .color(colors::INFO),
        ),
    )
    .await?;

    Ok(())
}
