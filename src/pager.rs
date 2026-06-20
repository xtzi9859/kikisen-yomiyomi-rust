use crate::types::{Context, Error};
use poise::serenity_prelude as serenity;
use std::time::Duration;

pub struct Pager {
    embeds: Vec<serenity::CreateEmbed>,
    timeout: Duration,
    ephemeral: bool,
}

impl Pager {
    pub fn new(embeds: Vec<serenity::CreateEmbed>) -> Self {

    }

    pub fn timeout(self, timeout: Duration) -> Self {

    }

    pub fn ephemeral(self, ephemeral: bool) -> Self {

    }

    pub async fn run(self, ctx: Context<'_>) -> Result<(), Error> {

    }
}
