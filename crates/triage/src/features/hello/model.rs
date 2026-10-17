use std::fmt;
use std::str::FromStr;

use sqlx::encode::IsNull;
use sqlx::error::BoxDynError;
use sqlx::sqlite::{SqliteArgumentsBuffer, SqliteTypeInfo, SqliteValueRef};
use sqlx::{Decode, Encode, Sqlite, Type};
use time::OffsetDateTime;
use ulid::Ulid;

use crate::error::{AppError, FieldError, Result};

pub const NAME_MAX: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HelloId(Ulid);

impl HelloId {
    pub fn generate() -> Self {
        Self(Ulid::generate())
    }
}

impl fmt::Display for HelloId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for HelloId {
    type Err = ulid::DecodeError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        s.parse().map(Self)
    }
}

impl Type<Sqlite> for HelloId {
    fn type_info() -> SqliteTypeInfo {
        <String as Type<Sqlite>>::type_info()
    }
}

impl Encode<'_, Sqlite> for HelloId {
    fn encode_by_ref(
        &self,
        buf: &mut SqliteArgumentsBuffer,
    ) -> std::result::Result<IsNull, BoxDynError> {
        Encode::<Sqlite>::encode(self.0.to_string(), buf)
    }
}

impl<'r> Decode<'r, Sqlite> for HelloId {
    fn decode(value: SqliteValueRef<'r>) -> std::result::Result<Self, BoxDynError> {
        Ok(<&str as Decode<Sqlite>>::decode(value)?.parse()?)
    }
}

/// A trimmed name of 1 to [`NAME_MAX`] characters.
// Decoding skips `parse`: the table's CHECK already holds the same bounds.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::Type)]
#[sqlx(transparent)]
pub struct HelloName(String);

impl HelloName {
    pub fn parse(raw: &str) -> Result<Self> {
        let name = raw.trim();
        let len = name.chars().count();
        if (1..=NAME_MAX).contains(&len) {
            return Ok(Self(name.to_owned()));
        }
        let message = if len == 0 {
            "must not be empty".to_owned()
        } else {
            format!("must be at most {NAME_MAX} characters")
        };
        Err(AppError::Validation(vec![FieldError {
            field: "name".into(),
            message,
        }]))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    pub id: HelloId,
    pub name: HelloName,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewHello {
    pub name: HelloName,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_trimmed() -> Result<()> {
        assert_eq!(HelloName::parse("  bob  ")?.as_str(), "bob");
        Ok(())
    }

    #[test]
    fn name_bounds_count_characters() {
        assert!(HelloName::parse(&"é".repeat(NAME_MAX)).is_ok());
        for bad in ["", "   ", &"a".repeat(NAME_MAX + 1)] {
            assert!(
                matches!(HelloName::parse(bad), Err(AppError::Validation(_))),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn id_round_trips_through_text() -> std::result::Result<(), ulid::DecodeError> {
        let id = HelloId::generate();
        assert_eq!(id.to_string().parse::<HelloId>()?, id);
        Ok(())
    }
}
