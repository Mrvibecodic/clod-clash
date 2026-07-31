# Update Log

The English changelog shown inside the app's update dialog. The publish
workflow (`publish-clod.yml`) extracts the section matching the released
version and injects it into `latest.json` as the updater notes. Keep every
section between `## v{version}` and `---`.

## v0.0.4-alpha

### Fixed

- The **Update** button in the new-version dialog did nothing when the release notes were empty
- **Go to Release Page** opened a wrong (upstream) repository
- Removed the internal `COMPATIBLE` placeholder from server names on the home screen

### Changed

- Subscriptions page cleaned up: only your subscriptions, with support and account buttons on each card
- Settings reorganized into a single column — everyday options on top, advanced ones below
- IPv6 is now disabled by default on desktop
- The update dialog now shows this changelog

---

## v0.0.3-alpha

### Added

- Server picker with proxy-group chips in a row
- Latency shown right on the home screen server row
- Session traffic counters (downloaded / uploaded since start)
- The window remembers its size separately for simple and advanced mode

### Changed

- Fewer tiles on the advanced home screen; technical sections moved to Settings

---
