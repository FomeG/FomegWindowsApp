# Backend contract v2 (performance, processes, AI)

Additions to the existing contract (see `src/lib/api.js`). All sizes are bytes, rates are bytes/second,
percentages are 0..100 floats, times are Unix milliseconds unless stated.

## System metrics

### Event `metrics` (every tick; default 1000 ms, see `set_interval`)
```json
{
  "t": 1764000000000,
  "cpu": { "total": 27.4, "cores": [12.0, 30.5, ...], "mhz": 3120 },
  "mem": { "used": 13900000000, "total": 16800000000, "avail": 2900000000,
           "committed": 24000000000, "commit_limit": 32000000000, "cached": 2500000000,
           "paged_pool": 600000000, "nonpaged_pool": 300000000 },
  "disks": [ { "id": "0", "name": "Đĩa 0 (C: D:)", "active": 1.5, "read": 5600000, "write": 130000 } ],
  "nets":  [ { "id": "Intel[R] Wi-Fi 6E AX211 160MHz", "name": "Wi-Fi", "kind": "wifi", "rx": 2400, "tx": 3000 } ],
  "gpus":  [ { "id": "0", "name": "NVIDIA GeForce RTX 3050 Laptop GPU", "util": 8.0,
               "engines": [ { "name": "3D", "util": 8.0 }, { "name": "Copy", "util": 0.0 },
                            { "name": "Video Decode", "util": 0.0 }, { "name": "Video Encode", "util": 0.0 } ],
               "ded_used": 500000000, "ded_total": 4000000000, "shared_used": 100000000, "shared_total": 8000000000 } ],
  "counts": { "processes": 313, "threads": 6052, "handles": 157737 },
  "uptime_s": 15600,
  "battery": { "percent": 100, "charging": false, "plugged": true }
}
```
`battery` is `null` on desktops. `disks`, `nets`, `gpus` keep a stable order and stable `id`s.
`nets` lists only adapters that are up (not loopback / virtual-filter duplicates).

### Command `system_info()` → static facts (call once)
```json
{ "cpu_name": "12th Gen Intel(R) Core(TM) i5-12450H", "cores": 8, "logical": 12, "base_mhz": 2000,
  "mem_total": 16800000000,
  "gpus":  [ { "id": "0", "name": "...", "ded_total": 4000000000, "shared_total": 8000000000 } ],
  "disks": [ { "id": "0", "name": "Đĩa 0 (C: D:)", "model": "KXG60ZNV1T02 NVMe KIOXIA 1024GB", "size": 1024000000000, "ssd": true } ],
  "os": "Windows 11 Home 26H2 26300.1234", "hostname": "DESKTOP-ABC", "user": "An", "boot_ms": 1763990000000,
  "os_installed_ms": 1730000000000,
  "mem_modules": [ { "slot": "Bottom - Slot 1 (left)", "maker": "Micron Technology", "part": "4ATF1G64HZ-3G2F1",
                     "size": 8589934592, "kind": "DDR4", "form": "SODIMM", "speed": 3200 } ],
  "mem_slots": 2, "board": "HP 8A4F",
  "system": "HP Victus by HP Gaming Laptop 15-fa0xxx", "bios": "AMI · F.28 · 08/19/2024" }
```
`mem_modules` / `mem_slots` / `board` / `system` (SMBIOS type 1 maker + product) / `bios` (type 0 vendor · version · date) come from the firmware SMBIOS table (`core/smbios.rs`, ~0.1 ms, read once and cached
with the rest of `system_info`). Any of them may be empty/0 when the firmware doesn't fill them in; `speed` is the configured
speed in MT/s (0 = unknown); JEDEC maker codes like `802C` are mapped to names.

### Command `metrics_history()` → columnar ring buffer (up to 1 h at the current interval)
```json
{ "t": [..], "cpu": [..], "mem": [..used bytes..],
  "disks": { "0": { "active": [..], "read": [..], "write": [..] } },
  "nets":  { "<id>": { "rx": [..], "tx": [..] } },
  "gpus":  { "0": [..util..] } }
```
All arrays have the same length as `t`. A device that appeared later is padded with 0.

### Command `set_interval({ ms })`
`ms` ∈ {500, 1000, 2000, 5000} or `0` = pause updates. Default 1000. Persists for the session.

### Command `set_visible({ visible })`
Frontend calls this on `document.visibilitychange` (and the backend also checks if the window is minimized).
While hidden the backend keeps sampling every `set_idle_interval` ms (default 5 s) for history but emits no `metrics`/`procs` events.

### Command `set_idle_interval({ ms })`
`ms` ∈ {5000 (default), 15000, 60000} or `0` = take no samples while hidden / minimized / paused (the 1 h history gets a gap). The frontend re-sends the saved value (`ui:idleMs`) at startup.

### Command `relaunch_admin()`
Starts the app's own exe with `ShellExecuteEx("runas")` (UAC prompt) and exits this instance. Declined UAC → `Err`, the app keeps running.

### Command `set_locale({ locale })`
`"vi"` (default) or `"en"`: language of backend messages (`tr!`, `src-tauri/locales/*.yml`) and of the AI assistant's replies.

## Auto update (`src-tauri/src/update.rs`, see `docs/auto-update-plan.md`)

### Command `update_info()` → `{ enabled, version, repo }`
`enabled` is true only in CI release builds (repo + public key embedded at compile time); self-built / dev copies never update themselves.

### Command `update_check()` → `null | { version, current, notes, date_ms }`
Reads `https://github.com/{repo}/releases/latest/download/latest.json` (30 s timeout). `null` = up to date. The found update is kept for `update_install`. Network / signature errors → `Err(string)` (localized).

### Command `update_install()`
Downloads the update found by the last `update_check`, verifies its signature, runs the NSIS installer (`/P /UPDATE /R`: passive, restart the app) and exits the app. Returns only on error (`Err`, the update stays available for a retry).

### Event `update_progress`
`{ stage: "download" | "install", got, total }` (bytes; `total` 0 when unknown), at most ~7 per second while downloading, one `install` event when the download is verified.

## Processes

### Command `set_watch({ procs })`
`procs: true` while the Processes page (or anything needing per-process rows) is mounted. When it turns
true the next `procs` event has `reset: true` with every process in `added`.

### Event `procs` (every tick while watched)
```json
{
  "t": 1764000000000,
  "reset": false,
  "added": [ { "pid": 17824, "ppid": 1234, "name": "claude.exe", "exe": "C:\\...\\claude.exe",
               "desc": "Claude", "publisher": "Anthropic", "cmd": "\"C:\\...\\claude.exe\" --type=renderer",
               "start_ms": 1763999000000, "session": 1, "elevated": false, "user": "ADMIN",
               "kind": "app" } ],
  "removed": [ 1111, 2222 ],
  "rows": [ [17824, 0.4, 1180000000, 0, 0.0, 326, 5708], ... ],
  "windows": { "17824": "Claude", "21384": "Brave" }
}
```
- `rows`: one per live process: `[pid, cpu %, private working set bytes, disk bytes/s (read+write), gpu %, threads, handles]`.
  CPU % is of the whole machine (like Task Manager, 0..100).
- `kind`: `"app"` (has a visible top-level window, or its ancestor in the same exe group does),
  `"background"`, or `"windows"` (system process: under %windir% or session 0 MS binaries).
  `kind` can change: the frontend should treat `windows` (sent every ~3 s, else `null`) as the source of truth for
  "has a window" and `added[].kind` as the initial guess.
- `exe`, `cmd`, `user` may be `""` when access is denied.

### Commands
- `proc_kill({ pids, tree })` → `[{ pid, ok, error? }]`. Immediate `TerminateProcess` (what `taskkill /F` does),
  with SeDebugPrivilege enabled, children first when `tree`. Waits up to 3 s for exit. Errors are Vietnamese strings.
- `proc_lookup({ targets })` → `[{ target, pid, name, exe, desc, mem, children, blocked? }]`: re-checks a `ket_thuc`
  proposal (target = PID or image name = every process with that name). One own sample, read-only. `blocked` for
  critical system processes, svchost.exe and the app itself.
- `proc_restart({ pid })` → `{ ok, pid?, error? }`. Reads exe + command line, kills, relaunches with the same
  command line. A non-elevated original is relaunched non-elevated (via the shell's token).
- `proc_open_location({ path })` → null (Explorer with the file selected).
- `proc_properties({ path })` → null (Windows file Properties dialog).
- `proc_set_priority({ pid, level })`, level ∈ `idle|below|normal|above|high|realtime` → `{ ok, error? }`.
- `get_icons({ paths })` → `{ "<path>": "data:image/png;base64,..." | null }`. 32×32 PNG; cached in backend.

## AI chat (3 providers: Anthropic, OpenAI, Custom OpenAI-compatible) — v3

Provider ids: `"anthropic"` (Anthropic Messages API, base `https://api.anthropic.com/v1`), `"openai"` (OpenAI Chat Completions,
base `https://api.openai.com/v1`), `"custom"` (any OpenAI-compatible Chat Completions endpoint; base URL required, key optional —
e.g. LM Studio `http://localhost:1234/v1`, Ollama `http://localhost:11434/v1`, OpenRouter, a proxy...).
Each provider keeps its own settings; only one is active.

### Commands
- `ai_get_config()` →
```json
{ "provider": "anthropic", "include_context": false, "tools": true,
  "providers": {
    "anthropic": { "model": "claude-opus-5-5", "effort": "medium", "base_url": "", "default_base_url": "https://api.anthropic.com/v1", "has_key": true },
    "openai":    { "model": "", "effort": "", "base_url": "", "default_base_url": "https://api.openai.com/v1", "has_key": false },
    "custom":    { "model": "", "effort": "", "base_url": "http://localhost:1234/v1", "default_base_url": "", "has_key": false } } }
```
  `model` may be `""` (OpenAI/custom start empty — the user picks one from `ai_list_models`). `base_url` `""` = use the default.
  `effort` is only meaningful for Anthropic (`low|medium|high|xhigh|max`); `""` for others.
- `ai_set_config({ provider?, include_context?, tools?, target?, model?, effort?, base_url?, api_key? })` → same shape as get.
  `tools` (default true) lets the assistant call the read-only agent tools (see "Agent tools").
  `provider` switches the active provider. `target` (a provider id) selects which provider's settings `model/effort/base_url/api_key`
  apply to (default: the active provider). `api_key: ""` deletes that provider's key. Keys are DPAPI-encrypted per provider in
  `%LOCALAPPDATA%\fomeg-windows-app\ai.json` (the old single-key format is migrated to the anthropic entry).
  Invalid input (bad URL, unknown provider/effort) rejects with a Vietnamese error string.
- `ai_list_models({ provider })` → `["model-a", "model-b", ...]` sorted; uses that provider's saved base URL + key
  (`GET {base}/models`). Also used as "Kiểm tra kết nối": rejects with a Vietnamese error (401 → key sai, connection refused → ...).
- `ai_send({ chat_id, text, disk?, ports?, cleanup? })` → `null` immediately; the reply streams as `ai` events. Uses the ACTIVE provider at send time.
  `disk: { focus: path | null }` attaches a `<disk_scan>` summary of the current disk scan (see "Disk usage → AI") to this
  user turn, independent of `include_context`; omit / `null` = none. The UI sends it only from the Disk page or "Hỏi trợ lý AI
  về thư mục này", and not again in the same chat unless the scan changed, a new folder is asked about, or items were deleted.
  `ports: true` attaches a `<ports>` block (see "Ports → AI"): the backend re-reads the port tables itself, so the UI never
  sends free text. The UI sends it from the Ports page: always for "Hỏi AI về các cổng", otherwise at most once per 60 s.
  Rejects if no model is chosen ("Chưa chọn model cho <provider>.") or the provider needs a key and has none.
  A chat may switch provider between turns: history is kept provider-neutral (text), and Anthropic-produced turns additionally
  keep their raw content blocks (thinking) which are replayed only to Anthropic.
- `ai_cancel({ chat_id })`, `ai_reset({ chat_id })` → null.
All AI command args are snake_case exactly as written.

### Event `ai` (unchanged)
```json
{ "chat_id": "c1", "kind": "delta", "text": "..." }
{ "chat_id": "c1", "kind": "thinking" }                 // Anthropic thinking block, or OpenAI-compatible reasoning_content
{ "chat_id": "c1", "kind": "done", "stop_reason": "end_turn", "usage": { "input_tokens": 1200, "output_tokens": 300 } }
{ "chat_id": "c1", "kind": "error", "message": "..." }
{ "chat_id": "c1", "kind": "tool", "id": "toolu_1", "name": "process_tree", "label": "cây tiến trình",
  "args": { "pid": 42 }, "phase": "start" | "done", "ok": true, "chars": 1830 }   // a read-only tool call of this turn
```
A turn with tools may stream text, then `tool` events, then more text (a `"\n\n"` delta separates rounds). `usage` sums all rounds.

## Agent tools (read-only) — `core/agent_tools.rs`, `core/mcp.rs`
Shared by the in-app assistant (tool-use loop in `ai_send`) and `fomeg-windows-app-mcp.exe` (MCP over stdio). Names: `system_overview`,
`hardware`, `metrics_history` (app only), `list_processes`, `process_report`, `process_tree`, `startup_items`, `services`,
`service_details`, `ports`, `drives`, `disk_usage` (current Disk scan, app only), `scan_folder` (measures one folder now, ≤ 12 s, tree `depth` 1–3), `folder_files`, `cleanup_report`, `app_info`.
Results are plain text ≤ 6000 chars. No tool writes, deletes, stops, ends or starts anything.
- `mcp_info()` → `{ path, exists, embedded }`. Release builds embed `fomeg-windows-app-mcp.exe` and extract it to
  `%LOCALAPPDATA%\fomeg-windows-app\fomeg-windows-app-mcp.exe` on launch (`embedded: true`); dev builds look next to the app's exe.
  The Settings page shows `claude mcp add fomeg-windows-app -- "<path>"` and a JSON `mcpServers` entry.
- Live app data for MCP (`core/agent_pipe.rs`): while the app runs it serves `\\.\pipe\fomeg-agent-<session>` (DACL =
  current user + SYSTEM, Medium label, remote clients rejected, ≤ 4 clients). One JSON per line:
  `{"op":"call","name","args"}` → `{"ok","text"}`, `{"op":"ping"}` → app info. Only Toolbox tools run there. `fomeg-windows-app-mcp`
  forwards every tool call through it when the pipe's server process is `fomeg-windows-app.exe`, else runs locally
  (retries the pipe every 5 s).
`stop_reason` for OpenAI-compatible providers is their `finish_reason` (`stop`, `length`, ...); `usage` may be null.

## Disk usage (trang Ổ cứng) — `core/du.rs`, `core/shell.rs`

One scan at a time (a new `du_start` cancels the previous one). The full folder tree lives in Rust; the UI asks only
for the children of folders it shows, so a whole drive (≈600k folders) costs ~45 MB in Rust and almost nothing in JS.
Files are NOT kept in the tree: `du_files` lists one folder's files on demand (the "[N tệp]" row).

| Command | Args | Returns |
|---|---|---|
| `du_drives` | – | `[{ root:"C:\\", letter:"C:", label, fs, kind: fixed\|removable\|network\|cdrom\|ram, total, free, ready }]` (network drives are detected locally and never queried; every drive answers within 1.5 s) |
| `du_start` | `root` (drive root or folder) | `Progress` (below); error text if the path can't be opened |
| `du_progress` | – | current `Progress` or `null` (used to restore the page) |
| `du_children` | `scan, ids[], sort: size\|alloc\|files\|dirs\|name\|mtime, desc, limit` | `[{ node: Node, path, children: Node[] (top-k by sort), more }]` |
| `du_files` | `path, limit` | `{ total, items: [{ name, size, alloc, mtime }] }` largest first |
| `du_cancel` / `du_close` | – | stop / stop and free the tree |
| `du_forget` | `scan, id` | drop a deleted folder from the tree (sizes subtracted from parents) |
| `du_forget_file` | `scan, dir, size, alloc` | same for one deleted file |
| `du_delete` | `paths[], permanent` | `true` if nothing was aborted; refuses drive roots / Windows / Program Files / ProgramData / Users / the profile root |
| `du_exists` | `path` | bool (after a native-menu command) |
| `du_open` | `path, is_dir` | folder → opens it in Explorer; file → Explorer with the file selected |
| `du_menu` | `path, is_dir, shift` | the native Explorer context menu at the cursor (plus app items "Quét thư mục này", "Hỏi trợ lý AI về thư mục này", "Sao chép đường dẫn"; "Rename" removed). `{kind:"none"}` \| `{kind:"app", id:"scan"\|"ask_ai"\|"copy_path"}` \| `{kind:"shell", verb}` |
| `du_lookup` | `paths[]` (≤ 200) | `[{ path (normalized), exists, is_dir, size, files, blocked? }]`; folder `size`/`files` come from the current scan (`null` if outside it); `blocked` = Vietnamese reason it can't be deleted (not absolute, missing, protected) |
| `du_delete_paths` | `paths[], permanent` | `[{ path, ok, error? }]`. One `SHFileOperation` for the batch, retried per item on failure; refuses relative and protected paths; deleted items are dropped from the current scan tree |
| `du_pick_folder` | – | folder path from the Windows folder picker, or `null` |

`Progress`: `{ scan, root, files, dirs, bytes, errors, elapsed_ms, done, cancelled }`, emitted as event `du` every 250 ms
while scanning and once at the end.

`Node`: `{ id, name, size, alloc, files, dirs, own_files, own_size, own_alloc, mtime (ms), done, denied?, link?, kids }`.
Totals are recursive and grow while the scan runs; `done` = this folder and everything below it is final. `own_*` = files
directly in the folder. `link` = junction/symlink (not followed, counted as 0). `denied` = couldn't be listed.

### Disk usage → AI

`Scan::ai_summary(focus)` builds the `<disk_scan>` text (≤ 4000 chars, ~6 ms on a 600k-folder C:): one header line
(root, state, totals, drive free space, unreadable folders), the 15 largest folders (expanding the largest node first,
≥ 1 % of the root), up to 15 "likely junk/cache" folders ≥ 100 MB matched by path-suffix rules (`JUNK` in `du.rs`;
installed programs, global npm packages, WinSxS/System32 and app `resources` are excluded), and, with `focus`, that
folder's 12 largest subfolders and 8 largest files. Sizes are compact (`9.1G`), dates `YYYY-MM`.

The model proposes deletions as a fenced `xoa` block, one `full path | short reason` per line (see "AI action proposals").
The review popup checks each path with `du_lookup`, offers "Xoá vĩnh viễn" (default: Recycle Bin), then calls `du_delete_paths`.

## Ports (trang Cổng) — `core/ports.rs`

| Command | Args | Returns |
|---|---|---|
| `ports_list` | `udp` | `{ uses: [PortUse], took_ms, pending }`. Listening TCP ports (+ bound UDP if `udp`), one row per (proto, port, pid), sorted by port. ~2 ms (IP Helper `GetExtendedTcpTable`/`GetExtendedUdpTable`, LISTENER + CONNECTIONS tables). `pending` = some PIDs have no name yet (the monitor was just woken; ask again in ~0.4 s) |
| `open_port` | `port` | opens `http://localhost:<port>` in the default browser via explorer.exe (not elevated); the URL is built from the number only |
| `open_tool` | `name: services\|tasks\|startup_folder` | opens services.msc / taskschd.msc / shell:startup; other names are rejected |

`PortUse`: `{ proto: "tcp"|"udp", port, addrs: ["0.0.0.0", "::"], pid, conns (established TCP), reach: "local"|"network"|"all", name, exe, cpu, mem (bytes) }`.
`name/exe/cpu/mem` come from the monitor's last sample: `ports_list` stores the PIDs (with a timestamp) and the monitor copies
just those PIDs' info each tick while the page asked within the last 6 s, so the Ports page needs no `set_watch`. Ending a
port's program uses `proc_kill`. The UI polls every 2 s only while the page is open, the window is visible and updates aren't paused.
The well-known port catalog (Vite 5173, PostgreSQL 5432…) and the "why not end this" rules (System/PID 4, svchost,
lsass/wininit/services/csrss/smss; WSL/Docker relays warn) live in `src/lib/ports.js`.

### Ports → AI

`ports::ai_summary` builds the `<ports>` block (≤ 3500 chars; ~2.5k chars for ~95 ports): one header line with counts, then
one compact line per port (`3000 tcp all node.exe(18820) 0kn 96MB`), ports open to all networks first. The model proposes
closing ports with a fenced `dong_cong` block, one `port | short reason` per line; the review popup re-reads `ports_list`,
blocks Windows-owned / free ports, and ends the chosen ports' PIDs with `proc_kill`.

## AI action proposals (shared)

Model replies may contain fenced blocks whose language is a registered action (`src/lib/aiActions.js`): `xoa` (delete
paths) and `dong_cong` (end the programs holding ports). Each line is `target | short reason`. `markdown.js` renders a card
(max 4 lines + "Xem lại…" button with `data-act` = kind, `data-idx` = action-block index in the message); `parseActionBlocks`
reads the raw text. Clicking sets `ai.review = { kind, items }` and `ActionReview.svelte` opens: it loads/validates the items
with that kind's handler in `src/lib/reviewKinds.js` (`load`, `options`, `confirmLabel`, `summary`, `run`, `after`), shows
per-item checkboxes (pre-checked unless blocked), "Chọn tất cả", the kind's extra options, then runs only the checked ones
and marks each row done / failed. The model never acts by itself, and the backend re-checks every target when acting.
To add a kind: register it in `aiActions.js` (title, button, parse), add a handler in `reviewKinds.js`, and teach the
block in `SYSTEM` (`core/ai.rs`).

## Apps (trang Ứng dụng) — `core/apps.rs`

```
apps_list() -> { apps: [App], took_ms, store_error: string|null }
App = { id, kind: 'win32'|'store', name, publisher, version, installed /* ms, 0 = ? */, est_bytes, location,
        location_guessed, icon /* exe/ico/png path for get_icons */, scope: 'machine'|'user', arch: '64-bit'|'32-bit'|'',
        can_uninstall, can_modify, msi, system, key /* registry key or package full name */, command, url }
id = 'w|m64|<key>' | 'w|m32|<key>' | 'w|u|<key>' | 's|<PackageFullName>'

apps_measure({ gen, paths }) -> ()        // biggest first, ≤ 6 threads; calling again or apps_measure_stop() cancels
event app_size { gen, path, bytes, files, done /* false = budget hit, bytes is a minimum */ }
apps_measure_stop() -> ()

apps_uninstall({ id }) -> name            // starts the app's own uninstaller (or RemovePackageAsync); one at a time
event app_uninstall { id, name, removed /* entry gone */, error: string|null, took_ms }
apps_modify({ id }) -> ()                 // ModifyPath / MsiExec /I, not tracked
apps_open({ id, what: 'folder'|'registry' }) -> ()
```

Related folders and leftovers (`core/leftovers.rs`):

```
apps_related({ id }) -> [Found]           // install folder first, then related folders, measured (≤ 5 s)
leftovers_scan() -> { items: [Found], took_ms, removed_known, windowsapps_error: string|null }
Found = { path, package: string|null, kind: 'folder'|'package'|'info', area, owner, reason, sure /* pre-check */,
          shared: string|null /* another installed app also matches */, bytes, files, newest, partial }
leftovers_remove_packages({ names }) -> [{ name, ok, error }]   // only packages NOT installed for this user; removed for all users
leftovers_forget({ name }) -> ()          // drop a removed app from the history
```

Folders are deleted with `du_delete_paths(paths, false)` (Recycle Bin). Uninstalling from the Apps page opens the shared
`ActionReview` popup with kind `go_cai`; checked folders are deleted only after `app_uninstall.removed` is true.

The backend re-reads every entry by `id`; the UI never sends paths or commands to run. AI: the `installed_apps` tool
(`name`, `sort: size|name|date`, `limit`, `include_system`, `measure`) – read-only, uninstalling is always the user's click.

## Cleanup

| Command | Args | Returns |
| --- | --- | --- |
| `cleanup_scan` | – | `{ id, items: CleanItem[], took_ms, admin }` after ≤ ~3.5 s (discovery ≤ 2.5 s + measuring ≤ 0.8 s) |
| `cleanup_apply` | `{ id, items: string[], keepRecent: bool }` | `{ results: [{ id, removed, skipped, kept_recent, bytes, error? }], items: CleanItem[], took_ms }` (`items` = the cleaned items, re-measured) |

Event `cleanup_item` → `{ scan, item: CleanItem }`: an item that was still `pending` when `cleanup_scan` returned has
finished measuring. Ignore it if `scan` is not the current scan id.

Event `cleanup_progress` (only while `cleanup_apply` runs, every 250 ms, skipped while the window is hidden; the
`measure` step is always sent) → `{ stage: 'delete'|'measure', done_files, total_files, freed_bytes, total_bytes, item,
recycle, elapsed_ms }`. `done_files` = files handled (removed + skipped + kept), counted by the delete threads with
atomics once per folder; totals come from the scan and exclude the Recycle Bin (emptied in parallel, `recycle: true`).
The UI (`CleanupProgress.svelte`, a 24 px line above the status bar on the Cleanup page only) caps the bar at 99 %
until the call returns, then shows the result for 6 s.

`CleanItem`: `{ id, group: 'temp'|'system'|'cache', name, note, paths: string[], bytes, files, recent_bytes,
recent_files, newest (ms, 0 = none), safety: 'safe'|'caution', default_on, pending, error? }`.
- Ids: fixed ones are `user_temp`, `windows_temp`, `error_reports`, `crash_dumps`, `windows_update`, `delivery_opt`,
  `shader_cache`, `inet_cache`, `recycle`. App caches are `cache:<lower-cased app name>` (e.g. `cache:google chrome`), one
  item per app with all its cache folders in `paths`.
- Items that need admin (`windows_temp`, `windows_update`, `delivery_opt`) come back with no `paths` and an `error` when
  the app is not elevated. The UI hides `cache` items under 5 MB once measured.
- `cleanup_apply` accepts only ids of the LATEST scan (≤ 30 min old) and never paths. It permanently deletes everything
  inside each path (the path itself is kept). Files that are open elsewhere, links (junction/symlink, never followed or
  removed), files without access, and (with `keepRecent`) files changed in the last 24 h are skipped silently and counted
  in `skipped` / `kept_recent`. `recycle` empties the Recycle Bin of all drives without UI. The scan stays valid; its
  cleaned items are replaced by the re-measured ones (the UI does not rescan).

### Cleanup → AI
`ai_send`'s `cleanup: true` attaches `<cleanup>` (≤ 3500 chars) built from the last scan: one line per item
`[id] name · size · files · new part · an toàn/cân nhắc · chọn sẵn` + up to two paths. The frontend sends it from the
Cleanup page when the scan (or the number of pending items) changed since the last successful send, or when "Hỏi AI" is
pressed. The model proposes cleaning in a fenced `don_dep` block, one `id | reason` per line; the shared review popup
maps ids to the current scan, offers "Giữ tệp mới" (initially the page's setting) and calls `cleanup_apply`.
