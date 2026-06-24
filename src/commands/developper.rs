use crate::types::{Context, Error};
use poise::serenity_prelude as serenity;

#[poise::command(prefix_command)]
pub async fn dev_save(ctx: Context<'_>) -> Result<(), Error> {
    ctx.reply("saved voice_state").await?;

    Ok(())
}

pub async fn check_is_developer(user_id: serenity::UserId) -> bool {
    if let Ok(dev_id_str) = std::env::var("DEVELOPPER_ID") {
        if let Ok(dev_id) = dev_id_str.parse::<u64>() {
            return user_id.get() == dev_id;
        }
    }

    false
}
