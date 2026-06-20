use crate::commands::voice_styles::autocomplete_voice_style;
use crate::db;
use crate::types::{Context, Error, colors};
use poise::serenity_prelude as serenity;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};

#[poise::command(
    slash_command,
    subcommands("us_speaker", "us_pitch", "us_speed", "us_intonation", "us_reset")
)]
pub async fn user_setting(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// 読み上げの話者を設定する
#[poise::command(slash_command, rename = "speaker")]
async fn us_speaker(
    ctx: Context<'_>,
    #[description = "話者（空欄でリセット）"]
    #[autocomplete = "autocomplete_voice_style"]
    style_id: Option<u32>,
) -> Result<(), Error> {
    if let Some(id) = style_id {
        if !ctx.data().voice_styles.iter().any(|vs| vs.style_id == id) {
            ctx.send(
                poise::CreateReply::default().embed(
                    serenity::CreateEmbed::new()
                        .description(format!(
                            "ID `{}` は存在しません。/voice_stylesで確認してください。",
                            id,
                        ))
                        .color(colors::ERROR),
                ),
            )
            .await?;
            return Ok(());
        }
    }

    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?
        .get() as i64;
    let user_id = ctx.author().id.get() as i64;

    upsert_user_setting(&ctx.data().db, guild_id, user_id, |m| {
        m.speaker_id = Set(style_id.map(|id| id as i32));
    })
    .await?;

    let description = match style_id {
        Some(id) => {
            let label = ctx
                .data()
                .voice_styles
                .iter()
                .find(|vs| vs.style_id == id)
                .map(|vs| vs.display_label.as_str())
                .unwrap_or("不明");
            format!("話者を`{}`に設定しました。", label)
        }
        None => "話者の設定を解除しました。\n（サーバーの既定値が使用されます。）".to_string(),
    };

    ctx.send(
        poise::CreateReply::default().embed(
            serenity::CreateEmbed::new()
                .description(description)
                .color(colors::SUCCEED),
        ),
    ).await?;

    Ok(())
}

/// 読み上げの速度を設定する
#[poise::command(slash_command, guild_only, rename = "speed")]
async fn us_speed(
    ctx: Context<'_>,
    #[description = "速度[0.50 〜 2.00]（空欄でリセット）"]
    #[min = 0.5]
    #[max = 2.0]
    speed: Option<f32>,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?
        .get() as i64;
    let user_id = ctx.author().id.get() as i64;

    upsert_user_setting(&ctx.data().db, guild_id, user_id, |m| {
        m.speed = Set(speed);
    })
    .await?;

    let description = match speed {
        Some(v) => format!("速度を`{:.2}`に設定しました。", v),
        None => "速度の設定を解除しました。\n（サーバーの既定値が使用されます。）".to_string(),
    };

    ctx.send(
        poise::CreateReply::default().embed(
            serenity::CreateEmbed::new()
                .description(description)
                .color(colors::SUCCEED),
        ),
    )
    .await?;
    Ok(())
}

/// 読み上げの音高を設定する
#[poise::command(slash_command, rename = "pitch")]
async fn us_pitch(
    ctx: Context<'_>,
    #[description = "音高[-0.15 〜 0.15]（空欄でリセット）"]
    #[min = -0.15]
    #[max = 0.15]
    pitch: Option<f32>,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?
        .get() as i64;
    let user_id = ctx.author().id.get() as i64;

    upsert_user_setting(&ctx.data().db, guild_id, user_id, |m| {
        m.pitch = Set(pitch);
    })
    .await?;

    let description = match pitch {
        Some(v) => format!("音高を`{:.2}`に設定しました。", v),
        None => "速度の設定を解除しました。\n（サーバーの既定値が使用されます。）".to_string(),
    };

    ctx.send(
        poise::CreateReply::default().embed(
            serenity::CreateEmbed::new()
                .description(description)
                .color(colors::SUCCEED),
        ),
    )
    .await?;

    Ok(())
}

/// 読み上げの抑揚を設定する
#[poise::command(slash_command, guild_only, rename = "intonation")]
async fn us_intonation(
    ctx: Context<'_>,
    #[description = "抑揚[0.00 〜 2.00]（空欄でリセット）"]
    #[min = 0.0]
    #[max = 2.0]
    intonation: Option<f32>,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?
        .get() as i64;
    let user_id = ctx.author().id.get() as i64;

    upsert_user_setting(&ctx.data().db, guild_id, user_id, |m| {
        m.intonation = Set(intonation);
    })
    .await?;

    let description = match intonation {
        Some(v) => format!("抑揚を`{:.2}`に設定しました。", v),
        None => "速度の設定を解除しました。\n（サーバーの既定値が使用されます。）".to_string(),
    };

    ctx.send(
        poise::CreateReply::default().embed(
            serenity::CreateEmbed::new()
                .description(description)
                .color(colors::SUCCEED),
        ),
    )
    .await?;
    Ok(())
}

/// 読み上げの設定をリセットする
#[poise::command(slash_command, guild_only, rename = "reset")]
async fn us_reset(ctx: Context<'_>) -> Result<(), Error> {
    use sea_orm::ActiveValue::Set;

    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?
        .get() as i64;
    let user_id = ctx.author().id.get() as i64;

    upsert_user_setting(&ctx.data().db, guild_id, user_id, |m| {
        m.speaker_id = Set(None);
        m.speed = Set(None);
        m.pitch = Set(None);
        m.intonation = Set(None);
    })
    .await?;

    ctx.send(
        poise::CreateReply::default().ephemeral(true).embed(
            serenity::CreateEmbed::new()
                .description("個人設定をリセットしました。")
                .color(colors::SUCCEED),
        ),
    )
    .await?;
    Ok(())
}

pub(crate) async fn upsert_user_setting<F>(
    db: &sea_orm::DatabaseConnection,
    guild_id: i64,
    user_id: i64,
    update_fn: F,
) -> Result<(), Error>
where
    F: FnOnce(&mut db::user_settings::ActiveModel),
{
    let existing = db::user_settings::Entity::find()
        .filter(db::user_settings::Column::GuildId.eq(guild_id))
        .filter(db::user_settings::Column::UserId.eq(user_id))
        .one(db)
        .await?;

    if let Some(model) = existing {
        let mut active = model.into();
        update_fn(&mut active);
        active.update(db).await?;
    } else {
        let mut active = db::user_settings::ActiveModel {
            guild_id: Set(guild_id),
            user_id: Set(user_id),
            ..Default::default()
        };
        update_fn(&mut active);
        active.insert(db).await?;
    }

    Ok(())
}
