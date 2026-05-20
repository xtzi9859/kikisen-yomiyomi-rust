use async_trait::async_trait;
use dotenvy::dotenv;
use poise::serenity_prelude as serenity;
use regex::Regex;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, Database, DatabaseConnection,
    DbBackend, EntityTrait, QueryFilter, Schema,
};
use songbird::SerenityInit;
use songbird::events::{Event, EventContext, EventHandler as VoiceEventHandler, TrackEvent};
use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use std::sync::{Arc, LazyLock};
use tempfile::Builder;
use tokio::sync::RwLock;
use tracing;
use tracing_appender::rolling;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};
use unicode_segmentation::UnicodeSegmentation;
use voicevox_core::nonblocking::{Onnxruntime, OpenJtalk, Synthesizer, VoiceModelFile};

mod db;

const DEFAULT_PREFIX: &str = "!";
const DEFAULT_SPEAKER_ID: i32 = 8;
const OPEN_JTALK_DIR: &str = "./voicevox_core/dict/open_jtalk_dic_utf_8-1.11";
const ONNXRUNTIME_FILENAME: &str =
    "./voicevox_core/onnxruntime/lib/libvoicevox_onnxruntime.so.1.17.3";
const ACCELERATION_MODE: voicevox_core::AccelerationMode = voicevox_core::AccelerationMode::Cpu;
const VVMS_DIR: &str = "./voicevox_core/models/vvms";

#[derive(Clone, Debug)]
pub struct VoiceStyleInfo {
    pub character_name: String,
    pub style_name: String,
    pub style_id: u32,
    pub display_label: String,
}
pub struct VoiceContextInfo {
    pub command_channel: serenity::ChannelId,
    pub text_channels: HashSet<serenity::ChannelId>,
}
struct Data {
    pub voice_to_text_map: Arc<RwLock<HashMap<serenity::ChannelId, VoiceContextInfo>>>,
    music_state: Arc<RwLock<MusicState>>,
    pub synthesizer:
        Arc<voicevox_core::nonblocking::Synthesizer<voicevox_core::nonblocking::OpenJtalk>>,
    pub db: DatabaseConnection,
    pub voice_styles: Vec<VoiceStyleInfo>,
    guild_settings_cache: Arc<RwLock<HashMap<serenity::GuildId, db::guild_settings::Model>>>,
}
#[derive(Clone)]
struct FileDeleter {
    _temp_file_path: Arc<tempfile::TempPath>,
}
struct MusicItem {
    url: String,
    title: String,
    is_ytdl: bool,
}
#[derive(serde::Deserialize)]
struct YtdlOutput {
    title: String,
    webpage_url: String,
}
struct MusicState {
    queue: VecDeque<MusicItem>,
    current_track: Option<songbird::tracks::TrackHandle>,
    volume: f32,
}
struct MusicEndHandler {
    ctx: serenity::Context,
    guild_id: serenity::GuildId,
    music_state: Arc<RwLock<MusicState>>,
    voice_to_text_map: Arc<RwLock<HashMap<serenity::ChannelId, VoiceContextInfo>>>,
}

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

static URL_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://\S+").expect("failed to compile regex url"));
static CODEBLOCK_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)```(?P<lang>[^\n\s]*)\s*\n?.*?```").expect("failed to compile regex codeblock")
});
//static INLINE_CODE_REGEX: LazyLock<Regex> =
//    LazyLock::new(|| Regex::new(r"`([^`]+)`").expect("failed to compile regex inline-code"));
static SPOILER_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\|\|.*?\|\|").expect("failed to compile regex spoiler"));
static QUOTE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^>{1,3}\s?").expect("failed to compile regex quote"));
static NEWLINE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\r?\n").expect("failed to compile regex newline"));
static ROLE_MENTION_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<@&(\d+)>").expect("failed to compile regex role-mention"));
static CHANNEL_MENTION_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<#(\d+)>").expect("failed to compile regex channel-mention"));
static CUSTOM_EMOJI_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<a?:(\w+):\d+>").expect("failed to compile regex custom-emoji"));

#[allow(dead_code)]
mod colors {
    pub const BOT: u32 = 0x99aab5;
    pub const INFO: u32 = 0x5865f2;
    pub const SUCCEED: u32 = 0x57F287;
    pub const WARN: u32 = 0xE67E22;
    pub const ERROR: u32 = 0xed4245;
}

#[async_trait]
impl VoiceEventHandler for FileDeleter {
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        None
    }
}

#[async_trait]
impl VoiceEventHandler for MusicEndHandler {
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        let ctx = self.ctx.clone();
        let guild_id = self.guild_id;
        let music_state = self.music_state.clone();
        let voice_to_text_map = self.voice_to_text_map.clone();

        tokio::spawn(async move {
            let _ = play_next_music(&ctx, guild_id, music_state, voice_to_text_map).await;
        });
        None
    }
}

fn sanitize_text(text: &str) -> String {
    let mut result = CODEBLOCK_REGEX
        .replace_all(text, |caps: &regex::Captures| {
            let lang = &caps["lang"];
            if lang.is_empty() {
                "コードブロック".to_string()
            } else {
                format!("コードブロック {}", lang)
            }
        })
        .into_owned();
    result = SPOILER_REGEX
        .replace_all(&result, "スポイラー")
        .into_owned();
    result = QUOTE_REGEX.replace_all(&result, "引用 ").into_owned();
    result = URL_REGEX.replace_all(&result, "URL").into_owned();
    result = NEWLINE_REGEX.replace_all(&result, " ").into_owned();
    result
}

fn format_message(message: &serenity::Message, ctx: &serenity::Context) -> String {
    let mut text = message.content.clone();
    let mut prefix = String::new();

    if let Some(ref referenced) = message.referenced_message {
        let mut author_name = referenced.author.name.clone();
        if let Some(guild_id) = message.guild_id {
            if let Some(guild) = ctx.cache.guild(guild_id) {
                if let Some(member) = guild.members.get(&referenced.author.id) {
                    author_name = member.display_name().to_owned();
                }
            }
        }
        if author_name == referenced.author.name {
            if let Some(global_name) = &referenced.author.global_name {
                author_name = global_name.clone();
            }
        }
        prefix.push_str(&format!("{}への返信 ", author_name));
    }

    if !message.message_snapshots.is_empty() {
        prefix.push_str("転送");
    }

    for user in &message.mentions {
        let tag_standard = format!("<@{}>", user.id);
        let tag_nickname = format!("<@!{}>", user.id);

        let mut display_name = user.name.clone();

        if let Some(guild_id) = message.guild_id {
            if let Some(guild) = ctx.cache.guild(guild_id) {
                if let Some(member) = guild.members.get(&user.id) {
                    display_name = member.display_name().to_owned();
                }
            }
        }

        if display_name == user.name {
            if let Some(global_name) = &user.global_name {
                display_name = global_name.clone();
            }
        }

        text = text.replace(&tag_standard, &format!("あっと{}", &display_name));
        text = text.replace(&tag_nickname, &format!("あっと{}", &display_name));
    }

    text = ROLE_MENTION_REGEX
        .replace_all(&text, |caps: &regex::Captures| {
            let role_id = caps[1].parse::<u64>().unwrap_or(0);
            if let Some(guild_id) = message.guild_id {
                if let Some(guild) = ctx.cache.guild(guild_id) {
                    if let Some(role) = guild.roles.get(&serenity::RoleId::new(role_id)) {
                        return format!("あっと{}", role.name);
                    }
                }
            }
            "不明なロール".to_string()
        })
        .into_owned();

    text = CHANNEL_MENTION_REGEX
        .replace_all(&text, |caps: &regex::Captures| {
            let chan_id = caps[1].parse::<u64>().unwrap_or(0);
            let channel_id = serenity::ChannelId::new(chan_id);
            if let Some(guild_id) = message.guild_id {
                if let Some(guild) = ctx.cache.guild(guild_id) {
                    if let Some(channel) = guild.channels.get(&channel_id) {
                        return channel.name.to_string();
                    }
                }
            }
            "不明なチャンネル".to_string()
        })
        .into_owned();

    text = CUSTOM_EMOJI_REGEX.replace_all(&text, "$1").into_owned();

    let mut demojized_text = String::new();
    for grapheme in text.graphemes(true) {
        if let Some(emoji) = emoji::lookup_by_glyph::lookup(grapheme) {
            let ja_name = emoji
                .annotations
                .iter()
                .find(|a| a.lang == "ja")
                .and_then(|a| a.tts)
                .unwrap_or(emoji.name);
            demojized_text.push_str(&format!(" {} ", ja_name));
        } else {
            demojized_text.push_str(grapheme);
        }
    }
    text = demojized_text;

    if !message.sticker_items.is_empty() {
        let sticker_names: Vec<String> = message
            .sticker_items
            .iter()
            .map(|s| s.name.to_string())
            .collect();
        text.push_str(&format!(" {}", sticker_names.join(" ")));
    }

    if !message.attachments.is_empty() {
        let mut descriptions = Vec::new();

        for attachment in &message.attachments {
            let desc = match attachment
                .content_type
                .as_deref()
                .and_then(|ct| ct.split_once('/'))
            {
                Some(("image", _)) => "画像ファイル",
                Some(("video", _)) => "動画ファイル",
                Some(("audio", _)) => "音声ファイル",
                _ => "添付ファイル",
            };
            descriptions.push(desc);
        }

        let attachment_text = descriptions.join(" ");

        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&attachment_text);
    }

    format!("{}{}", prefix, text)
}

async fn search_youtube(query: &str) -> Result<Vec<YtdlOutput>, Error> {
    let output = tokio::process::Command::new("yt-dlp")
        .args(&[
            "--dump-json",
            "--default-search",
            "ytsearch",
            &format!("ytsearch10:{}", query),
        ])
        .output()
        .await?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut results = Vec::new();

    for line in stdout.lines() {
        if let Ok(video) = serde_json::from_str::<YtdlOutput>(line) {
            results.push(video);
        }
    }

    Ok(results)
}

async fn get_youtube_info(url: &str) -> Result<YtdlOutput, Error> {
    let output = tokio::process::Command::new("yt-dlp")
        .args(&["--dump-json", "--no-playlist", url])
        .output()
        .await?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    if let Some(first_line) = stdout.lines().next() {
        let video = serde_json::from_str::<YtdlOutput>(first_line)?;
        Ok(video)
    } else {
        Err("動画情報の取得に失敗しました".into())
    }
}

async fn play_voicevox(
    ctx: &serenity::Context,
    guild_id: serenity::GuildId,
    text: &str,
    data: &Data,
    user_id: Option<serenity::UserId>,
) -> Result<(), Error> {
    let g_id = guild_id.get() as i64;

    let guild_settings = db::guild_settings::Entity::find()
        .filter(db::guild_settings::Column::GuildId.eq(g_id))
        .one(&data.db)
        .await
        .ok()
        .flatten();

    let user_settings = if let Some(uid) = user_id {
        db::user_settings::Entity::find()
            .filter(db::user_settings::Column::GuildId.eq(g_id))
            .filter(db::user_settings::Column::UserId.eq(uid.get() as i64))
            .one(&data.db)
            .await
            .ok()
            .flatten()
    } else {
        None
    };

    let speaker_id = user_settings
        .as_ref()
        .and_then(|u| u.speaker_id)
        .or_else(|| guild_settings.as_ref().and_then(|g| g.default_speaker_id))
        .unwrap_or(DEFAULT_SPEAKER_ID);

    let speed = user_settings
        .as_ref()
        .and_then(|u| u.speed)
        .or_else(|| guild_settings.as_ref().and_then(|g| g.default_speed))
        .unwrap_or(1.0);

    let pitch = user_settings
        .as_ref()
        .and_then(|u| u.pitch)
        .or_else(|| guild_settings.as_ref().and_then(|g| g.default_pitch))
        .unwrap_or(0.0);

    let intonation = user_settings
        .as_ref()
        .and_then(|u| u.intonation)
        .or_else(|| guild_settings.as_ref().and_then(|g| g.default_intonation))
        .unwrap_or(1.0);

    let style_id = voicevox_core::StyleId::new(speaker_id as u32);
    let mut audio_query = data.synthesizer.create_audio_query(text, style_id).await?;

    audio_query.speed_scale = speed;
    audio_query.pitch_scale = pitch;
    audio_query.intonation_scale = intonation;

    let audio_bytes = data
        .synthesizer
        .synthesis(&audio_query, style_id)
        .perform()
        .await?;

    let temp_file = Builder::new()
        .prefix("voicevox_")
        .suffix(".wav")
        .tempfile()?;

    let temp_file_path = temp_file.into_temp_path();
    tokio::fs::write(&temp_file_path, &audio_bytes).await?;

    let manager = songbird::get(ctx)
        .await
        .expect("failed to initialize songbird");
    if let Some(call_lock) = manager.get(guild_id) {
        let mut call = call_lock.lock().await;
        let input = songbird::input::File::new(temp_file_path.to_string_lossy().to_string());
        let handle = call.enqueue_input(input.into()).await;

        let deleter = FileDeleter {
            _temp_file_path: Arc::new(temp_file_path),
        };

        handle
            .add_event(Event::Track(TrackEvent::End), deleter.clone())
            .ok();
        handle
            .add_event(Event::Track(TrackEvent::Error), deleter)
            .ok();
    }

    Ok(())
}

async fn play_next_music(
    ctx: &serenity::Context,
    guild_id: serenity::GuildId,
    music_state: Arc<RwLock<MusicState>>,
    voice_to_text_map: Arc<RwLock<HashMap<serenity::ChannelId, VoiceContextInfo>>>,
) -> Result<(), Error> {
    let manager = songbird::get(ctx).await.expect("songbird error").clone();

    if let Some(handler_lock) = manager.get(guild_id) {
        let mut call = handler_lock.lock().await;
        let vc_id = call.current_channel().map(|c| c.0);

        let target_text_channel = if let Some(v_id) = vc_id {
            let map = voice_to_text_map.read().await;
            map.get(&serenity::ChannelId::from(v_id))
                .map(|info| info.command_channel)
        } else {
            None
        };

        let mut state = music_state.write().await;

        if let Some(next_item) = state.queue.pop_front() {
            let client = reqwest::Client::new();
            let source = if next_item.is_ytdl {
                songbird::input::YoutubeDl::new(client, next_item.url.clone()).into()
            } else {
                songbird::input::HttpRequest::new(client, next_item.url.clone()).into()
            };
            let track_handler = call.play(source);
            let _ = track_handler.set_volume(state.volume);

            let _ = track_handler.add_event(
                Event::Track(TrackEvent::End),
                MusicEndHandler {
                    ctx: ctx.clone(),
                    guild_id,
                    music_state: music_state.clone(),
                    voice_to_text_map: voice_to_text_map.clone(),
                },
            );

            state.current_track = Some(track_handler);
            if let Some(channel) = target_text_channel {
                let _ = channel
                    .say(&ctx.http, format!("再生中: {}", next_item.title))
                    .await;
            }
        } else {
            state.current_track = None;
            if let Some(channel) = target_text_channel {
                let _ = channel.say(&ctx.http, "キューが空になりました").await;
            }
        }
    }
    Ok(())
}

#[poise::command(prefix_command, aliases("s"))]
pub async fn skip(ctx: Context<'_>) -> Result<(), Error> {
    let state = ctx.data().music_state.read().await;
    if let Some(handle) = &state.current_track {
        let _ = handle.stop();
        ctx.say("スキップしました").await?;
    } else {
        ctx.say("再生していません").await?;
    }
    Ok(())
}

#[poise::command(prefix_command, aliases("vol"))]
pub async fn volume(ctx: Context<'_>, vol_input: f32) -> Result<(), Error> {
    let actual_vol = (vol_input / 100.0).clamp(0.0, 1.0);
    let mut state = ctx.data().music_state.write().await;

    state.volume = actual_vol;
    if let Some(handle) = &state.current_track {
        let _ = handle.set_volume(actual_vol);
    }
    drop(state);

    if let Some(gid) = ctx.guild_id() {
        let _ = upsert_guild_setting(&ctx.data(), gid, |m| {
            m.default_music_vol = Set(actual_vol);
        })
        .await;
    }

    ctx.say(format!("音量を`{}`に設定しました。", vol_input))
        .await?;
    Ok(())
}

#[poise::command(prefix_command, aliases("p"))]
pub async fn play(
    ctx: Context<'_>,
    file: Option<serenity::Attachment>,
    #[rest] query: Option<String>,
) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます")?;

    let mut item_to_add = None;
    let mut message_to_delete = None;

    if let Some(attachment) = file {
        item_to_add = Some(MusicItem {
            url: attachment.url.clone(),
            title: attachment.filename.clone(),
            is_ytdl: false,
        });
    } else if let Some(args) = query {
        let processing_msg = ctx.say("処理中…").await?;
        if args.starts_with("http") {
            let title = match get_youtube_info(&args).await {
                Ok(info) => info.title,
                Err(why) => {
                    tracing::warn!("failed to retrieve video info from URL: {:?}", why);
                    "不明なタイトル".to_string()
                }
            };

            item_to_add = Some(MusicItem {
                url: args.clone(),
                title: title,
                is_ytdl: true,
            });

            message_to_delete = Some(processing_msg);
        } else {
            let search_results = search_youtube(&args).await?;
            if search_results.is_empty() {
                processing_msg
                    .edit(
                        ctx,
                        poise::CreateReply::default().content("検索結果が見つかりませんでした"),
                    )
                    .await?;
                return Ok(());
            }

            let mut options = Vec::new();
            for video in search_results.iter().take(10) {
                let label = if video.title.chars().count() > 95 {
                    format!("{}...", video.title.chars().take(95).collect::<String>())
                } else {
                    video.title.clone()
                };
                options.push(serenity::CreateSelectMenuOption::new(
                    label,
                    &video.webpage_url,
                ));
            }

            let select_menu = serenity::CreateSelectMenu::new(
                "youtube_search_select",
                serenity::CreateSelectMenuKind::String { options },
            )
            .placeholder("再生する動画を選択してください。");

            processing_msg
                .edit(
                    ctx,
                    poise::CreateReply::default()
                        .content(format!("`{}`の検索結果", args))
                        .components(vec![serenity::CreateActionRow::SelectMenu(select_menu)]),
                )
                .await?;

            let interaction = processing_msg
                .message()
                .await?
                .await_component_interaction(ctx.serenity_context())
                .author_id(ctx.author().id)
                .timeout(std::time::Duration::from_secs(60))
                .await;

            if let Some(mci) = interaction {
                let selected_url = match &mci.data.kind {
                    serenity::ComponentInteractionDataKind::StringSelect { values } => {
                        values[0].clone()
                    }
                    _ => return Ok(()),
                };

                let selected_title = search_results
                    .into_iter()
                    .find(|v| v.webpage_url == selected_url)
                    .map(|v| v.title)
                    .unwrap_or_else(|| "Youtube Video".to_string());

                mci.create_response(
                    ctx.serenity_context(),
                    serenity::CreateInteractionResponse::UpdateMessage(
                        serenity::CreateInteractionResponseMessage::new()
                            .content("動画を処理中…")
                            .components(vec![]),
                    ),
                )
                .await?;

                item_to_add = Some(MusicItem {
                    url: selected_url,
                    title: selected_title,
                    is_ytdl: true,
                });

                message_to_delete = Some(processing_msg);
            } else {
                let _ = processing_msg
                    .edit(
                        ctx,
                        poise::CreateReply::default()
                            .content("タイムアウトしました。")
                            .components(vec![]),
                    )
                    .await;
            }
        }
    } else {
        ctx.say("ファイル、検索ワード、URLのいずれかを指定してください。")
            .await?;
    }

    if let Some(item) = item_to_add {
        ctx.say(format!("キューに追加しました: {}", item.title))
            .await?;

        let should_play = {
            let mut state = ctx.data().music_state.write().await;
            state.queue.push_back(item);
            state.current_track.is_none()
        };

        if should_play {
            play_next_music(
                ctx.serenity_context(),
                guild_id,
                ctx.data().music_state.clone(),
                ctx.data().voice_to_text_map.clone(),
            )
            .await?;
        }

        if let Some(msg) = message_to_delete {
            let _ = msg.delete(ctx).await;
        }
    }

    Ok(())
}

fn get_command_prefix<'a>(
    ctx: poise::PartialContext<'a, Data, Error>,
) -> poise::BoxFuture<'a, Result<Option<String>, Error>> {
    Box::pin(async move {
        let prefix = match ctx.guild_id {
            Some(gid) => get_guild_settings(ctx.data, gid).await.command_prefix,
            None => DEFAULT_PREFIX.to_string(),
        };
        Ok(Some(prefix))
    })
}

fn permission_from_str(s: &str) -> serenity::Permissions {
    match s {
        "administrator" => serenity::Permissions::ADMINISTRATOR,
        _ => serenity::Permissions::MANAGE_GUILD,
    }
}

async fn check_admin_permission(ctx: &Context<'_>) -> Result<bool, Error> {
    let guild_id = ctx.guild_id().ok_or("サーバー内でのみ実行可能です。")?;
    let settings = get_guild_settings(ctx.data(), guild_id).await;
    let required = permission_from_str(&settings.admin_permission);

    let Some(member) = ctx.author_member().await else {
        return Ok(false);
    };

    let permissions = ctx
        .guild()
        .map(|g| g.member_permissions(&*member))
        .unwrap_or(serenity::Permissions::empty());

    Ok(permissions.contains(required))
}

async fn get_guild_settings(data: &Data, guild_id: serenity::GuildId) -> db::guild_settings::Model {
    {
        let cache = data.guild_settings_cache.read().await;
        if let Some(settings) = cache.get(&guild_id) {
            return settings.clone();
        }
    }

    let settings = db::guild_settings::Entity::find()
        .filter(db::guild_settings::Column::GuildId.eq(guild_id.get() as i64))
        .one(&data.db)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| db::guild_settings::Model::default_for_guild(guild_id.get() as i64));

    data.guild_settings_cache
        .write()
        .await
        .insert(guild_id, settings.clone());

    settings
}

async fn reply_no_permission(ctx: &Context<'_>) -> Result<(), Error> {
    ctx.send(
        poise::CreateReply::default().ephemeral(true).embed(
            serenity::CreateEmbed::new()
                .description("このコマンドを使用する権限がありません。")
                .color(colors::ERROR),
        ),
    )
    .await?;

    Ok(())
}

async fn upsert_guild_setting<F>(
    data: &Data,
    guild_id: serenity::GuildId,
    update_fn: F,
) -> Result<(), Error>
where
    F: FnOnce(&mut db::guild_settings::ActiveModel),
{
    let existing = db::guild_settings::Entity::find()
        .filter(db::guild_settings::Column::GuildId.eq(guild_id.get() as i64))
        .one(&data.db)
        .await?;

    let updated = if let Some(model) = existing {
        let mut active: db::guild_settings::ActiveModel = model.into();
        update_fn(&mut active);
        active.update(&data.db).await?
    } else {
        let mut active: db::guild_settings::ActiveModel =
            db::guild_settings::Model::default_for_guild(guild_id.get() as i64).into();
        update_fn(&mut active);
        active.insert(&data.db).await?
    };

    data.guild_settings_cache
        .write()
        .await
        .insert(guild_id, updated);
    Ok(())
}

async fn upsert_user_setting<F>(
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

#[poise::command(slash_command)]
async fn restart(ctx: Context<'_>) -> Result<(), Error> {
    let embed = serenity::CreateEmbed::new()
        .color(colors::SUCCEED)
        .description("再起動します…");

    let reply = poise::CreateReply::default().embed(embed);
    ctx.send(reply).await?;

    tracing::info!("restart command executed; restarting...");

    ctx.framework().shard_manager().shutdown_all().await;
    std::process::exit(0);
}

fn build_voice_style_pages(styles: &[VoiceStyleInfo]) -> Vec<Vec<(String, String)>> {
    let mut pages: Vec<Vec<(String, String)>> = Vec::new();
    let mut current: Vec<(String, String)> = Vec::new();

    for style in styles {
        if current.len() >= 24 {
            pages.push(current);
            current = Vec::new();
        }
        current.push((
            format!("{}（{}）", style.character_name, style.style_name),
            format!("`{}`", style.style_id),
        ));
    }
    if !current.is_empty() {
        pages.push(current);
    }

    pages
}

fn voice_style_embed(pages: &[Vec<(String, String)>], page: usize) -> serenity::CreateEmbed {
    let mut embed = serenity::CreateEmbed::new()
        .title(format!("VOICEVOX 話者一覧（{}/{}）", page + 1, pages.len()))
        .color(colors::INFO)
        .footer(serenity::CreateEmbedFooter::new(
            "話者IDを /user_setting で設定できます",
        ));

    for (name, value) in &pages[page] {
        embed = embed.field(name, value, true);
    }

    embed
}

fn voice_style_buttons(
    prev_id: &str,
    next_id: &str,
    page: usize,
    total: usize,
) -> serenity::CreateActionRow {
    serenity::CreateActionRow::Buttons(vec![
        serenity::CreateButton::new(prev_id)
            .label("◀")
            .style(serenity::ButtonStyle::Secondary)
            .disabled(page == 0),
        serenity::CreateButton::new(next_id)
            .label("▶")
            .style(serenity::ButtonStyle::Secondary)
            .disabled(page >= total - 1),
    ])
}

#[poise::command(slash_command)]
pub async fn voice_styles(ctx: Context<'_>) -> Result<(), Error> {
    let pages = build_voice_style_pages(&ctx.data().voice_styles);

    if pages.is_empty() {
        ctx.say("読み込まれた話者がいません。").await?;
        return Ok(());
    }

    let total = pages.len();
    let mut current_page = 0usize;

    let ctx_id = ctx.id();
    let prev_id = format!("{}prev", ctx_id);
    let next_id = format!("{}next", ctx_id);

    let reply = ctx
        .send(
            poise::CreateReply::default()
                .ephemeral(true)
                .embed(voice_style_embed(&pages, current_page))
                .components(if total > 1 {
                    vec![voice_style_buttons(&prev_id, &next_id, current_page, total)]
                } else {
                    vec![]
                }),
        )
        .await?;

    if total == 1 {
        return Ok(());
    }

    let message = reply.message().await?;

    loop {
        let prev_id_c = prev_id.clone();
        let next_id_c = next_id.clone();

        let Some(press) = message
            .await_component_interaction(ctx.serenity_context())
            .author_id(ctx.author().id)
            .timeout(std::time::Duration::from_secs(120))
            .filter(move |m| m.data.custom_id == prev_id_c || m.data.custom_id == next_id_c)
            .await
        else {
            // タイムアウト: ボタンを無効化して終了
            let _ = reply
                .edit(
                    ctx,
                    poise::CreateReply::default()
                        .embed(voice_style_embed(&pages, current_page))
                        .components(vec![]),
                )
                .await;
            break;
        };

        if press.data.custom_id == prev_id {
            current_page = current_page.saturating_sub(1);
        } else {
            current_page = (current_page + 1).min(total - 1);
        }

        press
            .create_response(
                ctx.serenity_context(),
                serenity::CreateInteractionResponse::UpdateMessage(
                    serenity::CreateInteractionResponseMessage::new()
                        .embed(voice_style_embed(&pages, current_page))
                        .components(vec![voice_style_buttons(
                            &prev_id,
                            &next_id,
                            current_page,
                            total,
                        )]),
                ),
            )
            .await?;
    }

    Ok(())
}

async fn autocomplete_voice_style<'a>(
    ctx: poise::ApplicationContext<'_, Data, Error>,
    partial: &'a str,
) -> Vec<serenity::builder::AutocompleteChoice> {
    ctx.data()
        .voice_styles
        .iter()
        .filter(move |s| partial.is_empty() || s.display_label.contains(partial))
        .take(25)
        .map(|s| serenity::builder::AutocompleteChoice::new(s.display_label.clone(), s.style_id))
        .collect()
}

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

async fn autocomplete_permission<'a>(
    _ctx: Context<'_>,
    partial: &'a str,
) -> impl Iterator<Item = serenity::builder::AutocompleteChoice> + 'a {
    [
        ("サーバーの管理（manage_guild）", "manage_guild"),
        ("管理者（administrator）", "administrator"),
    ]
    .into_iter()
    .filter(move |(label, _)| partial.is_empty() || label.contains(partial))
    .map(|(label, value)| serenity::builder::AutocompleteChoice::new(label, value))
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
    #[min = 0]
    #[max = 3]
    reply_type: i32,
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
        m.default_pitch = Set(Some(intonation));
    })
    .await?;

    ctx.send(
        poise::CreateReply::default().ephemeral(true).embed(
            serenity::CreateEmbed::new()
                .description(format!(
                    "サーバーのデフォルト音高を`{:.2}`に設定しました。",
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

async fn autocomplete_server_settings<'a>(
    _ctx: Context<'_>,
    partial: &'a str,
) -> impl Iterator<Item = serenity::builder::AutocompleteChoice> + 'a {
    BOOL_SERVER_SETTINGS
        .iter()
        .filter(move |(_, label)| partial.is_empty() || label.contains(partial))
        .map(|(key, label)| serenity::builder::AutocompleteChoice::new(*label, *key))
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
        .find(|(k, _)| *k == setting.as_str())
        .map(|(_, l)| *l);

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

#[poise::command(
    slash_command,
    subcommands("us_speaker", "us_pitch", "us_speed", "us_intonation", "us_reset")
)]
pub async fn user_setting(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

#[poise::command(slash_command, rename = "speaker")]
async fn us_speaker(
    ctx: Context<'_>,
    #[autocomplete = "autocomplete_voice_style"] style_id: u32,
) -> Result<(), Error> {
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
                        "ID `{}` は存在しません。/voice_stylesで確認してください。",
                        style_id
                    ))
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
    let user_id = ctx.author().id.get() as i64;

    upsert_user_setting(&ctx.data().db, guild_id, user_id, |m| {
        m.speaker_id = Set(Some(style_id as i32));
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
                .description(format!("話者を `{}` に設定しました。", label))
                .color(colors::SUCCEED),
        ),
    )
    .await?;

    Ok(())
}

#[poise::command(slash_command, guild_only, rename = "speed")]
async fn us_speed(
    ctx: Context<'_>,
    #[description = "速度（0.50 〜 2.00）"]
    #[min = 0.5]
    #[max = 2.0]
    speed: f32,
) -> Result<(), Error> {
    use sea_orm::ActiveValue::Set;

    let guild_id = ctx
        .guild_id()
        .ok_or("サーバー内でのみ実行可能です。")?
        .get() as i64;
    let user_id = ctx.author().id.get() as i64;

    upsert_user_setting(&ctx.data().db, guild_id, user_id, |m| {
        m.speed = Set(Some(speed));
    })
    .await?;

    ctx.send(
        poise::CreateReply::default().ephemeral(true).embed(
            serenity::CreateEmbed::new()
                .description(format!("速度を `{:.2}` に設定しました。", speed))
                .color(colors::SUCCEED),
        ),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command, rename = "pitch")]
async fn us_pitch(
    ctx: Context<'_>,
    #[description = "音高（-0.15 〜 0.15）"]
    #[min = -0.15]
    #[max = 0.15]
    pitch: f32,
) -> Result<(), Error> {
    use sea_orm::ActiveValue::Set;

    let guild_id = ctx
        .guild_id()
        .ok_or("サーバー内でのみ実行可能です。")?
        .get() as i64;
    let user_id = ctx.author().id.get() as i64;

    upsert_user_setting(&ctx.data().db, guild_id, user_id, |m| {
        m.pitch = Set(Some(pitch));
    })
    .await?;

    ctx.send(
        poise::CreateReply::default().ephemeral(true).embed(
            serenity::CreateEmbed::new()
                .description(format!("音高を `{:.2}` に設定しました。", pitch))
                .color(colors::SUCCEED),
        ),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command, guild_only, rename = "intonation")]
async fn us_intonation(
    ctx: Context<'_>,
    #[description = "抑揚（0.00 〜 2.00）"]
    #[min = 0.0]
    #[max = 2.0]
    intonation: f32,
) -> Result<(), Error> {
    use sea_orm::ActiveValue::Set;

    let guild_id = ctx
        .guild_id()
        .ok_or("サーバー内でのみ実行可能です。")?
        .get() as i64;
    let user_id = ctx.author().id.get() as i64;

    upsert_user_setting(&ctx.data().db, guild_id, user_id, |m| {
        m.intonation = Set(Some(intonation));
    })
    .await?;

    ctx.send(
        poise::CreateReply::default().ephemeral(true).embed(
            serenity::CreateEmbed::new()
                .description(format!("抑揚を `{:.2}` に設定しました。", intonation))
                .color(colors::SUCCEED),
        ),
    )
    .await?;
    Ok(())
}

#[poise::command(slash_command, guild_only, rename = "reset")]
async fn us_reset(ctx: Context<'_>) -> Result<(), Error> {
    use sea_orm::ActiveValue::Set;

    let guild_id = ctx
        .guild_id()
        .ok_or("サーバー内でのみ実行可能です。")?
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

#[poise::command(slash_command, subcommands("connect"))]
pub async fn vc(_: Context<'_>) -> Result<(), Error> {
    Ok(())
}

#[poise::command(slash_command)]
pub async fn connect(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("This command is usable only in a guild.")?;

    let user_voice_state = ctx
        .guild()
        .and_then(|g| g.voice_states.get(&ctx.author().id).cloned());
    let connect_channel_id = match user_voice_state.and_then(|v| v.channel_id) {
        Some(id) => id,
        None => {
            ctx.send(
                poise::CreateReply::default().embed(
                    serenity::CreateEmbed::new()
                        .description("コマンドを使用するにはVCに参加してください。")
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
            format!("<#{}>", ctx.channel_id().get()),
            false,
        );

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}

async fn join_vc(
    ctx: Context<'_>,
    guild_id: serenity::GuildId,
    channel_id: serenity::ChannelId,
) -> Result<(), Error> {
    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("failed to initialize songbird");
    let _handler = manager.join(guild_id, channel_id).await;

    let mut map = ctx.data().voice_to_text_map.write().await;
    map.insert(
        channel_id,
        VoiceContextInfo {
            command_channel: ctx.channel_id(),
            text_channels: std::collections::HashSet::from([ctx.channel_id()]),
        },
    );

    let bot_name = ctx.cache().current_user().name.clone();
    let text = format!("{}が参加しました", bot_name);
    play_voicevox(ctx.serenity_context(), guild_id, &text, ctx.data(), None).await?;
    Ok(())
}

#[poise::command(slash_command)]
async fn age(
    ctx: Context<'_>,
    #[description = "Selected user"] user: Option<serenity::User>,
) -> Result<(), Error> {
    let u = user.as_ref().unwrap_or_else(|| ctx.author());
    let response = format!("{} account was created at {}", u.name, u.created_at());
    ctx.say(response).await?;
    Ok(())
}

async fn on_ready(
    _ctx: &serenity::Context,
    data_about_bot: &serenity::Ready,
    data: &Data,
) -> Result<(), Error> {
    let all_settings = db::guild_settings::Entity::find().all(&data.db).await?;

    let mut cache = data.guild_settings_cache.write().await;
    for settings in all_settings {
        let guild_id = serenity::GuildId::new(settings.guild_id as u64);
        cache.insert(guild_id, settings);
    }

    tracing::info!("ready, logged in as {}", data_about_bot.user.name);
    Ok(())
}

async fn on_message(
    ctx: &serenity::Context,
    new_message: &serenity::Message,
    data: &Data,
) -> Result<(), Error> {
    if new_message.author.bot {
        return Ok(());
    }
    let guild_id = match new_message.guild_id {
        Some(id) => id,
        None => return Ok(()),
    };

    let text_channel_id = new_message.channel_id;
    let mut is_target = false;
    {
        let map = data.voice_to_text_map.read().await;
        for info in map.values() {
            if info.text_channels.contains(&text_channel_id) {
                is_target = true;
                break;
            }
        }
    }

    if !is_target {
        return Ok(());
    }

    let skip_server_muted_user = true;

    if skip_server_muted_user {
        let is_server_muted = if let Some(guild) = ctx.cache.guild(guild_id) {
            guild
                .voice_states
                .get(&new_message.author.id)
                .map(|vs| vs.mute)
                .unwrap_or(false)
        } else {
            false
        };

        if is_server_muted {
            return Ok(());
        }
    }

    if new_message.content == "s" {
        let manager = songbird::get(ctx)
            .await
            .expect("failed to initialize songbird");

        if let Some(call_lock) = manager.get(guild_id) {
            let call = call_lock.lock().await;
            let queue = call.queue();

            if queue.current().is_some() {
                let _ = queue.skip();

                let reaction = serenity::ReactionType::Unicode("⏭️".to_string());
                if let Err(why) = new_message.react(&ctx.http, reaction).await {
                    tracing::error!(?why, "failed to add reaction");
                }
            }
        }
        return Ok(());
    }

    let mut text_to_read = format_message(new_message, ctx);
    text_to_read = sanitize_text(&text_to_read);
    if !text_to_read.is_empty() {
        play_voicevox(
            ctx,
            guild_id,
            &text_to_read,
            data,
            Some(new_message.author.id),
        )
        .await?;
    }
    Ok(())
}

async fn on_voice_state_update(
    ctx: &serenity::Context,
    old: &Option<serenity::VoiceState>,
    new: &serenity::VoiceState,
    data: &Data,
) -> Result<(), Error> {
    if new.user_id == ctx.cache.current_user().id {
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

    let member_name = member.display_name();
    let mut should_check_auto_disconnect = false;

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
                if new_id.get() == bot_channel_id.get() {
                    guild_settings
                        .read_vc_join
                        .then(|| format!("{}が参加しました", member_name))
                } else {
                    should_check_auto_disconnect = true;
                    guild_settings.read_vc_move.then(|| {
                        format!(
                            "{}が{}に参加しました",
                            member_name,
                            get_channel_name(new_id),
                        )
                    })
                }
            }
        }
        _ => None,
    };

    if let Some(text) = text_to_read {
        play_voicevox(ctx, guild_id, &text, data, Some(new.user_id)).await?;
    }

    if !should_check_auto_disconnect {
        return Ok(());
    }

    let voice_to_text_map = data.voice_to_text_map.read().await;
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
                drop(voice_to_text_map);
                manager.remove(guild_id).await.ok();
                let mut map = data.voice_to_text_map.write().await;
                map.remove(&channel_id);
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to initialize crypto provider");

    let file_appender = rolling::daily("./logs", "kikisen-yoiyomi.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .with(fmt::layer().with_writer(std::io::stdout))
        .with(fmt::layer().with_writer(non_blocking).with_ansi(false))
        .init();

    tracing::info!("initialized logging system");

    dotenv().ok();
    // Login with a bot token from the environment
    let token = env::var("TOKEN_YOMIYOMI").unwrap_or_else(|e| {
        tracing::error!(error = ?e, "expected a token in the environment");
        std::process::exit(1);
    });
    // Set gateway intents, which decides what events the bot will be notified about
    //let intents = serenity::GatewayIntents::non_privileged()
    //    | serenity::GatewayIntents::MESSAGE_CONTENT;
    let intents = serenity::GatewayIntents::all();

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![
                age(),
                vc(),
                restart(),
                play(),
                skip(),
                volume(),
                user_setting(),
                voice_styles(),
                server_setting(),
                server_settings(),
            ],
            prefix_options: poise::PrefixFrameworkOptions {
                dynamic_prefix: Some(get_command_prefix),
                ..Default::default()
            },
            event_handler: |ctx, event, _framework, data| {
                Box::pin(async move {
                    match event {
                        serenity::FullEvent::Ready { data_about_bot } => {
                            on_ready(ctx, &data_about_bot, &data).await?;
                        }
                        serenity::FullEvent::Message { new_message } => {
                            on_message(ctx, new_message, data).await?;
                        }
                        serenity::FullEvent::VoiceStateUpdate { old, new } => {
                            on_voice_state_update(ctx, old, new, data).await?;
                        }
                        &_ => {}
                    }
                    Ok(())
                })
            },
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;

                let db: DatabaseConnection = Database::connect("sqlite://database.db?mode=rwc")
                    .await
                    .expect("failed to connect to database");

                let builder = db.get_database_backend();
                let schema = Schema::new(DbBackend::Sqlite);

                let stmt_guild =
                    builder.build(&schema.create_table_from_entity(db::guild_settings::Entity));
                let _ = db.execute(stmt_guild).await;

                let stmt_user =
                    builder.build(&schema.create_table_from_entity(db::user_settings::Entity));
                let _ = db.execute(stmt_user).await;

                let synthesizer = Synthesizer::builder(
                    Onnxruntime::load_once()
                        .filename(ONNXRUNTIME_FILENAME)
                        .perform()
                        .await?,
                )
                .text_analyzer(OpenJtalk::new(OPEN_JTALK_DIR).await.unwrap())
                .acceleration_mode(ACCELERATION_MODE)
                .build()?;

                let mut entries = tokio::fs::read_dir(VVMS_DIR)
                    .await
                    .expect("vvm directory not found");
                let mut voice_styles = Vec::new();
                while let Some(entry) = entries.next_entry().await? {
                    let path = entry.path();

                    if path.extension().and_then(|s| s.to_str()) == Some("vvm") {
                        tracing::info!("loading vvm: {:?}", path.file_name());
                        let model = VoiceModelFile::open(&path).await?;
                        let _ = synthesizer.load_voice_model(&model).perform().await?;

                        for character in model.metas() {
                            for style in &character.styles {
                                let style_id: u32 = style.id.to_string().parse().unwrap_or(0);
                                voice_styles.push(VoiceStyleInfo {
                                    character_name: character.name.clone(),
                                    style_name: style.name.clone(),
                                    style_id,
                                    display_label: format!("{}（{}）", character.name, style.name),
                                });
                            }
                        }
                    }
                }
                voice_styles.sort_by_key(|s| s.style_id);

                Ok(Data {
                    voice_to_text_map: Arc::new(RwLock::new(HashMap::new())),
                    music_state: Arc::new(RwLock::new(MusicState {
                        queue: std::collections::VecDeque::new(),
                        current_track: None,
                        volume: 0.1,
                    })),
                    synthesizer: Arc::new(synthesizer),
                    db,
                    voice_styles,
                    guild_settings_cache: Arc::new(RwLock::new(HashMap::new())),
                })
            })
        })
        .build();

    // Create a new instance of the Client, logging in as a bot.
    let mut client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .register_songbird()
        .await
        .expect("error creating client");

    if let Err(why) = client.start().await {
        tracing::error!(?why, "client error");
    }
}
