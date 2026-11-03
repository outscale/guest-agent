# 📜 Changelog

All notable changes to this project will be documented in this file.  
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)  
and this project adheres to [Semantic Versioning](https://semver.org/).

---

## [Unreleased]

### 💥 Breaking
- (nothing yet)

### ✨ Added
- (nothing yet)

### 🛠️ Changed / Refactoring
- (nothing yet)

### 📝 Documentation
- `docs/hooks.md`: describe per-phase discovery, `MOUNT_POINT` (a block device),
  the actual time limits and SIGKILL scope, and the shipped example hooks and
  their order. Link it from the README.

### ⚰️ Deprecated
- (nothing yet)

### 🗑️ Removed
- (nothing yet)

### 🐛 Fixed
- Example snapshot hooks: the pod `completed-command` now runs after the
  filesystem thaw (`snapshot-completed.d/40-pod-exec` --> `90-pod-exec`), instead
  of on a still-frozen filesystem. `snapshot-commit.d/40-pod-exec` is renamed to
  `90-pod-exec` too, which also fixes the `.deb` conf files that already pointed
  to `90-pod-exec`. If you enabled the old `40-pod-exec` in these directories,
  rename it to `90-pod-exec`.

### 🔒 Security
- (nothing yet)

### 📦 Dependency updates
- (nothing yet)

### 🌱 Others
- (nothing yet)

---

## [1.0.0] - YYYY-MM-DD

### ✨ Added
- Initial release of the project.

---

## 🔑 How to use this file

1. During development:
   - Add entries under **[Unreleased]** in the right category.
   - Keep the wording concise and clear.
   - Link related issues/PRs where relevant (`#123`, [PR-456](https://github.com/org/repo/pull/456)).

2. When preparing a release:
   - Move items from **[Unreleased]** into a new version section (`[X.Y.Z] - YYYY-MM-DD`).
   - Leave **[Unreleased]** empty for future changes.

3. At release time:
   - GitHub automatically generates release notes from PR labels (`.github/release.yml`).
   - This `CHANGELOG.md` is the **permanent record** in the repository.