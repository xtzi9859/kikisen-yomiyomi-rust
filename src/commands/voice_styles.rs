use crate::types::{Context, Data, Error, VoiceStyleInfo, colors};
use poise::serenity_prelude as serenity;

pub async fn autocomplete_voice_style<'a>(
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
