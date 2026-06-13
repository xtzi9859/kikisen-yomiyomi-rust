use crate::commands::voice_styles::autocomplete_voice_style;
use crate::helpers::{check_admin_permission, reply_no_permission, upsert_guild_setting};
use crate::types::{Context, Error, colors};
use poise::serenity_prelude as serenity;
use sea_orm::ActiveValue::Set;

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
    subcommands(
        "server_admin_permission",
        "server_reply_type",
        "server_command_prefix"
    )
)]
pub async fn server_setting(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

#[poise::command(slash_command, rename = "permission")]
async fn server_admin_permission(
    ctx: Context<'_>,
    #[autocomplete = "autocomplete_permission"] permission: String,
) -> Result<(), Error> {
    let Some(member) = ctx.author_member().await else {
        return reply_no_permission(&ctx).await;
    };
    let has_permission = ctx
        .guild()
        .map(|g| {
            g.member_permissions(&*member)
                .contains(serenity::Permissions::MANAGE_GUILD)
        })
        .unwrap_or(false);
    if !has_permission {
        return reply_no_permission(&ctx).await;
    }

    let guild_id = ctx.guild_id().ok_or("サーバー内でのみ実行可能です。")?;
    upsert_guild_setting(ctx.data(), guild_id, |m| {
        m.admin_permission = Set(permission.clone());
    })
    .await?;

    ctx.send(
        poise::CreateReply::default().ephemeral(true).embed(
            serenity::CreateEmbed::new()
                .description(format!(
                    "サーバー設定の管理権限を`{}`に設定しました。",
                    permission
                ))
                .color(colors::SUCCEED),
        ),
    )
    .await?;

    Ok(())
}

#[poise::command(slash_command, rename = "reply_type")]
async fn server_reply_type(
    ctx: Context<'_>,
    #[autocomplete = "autocomplete_reply_prefix"] reply_type: i32,
) -> Result<(), Error> {
    if !check_admin_permission(&ctx).await? {
        return reply_no_permission(&ctx).await;
    }
    let guild_id = ctx.guild_id().ok_or("サーバー内でのみ実行可能です。")?;
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
        poise::CreateReply::default().ephemeral(true).embed(
            serenity::CreateEmbed::new()
                .description(format!("返信形式を`{}`に設定しました。", label))
                .color(colors::SUCCEED),
        ),
    )
    .await?;

    Ok(())
}

#[poise::command(slash_command, rename = "command_prefix")]
async fn server_command_prefix(ctx: Context<'_>, prefix: String) -> Result<(), Error> {
    if !check_admin_permission(&ctx).await? {
        return reply_no_permission(&ctx).await;
    }

    let guild_id = ctx.guild_id().ok_or("サーバー内でのみ実行可能です。")?;
    upsert_guild_setting(ctx.data(), guild_id, |m| {
        m.command_prefix = Set(prefix.clone());
    })
    .await?;

    ctx.send(
        poise::CreateReply::default().ephemeral(true).embed(
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

#[poise::command(slash_command, rename = "default_speaker_id")]
async fn server_speaker_id(
    ctx: Context<'_>,
    #[autocomplete = "autocomplete_voice_style"] style_id: u32,
) -> Result<(), Error> {
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
            poise::CreateReply::default().ephemeral(true).embed(
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
    let guild_id = ctx.guild_id().ok_or("サーバー内でのみ実行可能です。")?;
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
        poise::CreateReply::default().ephemeral(true).embed(
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

#[poise::command(slash_command, rename = "default_speed")]
async fn server_voice_speed(
    ctx: Context<'_>,
    #[min = 0.5]
    #[max = 2.0]
    speed: f32,
) -> Result<(), Error> {
    if !check_admin_permission(&ctx).await? {
        return reply_no_permission(&ctx).await;
    }
    let guild_id = ctx.guild_id().ok_or("サーバー内でのみ実行可能です。")?;
    upsert_guild_setting(&ctx.data(), guild_id, |m| {
        m.default_speed = Set(Some(speed));
    })
    .await?;

    ctx.send(
        poise::CreateReply::default().ephemeral(true).embed(
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

#[poise::command(slash_command, rename = "default_pitch")]
async fn server_voice_pitch(
    ctx: Context<'_>,
    #[min = -0.15]
    #[max = 0.15]
    pitch: f32,
) -> Result<(), Error> {
    if !check_admin_permission(&ctx).await? {
        return reply_no_permission(&ctx).await;
    }
    let guild_id = ctx.guild_id().ok_or("サーバー内でのみ実行可能です。")?;
    upsert_guild_setting(&ctx.data(), guild_id, |m| {
        m.default_pitch = Set(Some(pitch));
    })
    .await?;

    ctx.send(
        poise::CreateReply::default().ephemeral(true).embed(
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

#[poise::command(slash_command, rename = "default_intonation")]
async fn server_voice_intonation(
    ctx: Context<'_>,
    #[min = 0.0]
    #[max = 2.0]
    intonation: f32,
) -> Result<(), Error> {
    if !check_admin_permission(&ctx).await? {
        return reply_no_permission(&ctx).await;
    }
    let guild_id = ctx.guild_id().ok_or("サーバー内でのみ実行可能です。")?;
    upsert_guild_setting(&ctx.data(), guild_id, |m| {
        m.default_intonation = Set(Some(intonation));
    })
    .await?;

    ctx.send(
        poise::CreateReply::default().ephemeral(true).embed(
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

#[poise::command(slash_command, rename = "reset")]
async fn server_voice_reset(ctx: Context<'_>) -> Result<(), Error> {
    if !check_admin_permission(&ctx).await? {
        return reply_no_permission(&ctx).await;
    }
    let guild_id = ctx.guild_id().ok_or("サーバー内でのみ実行可能です。")?;
    upsert_guild_setting(&ctx.data(), guild_id, |m| {
        m.default_speaker_id = Set(None);
        m.default_speed = Set(None);
        m.default_pitch = Set(None);
        m.default_intonation = Set(None);
    })
    .await?;

    ctx.send(
        poise::CreateReply::default().ephemeral(true).embed(
            serenity::CreateEmbed::new()
                .description("サーバーのデフォルト音声設定をリセットしました。")
                .color(colors::SUCCEED),
        ),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command)]
pub async fn server_settings(
    ctx: Context<'_>,
    #[autocomplete = "autocomplete_server_settings"] setting: String,
    value: bool,
) -> Result<(), Error> {
    if !check_admin_permission(&ctx).await? {
        return reply_no_permission(&ctx).await;
    }

    let label = BOOL_SERVER_SETTINGS
        .iter()
        .find(|&&(k, _)| k == setting.as_str())
        .map(|&(_, l)| l);

    let Some(label) = label else {
        ctx.send(
            poise::CreateReply::default().ephemeral(true).embed(
                serenity::CreateEmbed::new()
                    .description("不明な設定項目です。")
                    .color(colors::ERROR),
            ),
        )
        .await?;
        return Ok(());
    };

    let guild_id = ctx.guild_id().ok_or("サーバー内でのみ実行可能です。")?;

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
        poise::CreateReply::default().ephemeral(true).embed(
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

async fn autocomplete_permission<'a>(
    _ctx: Context<'_>,
    partial: &'a str,
) -> impl Iterator<Item = serenity::builder::AutocompleteChoice> + 'a {
    [
        ("メッセージの管理（manage_messages）", "manage_messages"),
        ("チャンネルの管理（manage_channels）", "manage_channels"),
        (
            "メンバーのタイムアウト（moderate_members）",
            "moderate_members",
        ),
        ("サーバーの管理（manage_guild）", "manage_guild"),
        ("管理者（administrator）", "administrator"),
    ]
    .into_iter()
    .filter(move |(label, _)| partial.is_empty() || label.contains(partial))
    .map(|(label, value)| serenity::builder::AutocompleteChoice::new(label, value))
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
