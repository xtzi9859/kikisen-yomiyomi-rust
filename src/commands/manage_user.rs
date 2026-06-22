use crate::commands::user_setting::upsert_user_setting;
use crate::commands::voice_styles::{autocomplete_voice_style, build_voice_style_page_with_select};
use crate::db;
use crate::helpers::{Pager, check_admin_permission, reply_no_permission};
use crate::types::{Context, Error, colors};
use poise::serenity_prelude as serenity;
use sea_orm::{ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};

#[poise::command(
    slash_command,
    subcommands(
        "us_speaker",
        "us_pitch",
        "us_speed",
        "us_intonation",
        "us_reset",
        "us_show"
    )
)]
pub async fn manage_user(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// 他のユーザーの読み上げの話者を設定する
#[poise::command(slash_command, rename = "speaker")]
async fn us_speaker(
    ctx: Context<'_>,
    #[description = "設定を変更する対象のユーザー"] target: serenity::Member,
    #[description = "話者（空欄で一覧表示）"]
    #[autocomplete = "autocomplete_voice_style"]
    style_id: Option<u32>,
    #[description = "設定を解除して既定値に戻す"] reset: Option<bool>,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?
        .get() as i64;
    if !check_admin_permission(&ctx).await? {
        return reply_no_permission(&ctx).await;
    }
    let user_id = target.user.id.get() as i64;

    let id = if reset.unwrap_or(false) {
        None
    } else if let Some(id) = style_id {
        if !ctx.data().voice_styles.iter().any(|vs| vs.style_id == id) {
            ctx.send(
                poise::CreateReply::default().embed(
                    serenity::CreateEmbed::new()
                        .description(format!(
                            "ID`{}`は存在しません。一覧を確認するにはstyle_idを指定せずにコマンドを実行してください。",
                            id
                        ))
                        .color(colors::ERROR),
                ),
            )
            .await?;
            return Ok(());
        }
        Some(id)
    } else {
        let (embeds, select_options) = build_voice_style_page_with_select(&ctx.data().voice_styles);

        if embeds.is_empty() {
            ctx.send(
                poise::CreateReply::default().embed(
                    serenity::CreateEmbed::new()
                        .description("話者が読み込まれていません。")
                        .color(colors::ERROR),
                ),
            )
            .await?;
            return Ok(());
        }

        let selected = Pager::new(embeds)
            .with_select(select_options, "話者を選択")
            .run_with_select(ctx)
            .await?;

        match selected.and_then(|v| v.parse::<u32>().ok()) {
            Some(id) => Some(id),
            None => return Ok(()),
        }
    };

    upsert_user_setting(&ctx.data().db, guild_id, user_id, |m| {
        m.speaker_id = Set(id.map(|id| id as i32));
    })
    .await?;

    let description = match id {
        Some(id) => {
            let label = ctx
                .data()
                .voice_styles
                .iter()
                .find(|vs| vs.style_id == id)
                .map(|vs| vs.display_label.as_str())
                .unwrap_or("不明");
            format!(
                "{}の話者を`{}`に設定しました。",
                target.display_name(),
                label
            )
        }
        None => format!("{}の話者の設定を解除しました。", target.display_name()),
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

/// 他のユーザーの読み上げの速度を設定する
#[poise::command(slash_command, guild_only, rename = "speed")]
async fn us_speed(
    ctx: Context<'_>,
    #[description = "設定を変更する対象のユーザー"] target: serenity::Member,
    #[description = "速度[0.50 〜 2.00]（空欄でリセット）"]
    #[min = 0.5]
    #[max = 2.0]
    speed: Option<f32>,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?
        .get() as i64;
    if !check_admin_permission(&ctx).await? {
        return reply_no_permission(&ctx).await;
    }
    let user_id = target.user.id.get() as i64;

    upsert_user_setting(&ctx.data().db, guild_id, user_id, |m| {
        m.speed = Set(speed);
    })
    .await?;

    let description = match speed {
        Some(v) => format!(
            "{}の速度を`{:.2}`に設定しました。",
            target.display_name(),
            v
        ),
        None => format!(
            "{}の速度の設定を解除しました。\n（サーバーの既定値が使用されます。）",
            target.display_name()
        ),
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

/// 他のユーザーの読み上げの音高を設定する
#[poise::command(slash_command, rename = "pitch")]
async fn us_pitch(
    ctx: Context<'_>,
    #[description = "設定を変更する対象のユーザー"] target: serenity::Member,
    #[description = "音高[-0.15 〜 0.15]（空欄でリセット）"]
    #[min = -0.15]
    #[max = 0.15]
    pitch: Option<f32>,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?
        .get() as i64;
    if !check_admin_permission(&ctx).await? {
        return reply_no_permission(&ctx).await;
    }
    let user_id = target.user.id.get() as i64;

    upsert_user_setting(&ctx.data().db, guild_id, user_id, |m| {
        m.pitch = Set(pitch);
    })
    .await?;

    let description = match pitch {
        Some(v) => format!(
            "{}の音高を`{:.2}`に設定しました。",
            target.display_name(),
            v
        ),
        None => format!(
            "{}の速度の設定を解除しました。\n（サーバーの既定値が使用されます。）",
            target.display_name()
        ),
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

/// 他のユーザーの読み上げの抑揚を設定する
#[poise::command(slash_command, guild_only, rename = "intonation")]
async fn us_intonation(
    ctx: Context<'_>,
    #[description = "設定を変更する対象のユーザー"] target: serenity::Member,
    #[description = "抑揚[0.00 〜 2.00]（空欄でリセット）"]
    #[min = 0.0]
    #[max = 2.0]
    intonation: Option<f32>,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?
        .get() as i64;
    if !check_admin_permission(&ctx).await? {
        return reply_no_permission(&ctx).await;
    }
    let user_id = target.user.id.get() as i64;

    upsert_user_setting(&ctx.data().db, guild_id, user_id, |m| {
        m.intonation = Set(intonation);
    })
    .await?;

    let description = match intonation {
        Some(v) => format!(
            "{}の抑揚を`{:.2}`に設定しました。",
            target.display_name(),
            v
        ),
        None => format!(
            "{}の速度の設定を解除しました。\n（サーバーの既定値が使用されます。）",
            target.display_name()
        ),
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
async fn us_reset(
    ctx: Context<'_>,
    #[description = "設定を変更する対象のユーザー"] target: serenity::Member,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?
        .get() as i64;
    if !check_admin_permission(&ctx).await? {
        return reply_no_permission(&ctx).await;
    }
    let user_id = target.user.id.get() as i64;

    upsert_user_setting(&ctx.data().db, guild_id, user_id, |m| {
        m.speaker_id = Set(None);
        m.speed = Set(None);
        m.pitch = Set(None);
        m.intonation = Set(None);
    })
    .await?;

    ctx.send(
        poise::CreateReply::default().embed(
            serenity::CreateEmbed::new()
                .description(format!(
                    "{}の個人設定をリセットしました。",
                    target.display_name()
                ))
                .color(colors::SUCCEED),
        ),
    )
    .await?;
    Ok(())
}

//現在のユーザー設定を表示する
#[poise::command(slash_command, rename = "show")]
pub async fn us_show(
    ctx: Context<'_>,
    #[description = "設定を表示する対象のユーザー"] target: serenity::Member,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?;
    let user_id = target.user.id.get() as i64;

    let user_settings = db::user_settings::Entity::find()
        .filter(db::user_settings::Column::GuildId.eq(guild_id.get() as i64))
        .filter(db::user_settings::Column::UserId.eq(user_id))
        .one(&ctx.data().db)
        .await?;

    let speaker_id = user_settings.as_ref().and_then(|u| u.speaker_id);

    let speed = match user_settings.as_ref().and_then(|u| u.speed) {
        Some(s) => format!("{:.2}", s),
        None => "（未設定）".to_string(),
    };

    let intonation = match user_settings.as_ref().and_then(|u| u.intonation) {
        Some(i) => format!("{:.2}", i),
        None => "（未設定）".to_string(),
    };

    let pitch = match user_settings.as_ref().and_then(|u| u.pitch) {
        Some(p) => format!("{:.2}", p),
        None => "（未設定）".to_string(),
    };

    let speaker_label = speaker_id
        .and_then(|id| {
            ctx.data()
                .voice_styles
                .iter()
                .find(|vs| vs.style_id == id as u32)
                .map(|vs| vs.display_label.clone())
        })
        .unwrap_or_else(|| "（未設定）".to_string());

    let server_name = ctx
        .guild()
        .map(|g| g.name.clone())
        .unwrap_or_else(|| "不明な鯖".to_string());

    let embed = serenity::CreateEmbed::new()
        .title(format!("{}のユーザー設定", target.display_name()))
        .field("話者", speaker_label, false)
        .field("速度", speed, false)
        .field("音高", pitch, false)
        .field("抑揚", intonation, false)
        .footer(serenity::CreateEmbedFooter::new(server_name))
        .color(colors::INFO);

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}
