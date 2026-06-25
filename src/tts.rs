use crate::db;
use crate::types::{DEFAULT_SPEAKER_ID, Data, Error};
use poise::serenity_prelude as serenity;
use regex::Regex;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use songbird::events::{Event, EventContext, EventHandler, TrackEvent};
use std::sync::{Arc, LazyLock};
use tempfile::Builder;
use unicode_segmentation::UnicodeSegmentation;

const MAX_SYNTHESIS_LENGTH: usize = 150;

pub(crate) static URL_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://\S+").expect("failed to compile regex url"));
pub(crate) static CODEBLOCK_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)```(?P<lang>[^\n\s]*)\s*\n?.*?```")
        .expect("failed to compile regex: codeblock")
});
//pub(crate) static INLINE_CODE_REGEX: LazyLock<Regex> =
//    LazyLock::new(|| Regex::new(r"`([^`]+)`").expect("failed to compile regex inline-code"));
pub(crate) static SPOILER_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\|\|(.*?)\|\|").expect("failed to compile regex: spoiler"));
pub(crate) static QUOTE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^>{1,3}\s?").expect("failed to compile regex: quote"));
pub(crate) static ROLE_MENTION_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<@&(\d+)>").expect("failed to compile regex: role-mention"));
pub(crate) static CHANNEL_MENTION_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<#(\d+)>").expect("failed to compile regex: channel-mention"));
pub(crate) static CUSTOM_EMOJI_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<a?:(\w+):\d+>").expect("failed to compile regex: custom-emoji"));
pub(crate) static ENGLISH_WORD_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[a-zA-Z]+").expect("failed to compile regex: english"));

#[derive(Clone)]
pub struct FileDeleter {
    _temp_file_path: Arc<tempfile::TempPath>,
}

#[async_trait::async_trait]
impl EventHandler for FileDeleter {
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        None
    }
}

pub async fn play_voicevox(
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

/// discordのメッセージを読み上げに適した形に整形する
pub fn format_message(
    message: &serenity::Message,
    ctx: &serenity::Context,
    reply_prefix_type: i32,
) -> String {
    let mut text = message.content.clone();
    let mut prefix = String::new();

    if let Some(ref referenced) = message.referenced_message {
        // TODO: reply_prefix_typeが0, 1のときもauthor_nameを取得するのは無駄なので3, 4のときだけ取得するように改良する
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

        let prefix_text = match reply_prefix_type {
            0 => String::new(),
            1 => "返信 ".to_string(),
            2 => format!("{}への返信 ", author_name),
            3 => {
                let raw = format_message(referenced, ctx, 0);
                let sanitized = sanitize_text(&raw);
                let content_preview: String = sanitized.chars().take(20).collect();
                if content_preview.is_empty() {
                    format!("{}への返信 ", author_name)
                } else {
                    format!("{}の「{}」への返信 ", author_name, content_preview)
                }
            }
            _ => format!("{}への返信", author_name),
        };
        prefix.push_str(&prefix_text);
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

/// 正規表現を使ってmarkdown記号のパースをする
pub fn sanitize_text(text: &str) -> String {
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
    URL_REGEX.replace_all(&result, "URL").into_owned()
}

/// kanalizerを使用してテキスト中の英単語を一括でかなに変換する
pub fn apply_kanalizer(text: &str, kanalizer: &kanalizer::Kanalizer) -> String {
    let kanalizer_options = kanalizer::ConvertOptions {
        max_length: kanalizer::MaxLength::Auto,
        strategy: kanalizer::Strategy::Greedy,
        error_on_invalid_input: false,
        error_on_incomplete: true,
    };

    ENGLISH_WORD_REGEX
        .replace_all(text, |caps: &regex::Captures| {
            let word = &caps[0];

            if word.chars().all(|c| c.is_uppercase()) {
                return word.to_string();
            }

            kanalizer
                .convert(&caps[0].to_lowercase(), &kanalizer_options)
                .unwrap_or_else(|_| caps[0].to_string())
        })
        .into_owned()
}

pub fn split_text_for_synthesis(text: &str) -> Vec<String> {
    text.split(&['。', '、', '？', '！', '.', ',', '?', '!', '\n', '\r'][..])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s.chars().count() > MAX_SYNTHESIS_LENGTH {
                let truncated: String = s.chars().take(MAX_SYNTHESIS_LENGTH).collect();
                format!("{} 省略", truncated)
            } else {
                s.to_string()
            }
        })
        .collect()
}
