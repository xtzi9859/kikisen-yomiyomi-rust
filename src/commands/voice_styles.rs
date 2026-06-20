use crate::helpers::Pager;
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

fn build_voice_style_pages(styles: &[VoiceStyleInfo]) -> Vec<serenity::CreateEmbed> {
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

    let total_pages = pages.len().max(1);

    pages
        .into_iter()
        .enumerate()
        .map(|(page_idx, fields)| {
            let mut embed = serenity::CreateEmbed::new()
                .title("VOICEVOX 話者一覧")
                .footer(serenity::CreateEmbedFooter::new(format!(
                    "{}/{} 話者を`/user_setting speaker`で設定できます。",
                    page_idx + 1,
                    total_pages
                )))
                .color(colors::INFO);

            for (name, value) in &fields {
                embed = embed.field(name, value, true);
            }

            embed
        })
        .collect()
}

#[poise::command(slash_command)]
pub async fn voice_styles(ctx: Context<'_>) -> Result<(), Error> {
    let mut embeds = build_voice_style_pages(&ctx.data().voice_styles);

    if embeds.is_empty() {
        embeds = vec![
            serenity::CreateEmbed::new()
                .description("話者が読み込まれていません。")
        ];
    }

    Pager::new(embeds).run(ctx).await
}
