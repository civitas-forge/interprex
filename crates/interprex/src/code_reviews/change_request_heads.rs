use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::Repository;

const BRANCH_REF_PREFIX: &str = "refs/heads/";

/// Characters `git check-ref-format` forbids anywhere in a ref.
const FORBIDDEN_BRANCH_CHARACTERS: [char; 7] = ['~', '^', ':', '?', '*', '[', '\\'];

/// Why a string names no branch a change request can propose.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum InvalidHeadRef {
    #[error("head ref must be fully qualified as refs/heads/<branch>")]
    NotABranchRef,
    #[error("head ref names no branch")]
    NoBranch,
    #[error("branch name is one git refuses to create")]
    InvalidBranchName,
}

/// The branch a change request proposes, and the repository holding it.
///
/// A change request belongs to the repository it targets, while its head
/// branch can live in a fork of that repository, so the two are separate
/// facts and a caller states both. Construction reads the branch out of a
/// fully qualified head ref, so a value of this type always names a branch
/// that repository could hold, and no provider has to guess either half.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "SerializedHead", into = "SerializedHead")]
pub struct ChangeRequestHead {
    repository: Repository,
    branch: String,
}

/// The serialized form of a head, which reads back through the same
/// validation a caller's construction passes.
#[derive(Clone, Deserialize, Serialize)]
struct SerializedHead {
    repository: Repository,
    head_ref: String,
}

impl TryFrom<SerializedHead> for ChangeRequestHead {
    type Error = InvalidHeadRef;

    fn try_from(value: SerializedHead) -> std::result::Result<Self, Self::Error> {
        Self::new(value.repository, &value.head_ref)
    }
}

impl From<ChangeRequestHead> for SerializedHead {
    fn from(value: ChangeRequestHead) -> Self {
        Self {
            head_ref: format!("{BRANCH_REF_PREFIX}{}", value.branch),
            repository: value.repository,
        }
    }
}

impl ChangeRequestHead {
    /// Reads a fully qualified `refs/heads/<branch>` ref in `repository`.
    ///
    /// One spelling rather than two keeps every branch addressable: accepting a
    /// bare branch name as well would leave a branch literally named
    /// `refs/heads/main` unreachable, because that string also qualifies
    /// branch `main`. Written this way it is `refs/heads/refs/heads/main` and
    /// stays distinct.
    pub fn new(
        repository: Repository,
        head_ref: &str,
    ) -> std::result::Result<Self, InvalidHeadRef> {
        Ok(Self {
            repository,
            branch: head_branch(head_ref)?.to_owned(),
        })
    }

    #[must_use]
    pub fn repository(&self) -> &Repository {
        &self.repository
    }

    /// The branch this head names, without its `refs/heads/` qualification.
    #[must_use]
    pub fn branch(&self) -> &str {
        &self.branch
    }
}

fn head_branch(head_ref: &str) -> std::result::Result<&str, InvalidHeadRef> {
    let branch = head_ref
        .strip_prefix(BRANCH_REF_PREFIX)
        .ok_or(InvalidHeadRef::NotABranchRef)?;
    if branch.is_empty() {
        return Err(InvalidHeadRef::NoBranch);
    }
    if creatable_branch_name(branch) {
        Ok(branch)
    } else {
        Err(InvalidHeadRef::InvalidBranchName)
    }
}

/// Whether git would create a branch of this name, by the rules
/// `git check-ref-format` applies to `refs/heads/<branch>`.
fn creatable_branch_name(branch: &str) -> bool {
    if branch == "@"
        || branch == "HEAD"
        || branch.starts_with('-')
        || branch.ends_with('.')
        || branch.contains("..")
        || branch.contains("@{")
    {
        return false;
    }
    if branch.chars().any(|character| {
        character.is_ascii_control()
            || character == ' '
            || FORBIDDEN_BRANCH_CHARACTERS.contains(&character)
    }) {
        return false;
    }
    branch.split('/').all(|component| {
        !component.is_empty() && !component.starts_with('.') && !component.ends_with(".lock")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox() -> Repository {
        Repository::new("civitas-forge", "sandbox").expect("repository")
    }

    #[test]
    fn head_reads_one_ref_spelling_so_every_branch_stays_addressable() {
        for (head_ref, branch) in [
            ("refs/heads/main", "main"),
            ("refs/heads/feat/open-request", "feat/open-request"),
            ("refs/heads/refs/heads/main", "refs/heads/main"),
        ] {
            let head = ChangeRequestHead::new(sandbox(), head_ref).expect("branch ref");
            assert_eq!(head.branch(), branch);
            assert_eq!(head.repository(), &sandbox());
        }
    }

    #[test]
    fn head_states_why_a_string_names_no_branch() {
        for unqualified in ["", "main", "refs/tags/v1.1.0", "refs/remotes/origin/main"] {
            assert_eq!(
                ChangeRequestHead::new(sandbox(), unqualified),
                Err(InvalidHeadRef::NotABranchRef),
                "{unqualified:?}"
            );
        }
        assert_eq!(
            ChangeRequestHead::new(sandbox(), "refs/heads/"),
            Err(InvalidHeadRef::NoBranch)
        );
        for uncreatable in [
            "refs/heads/@",
            "refs/heads/HEAD",
            "refs/heads/-topic",
            "refs/heads/main.",
            "refs/heads/ma..in",
            "refs/heads/ma@{in",
            "refs/heads/ma:in",
            "refs/heads/ma in",
            "refs/heads/main\n",
            "refs/heads/ma~in",
            "refs/heads/ma[in",
            "refs/heads/ma\\in",
            "refs/heads/feat//open",
            "refs/heads/feat/",
            "refs/heads//feat",
            "refs/heads/feat/.hidden",
            "refs/heads/feat/open.lock",
        ] {
            assert_eq!(
                ChangeRequestHead::new(sandbox(), uncreatable),
                Err(InvalidHeadRef::InvalidBranchName),
                "{uncreatable:?}"
            );
        }
    }

    #[test]
    fn head_keeps_the_branch_characters_git_permits() {
        for permitted in [
            "refs/heads/mai\u{00a0}n",
            "refs/heads/feature.lockfile",
            "refs/heads/rele.ase",
            "refs/heads/ma@in",
            "refs/heads/feat/-topic",
            "refs/heads/feat/HEAD",
        ] {
            assert!(
                ChangeRequestHead::new(sandbox(), permitted).is_ok(),
                "{permitted:?}"
            );
        }
    }
}
