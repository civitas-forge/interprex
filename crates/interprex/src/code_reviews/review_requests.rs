use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{ReviewActor, ReviewRequestId, ReviewTeam};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewTarget {
    Actor(ReviewActor),
    Team(ReviewTeam),
    Unavailable,
}

/// One provider address to add to the outstanding reviewer set.
///
/// User and bot values are logins. A team value is its canonical provider
/// identifier, such as `organization/team-slug` on GitHub. Targets observed
/// in a read can contain richer facts or unavailable identities, so writes use
/// this deliberately narrower shape.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewRequestTarget {
    User(String),
    Bot(String),
    Team(String),
}

/// What a provider can currently observe at one review-request address.
///
/// A matching result confirms only that the address resolves to the requested
/// category of actor or team. It does not establish that the identity can be
/// assigned to a particular change request, that an application is installed,
/// or that a later review request will be delivered.
///
/// [`Self::NotResolvable`] means the target was not found or was not visible
/// to the credentials used for the inspection. It is not proof that the target
/// does not exist.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewRequestTargetInspection {
    Matching(ReviewTarget),
    KindMismatch(ReviewTarget),
    NotResolvable,
}

impl ReviewRequestTargetInspection {
    /// Classifies a provider observation against the category the caller
    /// requested.
    ///
    /// Providers pass through the identity and category they observed. This
    /// constructor owns the matching rules so adapters never infer an actor
    /// category from [`ReviewRequestTarget`].
    #[must_use]
    pub fn from_observation(requested: &ReviewRequestTarget, observed: ReviewTarget) -> Self {
        let matching = match (requested, &observed) {
            (
                ReviewRequestTarget::User(_),
                ReviewTarget::Actor(ReviewActor {
                    kind: super::ReviewActorKind::User | super::ReviewActorKind::EnterpriseUser,
                    ..
                }),
            )
            | (
                ReviewRequestTarget::Bot(_),
                ReviewTarget::Actor(ReviewActor {
                    kind: super::ReviewActorKind::Bot,
                    ..
                }),
            )
            | (ReviewRequestTarget::Team(_), ReviewTarget::Team(_)) => true,
            (_, ReviewTarget::Unavailable) => return Self::NotResolvable,
            _ => false,
        };
        if matching {
            Self::Matching(observed)
        } else {
            Self::KindMismatch(observed)
        }
    }
}

/// One outstanding request, including one whose target became unavailable.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewRequest {
    pub id: ReviewRequestId,
    pub target: ReviewTarget,
    /// The provider address that can request this target again, when one is
    /// available. This is independent of the target's observed actor or team
    /// category.
    pub request_target: Option<ReviewRequestTarget>,
    /// When the platform recorded the request that is still outstanding.
    ///
    /// A platform can list its outstanding requests without timing them, so a
    /// provider reads the request events separately and matches each
    /// outstanding request to the event that created it. A provider reports
    /// `None` when that match fails: the request predates the retained event
    /// history, or the target carries no identity to match against, which is
    /// the case for every [`ReviewTarget::Unavailable`] a provider returns. A
    /// provider never substitutes a nearby timestamp, so a caller measuring
    /// how long a request has been outstanding reads `None` as no measurement
    /// rather than an approximate one.
    ///
    /// Where the outstanding requests and the request events are separate
    /// reads with no snapshot across them, this is the time on the target's
    /// latest surviving request event when the events were read: a target
    /// re-requested between the two reads reports the newer request's time.
    pub requested_at: Option<DateTime<Utc>>,
    pub as_code_owner: bool,
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::{ReviewActorId, ReviewActorKind, ReviewTeamId, ReviewTeamKind};

    fn actor(kind: ReviewActorKind) -> ReviewTarget {
        ReviewTarget::Actor(ReviewActor {
            id: ReviewActorId::new("actor").expect("actor id"),
            login: "reviewer".to_owned(),
            kind,
        })
    }

    #[test]
    fn inspection_classifies_actual_actor_kinds() {
        for kind in [ReviewActorKind::User, ReviewActorKind::EnterpriseUser] {
            let observed = actor(kind);
            assert_eq!(
                ReviewRequestTargetInspection::from_observation(
                    &ReviewRequestTarget::User("reviewer".to_owned()),
                    observed.clone(),
                ),
                ReviewRequestTargetInspection::Matching(observed)
            );
        }

        for kind in [
            ReviewActorKind::Bot,
            ReviewActorKind::Placeholder,
            ReviewActorKind::Organization,
        ] {
            let observed = actor(kind);
            assert_eq!(
                ReviewRequestTargetInspection::from_observation(
                    &ReviewRequestTarget::User("reviewer".to_owned()),
                    observed.clone(),
                ),
                ReviewRequestTargetInspection::KindMismatch(observed)
            );
        }

        let bot = actor(ReviewActorKind::Bot);
        assert_eq!(
            ReviewRequestTargetInspection::from_observation(
                &ReviewRequestTarget::Bot("reviewer".to_owned()),
                bot.clone(),
            ),
            ReviewRequestTargetInspection::Matching(bot)
        );
        for kind in [
            ReviewActorKind::User,
            ReviewActorKind::EnterpriseUser,
            ReviewActorKind::Placeholder,
            ReviewActorKind::Organization,
        ] {
            let observed = actor(kind);
            assert_eq!(
                ReviewRequestTargetInspection::from_observation(
                    &ReviewRequestTarget::Bot("reviewer".to_owned()),
                    observed.clone(),
                ),
                ReviewRequestTargetInspection::KindMismatch(observed)
            );
        }
    }

    #[test]
    fn inspection_classifies_actor_team_and_unavailable_boundaries() {
        let team = ReviewTarget::Team(ReviewTeam {
            id: ReviewTeamId::new("team").expect("team id"),
            slug: "maintainers".to_owned(),
            name: "Maintainers".to_owned(),
            kind: ReviewTeamKind::Organization,
        });
        assert_eq!(
            ReviewRequestTargetInspection::from_observation(
                &ReviewRequestTarget::Team("example/maintainers".to_owned()),
                team.clone(),
            ),
            ReviewRequestTargetInspection::Matching(team.clone())
        );
        assert_eq!(
            ReviewRequestTargetInspection::from_observation(
                &ReviewRequestTarget::Bot("maintainers".to_owned()),
                team.clone(),
            ),
            ReviewRequestTargetInspection::KindMismatch(team)
        );
        let user = actor(ReviewActorKind::User);
        assert_eq!(
            ReviewRequestTargetInspection::from_observation(
                &ReviewRequestTarget::Team("example/maintainers".to_owned()),
                user.clone(),
            ),
            ReviewRequestTargetInspection::KindMismatch(user)
        );
        assert_eq!(
            ReviewRequestTargetInspection::from_observation(
                &ReviewRequestTarget::Team("example/maintainers".to_owned()),
                ReviewTarget::Unavailable,
            ),
            ReviewRequestTargetInspection::NotResolvable
        );
    }

    #[test]
    fn observed_target_kind_and_request_address_are_independent() {
        let organization_team = ReviewRequest {
            id: ReviewRequestId::new("request-organization").expect("request id"),
            target: ReviewTarget::Team(ReviewTeam {
                id: ReviewTeamId::new("team-organization").expect("team id"),
                slug: "maintainers".to_owned(),
                name: "Maintainers".to_owned(),
                kind: ReviewTeamKind::Organization,
            }),
            request_target: None,
            requested_at: None,
            as_code_owner: false,
        };
        let enterprise_team = ReviewRequest {
            id: ReviewRequestId::new("request-enterprise").expect("request id"),
            target: ReviewTarget::Team(ReviewTeam {
                id: ReviewTeamId::new("team-enterprise").expect("team id"),
                slug: "security".to_owned(),
                name: "Security".to_owned(),
                kind: ReviewTeamKind::Enterprise,
            }),
            request_target: Some(ReviewRequestTarget::Team("security".to_owned())),
            requested_at: Utc.timestamp_opt(3, 0).single(),
            as_code_owner: false,
        };

        assert_eq!(organization_team.request_target, None);
        assert_eq!(
            enterprise_team.request_target,
            Some(ReviewRequestTarget::Team("security".to_owned()))
        );
        assert_eq!(organization_team.requested_at, None);
        assert_eq!(
            enterprise_team.requested_at,
            Utc.timestamp_opt(3, 0).single()
        );
    }
}
