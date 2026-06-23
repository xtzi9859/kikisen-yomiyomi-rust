use crate::types::{Context, Error};
use poise::serenity_prelude as serenity;
use std::time::Duration;

const DEFAULT_TIMEOUT_SECS: u64 = 120;

pub struct Pager {
    embeds: Vec<serenity::CreateEmbed>,
    timeout: Duration,
    ephemeral: bool,
    select_options: Option<Vec<Vec<serenity::CreateSelectMenuOption>>>,
    select_placeholder: String,
}

impl Pager {
    pub fn new(embeds: Vec<serenity::CreateEmbed>) -> Self {
        Self {
            embeds,
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            ephemeral: false,
            select_options: None,
            select_placeholder: "選択してください".to_string(),
        }
    }

    #[allow(dead_code)]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    #[allow(dead_code)]
    pub fn ephemeral(mut self, ephemeral: bool) -> Self {
        self.ephemeral = ephemeral;
        self
    }

    /// 各ページに表示するドロップダウンリストの選択肢を設定する。
    /// `options_per_page` は embeds と同じ添字（ページ番号）で対応させる。
    /// 該当ページの要素が空の場合、そのページにはドロップダウンを表示しない。
    pub fn with_select(
        mut self,
        options_per_page: Vec<Vec<serenity::CreateSelectMenuOption>>,
        placeholder: impl Into<String>,
    ) -> Self {
        self.select_options = Some(options_per_page);
        self.select_placeholder = placeholder.into();
        self
    }

    fn buttons(
        first_id: &str,
        prev_id: &str,
        page_id: &str,
        next_id: &str,
        last_id: &str,
        page: usize,
        total: usize,
    ) -> serenity::CreateActionRow {
        serenity::CreateActionRow::Buttons(vec![
            serenity::CreateButton::new(first_id)
                .label("<<")
                .style(serenity::ButtonStyle::Danger)
                .disabled(page == 0),
            serenity::CreateButton::new(prev_id)
                .label("<")
                .style(serenity::ButtonStyle::Primary)
                .disabled(page == 0),
            serenity::CreateButton::new(page_id)
                .label(format!("{}/{}", page + 1, total))
                .style(serenity::ButtonStyle::Secondary)
                .disabled(true),
            serenity::CreateButton::new(next_id)
                .label(">")
                .style(serenity::ButtonStyle::Primary)
                .disabled(page >= total - 1),
            serenity::CreateButton::new(last_id)
                .label(">>")
                .style(serenity::ButtonStyle::Danger)
                .disabled(page >= total - 1),
        ])
    }

    fn components_for_page(
        &self,
        page: usize,
        total: usize,
        first_id: &str,
        prev_id: &str,
        page_id: &str,
        next_id: &str,
        last_id: &str,
        select_id: &str,
    ) -> Vec<serenity::CreateActionRow> {
        let mut rows = Vec::new();

        if let Some(options_per_page) = &self.select_options {
            if let Some(options) = options_per_page.get(page) {
                if !options.is_empty() {
                    rows.push(serenity::CreateActionRow::SelectMenu(
                        serenity::CreateSelectMenu::new(
                            select_id,
                            serenity::CreateSelectMenuKind::String {
                                options: options.clone(),
                            },
                        )
                        .placeholder(self.select_placeholder.clone()),
                    ));
                }
            }
        }

        if total > 1 {
            rows.push(Self::buttons(
                first_id, prev_id, page_id, next_id, last_id, page, total,
            ));
        }

        rows
    }

    /// ページャーを実行する（ドロップダウンの選択結果は捨てる）。
    pub async fn run(self, ctx: Context<'_>) -> Result<(), Error> {
        self.run_inner(ctx).await?;
        Ok(())
    }

    /// ページャーを実行し、ドロップダウンリストが選択された場合はその値（custom value）を返す。
    /// タイムアウトした場合やボタン操作のみで終了した場合は `Ok(None)` を返す。
    pub async fn run_with_select(self, ctx: Context<'_>) -> Result<Option<String>, Error> {
        self.run_inner(ctx).await
    }

    async fn run_inner(self, ctx: Context<'_>) -> Result<Option<String>, Error> {
        if self.embeds.is_empty() {
            return Ok(None);
        }

        let total = self.embeds.len();
        let mut current_page = 0usize;

        let ctx_id = ctx.id();
        let first_id = format!("{}pg_first", ctx_id);
        let prev_id = format!("{}pg_prev", ctx_id);
        let page_id = format!("{}pg_page", ctx_id);
        let next_id = format!("{}pg_next", ctx_id);
        let last_id = format!("{}pg_last", ctx_id);
        let select_id = format!("{}pg_select", ctx_id);

        let components = self.components_for_page(
            current_page, total, &first_id, &prev_id, &page_id, &next_id, &last_id, &select_id,
        );
        let has_components = !components.is_empty();

        let reply = ctx
            .send(
                poise::CreateReply::default()
                    .ephemeral(self.ephemeral)
                    .embed(self.embeds[current_page].clone())
                    .components(components),
            )
            .await?;

        if !has_components {
            return Ok(None);
        }

        let message = reply.message().await?;

        loop {
            let first_id_c = first_id.clone();
            let prev_id_c = prev_id.clone();
            let next_id_c = next_id.clone();
            let last_id_c = last_id.clone();
            let select_id_c = select_id.clone();

            let Some(press) = message
                .await_component_interaction(ctx.serenity_context())
                .author_id(ctx.author().id)
                .timeout(self.timeout)
                .filter(move |m| {
                    m.data.custom_id == first_id_c
                        || m.data.custom_id == prev_id_c
                        || m.data.custom_id == next_id_c
                        || m.data.custom_id == last_id_c
                        || m.data.custom_id == select_id_c
                })
                .await
            else {
                let _ = reply
                    .edit(
                        ctx,
                        poise::CreateReply::default()
                            .embed(self.embeds[current_page].clone())
                            .components(vec![]),
                    )
                    .await;
                return Ok(None);
            };

            if press.data.custom_id == select_id {
                let selected_value = match &press.data.kind {
                    serenity::ComponentInteractionDataKind::StringSelect { values } => {
                        values.first().cloned()
                    }
                    _ => None,
                };

                press
                    .create_response(
                        ctx.serenity_context(),
                        serenity::CreateInteractionResponse::UpdateMessage(
                            serenity::CreateInteractionResponseMessage::new()
                                .embed(self.embeds[current_page].clone())
                                .components(vec![]),
                        ),
                    )
                    .await?;

                return Ok(selected_value);
            }

            if press.data.custom_id == first_id {
                current_page = 0;
            } else if press.data.custom_id == prev_id {
                current_page = current_page.saturating_sub(1);
            } else if press.data.custom_id == next_id {
                current_page = (current_page + 1).min(total - 1);
            } else if press.data.custom_id == last_id {
                current_page = total - 1;
            }

            let components = self.components_for_page(
                current_page, total, &first_id, &prev_id, &page_id, &next_id, &last_id, &select_id,
            );

            press
                .create_response(
                    ctx.serenity_context(),
                    serenity::CreateInteractionResponse::UpdateMessage(
                        serenity::CreateInteractionResponseMessage::new()
                            .embed(self.embeds[current_page].clone())
                            .components(components),
                    ),
                )
                .await?;
        }
    }
}
