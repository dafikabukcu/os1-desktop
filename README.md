# OS1 Windows Client

Windows-first OS1 prototype built with Tauri, React, TypeScript, and Rust.

## Run The Web Preview

```powershell
npm.cmd install
npm.cmd run dev
```

Open `http://127.0.0.1:1420`.

The web preview renders the UI and boot sequence, but live Realtime voice is only enabled inside Tauri because the OpenAI API key is kept on the Rust side.

## Run The Tauri App

Create `.env` from the example and put your OpenAI key there:

```powershell
Copy-Item .env.example .env
notepad .env
npm.cmd run tauri:dev
```

The `Begin` control starts a WebRTC Realtime session, requests microphone access, sends the SDP offer to Rust, and applies the SDP answer returned by OpenAI.

## Current State

- Boot sequence: ported from the original OS1 Three.js helix/ring animation.
- Voice surface: real browser WebRTC flow wired to a Tauri Realtime call command.
- Hermes detection: WSL-first runtime detection with selected profile status.
- Hermes profiles: first-run naming creates an OS1-managed profile under WSL
  `~/.hermes/profiles/<name>` and stores the selected identity locally.
- Hermes providers: setup and Settings can configure either Codex subscription
  auth (`openai-codex`, `gpt-5.5`) or the OpenAI API key (`openai`, `gpt-5.5`).
- Terminal and doctor: Settings includes a profile-aware terminal and Hermes
  doctor runner using the selected `HERMES_HOME`.
- Credential storage: development still reads `OPENAI_API_KEY` from `.env`.
  Codex auth import reads `%USERPROFILE%\.codex\auth.json` and writes profile
  auth into WSL. Product-grade encrypted credential storage is still pending.

## Windows Hermes Setup

OS1 expects WSL to be available. If Hermes is missing or outdated, first-run
setup offers install/repair actions. After the assistant name is confirmed,
OS1 checks the selected WSL distro, creates the profile, configures the chosen
provider, and verifies Hermes with a short `hermes chat` call before entering
the main surface.

Generated WSL state is not part of the repo:

- `~/.hermes/hermes-agent`
- `~/.hermes/profiles/<assistant>`
- `~/.hermes/profiles/<assistant>/.env`
- `~/.hermes/profiles/<assistant>/auth.json`
- `%USERPROFILE%\.codex\auth.json`
