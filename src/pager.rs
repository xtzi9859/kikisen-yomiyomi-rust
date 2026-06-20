use crate::types::{Context, Error};
use poise::serenity_prelude as serenity;
use std::time::Duration;

const DEFAULT_TIMEOUT_SECS: u64 = 120;

pub struct Pager {
    embeds: Vec<serenity::CreateEmbed>,
    timeout: Duration,
    ephemeral: bool,
}

impl Pager {
    pub fn new(embeds: Vec<serenity::CreateEmbed>) -> Self {
        Self {
            embeds,
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            ephemeral: false,
        }
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn ephemeral(mut self, ephemeral: bool) -> Self {
        self.ephemeral = ephemeral;
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

    pub async fn run(self, ctx: Context<'_>) -> Result<(), Error> {
        let Pager {
            embeds,
            timeout,
            ephemeral,
        } = self;

        if embeds.is_empty() {
            return Ok(());
        }

        let total = embeds.len();
        let mut current_page = 0usize;

        let ctx_id = ctx.id();
        let first_id = format!("{}pg_first", ctx_id);
        let prev_id = format!("{}pg_prev", ctx_id);
        let page_id = format!("{}pg_page", ctx_id);
        let next_id = format!("{}pg_next", ctx_id);
        let last_id = format!("{}pg_last", ctx_id);

        let reply = ctx
            .send(
                poise::CreateReply::default()
                    .ephemeral(ephemeral)
                    .embed(embeds[current_page].clone())
                    .components(if total > 1{
                        vec![Self::buttons(
                            &first_id, &prev_id, &page_id, &next_id, &last_id, current_page, total,
                        )]
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
            let first_id_c = first_id.clone();
            let prev_id_c = prev_id.clone();
            let next_id_c = next_id.clone();
            let last_id_c = last_id.clone();

            let Some(press) = message
                .await_component_interaction(ctx.serenity_context())
                .author_id(ctx.author().id)
                .timeout(timeout)
                .filter(move |m| {
                    m.data.custom_id == first_id_c
                        || m.data.custom_id == prev_id_c
                        || m.data.custom_id == next_id_c
                        || m.data.custom_id == last_id_c
                })
                .await
            else {
                let _ = reply
                    .edit(
                        ctx,
                        poise::CreateReply::default()
                            .embed(embeds[current_page].clone())
                            .components(vec![])
                    )
                    .await;
                break;
            };

            if press.data.custom_id == first_id {
                current_page = 0;
            } else if press.data.custom_id == prev_id {
                current_page = current_page.saturating_sub(1);
            } else if press.data.custom_id == next_id {
                current_page = (current_page + 1).min(total - 1);
            } else if press.data.custom_id == last_id {
                current_page = total - 1;
            }

            press
                .create_response(
                    ctx.serenity_context(),
                    serenity::CreateInteractionResponse::UpdateMessage(
                        serenity::CreateInteractionResponseMessage::new()
                            .embed(embeds[current_page].clone())
                            .components(vec![Self::buttons(
                                &first_id, &prev_id, &page_id, &next_id, &last_id, current_page, total,
                            )]),
                    ),
                )
                .await?;
        }

        Ok(())
    }
}
