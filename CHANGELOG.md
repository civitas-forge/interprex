# Changelog

## Unreleased

- A GitHub review thread whose end line a commit deleted is read as a thread
  with no current position instead of failing the whole observation. GitHub
  reports such a thread with a null `line` and the `startLine` it began at,
  which happens whenever a finding is acted on by deleting the code it named;
  one of them previously made every read of the change request return
  `unrepresentable github data: review range has a start line without an end line`.

## 7.0.1

- Repeating a submitted GitHub review publication recognizes its reviewer,
  publication key, full-submission digest and disposition after GitHub retargets
  the attached head or dismisses its decision. A retry leaves a dismissed
  decision withdrawn. New and pending publications still require the requested
  revision.
- Corrected the review-revision documentation: the observed attached head can
  change after submission and does not establish an immutable reviewed range.

## 7.0.0

- Breaking: open change-request observations on private GitHub repositories
  require Contents read permission in addition to Pull requests read, because
  they now read the current target branch directly.
- `Mergeability::Unknown` now means no answer is available for the observed
  source and target, including an answer discarded because it described an older
  target revision.

- Open GitHub change requests and their branch-freshness observations read the
  target branch directly, so a stale base SHA in the pull-request response does
  not break review loops after another pull request merges. Historical review
  revisions remain unchanged. A mergeability answer for the old base becomes
  unknown until GitHub reports the current base. Closed and merged requests
  retain their historical base for both observations.
- Breaking: `ProviderError::BranchRevisionChanged` distinguishes a moved target
  from a missing branch and includes the repository, branch, expected SHA and
  observed SHA. Consumers with exhaustive error matches must handle it.

## 6.1.0

### Credential errors

- Breaking: `ProviderError::MissingCredential` carries the configuration entry
  the operation wanted and the source the provider read, so a missing app
  credential reports `[provider.github.apps.<slug>]` and the `.interprex.toml`
  path consulted instead of the slug alone. Consumers that construct or
  exhaustively match the variant supply the new `entry` and `origin` fields.

### Review publication

- `interprex-github` recovers a review publication that a pending review from an
  earlier round blocks. GitHub accepts one pending review per author per change
  request, so a review left pending under a publication key no later submission
  matches rejected every create that followed, until a person deleted it. The
  provider now deletes that review and creates once more. It deletes no review
  that is submitted or authored by anyone else, and it reuses, rather than
  deletes, a pending review carrying the submission's own publication key.

### Review dismissal

- Added `ReviewPublishingProvider::dismiss_review`, which withdraws the decision
  a published review carries and records a `ReviewDismissalMessage` as the
  visible reason for it. The review keeps its summary and findings; only its
  disposition becomes dismissed.
- The GitHub provider dismisses through the reviewer's named App credential. It
  reads the review before writing, refuses one another reviewer identity
  published and one carrying no decision, answers an already dismissed review
  with success, and reads the review back so a lost dismissal response
  reconciles rather than fails.
- `interprex-test` applies dismissals to seeded reviews and reports every
  dismissal that withdrew a decision through `FakeProvider::review_dismissals`.

## 5.0.0

### Breaking changes

- Replaced the lossy provider-neutral `Ruleset` and `RequiredCheck` values and
  the `CodeHostingProvider::rulesets` and `upsert_ruleset` methods with
  `SourceCodeConfigurationProvider` and `AppliedSourceRequirementsProvider`.
  Provider implementations remove the deleted methods, implement the complete
  provider-specific configuration type, and expose exact-revision policy facts
  through the object-safe applied-requirements capability.
- Moved `BranchUpdateRequirement` into source-code configuration and removed it
  from `BranchUpdateObservation`. Consumers combine the applied requirement with
  code-review branch freshness when deciding whether to update.

### GitHub source configuration

- `interprex-github` now expands paginated repository ruleset summaries through
  their detail reads and applies complete repository-owned branch, tag and push
  rulesets. Inherited, incomplete and unknown writable forms return explicit
  errors instead of becoming partial configuration.
- `interprex-github` now reads branch-applicable rules and classic protection,
  then matches their required checks against check runs and legacy commit
  statuses at an exact head revision. Application-bound requirements retain
  their GitHub App identity; incomplete and unreadable provider answers return
  explicit errors.

### Branch updates

- Added `BranchUpdatesProvider` for observing whether an exact change-request
  head contains the target-branch tip and for requesting an update only while
  that observed head remains current. The GitHub provider distinguishes a stale
  head from provider lookup, credential and refusal errors.

### Test provider

- `interprex-test` can seed an applied-requirements observation or provider
  error for an exact repository, target branch, base revision and head revision.
- `interprex-test` can seed and exercise exact change-request branch-update
  observations and operations.

## 4.0.1

### Changed

- `interprex-github` now signs GitHub App JWTs with Octocrab's AWS-LC backend,
  removing the advised `rsa` crate from its dependency graph. Public APIs and
  the `.interprex.toml` App credential format are unchanged.

## 4.0.0

### Breaking change

- `ReviewCommentId` no longer implements `Ord` or `PartialOrd` because provider
  identifiers carry no provider-neutral ordering meaning. Consumers must retain
  the stable provider order of comment collections instead of sorting or
  comparing comment IDs.

### Added

- Added `ReviewPublishingProvider`, complete review-submission models, and a
  stateful fake implementation for consumer tests.
- Added GitHub review publication through named App credentials. The provider
  creates each complete pending review in one request and can resume it from a
  retained publication key after an interrupted caller loses the submission.
- Scoped publication keys by repository, change request, provider application
  ID, and bot actor ID. Distinct reviewer identities can reuse a key.
- Defined stable provider order for review comment collections while keeping
  comment identifiers opaque and non-orderable.
