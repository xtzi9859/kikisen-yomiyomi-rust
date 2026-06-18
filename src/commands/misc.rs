use crate::types::{Context, Error, colors};
use poise::serenity_prelude as serenity;

/// botを再起動する
#[poise::command(slash_command)]
pub async fn restart(ctx: Context<'_>) -> Result<(), Error> {
    ctx.send(
        poise::CreateReply::default().embed(
            serenity::CreateEmbed::new()
                .color(colors::SUCCEED)
                .description("再起動します…"),
        ),
    )
    .await?;

    tracing::info!("restart command executed; restarting...");

    ctx.framework().shard_manager().shutdown_all().await;
    std::process::exit(0);
}

/// show age of user executed this command or specified.
#[poise::command(slash_command)]
pub async fn age(
    ctx: Context<'_>,
    #[description = "Selected user"]
    user: Option<serenity::User>,
) -> Result<(), Error> {
    let u = user.as_ref().unwrap_or_else(|| ctx.author());
    let response = format!("{} account was created at {}", u.name, u.created_at());
    ctx.say(response).await?;
    Ok(())
}
