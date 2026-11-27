---
name: changelog
description: Write or update the bilingual changelog (CHANGELOG.vi.md + CHANGELOG.md, Keep a Changelog) of Vietnamese's Ultimate W App – add user-visible changes to [Unreleased], or prepare the notes of a release before `npm run bump`. Use when the user asks for a changelog, release notes, "có gì mới", or to prepare / publish a new version.
---

# Changelog (Keep a Changelog 1.1.0, two files)

The notes are read by `src/lib/changelog.js` (app: Settings → Có gì mới, the toast after an update, the update bar) and
`scripts/release-notes.mjs` (GitHub Release body: Vietnamese on top, English below, then install steps + SHA-256).
Both files MUST keep the same versions, the same sections and the same number of lines, in the same order.

| File | Language | Section names (in this order, only the ones that have items) |
| --- | --- | --- |
| `CHANGELOG.vi.md` | Vietnamese | `### Thêm mới`, `### Thay đổi`, `### Ngừng hỗ trợ`, `### Đã gỡ bỏ`, `### Sửa lỗi`, `### Bảo mật` |
| `CHANGELOG.md` | English | `### Added`, `### Changed`, `### Deprecated`, `### Removed`, `### Fixed`, `### Security` |

Entry shape:

```markdown
## [0.3.1] - 2026-12-01

One-sentence summary of the release (optional; becomes the Release heading).

### Added

- **Short bold lead.** One or two sentences: what the user sees, where (page / button), numbers when there are any.
```

## Steps

1. **Collect.** Last tag: `git describe --tags --abbrev=0`. Then `git log --no-merges --pretty='%h %s' <tag>..HEAD` and
   `git diff --stat <tag>..HEAD -- src src-tauri mcp`; open the diffs of anything whose effect on the user is unclear.
   Read the current `## [Unreleased]` of both files – items already there were written when the change was made; keep them
   unless they are wrong.
2. **Filter.** Keep only what a user notices: features, visible behaviour, speed / RAM (with numbers when measured), fixes
   of bugs a user could hit, security. Drop CI, refactors, renames, docs, dependency bumps (unless they fix a user-facing or
   security problem), internal tests.
3. **Write** both files under `## [Unreleased]` (or under the version if it was already bumped). Plain words from the user's
   side, no function / file names; name pages and buttons as the UI does (Vietnamese labels from `src/lib/i18n/vi`, English
   from `src/lib/i18n/en`). One change per line, bold lead ending with a period. Vietnamese first, then translate the same
   items into English (not word for word – natural English).
4. **Release** (only when the user asks for a version): `npm run bump <x.y.z|patch|minor|major>` turns `[Unreleased]` into
   `[x.y.z] - date` in both files and fixes the compare links at the bottom. Add the summary line if missing.
5. **Check:** `npm run notes [version]` prints the Release body – both languages present, no empty sections, no `- ` stubs.
6. Don't commit unless the user asks (commit message: `chore: release vX.Y.Z` / `docs: update changelog`).
