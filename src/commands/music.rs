use crate::helpers::{get_guild_settings, upsert_guild_setting};
use crate::music::{MusicItem, MusicState, play_next_music};
use crate::types::{Context, Error, Data};
use poise::serenity_prelude as serenity;
use sea_orm::ActiveValue::Set;
use std::{collections::VecDeque, sync::Arc};
use tokio::sync::RwLock;

#[derive(serde::Deserialize)]
pub(crate) struct YtdlOutput {
    title: String,
    webpage_url: String,
}

async fn get_guild_music_state(
    data: &Data,
    guild_id: serenity::GuildId,
) -> Arc<RwLock<MusicState>> {
    {
        let states = data.music_state.read().await;
        if let Some(state) = states.get(&guild_id) {
            return state.clone();
        }
    }

    let initial_vol = get_guild_settings(data, guild_id).await.default_music_vol;

    let mut states = data.music_state.write().await;
    states
        .entry(guild_id)
        .or_insert_with(|| {
            Arc::new(RwLock::new(MusicState {
                queue: VecDeque::new(),
                current_track: None,
                volume: initial_vol,
            }))
        })
        .clone()
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

        let state_arc = get_guild_music_state(ctx.data(), guild_id).await;
        let should_play = {
            let mut state = state_arc.write().await;
            state.queue.push_back(item);
            state.current_track.is_none()
        };

        if should_play {
            play_next_music(
                ctx.serenity_context(),
                guild_id,
                state_arc,
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

#[poise::command(prefix_command, aliases("s"))]
pub async fn skip(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("サーバー内でのみ実行可能です。")?;
    let state_arc = get_guild_music_state(ctx.data(), guild_id).await;
    let state = state_arc.read().await;
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
    let guild_id = ctx.guild_id().ok_or("サーバー内でのみ実行可能です。")?;
    if vol_input < 0.0 || vol_input > 100.0 {
        let _ = ctx.reply("音量は0～100の範囲内で入力してください。").await;
        return Ok(());
    }

    let actual_vol = (vol_input / 100.0).clamp(0.0, 1.0);
    let state_arc = get_guild_music_state(ctx.data(), guild_id).await;
    let mut state = state_arc.write().await;

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

    ctx.say(format!("音量を`{}`に設定しました。", vol_input.clamp(0.0, 100.0)))
        .await?;
    Ok(())
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
