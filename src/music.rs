use crate::types::{Error, VoiceContextInfo, colors};
use poise::serenity_prelude as serenity;
use songbird::events::{Event, EventContext, EventHandler, TrackEvent};
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct MusicItem {
    pub url: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration: Option<u64>,
    pub thumbnail: Option<String>,
    pub is_ytdl: bool,
}

pub struct MusicState {
    pub queue: VecDeque<MusicItem>,
    pub current_track: Option<songbird::tracks::TrackHandle>,
    pub volume: f32,
}

pub struct MusicEndHandler {
    ctx: serenity::Context,
    guild_id: serenity::GuildId,
    music_state: Arc<RwLock<MusicState>>,
    voice_to_text_map: Arc<RwLock<HashMap<serenity::ChannelId, VoiceContextInfo>>>,
}

#[async_trait::async_trait]
impl EventHandler for MusicEndHandler {
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

/// 秒数をm:ss形式の文字列に変換する
pub fn format_duration(sec: u64) -> String {
    let h = sec / 3600;
    let m = (sec % 3600) / 60;
    let s = sec % 60;

    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

pub fn build_now_playing_embed(item: &MusicItem) -> serenity::CreateEmbed {
    let mut description = item.title.clone();
    if let Some(artist) = &item.artist {
        description.push_str(&format!(" ― {}", artist));
    }
    if let Some(album) = &item.album {
        description.push_str(&format!("（{}）", album));
    }

    let mut embed = serenity::CreateEmbed::new()
        .title("再生開始")
        .description(description)
        .color(colors::BOT);

    if let Some(thumb) = &item.thumbnail {
        embed = embed.thumbnail(thumb.clone());
    }

    if let Some(dur) = item.duration {
        embed = embed.footer(serenity::CreateEmbedFooter::new(format!(
            "{}",
            format_duration(dur)
        )));
    }

    embed
}

pub async fn play_next_music(
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
                let embed = build_now_playing_embed(&next_item);
                let _ = channel
                    .send_message(&ctx.http, serenity::CreateMessage::new().embed(embed))
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
