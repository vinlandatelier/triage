# Triage

Autonomous agents that handle life admin for you end-to-end. You just review and approve.

Powered by your Claude Code subscription.

## Development

```sh
brew install just && just setup
```

`just check` runs every quality gate. `just dev` serves the app at `http://localhost:7311`: Vite for the UI, with `/api` proxied to the daemon on 7312, both reloading on change.

## License

MIT
