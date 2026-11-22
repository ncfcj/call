//! Regras de tela sem navegador: o que o crate `web` mostra, testável no alvo nativo.

use proto::Message;

/// Até duas iniciais em maiúsculas, para o ícone do servidor ("Amigos da Ana" vira "AD").
pub fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect::<String>()
        .to_uppercase()
}

/// Minutos sem mensagens do mesmo autor para mostrar o cabeçalho (autor e hora) de novo.
pub const GROUP_GAP_MINUTES: i64 = 5;

/// A mensagem começa um grupo novo (com autor e hora) se é a primeira, se mudou o autor, ou se
/// passaram 5 minutos desde a anterior.
pub fn starts_group(prev: Option<&Message>, message: &Message) -> bool {
    let Some(prev) = prev else { return true };
    let same_author = prev.author.as_ref().map(|a| a.id) == message.author.as_ref().map(|a| a.id);
    let gap = message.created_at - prev.created_at;
    !same_author || gap.num_minutes() >= GROUP_GAP_MINUTES
}

/// "Ana está digitando..." ou "Ana, Bia estão digitando...". `None` quando ninguém digita.
pub fn typing_label(names: &[String]) -> Option<String> {
    match names {
        [] => None,
        [one] => Some(format!("{one} está digitando...")),
        many => Some(format!("{} estão digitando...", many.join(", "))),
    }
}

/// Opções de um seletor de dispositivos: sem o `default` (o "Padrão do sistema" já é a primeira
/// opção da lista) e com nome genérico quando o navegador ainda não deu permissão.
pub fn device_options<'a>(
    devices: impl IntoIterator<Item = (&'a str, &'a str)>,
    noun: &str,
) -> Vec<(String, String)> {
    devices
        .into_iter()
        .filter(|(id, _)| *id != "default")
        .enumerate()
        .map(|(i, (id, label))| {
            let label =
                if label.is_empty() { format!("{noun} {}", i + 1) } else { label.to_owned() };
            (id.to_owned(), label)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};
    use proto::{ChannelId, MessageId, User, UserId};
    use uuid::Uuid;

    use super::*;

    #[test]
    fn initials_take_the_first_letters_of_two_words() {
        assert_eq!(initials("Amigos da Ana"), "AD");
        assert_eq!(initials("  turma   do   rock "), "TD");
        assert_eq!(initials("Solo"), "S");
        assert_eq!(initials("ágape érico"), "ÁÉ");
        assert_eq!(initials(""), "");
    }

    fn msg(author: Option<u128>, minute: i64) -> Message {
        Message {
            id: MessageId(Uuid::from_u128(u128::try_from(minute).unwrap() + 1)),
            channel_id: ChannelId(Uuid::nil()),
            author: author.map(|n| User {
                id: UserId(Uuid::from_u128(n)),
                username: format!("u{n}"),
                display_name: format!("U{n}"),
            }),
            content: "oi".into(),
            created_at: Utc.timestamp_opt(1_800_000_000, 0).unwrap() + Duration::minutes(minute),
            edited_at: None,
        }
    }

    #[test]
    fn group_starts_on_first_message_new_author_or_after_five_minutes() {
        assert!(starts_group(None, &msg(Some(1), 0)), "primeira mensagem");
        assert!(!starts_group(Some(&msg(Some(1), 0)), &msg(Some(1), 4)), "mesmo autor, 4 min");
        assert!(starts_group(Some(&msg(Some(1), 0)), &msg(Some(1), 5)), "mesmo autor, 5 min");
        assert!(starts_group(Some(&msg(Some(1), 0)), &msg(Some(2), 0)), "outro autor");
        assert!(starts_group(Some(&msg(Some(1), 0)), &msg(None, 0)), "conta apagada");
        assert!(!starts_group(Some(&msg(None, 0)), &msg(None, 1)), "duas de conta apagada");
    }

    #[test]
    fn typing_label_reads_naturally() {
        assert_eq!(typing_label(&[]), None);
        assert_eq!(typing_label(&["Ana".into()]).unwrap(), "Ana está digitando...");
        assert_eq!(
            typing_label(&["Ana".into(), "Bia".into()]).unwrap(),
            "Ana, Bia estão digitando..."
        );
    }

    #[test]
    fn device_options_skip_default_and_name_unlabeled_devices() {
        let devices = [("default", "Padrão"), ("a", "Headset"), ("b", ""), ("c", "")];
        assert_eq!(
            device_options(devices, "Microfone"),
            [
                ("a".to_owned(), "Headset".to_owned()),
                ("b".to_owned(), "Microfone 2".to_owned()),
                ("c".to_owned(), "Microfone 3".to_owned()),
            ]
        );
        assert!(device_options([("default", "Padrão")], "Saída").is_empty());
    }
}
