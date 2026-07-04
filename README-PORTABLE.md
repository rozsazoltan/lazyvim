# Portable LazyVim package

Run the `lazyvim` executable from any project directory:

```bash
./lazyvim .
```

On Windows:

```powershell
.\lazyvim.exe .
```

By default, all LazyVim config, plugins, cache, state, Mason packages, and lock files are stored under:

```text
~/.lazyvim
```

Override the location with:

```bash
LAZYVIM_HOME=/path/to/lazyvim-home ./lazyvim
```

or:

```bash
./lazyvim --home /path/to/lazyvim-home
```

Useful commands:

```bash
lazyvim where
lazyvim doctor
lazyvim sync
lazyvim restore
lazyvim update
lazyvim clean
```
