# Rollback plan

HEAD: `7a7e42ed6ee5d4f36ed4c0a7d2ed2caf3dbaa007`
Base: `HEAD~1` (`43f09bbc40d9c509f4de399f9a9ecc1927377c97`)

Reverts are listed newest → oldest. Each `git revert` was
dry-run via `git merge-tree --write-tree` against current HEAD,
so the working tree was not touched during verification.

| # | sha | revertable | command | subject |
|---|---|---|---|---|
| 1 | `7a7e42e` | ✓ | `git revert 7a7e42e` | iter-1: fix API compat, clippy, test issues — all gates green |

## Notes

- `7a7e42e` — clean revert
