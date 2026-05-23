use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use poise::serenity_prelude as serenity;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;

pub const DEVELOPPER_ID: i64 = 824257607052689428;
pub const DEFAULT_SPEAKER_ID: i32 = 8;
pub const DEFAULT_PREFIX: &str = "!";

#[allow(dead_code)]
pub mod colors {
    pub const BOT: u32 = 0x99aab5;
    pub const INFO: u32 = 0x5865f2;
    pub const SUCCEED: u32 = 0x57F287;
    pub const WARN: u32 = 0xE67E22;
    pub const ERROR: u32 = 0xed4245;
}

pub struct VoiceContextInfo {
    pub command_channel: serenity::ChannelId,
    pub text_channels: HashSet<serenity::ChannelId>,
}

pub struct VoiceStyleInfo {
    pub character_name: String,
    pub style_name: String,
    pub style_id: u32,
    pub display_label: String,
}

pub struct Data {
    pub db: sea_orm::DatabaseConnection,
    pub synthesizer: Arc<voicevox_core::nonblocking::Synthesizer<voicevox_core::nonblocking::OpenJtalk>>,
    pub voice_styles: Vec<VoiceStyleInfo>,
    pub voice_to_text_map: Arc<RwLock<HashMap<serenity::ChannelId, VoiceContextInfo>>>,
    pub music_state: Arc<RwLock<HashMap<serenity::GuildId, Arc<RwLock<crate::music::MusicState>>>>>,
    pub guild_settings_cache: Arc<RwLock<HashMap<serenity::GuildId, crate::db::guild_settings::Model>>>,
    pub kanalizer: kanalizer::Kanalizer,
}
