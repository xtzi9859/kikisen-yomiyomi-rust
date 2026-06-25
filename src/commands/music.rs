use crate::helpers::{get_guild_settings, upsert_guild_setting};
use crate::music::{MusicItem, MusicState, format_duration, play_next_music};
use crate::types::{Context, Data, Error, colors};
use poise::serenity_prelude as serenity;
use sea_orm::ActiveValue::Set;
use std::{collections::VecDeque, sync::Arc, time::Duration};
use tokio::sync::RwLock;

#[derive(serde::Deserialize)]
pub(crate) struct YtdlOutput {
    title: String,
    #[serde(default)]
    webpage_url: String,
    #[serde(default)]
    uploader: Option<String>,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    thumbnail: Option<String>,
    #[serde(default)]
    upload_date: Option<String>,
}

#[derive(serde::Deserialize)]
struct FfprobeOutput {
    #[serde(default)]
    format: FfprobeFormat,
}

#[derive(serde::Deserialize, Default)]
struct FfprobeFormat {
    #[serde(default)]
    duration: Option<String>,
    #[serde(default)]
    tags: Option<FfprobeTags>,
}

#[derive(serde::Deserialize, Default)]
struct FfprobeTags {
    #[serde(alias = "TITLE", alias = "Title")]
    title: Option<String>,
    #[serde(alias = "ARTIST", alias = "Artist")]
    artist: Option<String>,
    #[serde(alias = "ALBUM", alias = "Album")]
    album: Option<String>,
    #[serde(alias = "DATE", alias = "Date", alias = "YEAR", alias = "Year")]
    date: Option<String>,
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

    if !get_guild_settings(&ctx.data(), guild_id)
        .await
        .music_enabled
    {
        ctx.send(
            poise::CreateReply::default().embed(
                serenity::CreateEmbed::new()
                    .description("音楽再生機能はこのサーバーでは無効化されています。")
                    .color(colors::ERROR),
            ),
        )
        .await?;

        return Ok(());
    }

    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("failed to initialize songbird");
    if manager.get(guild_id).is_none() {
        ctx.send(
            poise::CreateReply::default().embed(
                serenity::CreateEmbed::new()
                    .description("botがVCに参加していません。")
                    .color(colors::WARN),
            ),
        )
        .await?;

        return Ok(());
    }

    let state_arc = get_guild_music_state(ctx.data(), guild_id).await;
    {
        let mut state = state_arc.write().await;
        if state.notify_channel.is_none() {
            state.notify_channel = Some(ctx.channel_id());
        }
    }

    let mut item_to_add = None;
    let mut message_to_delete = None;

    if let Some(attachment) = file {
        let (title, artist, album, release_year, duration, album_art) =
            get_localfile_metadata(&attachment.url).await;

        item_to_add = Some(MusicItem {
            url: attachment.url.clone(),
            title: title.unwrap_or_else(|| attachment.filename.clone()),
            artist,
            album,
            release_year,
            duration,
            albumart: album_art,
            thumbnail: None,
            is_ytdl: false,
        });
    } else if let Some(args) = query {
        let processing_msg = ctx.say("処理中…").await?;
        if args.starts_with("http") {
            if looks_like_playlist(&args) {
                match get_youtube_playlist_info(&args).await {
                    Ok(videos) => {
                        let items: Vec<MusicItem> =
                            videos.iter().map(music_item_from_ytdl).collect();

                        let (should_play, added_count) = {
                            let mut state = state_arc.write().await;
                            let should_play = state.current_track.is_none();
                            let added_count = items.len();
                            state.queue.extend(items);
                            (should_play, added_count)
                        };

                        processing_msg
                            .edit(
                                ctx,
                                poise::CreateReply::default().content(format!(
                                    "プレイリストから{}曲をキューに追加しました。",
                                    added_count
                                )),
                            )
                            .await?;

                        if should_play {
                            play_next_music(
                                ctx.serenity_context(),
                                guild_id,
                                state_arc,
                                ctx.data().voice_to_text_map.clone(),
                            )
                            .await?;
                        }
                    }

                    Err(e) => {
                        processing_msg
                            .edit(
                                ctx,
                                poise::CreateReply::default()
                                    .content(format!("プレイリストの取得に失敗しました。: {}", e)),
                            )
                            .await?;
                    }
                }

                return Ok(());
            }

            let info = match get_youtube_info(&args).await {
                Ok(info) => info,
                Err(_) => YtdlOutput {
                    title: "不明なタイトル".to_string(),
                    webpage_url: args.clone(),
                    uploader: None,
                    upload_date: None,
                    duration: None,
                    thumbnail: None,
                },
            };

            item_to_add = Some(music_item_from_ytdl(&info));
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

                let selected_item = search_results
                    .into_iter()
                    .find(|v| v.webpage_url == selected_url)
                    .map(|info| music_item_from_ytdl(&info))
                    .unwrap_or_else(|| MusicItem {
                        url: selected_url.clone(),
                        title: "Youtube Video".to_string(),
                        artist: None,
                        release_year: None,
                        album: None,
                        duration: None,
                        thumbnail: None,
                        albumart: None,
                        is_ytdl: true,
                    });

                mci.create_response(
                    ctx.serenity_context(),
                    serenity::CreateInteractionResponse::UpdateMessage(
                        serenity::CreateInteractionResponseMessage::new()
                            .content("動画を処理中…")
                            .components(vec![]),
                    ),
                )
                .await?;

                item_to_add = Some(selected_item);
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
        let should_play = {
            let mut state = state_arc.write().await;
            state.queue.push_back(item.clone());
            state.current_track.is_none()
        };

        if !should_play {
            let embed = build_queue_added_embed(&item);
            if let Some(art) = &item.albumart {
                let attachment = serenity::CreateAttachment::bytes(art.clone(), "albumart.jpg");
                ctx.channel_id()
                    .send_message(
                        ctx.serenity_context(),
                        serenity::CreateMessage::new()
                            .embed(embed.thumbnail("attachment://albumart.jpg"))
                            .add_file(attachment),
                    )
                    .await?;
            } else {
                ctx.send(poise::CreateReply::default().embed(embed)).await?;
            }
        }

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
pub async fn skip(ctx: Context<'_>, count: Option<u32>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?;

    if !get_guild_settings(&ctx.data(), guild_id)
        .await
        .music_enabled
    {
        ctx.send(
            poise::CreateReply::default().embed(
                serenity::CreateEmbed::new()
                    .description("音楽再生機能はこのサーバーでは無効化されています。")
                    .color(colors::ERROR),
            ),
        )
        .await?;

        return Ok(());
    }

    let state_arc = get_guild_music_state(ctx.data(), guild_id).await;
    let skip_count = count.unwrap_or(1).max(1) as usize;
    let mut state = state_arc.write().await;

    if state.current_track.is_none() {
        ctx.say("再生していません。").await?;
        return Ok(());
    }

    let to_drop = skip_count.saturating_sub(1).min(state.queue.len());
    for _ in 0..to_drop {
        state.queue.pop_front();
    }

    let actual_skipped = to_drop + 1;

    if let Some(handle) = &state.current_track {
        let _ = handle.stop();
    }

    drop(state);

    if actual_skipped < skip_count {
        ctx.say(format!(
            "キューの残りが少なかったので{}曲スキップしました。",
            actual_skipped,
        ))
        .await?;
    } else {
        ctx.say(format!("{}曲スキップしました。", actual_skipped))
            .await?;
    }

    Ok(())
}

#[poise::command(prefix_command, aliases("vol"))]
pub async fn volume(ctx: Context<'_>, vol_input: f32) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?;

    if !get_guild_settings(&ctx.data(), guild_id)
        .await
        .music_enabled
    {
        ctx.send(
            poise::CreateReply::default().embed(
                serenity::CreateEmbed::new()
                    .description("音楽再生機能はこのサーバーでは無効化されています。")
                    .color(colors::ERROR),
            ),
        )
        .await?;

        return Ok(());
    }

    if !vol_input.is_finite() || vol_input < 0.0 || vol_input > 100.0 {
        ctx.send(
            poise::CreateReply::default().reply(true).embed(
                serenity::CreateEmbed::new()
                    .description("音量は0～100の範囲で入力してください。")
                    .color(colors::WARN),
            ),
        )
        .await?;

        return Ok(());
    }

    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("failed to initialize songbird");
    if manager.get(guild_id).is_none() {
        ctx.send(
            poise::CreateReply::default().embed(
                serenity::CreateEmbed::new()
                    .description("botがVCに参加していません。")
                    .color(colors::WARN),
            ),
        )
        .await?;

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

    ctx.say(format!(
        "音楽の音量を`{}`に設定しました。",
        vol_input.clamp(0.0, 100.0)
    ))
    .await?;
    Ok(())
}

#[poise::command(prefix_command, aliases("ps", "resume", "unpause", "toggle", "tg"))]
pub async fn pause(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?;

    if !get_guild_settings(&ctx.data(), guild_id)
        .await
        .music_enabled
    {
        ctx.send(
            poise::CreateReply::default().embed(
                serenity::CreateEmbed::new()
                    .description("音楽再生機能はこのサーバーでは無効化されています。")
                    .color(colors::ERROR),
            ),
        )
        .await?;

        return Ok(());
    }

    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("failed to initialize songbird");
    if manager.get(guild_id).is_none() {
        ctx.send(
            poise::CreateReply::default().embed(
                serenity::CreateEmbed::new()
                    .description("botがVCに参加していません。")
                    .color(colors::WARN),
            ),
        )
        .await?;

        return Ok(());
    }

    let mut current_play_mode = None;
    let mut track_handle = None;

    {
        let state_arc = get_guild_music_state(ctx.data(), guild_id).await;
        let state = state_arc.read().await;
        if let Some(handle) = &state.current_track {
            track_handle = Some(handle.clone());
        }
    }

    if let Some(handle) = &track_handle {
        if let Ok(info) = handle.get_info().await {
            current_play_mode = Some(info.playing);
        }
    }

    let state_arc = get_guild_music_state(ctx.data(), guild_id).await;
    let mut _state = state_arc.write().await;

    match current_play_mode {
        Some(songbird::tracks::PlayMode::Play) => {
            if let Some(handle) = &track_handle {
                let _ = handle.pause();
                ctx.say("音楽を一時停止しました。").await?;
            }
        }

        Some(songbird::tracks::PlayMode::Pause) => {
            if let Some(handle) = &track_handle {
                let _ = handle.play();
                ctx.say("音楽を再開しました。").await?;
            }
        }

        _ => {
            ctx.say("音楽を再生していません。").await?;
        }
    }

    Ok(())
}

#[poise::command(prefix_command)]
pub async fn clear(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?;

    if !get_guild_settings(&ctx.data(), guild_id)
        .await
        .music_enabled
    {
        ctx.send(
            poise::CreateReply::default().embed(
                serenity::CreateEmbed::new()
                    .description("音楽再生機能はこのサーバーでは無効化されています。")
                    .color(colors::ERROR),
            ),
        )
        .await?;

        return Ok(());
    }

    let state_arc = get_guild_music_state(ctx.data(), guild_id).await;

    let cleared_count = {
        let mut state = state_arc.write().await;
        let count = state.queue.len();
        state.queue.clear();
        count
    };

    if cleared_count == 0 {
        ctx.say("キューは既に空です。").await?;
    } else {
        ctx.say("キューをクリアしました。").await?;
    }

    Ok(())
}

#[poise::command(prefix_command)]
pub async fn seek(ctx: Context<'_>, input: String) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?;

    if !get_guild_settings(&ctx.data(), guild_id)
        .await
        .music_enabled
    {
        ctx.send(
            poise::CreateReply::default().embed(
                serenity::CreateEmbed::new()
                    .description("音楽再生機能はこのサーバーでは無効化されています。")
                    .color(colors::ERROR),
            ),
        )
        .await?;

        return Ok(());
    }

    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("failed to initialize songbird");
    if manager.get(guild_id).is_none() {
        ctx.send(
            poise::CreateReply::default().embed(
                serenity::CreateEmbed::new()
                    .description("botがVCに参加していません。")
                    .color(colors::WARN),
            ),
        )
        .await?;

        return Ok(());
    }

    let state_arc = get_guild_music_state(ctx.data(), guild_id).await;
    let state = state_arc.read().await;

    let handle = match &state.current_track {
        Some(h) => h,
        None => {
            ctx.say("音楽を再生していません。").await?;
            return Ok(());
        }
    };

    let is_relative = input.starts_with('+') || input.starts_with('-');
    let is_negative = input.starts_with('-');

    let time_str = if is_relative { &input[1..] } else { &input };

    let target_seconds = match parse_time_to_secs(time_str) {
        Some(secs) => secs,
        None => {
            ctx.say("時間の形式が不正です。秒数または「h:m:s」などの形式で入力してください。")
                .await?;
            return Ok(());
        }
    };

    let final_duration = if is_relative {
        let info = handle.get_info().await?;
        let current_position = info.position;

        if is_negative {
            current_position
                .checked_sub(Duration::from_secs(target_seconds))
                .unwrap_or(Duration::from_secs(0))
        } else {
            current_position + Duration::from_secs(target_seconds)
        }
    } else {
        Duration::from_secs(target_seconds)
    };

    match handle.seek_async(final_duration).await {
        Ok(actual_time) => {
            let secs = actual_time.as_secs();
            ctx.say(format!(
                "再生位置を `{}:{:02}` に変更しました",
                secs / 60,
                secs % 60
            ))
            .await?;
        }
        Err(e) => {
            ctx.say(format!("シークに失敗しました。: {:?}", e)).await?;
        }
    }

    Ok(())
}

#[poise::command(prefix_command, aliases("q"))]
pub async fn queue(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?;

    if !get_guild_settings(&ctx.data(), guild_id)
        .await
        .music_enabled
    {
        ctx.send(
            poise::CreateReply::default().embed(
                serenity::CreateEmbed::new()
                    .description("音楽再生機能はこのサーバーでは無効化されています。")
                    .color(colors::ERROR),
            ),
        )
        .await?;

        return Ok(());
    }

    let state_arc = get_guild_music_state(ctx.data(), guild_id).await;
    let state = state_arc.read().await;

    if state.queue.is_empty() {
        ctx.say("キューは空です。").await?;
        return Ok(());
    }

    let total_count = state.queue.len();
    let pages = build_queue_pages(&state.queue);
    drop(state);

    let total = pages.len();
    let mut current_page = 0usize;

    let ctx_id = ctx.id();
    let prev_id = format!("{}prev", ctx_id);
    let next_id = format!("{}next", ctx_id);

    let reply = ctx
        .send(
            poise::CreateReply::default()
                .embed(queue_embed(&pages, current_page, total_count))
                .components(if total > 1 {
                    vec![queue_buttons(&prev_id, &next_id, current_page, total)]
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
            let _ = reply
                .edit(
                    ctx,
                    poise::CreateReply::default()
                        .embed(queue_embed(&pages, current_page, total_count))
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
                        .embed(queue_embed(&pages, current_page, total_count))
                        .components(vec![queue_buttons(&prev_id, &next_id, current_page, total)]),
                ),
            )
            .await?;
    }

    Ok(())
}

#[poise::command(prefix_command, aliases("n"))]
pub async fn now(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx
        .guild_id()
        .ok_or("このコマンドはサーバー内でのみ実行できます。")?;

    if !get_guild_settings(&ctx.data(), guild_id)
        .await
        .music_enabled
    {
        ctx.send(
            poise::CreateReply::default().embed(
                serenity::CreateEmbed::new()
                    .description("音楽再生機能はこのサーバーでは無効化されています。")
                    .color(colors::ERROR),
            ),
        )
        .await?;

        return Ok(());
    }

    ctx.send(
        poise::CreateReply::default().embed(serenity::CreateEmbed::new().title("Now playing")),
    )
    .await?;

    Ok(())
}

fn parse_time_to_secs(s: &str) -> Option<u64> {
    let parts: Vec<&str> = s.split(':').collect();
    match parts.as_slice() {
        [s] => s.parse::<u64>().ok(),

        [m, s] => {
            let minutes = m.parse::<u64>().ok()?;
            let seconds = s.parse::<u64>().ok()?;
            Some(minutes * 60 + seconds)
        }

        [h, m, s] => {
            let hours = h.parse::<u64>().ok()?;
            let minutes = m.parse::<u64>().ok()?;
            let seconds = s.parse::<u64>().ok()?;
            Some(hours * 3600 + minutes * 60 + seconds)
        }

        _ => None,
    }
}

fn looks_like_playlist(url: &str) -> bool {
    url.contains("list=") || url.contains("/playlist?")
}

fn build_queue_pages(queue: &VecDeque<MusicItem>) -> Vec<Vec<(String, String)>> {
    let mut pages: Vec<Vec<(String, String)>> = Vec::new();
    let mut current: Vec<(String, String)> = Vec::new();

    for (i, item) in queue.iter().enumerate() {
        if current.len() >= 24 {
            pages.push(current);
            current = Vec::new();
        }

        let mut value = item.title.clone();
        if let Some(artist) = &item.artist {
            value.push_str(&format!(" ― {}", artist));
        }
        if let Some(dur) = item.duration {
            value.push_str(&format!(" `[{}]`", format_duration(dur)));
        }

        current.push((format!("{}.", i + 1), value));
    }
    if !current.is_empty() {
        pages.push(current);
    }

    pages
}

fn queue_embed(
    pages: &[Vec<(String, String)>],
    page: usize,
    total_count: usize,
) -> serenity::CreateEmbed {
    let mut embed = serenity::CreateEmbed::new()
        .title(format!(
            "再生キュー（{}曲）　ページ {}/{}",
            total_count,
            page + 1,
            pages.len()
        ))
        .color(colors::INFO);

    for (name, value) in &pages[page] {
        embed = embed.field(name, value, false);
    }

    embed
}

fn queue_buttons(
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
                notify_channel: None,
            }))
        })
        .clone()
}

fn build_queue_added_embed(item: &MusicItem) -> serenity::CreateEmbed {
    let mut description = item.title.clone();
    if let Some(artist) = &item.artist {
        description.push_str(&format!(" ― {}", artist));
    }
    if let Some(album) = &item.album {
        description.push_str(&format!(" / {}", album));
    }
    if let Some(year) = &item.release_year {
        description.push_str(&format!(" ({})", year));
    }

    let mut embed = serenity::CreateEmbed::new()
        .title("キューに追加しました")
        .description(description)
        .color(colors::SUCCEED);

    if item.is_ytdl {
        embed = embed.url(item.url.clone());
    }

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

fn music_item_from_ytdl(info: &YtdlOutput) -> MusicItem {
    let release_year = info
        .upload_date
        .as_deref()
        .and_then(|d| d.get(..4))
        .and_then(|y| y.parse::<u32>().ok());
    MusicItem {
        url: info.webpage_url.clone(),
        title: info.title.clone(),
        artist: info.uploader.clone(),
        album: None,
        release_year,
        duration: info.duration.map(|d| d as u64),
        thumbnail: info.thumbnail.clone(),
        albumart: None,
        is_ytdl: true,
    }
}

async fn get_localfile_metadata(
    url: &str,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<u32>,
    Option<u64>,
    Option<Vec<u8>>,
) {
    let (title, artist, album, release_year, duration) = get_ffprobe_metadata(url)
        .await
        .unwrap_or((None, None, None, None, None));

    let album_art = extract_album_art(url).await;

    (title, artist, album, release_year, duration, album_art)
}

async fn extract_album_art(url: &str) -> Option<Vec<u8>> {
    let tmp_path = format!("/tmp/kikisen_albumart_{}.jpg", std::process::id());

    let status = tokio::process::Command::new("ffmpeg")
        .args(&["-y", "-i", url, "-an", "-vcodec", "copy", &tmp_path])
        .output()
        .await
        .ok()?;

    if !status.status.success() {
        return None;
    }

    let file_data = tokio::fs::read(&tmp_path).await.ok()?;
    let _ = tokio::fs::remove_file(&tmp_path).await;

    if file_data.is_empty() {
        None
    } else {
        Some(file_data)
    }
}

async fn get_ffprobe_metadata(
    url: &str,
) -> Option<(
    Option<String>,
    Option<String>,
    Option<String>,
    Option<u32>,
    Option<u64>,
)> {
    let output = tokio::process::Command::new("ffprobe")
        .args(&["-v", "quiet", "-print_format", "json", "-show_format", url])
        .output()
        .await
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let probe: FfprobeOutput = serde_json::from_str(&stdout).ok()?;

    let duration = probe
        .format
        .duration
        .as_deref()
        .and_then(|d| d.parse::<f64>().ok())
        .map(|d| d as u64);

    let (title, artist, album, release_year) = if let Some(tags) = probe.format.tags {
        let year = tags
            .date
            .as_deref()
            .and_then(|d| d.get(..4))
            .and_then(|y| y.parse::<u32>().ok());
        (tags.title, tags.artist, tags.album, year)
    } else {
        (None, None, None, None)
    };

    Some((title, artist, album, release_year, duration))
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

async fn get_youtube_playlist_info(url: &str) -> Result<Vec<YtdlOutput>, Error> {
    let output = tokio::process::Command::new("yt-dlp")
        .args(&[
            "--dump-json",
            "--flat-playlist",
            "--yes-playlist",
            "--playlist-end",
            "100",
            url,
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

    if results.is_empty() {
        return Err("プレイリストの取得に失敗しました。".into());
    }

    Ok(results)
}
