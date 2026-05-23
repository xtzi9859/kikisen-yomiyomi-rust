mod commands;
mod db;
mod events;
mod helpers;
mod music;
mod tts;
mod types;

use poise::serenity_prelude as serenity;
use sea_orm::{ConnectionTrait, Database, DatabaseConnection,DbBackend, Schema};
use std::{collections::HashMap, sync::Arc};
use songbird::SerenityInit;
use tracing_appender::rolling;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use voicevox_core::nonblocking::{Onnxruntime, OpenJtalk, Synthesizer, VoiceModelFile};
use tokio::sync::RwLock;
use types::{Data, VoiceStyleInfo};

const OPEN_JTALK_DIR: &str = "./voicevox_core/dict/open_jtalk_dic_utf_8-1.11";
const ONNXRUNTIME_FILENAME: &str =
    "./voicevox_core/onnxruntime/lib/libvoicevox_onnxruntime.so.1.17.3";
const ACCELERATION_MODE: voicevox_core::AccelerationMode = voicevox_core::AccelerationMode::Cpu;
const VVMS_DIR: &str = "./voicevox_core/models/vvms";

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

    dotenvy::dotenv().ok();
    let token = std::env::var("TOKEN_YOMIYOMI").unwrap_or_else(|e| {
        tracing::error!(error = ?e, "expected a token in the environment");
        std::process::exit(1);
    });
    let intents = serenity::GatewayIntents::all();

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![
                commands::age(),
                commands::auto_connect(),
                commands::vc(),
                commands::restart(),
                commands::play(),
                commands::skip(),
                commands::volume(),
                commands::user_setting(),
                commands::voice_styles(),
                commands::server_setting(),
                commands::server_settings(),
                commands::server_voice(),
                commands::bot_whitelist(),
            ],
            prefix_options: poise::PrefixFrameworkOptions {
                dynamic_prefix: Some(helpers::get_command_prefix),
                ..Default::default()
            },
            event_handler: |ctx, event, _framework, data| {
                Box::pin(async move {
                    match event {
                        serenity::FullEvent::Ready { data_about_bot } => {
                            events::voice::on_ready(ctx, &data_about_bot, &data).await?;
                        }
                        serenity::FullEvent::Message { new_message } => {
                            events::message::on_message(ctx, new_message, data).await?;
                        }
                        serenity::FullEvent::VoiceStateUpdate { old, new } => {
                            events::voice::on_voice_state_update(ctx, old, new, data).await?;
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

                let stmt_bot_whitelist =
                    builder.build(&schema.create_table_from_entity(db::bot_whitelist::Entity));
                let _ = db.execute(stmt_bot_whitelist).await;

                let stmt_auto_connect = builder.build(&schema.create_table_from_entity(db::auto_connections::Entity));
                let _ = db.execute(stmt_auto_connect).await;

                let stmt_reading_targets = builder.build(&schema.create_table_from_entity(db::reading_targets::Entity));
                let _ = db.execute(stmt_reading_targets).await;

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
                    db,
                    synthesizer: Arc::new(synthesizer),
                    voice_styles,
                    voice_to_text_map: Arc::new(RwLock::new(HashMap::new())),
                    music_state: Arc::new(RwLock::new(HashMap::new())),
                    guild_settings_cache: Arc::new(RwLock::new(HashMap::new()),),
                    kanalizer: kanalizer::Kanalizer::new(),
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

    client.start().await.expect("failed to start client");
}
