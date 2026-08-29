# Changelog

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
