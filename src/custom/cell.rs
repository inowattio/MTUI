#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Continuation {
    Middle,
    End,
    Detached,
}

impl Continuation {
    pub fn marker(self, owner: impl std::fmt::Display) -> String {
        match self {
            Self::Middle => "|".to_string(),
            Self::End => format!("`- {owner}"),
            Self::Detached => format!("-> {owner}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CustomCell {
    Value(String),
    PartOf {
        owner: u16,
        continuation: Continuation,
    },
}

impl CustomCell {
    pub fn detached(self) -> Self {
        match self {
            Self::PartOf { owner, .. } => Self::PartOf {
                owner,
                continuation: Continuation::Detached,
            },
            value => value,
        }
    }

    pub fn value(self) -> Option<String> {
        match self {
            Self::Value(text) => Some(text),
            Self::PartOf { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuation_markers_are_ascii_and_name_the_owner() {
        assert_eq!(Continuation::Middle.marker(100), "|");
        assert_eq!(Continuation::End.marker(100), "`- 100");
        assert_eq!(Continuation::Detached.marker("9C40"), "-> 9C40");
    }

    #[test]
    fn detaching_keeps_the_owner_and_leaves_values_alone() {
        let part = CustomCell::PartOf {
            owner: 100,
            continuation: Continuation::Middle,
        };
        assert_eq!(
            part.detached(),
            CustomCell::PartOf {
                owner: 100,
                continuation: Continuation::Detached,
            }
        );
        let value = CustomCell::Value("1.5 V".to_string());
        assert_eq!(value.clone().detached(), value);
    }
}
