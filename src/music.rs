use crate::types::{Error, VoiceContextInfo};
use poise::serenity_prelude as serenity;
use songbird::events::{Event, EventContext, EventHandler, TrackEvent};
use std::{collections::{HashMap, VecDeque}, sync::Arc};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct MusicItem {
    pub url: String,
    pub title: String,
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
