use crate::types::{Data, Error, VoiceStyleInfo, colors};
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

pub(crate) fn build_voice_style_page_with_select(
    styles: &[VoiceStyleInfo],
) -> (
    Vec<serenity::CreateEmbed>,
    Vec<Vec<serenity::CreateSelectMenuOption>>,
) {
    let mut pages: Vec<Vec<(String, String, u32)>> = Vec::new();
    let mut current: Vec<(String, String, u32)> = Vec::new();

    for style in styles {
        if current.len() >= 24 {
            pages.push(current);
            current = Vec::new();
        }
        current.push((
            style.display_label.clone(),
            format!("`{}`", style.style_id),
            style.style_id,
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
                .title("VOICEVOXの話者一覧")
                .footer(serenity::CreateEmbedFooter::new(format!(
                    "{}/{} ページ　下のリストから話者を選択すると設定されます。",
                    page_idx + 1,
                    total_pages
                )))
                .color(colors::INFO);

            let mut options = Vec::with_capacity(fields.len());
            for (name, value, style_id) in &fields {
                embed = embed.field(name, value, true);
                options.push(serenity::CreateSelectMenuOption::new(
                    name.clone(),
                    style_id.to_string(),
                ));
            }

            (embed, options)
        })
        .unzip()
}
