# Repository instructions

These instructions apply to every agent working anywhere in this repository.

## Checkout setup

Run `lefthook install` before editing files. Repeat it in every new clone or
agent tree; the command is idempotent.

If Lefthook or another quality tool is unavailable, install the repository's
pinned tools and activate them before installing the hook:

```sh
scripts/install-quality-tools .quality-tools
export PATH="$PWD/.quality-tools/bin:$PATH"
lefthook install
```

Do not bypass hooks with `LEFTHOOK=0` or `git commit --no-verify`.

## Verification

`scripts/quality` is the repository's single quality command. Run it before
committing. The pre-commit hook and GitHub Actions call the same command.
