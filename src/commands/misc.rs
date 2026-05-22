use crate::types::{Error, Context, colors};
use poise::serenity_prelude as serenity;

#[poise::command(slash_command)]
pub async fn restart(ctx: Context<'_>) -> Result<(), Error> {
    let embed = serenity::CreateEmbed::new()
        .color(colors::SUCCEED)
        .description("再起動します…");

    let reply = poise::CreateReply::default().embed(embed);
    ctx.send(reply).await?;

    tracing::info!("restart command executed; restarting...");

    ctx.framework().shard_manager().shutdown_all().await;
    std::process::exit(0);
}

#[poise::command(slash_command)]
pub async fn age(
    ctx: Context<'_>,
    #[description = "Selected user"] user: Option<serenity::User>,
) -> Result<(), Error> {
    let u = user.as_ref().unwrap_or_else(|| ctx.author());
    let response = format!("{} account was created at {}", u.name, u.created_at());
    ctx.say(response).await?;
    Ok(())
}
