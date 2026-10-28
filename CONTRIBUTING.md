I would love it!

Just dont try to add crazy options...

...like alternate keys. monster
## Releasing

Bump `version` in `Cargo.toml` and `herdr-plugin.toml` (the release fails if they disagree with the tag), then push a tag:

```sh
git tag v0.1.1 && git push origin v0.1.1
```

The `release` workflow builds macOS and Linux binaries, and attaches them to a GitHub release. `scripts/install.sh` downloads the one matching the plugin version at install time, and only compiles from source when there isn't one.
