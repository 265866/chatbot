use anyhow::bail;
use serenity::all::{ComponentInteraction, Context, CreateButton, EditMessage};

use crate::chat;

use super::super::Handler;

impl Handler {
    pub async fn prev(
        &self,
        mut component: ComponentInteraction,
        ctx: Context,
    ) -> anyhow::Result<()> {
        let data = self.data.clone();

        let mut user_map = data.user_map.write().await;
        let engine = user_map.entry(component.user.clone()).or_insert_with({
            data.config.write().await.update();
            let config = data.config.read().await.clone();
            || chat::engine::ChatEngine::new(config, component.user.id)
        });

        let message = engine
            .find_mut(component.message.id)
            .ok_or(anyhow::anyhow!("message not found in engine"))?;

        if !message.backward {
            bail!("message is already at the end of the context");
        }

        let content = message.backward().content.clone();

        let (can_go_fwd, emoji) = match message.forward {
            true => ("next", '⏩'),
            false => ("regen", '♻'),
        };

        component
            .message
            .edit(
                ctx.http.clone(),
                EditMessage::new()
                    .content(content)
                    .button(
                        CreateButton::new("prev")
                            .label("")
                            .emoji('⏪')
                            .style(serenity::all::ButtonStyle::Secondary)
                            .disabled(!message.backward),
                    )
                    .button(
                        CreateButton::new("next")
                            .label("")
                            .emoji('⏩')
                            .style(serenity::all::ButtonStyle::Secondary)
                            .disabled(false),
                    ),
            )
            .await?;

        Ok(())
    }
}
