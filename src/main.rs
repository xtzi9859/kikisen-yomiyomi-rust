use async_trait::async_trait;
use dotenvy::dotenv;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::env;
use std::sync::{Arc, LazyLock};
use tempfile::Builder;
use tokio::sync::RwLock;

use tracing;
use tracing_appender::rolling;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use poise::serenity_prelude as serenity;
use songbird::SerenityInit;
use songbird::events::{Event, EventContext, EventHandler as VoiceEventHandler, TrackEvent};

use unicode_segmentation::UnicodeSegmentation;

pub struct VoiceContextInfo {
    pub command_channel: serenity::ChannelId,
    pub text_channels: HashSet<serenity::ChannelId>,
}
struct Data {
    pub voice_to_text_map: Arc<RwLock<HashMap<serenity::ChannelId, VoiceContextInfo>>>,
}
#[derive(Clone)]
struct FileDeleter {
    _temp_file_path: Arc<tempfile::TempPath>,
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

    text = CHANNEL_MENTION_REGEX.replace_all(&text, |caps: &regex::Captures| {
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
    }).into_owned();

    text = CUSTOM_EMOJI_REGEX.replace_all(&text, "$1").into_owned();

    let mut demojized_text = String::new();
    for grapheme in text.graphemes(true) {
        if let Some(emoji) = emoji::lookup_by_glyph::lookup(grapheme) {
            let ja_name = emoji.annotations.iter()
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
        let sticker_names: Vec<String> = message.sticker_items.iter()
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

async fn play_voicevox(
    ctx: &serenity::Context,
    guild_id: serenity::GuildId,
    text: &str,
) -> Result<(), Error> {
    let client = reqwest::Client::new();
    let query_url = format!(
        "http://192.168.0.3:50021/audio_query?speaker=1&text={}",
        urlencoding::encode(&text)
    );

    let query_response = client.post(&query_url).send().await?.text().await?;

    let synthesis_url = "http://192.168.0.3:50021/synthesis?speaker=1";
    let audio_bytes = client
        .post(synthesis_url)
        .header("Content-Type", "application/json")
        .body(query_response)
        .send()
        .await?
        .bytes()
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

#[poise::command(slash_command)]
async fn connect(ctx: Context<'_>) -> Result<(), Error> {
    let (guild_id, voice_channel_id) = {
        let guild = ctx
            .guild()
            .ok_or("this command is usable only in guilds.")?;
        let channel_id = guild
            .voice_states
            .get(&ctx.author().id)
            .and_then(|voice_state| voice_state.channel_id);
        (guild.id, channel_id)
    };

    let connect_to = match voice_channel_id {
        Some(channel) => channel,
        None => {
            ctx.say("you need to join a VC before using this command")
                .await?;
            return Ok(());
        }
    };

    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("failed to initialize songbird")
        .clone();

    if let Ok(_) = manager.join(guild_id, connect_to).await {
        ctx.say(format!("connected <#{}>", connect_to)).await?;
        let text_channel_id = ctx.channel_id();
        {
            let mut map = ctx.data().voice_to_text_map.write().await;
            let info = map.entry(connect_to).or_insert_with(|| VoiceContextInfo {
                command_channel: text_channel_id,
                text_channels: HashSet::new(),
            });

            info.command_channel = text_channel_id;
            info.text_channels.insert(text_channel_id);

            play_voicevox(
                ctx.serenity_context(),
                guild_id,
                "聞き専読み読みくんが参加しました",
            )
            .await?;
        }
    } else {
        ctx.say("failed to connect VC").await?;
    }

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

async fn on_ready(_ctx: &serenity::Context, data_about_bot: &serenity::Ready) -> Result<(), Error> {
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
    if new_message.content == "!ping" {
        if let Err(why) = new_message.channel_id.say(&ctx.http, "pong!").await {
            tracing::error!(?why, "error sending message");
        }
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
        play_voicevox(ctx, guild_id, &text_to_read).await?;
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

    let get_channel_name = |chan_id: serenity::ChannelId| -> String {
        if let Some(guild) = ctx.cache.guild(guild_id) {
            if let Some(channel) = guild.channels.get(&chan_id) {
                return channel.name.clone();
            }
        }
        "不明なチャンネル".to_string()
    };

    let member_name = member.display_name();

    let text_to_read = match (old_channel_id, new_channel_id) {
        (None, Some(new_id)) => {
            if new_id.get() == bot_channel_id.get() {
                Some(format!("{}が参加しました", member_name))
            } else {
                let chan_name = get_channel_name(new_id);
                Some(format!("{}が{}に参加しました", member_name, chan_name))
            }
        }
        (Some(old_id), None) => {
            if old_id.get() == bot_channel_id.get() {
                Some(format!("{}が退出しました", member_name))
            } else {
                let chan_name = get_channel_name(old_id);
                Some(format!("{}が{}から退出しました", member_name, chan_name))
            }
        }
        (Some(old_id), Some(new_id)) => {
            if old_id == new_id {
                let old_stream = old.as_ref().and_then(|s| s.self_stream).unwrap_or(false);
                let new_stream = new.self_stream.unwrap_or(false);

                let old_video = old.as_ref().map(|s| s.self_video).unwrap_or(false);
                let new_video = new.self_video;

                if !old_stream && new_stream {
                    Some(format!("{}が配信を開始しました", member_name))
                } else if !old_video && new_video {
                    Some(format!("{}がカメラをオンにしました", member_name))
                } else {
                    None
                }
            } else {
                if new_id.get() == bot_channel_id.get() {
                    Some(format!("{}が参加しました", member_name))
                } else {
                    let chan_name = get_channel_name(new_id);
                    Some(format!("{}が{}に参加しました", member_name, chan_name))
                }
            }
        }
        _ => None
    };

    if let Some(text) = text_to_read {
        play_voicevox(ctx, guild_id, &text).await?;
    }

    let member_count = {
        let mut count = 0;
        if let Some(guild) = ctx.cache.guild(guild_id) {
            for (user_id, state) in &guild.voice_states {
                if state.channel_id.map(|c| c.get()) == Some(bot_channel_id.into()) {
                    let is_bot = ctx.cache.user(*user_id).map(|u| u.bot).unwrap_or(false);
                    if !is_bot {
                        count += 1;
                    }
                }
            }
        }
        count
    };

    if member_count == 0 {
        let _ = manager.remove(guild_id).await;

        let command_channel = {
            let mut map = data.voice_to_text_map.write().await;
            map.remove(&serenity::ChannelId::from(bot_channel_id))
                .map(|info| info.command_channel)
        };

        if let Some(channel_id) = command_channel {
            let _ = channel_id
                .say(
                    &ctx.http,
                    "No users left in the voice channel; automatically disconnected.",
                )
                .await;
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
            commands: vec![age(), connect(), restart()],
            event_handler: |ctx, event, _framework, data| {
                Box::pin(async move {
                    match event {
                        serenity::FullEvent::Ready { data_about_bot } => {
                            on_ready(ctx, &data_about_bot).await?;
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
                Ok(Data {
                    voice_to_text_map: Arc::new(RwLock::new(HashMap::new())),
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
