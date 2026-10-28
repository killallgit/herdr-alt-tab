# herdr-alt-tab

Alt+Tab for [Herdr](https://herdr.dev): hold Alt and press Tab to walk back through panes in recently used order, across workspaces. Alt+Shift+Tab walks forward. Tap Alt+Tab, let go, tap again to toggle between two panes.

No options.

## Install

```sh
herdr plugin install killallgit/herdr-alt-tab
```

Needs `cargo` to build. The install also binds `alt+tab` and `alt+shift+tab` in your Herdr config. If that step fails, run it yourself once the problem is fixed:

```sh
herdr plugin action invoke killallgit.herdr-alt-tab.setup-keys
```

To take the bindings out again, for instance before uninstalling:

```sh
herdr plugin action invoke killallgit.herdr-alt-tab.remove-keys
```
