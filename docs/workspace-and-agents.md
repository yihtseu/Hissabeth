# Workspace and agents

## Desktop editing

The desktop workspace has a menu bar, command toolbar, view navigator, analysis area, resizable prescription dock, and status bar. Narrow windows use a navigator drawer and keep every view available through the toolbar selector; wide tables scroll within their dock. Reka UI supplies menus, toolbars, dialogs, and settings tabs; Splitpanes handles dock resizing, VueUse handles window/media preferences, Pinia shares local state, and Vue I18n provides Simplified Chinese and US English. The frontend builds with TypeScript 6.

Open **Edit** to change system properties, object and image profiles, surface rows, materials, apertures, and gap solves on the shared draft. **Configurations** edits the configuration table and active selection. Apply the draft atomically with **Ctrl+S**, the Edit menu, or the toolbar. **Ctrl/Cmd+B** toggles the navigator and **Ctrl/Cmd+J** toggles the prescription dock.

The **Assistant** sidebar supports conversation-driven editing of the same project through MCP. Apply or revert an outstanding desktop draft before sending a conversation turn. Saved views refresh automatically after MCP writes. A stale revision blocks a conflicting save, and analyses use the revision returned by the engine. Desktop views discard superseded or mismatched replies and retain sampling and spectral controls across saved-project refreshes.

Agents stage single or multiple surface insertions and contiguous removals through the read-only `surface_edit` transform, then save with `system_set`. The transform retains 1–128 internal rows and maps the physical stop, pickup sources, and half-open overall-length intervals. Retained solves cannot reference removed gaps; deleting their owners in the same range is allowed. Removing the stop selects the next retained row, or the final preceding row. Insertions create plane, unclipped, zero-gap dummy surfaces in the incident medium. Saving resolves constraints and drawing envelopes atomically. The transform rejects drafts carrying configuration overrides, since operand surface indexes are not remapped.

## Revision history and project files

The workspace retains up to 64 in-memory optical history snapshots. Use `history_list` for entries and undo/redo availability, `history_undo` and `history_redo` to step through edits, and `history_restore` to restore a snapshot by index. `history_compare` defaults to the previous and current snapshots and reports per-surface prescription changes and deltas in focal length, back focal length, f-number, and RMS spot. History mutations require the current revision. Restoring a snapshot restores the prescription in the active project. Use the project tools to switch the linked file.

A workspace can hold multiple optical projects with one active prescription. `project_list` lists the active linked file, files in the workspace projects directory, and recent paths. `project_open` opens native Hissabeth JSON or ZMX and links a native project for subsequent saves. `project_save_as` saves the current prescription to a new native file and links subsequent saves to it. Relative Save As paths resolve inside the workspace projects directory. Use the current revision when opening or saving a project.

## Settings and preferences

Open the separate native Settings window with **Tools → Settings** or **Ctrl/Cmd+,**. **Tools → Agent connections** opens its Agents page. Opening Settings again focuses the existing window. Apply saves the form; Cancel reloads saved preferences. Settings changes reach both windows after a successful write. Edits made during saving remain in the form, while an external update preserves an edited form for explicit reload. Failed writes retain the unsaved form. Invalid settings files show a warning and remain unchanged until Apply, including when saving the displayed defaults.

| Settings page     | Active controls                                                                                       |
| ----------------- | ----------------------------------------------------------------------------------------------------- |
| General           | Interface language, startup view, restore last view                                                   |
| Appearance        | System/light/dark/high-contrast theme, blue/teal/amber accent, density, font size, reduced motion     |
| Workspace         | Navigator, prescription dock, metric/status bars, dock proportions                                    |
| Display           | Summary metric decimals, ray direction arrows, plot grids                                             |
| Analysis defaults | Grid/hexapolar sampling, grid points/rings, report precision, engine polling interval                 |
| Agents            | Detect installed CLIs, connect selected Codex/Claude Code agents, selected-agent startup registration |

Preferences use Tauri Store in `preferences.json` beside the optical workspace, with a separate revision for window conflicts. They do not change the optical prescription revision. Sampling defaults update controls still using the previous default and preserve manual selections. Theme changes redraw plots; contrast outlines preserve saved field and wavelength colors. UI report labels translate; canonical report text downloads remain in English. Raw project JSON, material names, and engine diagnostic details retain their source text.

## Connect agent clients

### Codex and Claude Code

Keep the desktop app or the headless engine running while using an external agent. In Settings → Agents, select Codex and/or Claude Code and choose Connect selected agents. Registration compares structured commands, arguments, and workspace paths, keeps one `hissabeth` entry, and removes duplicate aliases for the current workspace. Repeating registration makes no configuration changes. A name occupied by a different service blocks registration. Startup registration is off by default and runs only for saved selected agents when enabled. Restart an external agent after changing its registration.

The **Assistant** toolbar button toggles a resizable right sidebar alongside the optical views and prescription dock. Narrow windows show it as a right overlay. The sidebar uses the respective agent SDKs and provides an agent selector, a model dropdown populated from the selected local agent, streaming replies, expandable tool activity, and Stop. **Refresh models** reloads the catalog; **Custom model…** accepts an explicit model ID. Changing the model starts a separate conversation and retains the composer draft. Selecting history restores its model. Install Node.js 22 or later and the selected agent CLI, and configure its authentication. The build bundles the SDK bridge into `dist/agent-bridge.mjs` and includes it in desktop resources. Each conversation injects the current workspace's MCP service directly. Credentials stay in the local agent environment. [Codex SDK](https://developers.openai.com/codex/sdk/) · [Claude Agent SDK](https://code.claude.com/docs/en/agent-sdk)

The Codex bridge explicitly sets `default_tools_approval_mode = "approve"` for its injected `hissabeth` server so optical reads and writes can execute with its noninteractive approval policy. Approval applies to that conversation’s Hissabeth service. The bridge disables shell access and uses a read-only filesystem sandbox.

### ChatGPT

For local noncommercial use, run `bun run setup:chatgpt` from this repository with Git, Bun, and Node.js 22 or later. This installs the official [Sign-in with ChatGPT DevKit](https://github.com/openai/sign-in-with-chatgpt-devkit) separately in Hissabeth’s user configuration directory. Select **ChatGPT** in **Assistant**, choose **Continue with ChatGPT**, and authorize ChatGPT plan usage in the browser. Select a model to chat. The account section shows your connected identity and opens ChatGPT’s usage settings. ChatGPT receives your conversation messages and provides text replies. Select Codex or Claude for optical workspace tools. Credentials stay in the local bridge and are encrypted with a key held by the OS credential store. On Linux, an unlocked Secret Service keyring is required. The DevKit’s [noncommercial license](https://github.com/openai/sign-in-with-chatgpt-devkit/blob/main/LICENSE) is included with the separate installation. The sign-in and account controls use the DevKit’s [official ChatGPT SVGs](https://github.com/openai/sign-in-with-chatgpt-devkit/tree/main/assets/brand).

### Conversation history

The history list stores conversation titles, agents, models, messages, and backend resume tokens in `chat-history.json` beside the optical workspace. Selecting a conversation restores its messages and continues its SDK context against the current saved project, including after an application restart. New conversation and agent/model changes create a separate conversation. Closing the sidebar retains the active turn and composer draft. Stop retains completed optical writes. History writes replace the file atomically; an unreadable history file surfaces an error and blocks new writes.

## MCP transport and broker

The application stores the prescription and endpoint descriptor under `$XDG_DATA_HOME/hissabeth` (default `~/.local/share/hissabeth`) on Unix and `%LOCALAPPDATA%/hissabeth` on Windows. `HISSABETH_DATA_DIR` selects a workspace; the desktop and agents must select the same one. The broker binds to loopback and authenticates with an OS-generated 256-bit token. Workspace leases prevent concurrent writers; atomic saves and revision checks protect edits. The relay rereads the endpoint for each request so an existing relay can reconnect after an engine restart.

Tool results contain one JSON document in MCP text content, including array results. The relay reads complete outbound replies. Incoming request limits and socket timeouts still apply.

The executable provides `--engine` for the headless broker, `--mcp` for the stdio relay, and `--detect-agents` and `--configure-agents` for client registration. Start the broker by launching the desktop app or running `--engine`, then configure the client to launch the same executable with `--mcp` and the same `HISSABETH_DATA_DIR`.

Read `workspace_status` first and `system_get` before editing. Every mutating tool requires the latest revision. Use the broker for writes to the active project. `capabilities` returns the tool schemas and current scope. For example, ask the agent to read the prescription, design a BK7 singlet with a 50 mm focal length, focus it, and report the RMS spot radius.

### Recovery

Invalid saved projects open a connected recovery workspace in the desktop app and headless broker. The interface shows the reason and project path and opens the conversation for repair. Agents use `workspace_retry` or `workspace_recover`; optical tools remain blocked until recovery succeeds. Replacement preserves the original under a unique `system.rejected-*.json` path. Changed disk contents and stale recovery revisions reject the request. Unreadable files require access repair, and a missing project with recovery artifacts stays in recovery.

## Live agent exercises

`python3 src-tauri/tests/agent_live.py` runs the installed Claude Code client against an isolated workspace in `src-tauri/target`, using the existing executable. Add `--bench` to evaluate free-space design, obstruction diagnosis, repair, and assembly export; `--model MODEL_ID` selects an explicit model. Otherwise the client's configured model is used. `--bench-contract` scopes evaluation to mount fields and persistence. The harness checks actual MCP calls, stale-write rejection, saved parameters, revisions, and disk/export equality independently of the final answer. Transcripts and evidence remain beside the test workspace. It requires model-service access and a loopback socket; it does not register a personal MCP server.
