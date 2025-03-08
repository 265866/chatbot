use std::{
    process::exit,
    sync::{Arc, mpsc},
};

use events::HandlerResult;
pub use framework::Data;
use serenity::{
    all::{Context, EventHandler, Interaction, Message, MessageUpdateEvent, Ready},
    async_trait,
};
use tokio::signal::unix::{SignalKind, signal};
use tokio::task::JoinHandle;

mod buttons;
mod events;
pub mod framework;

pub struct Handler {
    pub data: Data,
}
impl Handler {
    pub fn new(data: Data) -> (Arc<Self>, JoinHandle<()>) {
        let handler = Arc::new(Self { data });

        let handle = tokio::spawn({
            let handler = handler.clone();
            let shutdown_rx = setup_ctrlc_handler();
            async move {
                shutdown_rx
                    .recv()
                    .expect("Failed to receive shutdown signal");

                if let Err(err) = handler.shutdown().await {
                    log::error!("Error shutting down: {err}");
                }

                exit(0);
            }
        });

        (handler, handle)
    }
}

fn setup_ctrlc_handler() -> mpsc::Receiver<()> {
    let (sender, receiver) = mpsc::channel();

    tokio::spawn({
        let mut term_signal = signal(SignalKind::terminate()).unwrap();
        let mut int_signal = signal(SignalKind::interrupt()).unwrap();
        let mut hup_signal = signal(SignalKind::hangup()).unwrap();

        async move {
            tokio::select! {
                _ = term_signal.recv() => {
                    log::info!("SIGTERM received, shutting down...");
                    let _ = sender.send(());
                },
                _ = int_signal.recv() => {
                    println!("");
                    log::info!("SIGINT received, shutting down...");
                    let _ = sender.send(());
                },
                _ = hup_signal.recv() => {
                    log::info!("SIGHUP received, shutting down...");
                    let _ = sender.send(());
                },
            };
        }
    });

    receiver
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        log::info!("{} is connected!", ready.user.name);

        ctx.set_presence(None, serenity::all::OnlineStatus::Online);

        // TODO mobile status
        // ctx.shard
        //     .send_to_shard(serenity::all::ShardRunnerMessage::Message(
        //         tungstenite::Message::Text(
        //             serde_json::to_string(&serde_json::json!({
        //                 "op": 3,  // Status Update opcode
        //                 "d": {
        //                     "since": null,
        //                     "activities": [],
        //                     "status": "online",
        //                     "afk": false,
        //                     "client_info": {
        //                         "$os": "android",  // Try setting mobile OS flag
        //                         "$browser": "Discord Android"
        //                     }
        //                 }
        //             }))
        //             .unwrap(),
        //         ),
        //     ));

        self.data.context.write().await.replace(Arc::new(ctx));
    }

    async fn message(&self, ctx: Context, msg: Message) {
        if let HandlerResult::Err(error) = self.on_message(ctx, msg).await {
            Self::on_error(error).await;
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let HandlerResult::Err(error) = self.on_interaction(ctx, interaction).await {
            Self::on_error(error).await;
        }
    }

    async fn message_update(
        &self,
        ctx: Context,
        old_if_available: Option<Message>,
        new: Option<Message>,
        event: MessageUpdateEvent,
    ) {
        if let HandlerResult::Err(error) = self.on_edit(ctx, old_if_available, new, event).await {
            Self::on_error(error).await;
        }
    }
}

impl Handler {
    async fn shutdown(&self) -> anyhow::Result<()> {
        log::info!("Shutdown signal received, waiting for locks and shutting down...");
        let user_map = self.data.user_map.write().await;
        let context = self.data.context.write().await;

        let mut messages = Vec::new();
        for (_, engine) in user_map.iter() {
            for message_identifier in engine.write().await.shutdown().await? {
                if let Some(context) = context.as_ref() {
                    if let Some(message) = message_identifier.to_message(&context.http).await {
                        messages.push(message);
                    }
                }
            }
        }

        if let Some(context) = context.as_ref() {
            context.set_presence(None, serenity::all::OnlineStatus::Offline);

            for mut message in messages {
                let _ = self
                    .disable_buttons(&mut message, &context)
                    .await
                    .map_err(|why| {
                        log::error!("Error disabling buttons: {why:?}");
                    });
            }

            context.shard.shutdown_clean();
        }

        log::info!("Graceful shutdown complete!");

        Ok(())
    }
}
