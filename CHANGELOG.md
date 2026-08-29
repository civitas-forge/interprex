# Changelog

## 3.2.0

- Added `ReviewPublishingProvider`, complete review-submission models, and a
  stateful fake implementation for consumer tests.
- Added GitHub review publication through named App credentials. The provider
  creates each complete pending review in one request and can resume it from a
  retained publication key after an interrupted caller loses the submission.
- Scoped publication keys by repository, change request, provider application
  ID, and bot actor ID. Distinct reviewer identities can reuse a key.
- Defined stable provider order for review comment collections while keeping
  comment identifiers opaque and non-orderable.
