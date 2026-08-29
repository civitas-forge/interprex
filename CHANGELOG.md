# Changelog

## Unreleased

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
