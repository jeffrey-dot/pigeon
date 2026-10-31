//! How a session's box is picked when more than one client is attached (protocol §6).

/// The policy the first `attach` of a session chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizePolicy {
    /// Whoever last focused, typed or resized owns the size.
    Latest,
    /// The smallest box any attached client asked for, so nothing is cut off.
    Smallest,
    /// Only the client that first attached may resize.
    Locked,
}

impl SizePolicy {
    /// The `attach.size_policy` field; anything unknown (or absent) is the default `latest`,
    /// so a newer client asking for a policy this host lacks still gets a working session.
    pub fn from_name(name: Option<&str>) -> Self {
        match name {
            Some("smallest") => Self::Smallest,
            Some("locked") => Self::Locked,
            _ => Self::Latest,
        }
    }

    /// The `size_state.source` value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Latest => "latest",
            Self::Smallest => "smallest",
            Self::Locked => "locked",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_names_round_trip() {
        for policy in [SizePolicy::Latest, SizePolicy::Smallest, SizePolicy::Locked] {
            assert_eq!(SizePolicy::from_name(Some(policy.as_str())), policy);
        }
    }

    #[test]
    fn unknown_or_missing_names_fall_back_to_latest() {
        assert_eq!(SizePolicy::from_name(None), SizePolicy::Latest);
        assert_eq!(SizePolicy::from_name(Some("")), SizePolicy::Latest);
        assert_eq!(SizePolicy::from_name(Some("SMALLEST")), SizePolicy::Latest);
        assert_eq!(SizePolicy::from_name(Some("biggest")), SizePolicy::Latest);
    }
}
