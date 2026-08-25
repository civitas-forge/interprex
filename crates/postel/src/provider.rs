pub const REPO_PROVIDER_ENV: &str = "POSTEL_REPO_PROVIDER";
pub const TRACKER_PROVIDER_ENV: &str = "POSTEL_TRACKER_PROVIDER";
pub const CODE_REVIEWS_PROVIDER_ENV: &str = "POSTEL_CODE_REVIEWS_PROVIDER";
pub const JOBS_PROVIDER_ENV: &str = "POSTEL_JOBS_PROVIDER";
pub const RELEASES_PROVIDER_ENV: &str = "POSTEL_RELEASES_PROVIDER";
pub const DEFAULT_PROVIDER: &str = "github";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderSelections {
    pub repo: String,
    pub tracker: String,
    pub code_reviews: String,
    pub jobs: String,
    pub releases: String,
}

impl ProviderSelections {
    pub fn from_lookup(mut lookup: impl FnMut(&str) -> Option<String>) -> Self {
        let value = |value: Option<String>| {
            value
                .filter(|candidate| !candidate.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_PROVIDER.to_owned())
        };
        Self {
            repo: value(lookup(REPO_PROVIDER_ENV)),
            tracker: value(lookup(TRACKER_PROVIDER_ENV)),
            code_reviews: value(lookup(CODE_REVIEWS_PROVIDER_ENV)),
            jobs: value(lookup(JOBS_PROVIDER_ENV)),
            releases: value(lookup(RELEASES_PROVIDER_ENV)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CODE_REVIEWS_PROVIDER_ENV, DEFAULT_PROVIDER, ProviderSelections};

    #[test]
    fn selections_are_independent_and_default_to_github() {
        let selections = ProviderSelections::from_lookup(|name| {
            (name == CODE_REVIEWS_PROVIDER_ENV).then(|| "gitlab".to_owned())
        });
        assert_eq!(selections.code_reviews, "gitlab");
        assert_eq!(selections.repo, DEFAULT_PROVIDER);
        assert_eq!(selections.jobs, DEFAULT_PROVIDER);
    }

    #[test]
    fn blank_selection_is_treated_as_unset() {
        let selections = ProviderSelections::from_lookup(|_| Some("  ".to_owned()));
        assert_eq!(selections.tracker, DEFAULT_PROVIDER);
    }
}
