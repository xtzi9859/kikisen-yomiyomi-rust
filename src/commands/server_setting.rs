use crate::commands::voice_styles::autocomplete_voice_style;
use crate::db;
use crate::helpers::{Pager, check_admin_permission, reply_no_permission, upsert_guild_setting};
use crate::types::{Context, Error, colors};
use poise::serenity_prelude as serenity;
use sea_orm::ActiveValue::Set;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, ModelTrait, QueryFilter};

const REPLY_PREFIX_TYPES: &[(i32, &str)] = &[
    (0, "なし"),
    (1, "「返信」"),
    (2, "「○○への返信」"),
    (3, "「○○の××への返信」"),
];

const BOOL_SERVER_SETTINGS: &[(&str, &str)] = &[
    ("read_embed", "embedの中身を読む"),
    (
        "read_non_vc_user",
        "VCに参加していないユーザーのメッセージを読む",
    ),
    (
        "read_server_muted",
        "サーバーミュートされたユーザーのメッセージを読む",
    ),
    ("read_username", "メッセージの先頭に送信者の名前を読む"),
    ("read_spoiler", "スポイラーの中身を読む"),
    ("read_only_mentioned", "botがメンションされた時だけ読む"),
    ("read_silent", "@silentが付与されたメッセージを読む"),
    ("read_vc_join", "VC参加を読み上げる"),
    ("read_vc_leave", "VC退出を読み上げる"),
    ("read_vc_move", "別のVCの状態を読み上げる"),
    ("read_vc_camera_on", "カメラONを読み上げる"),
    ("read_vc_camera_off", "カメラOFFを読み上げる"),
    ("read_vc_stream_start", "画面共有の開始を読み上げる"),
    ("read_vc_stream_stop", "画面共有の終了を読み上げる"),
    ("music_enabled", "音楽再生機能を有効化する"),
    ("restrict_music_skip", "他人の曲のスキップを制限する"),
];

#[poise::command(
    slash_command,
    subcommands("server_reply_type", "server_command_prefix")
)]
pub async fn server_setting(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// 読み上げるメッセージが返信だった場合に先頭につく接頭辞の形式を設定する
#[poise::command(slash_command, rename = "reply_type")]
async fn server_reply_type(
    ctx: Context<'_>,
    #[autocomplete = "autocomplete_reply_prefix"] reply_type: i32,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?;
    if !check_admin_permission(&ctx).await? {
        return reply_no_permission(&ctx).await;
    }

    upsert_guild_setting(ctx.data(), guild_id, |m| {
        m.reply_prefix_type = Set(reply_type);
    })
    .await?;

    let label = match reply_type {
        0 => "なし",
        1 => "「返信」",
        2 => "「○○への返信」",
        3 => "「○○の××への返信",
        _ => unreachable!(),
    };
    ctx.send(
        poise::CreateReply::default().embed(
            serenity::CreateEmbed::new()
                .description(format!("返信形式を`{}`に設定しました。", label))
                .color(colors::SUCCEED),
        ),
    )
    .await?;

    Ok(())
}

/// テキストコマンドのプレフィックスを変更する（デフォルトは「!」）
#[poise::command(slash_command, rename = "command_prefix")]
async fn server_command_prefix(ctx: Context<'_>, prefix: String) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?;
    if !check_admin_permission(&ctx).await? {
        return reply_no_permission(&ctx).await;
    }

    upsert_guild_setting(ctx.data(), guild_id, |m| {
        m.command_prefix = Set(prefix.clone());
    })
    .await?;

    ctx.send(
        poise::CreateReply::default().embed(
            serenity::CreateEmbed::new()
                .description(format!("プレフィックスを`{}`に設定しました。", prefix))
                .color(colors::SUCCEED),
        ),
    )
    .await?;

    Ok(())
}

#[poise::command(
    slash_command,
    subcommands(
        "server_speaker_id",
        "server_voice_speed",
        "server_voice_pitch",
        "server_voice_intonation",
        "server_voice_reset"
    )
)]
pub async fn server_voice(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// サーバーのデフォルトの話者を設定する
#[poise::command(slash_command, rename = "default_speaker_id")]
async fn server_speaker_id(
    ctx: Context<'_>,
    #[autocomplete = "autocomplete_voice_style"] style_id: u32,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?;
    if !check_admin_permission(&ctx).await? {
        return reply_no_permission(&ctx).await;
    }

    if !ctx
        .data()
        .voice_styles
        .iter()
        .any(|vs| vs.style_id == style_id)
    {
        ctx.send(
            poise::CreateReply::default().embed(
                serenity::CreateEmbed::new()
                    .description(format!(
                        "`{}`は存在しません。/voice_stylesで確認してください。",
                        style_id
                    ))
                    .color(colors::ERROR),
            ),
        )
        .await?;
        return Ok(());
    }

    upsert_guild_setting(ctx.data(), guild_id, |m| {
        m.default_speaker_id = Set(Some(style_id as i32));
    })
    .await?;

    let label = ctx
        .data()
        .voice_styles
        .iter()
        .find(|vs| vs.style_id == style_id)
        .map(|vs| vs.display_label.as_str())
        .unwrap_or("不明");
    ctx.send(
        poise::CreateReply::default().embed(
            serenity::CreateEmbed::new()
                .description(format!(
                    "サーバーのデフォルト話者を`{}`に設定しました。",
                    label
                ))
                .color(colors::SUCCEED),
        ),
    )
    .await?;

    Ok(())
}

/// サーバーのデフォルトの話速を設定する
#[poise::command(slash_command, rename = "default_speed")]
async fn server_voice_speed(
    ctx: Context<'_>,
    #[min = 0.5]
    #[max = 2.0]
    speed: f32,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?;
    if !check_admin_permission(&ctx).await? {
        return reply_no_permission(&ctx).await;
    }

    upsert_guild_setting(&ctx.data(), guild_id, |m| {
        m.default_speed = Set(Some(speed));
    })
    .await?;

    ctx.send(
        poise::CreateReply::default().embed(
            serenity::CreateEmbed::new()
                .description(format!(
                    "サーバーのデフォルト速度を`{:.2}`に設定しました。",
                    speed
                ))
                .color(colors::SUCCEED),
        ),
    )
    .await?;

    Ok(())
}

/// サーバーのデフォルトの音高を設定する
#[poise::command(slash_command, rename = "default_pitch")]
async fn server_voice_pitch(
    ctx: Context<'_>,
    #[min = -0.15]
    #[max = 0.15]
    pitch: f32,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?;
    if !check_admin_permission(&ctx).await? {
        return reply_no_permission(&ctx).await;
    }

    upsert_guild_setting(&ctx.data(), guild_id, |m| {
        m.default_pitch = Set(Some(pitch));
    })
    .await?;

    ctx.send(
        poise::CreateReply::default().embed(
            serenity::CreateEmbed::new()
                .description(format!(
                    "サーバーのデフォルト音高を`{:.2}`に設定しました。",
                    pitch
                ))
                .color(colors::SUCCEED),
        ),
    )
    .await?;
    Ok(())
}

/// サーバーのデフォルトの抑揚を設定する
#[poise::command(slash_command, rename = "default_intonation")]
async fn server_voice_intonation(
    ctx: Context<'_>,
    #[min = 0.0]
    #[max = 2.0]
    intonation: f32,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?;
    if !check_admin_permission(&ctx).await? {
        return reply_no_permission(&ctx).await;
    }

    upsert_guild_setting(&ctx.data(), guild_id, |m| {
        m.default_intonation = Set(Some(intonation));
    })
    .await?;

    ctx.send(
        poise::CreateReply::default().embed(
            serenity::CreateEmbed::new()
                .description(format!(
                    "サーバーのデフォルト抑揚を`{:.2}`に設定しました。",
                    intonation
                ))
                .color(colors::SUCCEED),
        ),
    )
    .await?;
    Ok(())
}

/// サーバーのデフォルトのボイス設定をリセットする
#[poise::command(slash_command, rename = "reset")]
async fn server_voice_reset(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?;
    if !check_admin_permission(&ctx).await? {
        return reply_no_permission(&ctx).await;
    }

    upsert_guild_setting(&ctx.data(), guild_id, |m| {
        m.default_speaker_id = Set(None);
        m.default_speed = Set(None);
        m.default_pitch = Set(None);
        m.default_intonation = Set(None);
    })
    .await?;

    ctx.send(
        poise::CreateReply::default().embed(
            serenity::CreateEmbed::new()
                .description("サーバーのデフォルト音声設定をリセットしました。")
                .color(colors::SUCCEED),
        ),
    )
    .await?;
    Ok(())
}

/// サーバー設定のON/OFFを行う
#[poise::command(slash_command)]
pub async fn server_settings(
    ctx: Context<'_>,
    #[autocomplete = "autocomplete_server_settings"] setting: String,
    value: bool,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?;
    if !check_admin_permission(&ctx).await? {
        return reply_no_permission(&ctx).await;
    }

    let label = BOOL_SERVER_SETTINGS
        .iter()
        .find(|&&(k, _)| k == setting.as_str())
        .map(|&(_, l)| l);

    let Some(label) = label else {
        ctx.send(
            poise::CreateReply::default().embed(
                serenity::CreateEmbed::new()
                    .description("不明な設定項目です。")
                    .color(colors::ERROR),
            ),
        )
        .await?;
        return Ok(());
    };

    upsert_guild_setting(&ctx.data(), guild_id, |m| match setting.as_str() {
        "read_embed" => m.read_embed = Set(value),
        "read_non_vc_user" => m.read_non_vc_user = Set(value),
        "read_server_muted" => m.read_server_muted = Set(value),
        "read_username" => m.read_username = Set(value),
        "read_spoiler" => m.read_spoiler = Set(value),
        "read_only_mentioned" => m.read_only_mentioned = Set(value),
        "read_silent" => m.read_silent = Set(value),
        "read_vc_join" => m.read_vc_join = Set(value),
        "read_vc_leave" => m.read_vc_leave = Set(value),
        "read_vc_move" => m.read_vc_move = Set(value),
        "read_vc_camera_on" => m.read_vc_camera_on = Set(value),
        "read_vc_camera_off" => m.read_vc_camera_off = Set(value),
        "read_vc_stream_start" => m.read_vc_stream_start = Set(value),
        "read_vc_stream_stop" => m.read_vc_stream_stop = Set(value),
        "music_enabled" => m.music_enabled = Set(value),
        "restrict_music_skip" => m.restrict_music_skip = Set(value),
        _ => {}
    })
    .await?;

    ctx.send(
        poise::CreateReply::default().embed(
            serenity::CreateEmbed::new()
                .description(format!(
                    "**{}**を`{}`に設定しました。",
                    label,
                    if value { "ON" } else { "OFF" }
                ))
                .color(colors::SUCCEED),
        ),
    )
    .await?;

    Ok(())
}

#[poise::command(
    slash_command,
    subcommands("server_manager_add", "server_manager_remove", "server_manager_list",)
)]
pub async fn server_manager(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// サーバー設定を行えるユーザーまたはロールを追加する
#[poise::command(slash_command, rename = "add")]
pub async fn server_manager_add(
    ctx: Context<'_>,
    #[description = "サーバー設定の管理者に追加するユーザー"] user: Option<serenity::Member>,
    #[description = "サーバー設定の管理者に追加するロール"] role: Option<serenity::Role>,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?
        .get() as i64;
    if !check_admin_permission(&ctx).await? {
        return reply_no_permission(&ctx).await;
    }

    let (manager_id, is_role, manager_name) = match (user, role) {
        (Some(_), Some(_)) => {
            ctx.send(
                poise::CreateReply::default().embed(
                    serenity::CreateEmbed::new()
                        .description("ユーザーとロールは同時には指定できません。どちらか一方のみを指定してください。")
                        .color(colors::WARN),
                ),
            )
            .await?;
            return Ok(());
        }
        (None, None) => {
            ctx.send(
                poise::CreateReply::default().embed(
                    serenity::CreateEmbed::new()
                        .description("ユーザーまたはロールのどちらか一方を指定してください。")
                        .color(colors::WARN),
                ),
            )
            .await?;
            return Ok(());
        }
        (Some(m), None) => (m.user.id.get() as i64, false, m.display_name().into()),
        (None, Some(r)) => (r.id.get() as i64, true, r.name.clone()),
    };

    let db = &ctx.data().db;

    let existing = db::server_manager::Entity::find()
        .filter(db::server_manager::Column::GuildId.eq(guild_id))
        .filter(db::server_manager::Column::ManagerId.eq(manager_id))
        .one(db)
        .await?;

    if existing.is_some() {
        ctx.send(
            poise::CreateReply::default().embed(
                serenity::CreateEmbed::new()
                    .description("このユーザーまたはロールは既に管理者に追加されています。")
                    .color(colors::WARN),
            ),
        )
        .await?;

        return Ok(());
    }

    db::server_manager::ActiveModel {
        guild_id: Set(guild_id),
        manager_id: Set(manager_id),
        is_role: Set(is_role),
        ..Default::default()
    }
    .insert(db)
    .await?;

    ctx.send(
        poise::CreateReply::default().embed(
            serenity::CreateEmbed::new()
                .description(format!(
                    "`{}`をサーバー設定の管理者に追加しました。",
                    manager_name,
                ))
                .color(colors::SUCCEED),
        ),
    )
    .await?;

    Ok(())
}

/// サーバー設定を行えるユーザーまたはロールを削除する
#[poise::command(slash_command, rename = "remove")]
pub async fn server_manager_remove(
    ctx: Context<'_>,
    #[description = "サーバー設定の管理者から削除するユーザー"] user: Option<serenity::Member>,
    #[description = "サーバー設定の管理者から削除するロール"] role: Option<serenity::Role>,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?
        .get() as i64;
    if !check_admin_permission(&ctx).await? {
        return reply_no_permission(&ctx).await;
    }

    let (manager_id, _is_role, manager_name) = match (user, role) {
        (Some(_), Some(_)) => {
            ctx.send(
                poise::CreateReply::default().embed(
                    serenity::CreateEmbed::new()
                        .description("ユーザーとロールは同時には指定できません。どちらか一方のみを指定してください。")
                        .color(colors::WARN),
                ),
            )
            .await?;
            return Ok(());
        }
        (None, None) => {
            ctx.send(
                poise::CreateReply::default().embed(
                    serenity::CreateEmbed::new()
                        .description("ユーザーまたはロールのどちらか一方を指定してください。")
                        .color(colors::WARN),
                ),
            )
            .await?;
            return Ok(());
        }
        (Some(m), None) => (m.user.id.get() as i64, false, m.display_name().into()),
        (None, Some(r)) => (r.id.get() as i64, true, r.name.clone()),
    };

    let db = &ctx.data().db;

    let existing = db::server_manager::Entity::find()
        .filter(db::server_manager::Column::GuildId.eq(guild_id))
        .filter(db::server_manager::Column::ManagerId.eq(manager_id))
        .one(db)
        .await?;

    let manager_model = match existing {
        Some(model) => model,
        None => {
            ctx.send(
                poise::CreateReply::default().embed(
                    serenity::CreateEmbed::new()
                        .description("このユーザーまたはロールは管理者ではありません。")
                        .color(colors::WARN),
                ),
            )
            .await?;

            return Ok(());
        }
    };

    manager_model.delete(db).await?;

    ctx.send(
        poise::CreateReply::default().embed(
            serenity::CreateEmbed::new()
                .description(format!(
                    "`{}`をサーバー設定の管理者から削除しました。",
                    manager_name,
                ))
                .color(colors::SUCCEED),
        ),
    )
    .await?;

    Ok(())
}

/// サーバー設定を行えるユーザーまたはロールの一覧を表示する
#[poise::command(slash_command, rename = "list")]
pub async fn server_manager_list(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?
        .get() as i64;
    let db = &ctx.data().db;
    let manager_list = db::server_manager::Entity::find()
        .filter(db::server_manager::Column::GuildId.eq(guild_id))
        .all(db)
        .await?;

    if manager_list.is_empty() {
        ctx.send(
            poise::CreateReply::default().embed(
                serenity::CreateEmbed::new()
                    .description("サーバー設定の管理者が登録されていません。")
                    .color(colors::WARN),
            ),
        )
        .await?;

        return Ok(());
    }

    let mut embeds = Vec::new();

    for chunk in manager_list.chunks(25) {
        let mut embed = serenity::CreateEmbed::new()
            .title("サーバー設定管理者一覧")
            .color(colors::INFO);

        for manager in chunk.iter() {
            let mention = if manager.is_role {
                format!("<@&{}>", manager.manager_id)
            } else {
                format!("<@{}>", manager.manager_id)
            };

            embed = embed.field("", mention, true)
        }

        embeds.push(embed)
    }

    Pager::new(embeds).run(ctx).await?;

    Ok(())
}

async fn autocomplete_server_settings<'a>(
    _ctx: Context<'_>,
    partial: &'a str,
) -> impl Iterator<Item = serenity::builder::AutocompleteChoice> + 'a {
    BOOL_SERVER_SETTINGS
        .iter()
        .filter(move |(_, label)| partial.is_empty() || label.contains(partial))
        .map(|(key, label)| serenity::builder::AutocompleteChoice::new(*label, *key))
}

async fn autocomplete_reply_prefix<'a>(
    _ctx: Context<'_>,
    partial: &'a str,
) -> impl Iterator<Item = serenity::builder::AutocompleteChoice> + 'a {
    REPLY_PREFIX_TYPES
        .iter()
        .filter(move |(_, label)| partial.is_empty() || label.contains(partial))
        .map(|(key, label)| serenity::builder::AutocompleteChoice::new(*label, *key))
}
