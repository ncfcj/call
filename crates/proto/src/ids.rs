//! IDs tipados, para o compilador impedir trocar um `ChannelId` por um `UserId`.

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

macro_rules! id_type {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl From<Uuid> for $name {
            fn from(id: Uuid) -> Self {
                Self(id)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

id_type!(UserId);
id_type!(GuildId);
id_type!(ChannelId);
id_type!(MessageId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_plain_uuids_on_the_wire_and_in_text() {
        let raw = Uuid::from_u128(0xabc);
        let id = UserId::from(raw);
        assert_eq!(id.0, raw);
        assert_eq!(id.to_string(), raw.to_string());
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{raw}\""));
        assert_eq!(serde_json::from_str::<UserId>(&json).unwrap(), id);
        assert!(serde_json::from_str::<ChannelId>("\"não é uuid\"").is_err());
    }

    #[test]
    fn ids_order_like_their_uuids() {
        assert!(MessageId(Uuid::from_u128(1)) < MessageId(Uuid::from_u128(2)));
        assert_eq!(GuildId(Uuid::nil()), GuildId::from(Uuid::nil()));
    }
}
