use std::vec;

use anyhow::{anyhow, Result};
use branch_context::{Message, Messages};
use indexmap::IndexMap;
use serenity::all::MessageId;

use crate::{chat::prompt::SystemPromptBuilder, utils};

use super::message::ChatMessage;

pub struct ChatContext {
    messages: IndexMap<u64, Messages<ChatMessage>>,
    pub system_prompt: SystemPromptBuilder,
}

impl ChatContext {
    pub fn new(system_prompt: &SystemPromptBuilder) -> Self {
        Self {
            messages: IndexMap::new(),
            system_prompt: system_prompt.clone(),
        }
    }

    pub fn add_message(
        &mut self,
        message: impl Into<Message<ChatMessage>>,
        id: Option<impl Into<u64>>,
    ) {
        let message = Messages::new(message.into(), id.map(|id| id.into()));
        self.messages.insert(message.id, message);
    }

    pub fn add_user_message(&mut self, message: String, id: MessageId) {
        self.add_message(
            ChatMessage {
                role: "user".to_string(),
                content: message,
                ..Default::default()
            },
            Some(id),
        );
    }

    pub fn latest(&self) -> Option<&Messages<ChatMessage>> {
        self.messages.last().map(|(_, m)| m)
    }
    pub fn latest_with_role(&self, user: String) -> Option<&Messages<ChatMessage>> {
        self.messages
            .iter()
            .rev()
            .find(|(_, m)| m.selected().role == user)
            .map(|(_, m)| m)
    }

    pub fn find(&self, id: impl Into<u64>) -> Option<&Messages<ChatMessage>> {
        self.messages.get(&id.into())
    }
    pub fn get(&self, index: usize) -> Option<&Messages<ChatMessage>> {
        self.messages.get_index(index).map(|(_, m)| m)
    }
    pub fn find_mut(&mut self, id: impl Into<u64>) -> Option<&mut Messages<ChatMessage>> {
        self.messages.get_mut(&id.into())
    }
    pub fn get_mut(&mut self, index: usize) -> Option<&mut Messages<ChatMessage>> {
        self.messages.get_index_mut(index).map(|(_, m)| m)
    }
    pub fn find_full(&self, id: impl Into<u64>) -> Option<(usize, &u64, &Messages<ChatMessage>)> {
        self.messages.get_full(&id.into())
    }
    pub fn find_full_mut(
        &mut self,
        id: impl Into<u64>,
    ) -> Option<(usize, &u64, &mut Messages<ChatMessage>)> {
        self.messages.get_full_mut(&id.into())
    }

    pub async fn get_context(
        &mut self,
        recalling: bool,
    ) -> (Vec<ChatMessage>, Option<Vec<ChatMessage>>) {
        if self.messages.is_empty() {
            let last_message_time = chrono::Utc::now();

            let system_prompt = self
                .system_prompt
                .clone()
                .build(last_message_time, recalling);

            let context = vec![ChatMessage {
                role: "system".to_string(),
                content: system_prompt.to_string(),
                ..Default::default()
            }];

            println!("context: {:?}", context);

            return (context, None);
        }

        let mut ctx = vec![];

        // Add the messages
        self.messages
            .iter()
            .for_each(|(_, messages)| ctx.push(messages.selected().clone()));

        // unwrapping is safe because we know the context is not empty
        let last_message_time = ctx.last().map(|m| m.sent_at).unwrap();

        // if the STM is full, we will remove the last 20% of the STM from the beginning (oldest)

        // 50 stm, 50 max stm
        let drained = if self.messages.len() >= self.system_prompt.max_stm {
            let to_remove = self.messages.len() - ((self.system_prompt.max_stm * 4) / 5);
            println!("context close to or full, draining {to_remove} messages");
            Some(
                self.messages
                    .drain(0..to_remove)
                    .map(|(_, m)| m.into_selected())
                    .collect::<Vec<ChatMessage>>(),
            )
        } else {
            None
        };

        let system_prompt = self
            .system_prompt
            .clone()
            .build(last_message_time, recalling);

        let mut context = vec![ChatMessage {
            role: "system".to_string(),
            content: system_prompt.to_string(),
            ..Default::default()
        }];
        // let mut context = vec![]; // todo for testing
        context.extend(ctx);

        println!("context: {:?}", context);

        (context, drained)
    }

    // gets context but excludes the last message and the user prompt is taken as string-only
    pub async fn get_regen_context(
        &mut self,
        recalling: bool,
    ) -> (Vec<ChatMessage>, Option<Vec<ChatMessage>>) {
        // context.push(CompletionMessage {
        //     role: "system".to_string(),
        //     content: "Please send a different response than you'd usually do, but keep the same tone and style as you normally would, following all previous instructions".to_string(),
        // });

        let (mut context, drained) = self.get_context(recalling).await;

        // take off the last two, keep the second to last
        if let Some(pos) = context.iter().rposition(|m| m.role == "assistant") {
            context.remove(pos);
        }

        (context, drained)
    }

    pub async fn freewill_context(
        &mut self,
        recalling: bool,
    ) -> Result<(Vec<ChatMessage>, Option<Vec<ChatMessage>>)> {
        let (mut context, drained) = self.get_context(recalling).await;

        let message = ChatMessage {
            role: "user".to_string(),
            content: format!(
                "*it's been around {} since you last said something, and the user did not respond. your next response should attempt to pull the user back into the conversation. please respond once again, making sure to keep the same tone and style as you normally would, following all previous instructions, yet keeping the time difference in mind. your response should only contain the actual response, not your thoughts or anything else.*\n\n\"...\"",
                utils::time_to_string(self.time_since_last()?)
            ),
            ..Default::default()
        };

        // id-less message
        self.add_message(message.clone(), None::<u64>);

        context.push(message);

        Ok((context, drained))
    }

    pub fn time_since_last(&self) -> anyhow::Result<chrono::Duration> {
        let last = self
            .latest()
            .ok_or(anyhow!("Context is empty, nothing to freewill out of"))?;

        Ok(chrono::Utc::now() - last.selected().sent_at)
    }

    pub fn add_long_term_memories(&mut self, memories: Vec<String>) {
        self.system_prompt.add_long_term_memories(memories);
    }
}
