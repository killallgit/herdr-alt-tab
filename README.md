# herdr-alt-tab

[unnecessary ci status badges]

`<insert-ai-generated-image-of-a-terminal-with-a-purple-gradient>`

Adds the ability to use `alt+tab` and `shift+alt+tab` to cycle through panes in recent order like a normal person. There are no options to configure. There's no flashy picker-overlay or fuzzy-find AI b*llshit.

## Install

```sh
herdr plugin install killallgit/herdr-alt-tab
```

The install downloads a prebuilt binary for your platform, and only falls back to compiling with `cargo` when there isn't one. It automatically binds `alt+tab` and `alt+shift+tab` in your Herdr config. If that step fails or you've got a problem with this, run it yourself.

```sh
herdr plugin action invoke killallgit.herdr-alt-tab.setup-keys
```

To take the bindings out again, (for instance before uninstalling).

```sh
herdr plugin action invoke killallgit.herdr-alt-tab.remove-keys
```
