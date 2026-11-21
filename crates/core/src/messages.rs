use proto::{Message, MessageId};

/// Histórico carregado de um canal, em ordem cronológica e sem duplicatas.
///
/// As mensagens chegam por três caminhos que se sobrepõem: a página inicial do histórico, o
/// `MESSAGE_CREATE` do WebSocket e a recarga `?after=` depois de uma reconexão. Como o UUIDv7 é
/// ordenável por tempo, ordenar por id basta.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MessageLog {
    messages: Vec<Message>,
    /// Ainda há mensagens mais antigas no servidor (vale até uma página voltar incompleta).
    pub has_older: bool,
}

impl MessageLog {
    /// Log a partir da primeira página. `page_full` diz se ela veio com o `limit` inteiro.
    pub fn from_latest(page: Vec<Message>, page_full: bool) -> Self {
        let mut log = Self { messages: Vec::new(), has_older: page_full };
        log.merge(page);
        log
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn oldest_id(&self) -> Option<MessageId> {
        self.messages.first().map(|m| m.id)
    }

    pub fn newest_id(&self) -> Option<MessageId> {
        self.messages.last().map(|m| m.id)
    }

    /// Junta mensagens novas ou antigas. Devolve quantas eram inéditas.
    pub fn merge(&mut self, incoming: impl IntoIterator<Item = Message>) -> usize {
        let mut added = 0;
        for message in incoming {
            match self.messages.binary_search_by_key(&message.id, |m| m.id) {
                Ok(existing) => self.messages[existing] = message,
                Err(pos) => {
                    self.messages.insert(pos, message);
                    added += 1;
                }
            }
        }
        added
    }

    /// Junta uma página `before=` e anota se ainda há mais antigas.
    pub fn merge_older(&mut self, page: Vec<Message>, page_full: bool) {
        self.has_older = page_full;
        self.merge(page);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use proto::ChannelId;
    use uuid::Uuid;

    fn msg(n: u128) -> Message {
        Message {
            id: MessageId(Uuid::from_u128(n)),
            channel_id: ChannelId(Uuid::nil()),
            author: None,
            content: format!("msg {n}"),
            created_at: Utc::now(),
            edited_at: None,
        }
    }

    fn contents(log: &MessageLog) -> Vec<&str> {
        log.messages().iter().map(|m| m.content.as_str()).collect()
    }

    #[test]
    fn merges_out_of_order_without_duplicates() {
        let mut log = MessageLog::from_latest(vec![msg(3), msg(4)], true);
        // O MESSAGE_CREATE da 5 chega, e a recarga de reconexão traz a 4 e a 5 de novo.
        assert_eq!(log.merge([msg(5)]), 1);
        assert_eq!(log.merge([msg(4), msg(5), msg(6)]), 1);
        log.merge_older(vec![msg(1), msg(2)], false);
        assert_eq!(contents(&log), ["msg 1", "msg 2", "msg 3", "msg 4", "msg 5", "msg 6"]);
        assert!(!log.has_older);
        assert_eq!(log.oldest_id(), Some(MessageId(Uuid::from_u128(1))));
        assert_eq!(log.newest_id(), Some(MessageId(Uuid::from_u128(6))));
    }
}
