# Playtesting with an agent

A person plays; an agent watches the files the game writes and stages scenes.
Start with `powershell -ExecutionPolicy Bypass -File .\scripts\playtest.ps1`
(Kino by default; `-Map t5:zombie_pentagon` for Five, `-NoBuild` to skip building).

## Marking a problem

Press **F8** (or type `playmark <note>` in the console) when something looks wrong.
It saves `iw4l-artifacts/playtest/<session>/<NNN>/`:

- `note.txt`: the note, the map and what was queued
- `log_tail.txt`: the last 300 log lines
- a screenshot under `iw4l-artifacts/screenshots/playtest/<session>/<NNN>.png`
- a state dump `iw4l-artifacts/dumps/*-playtest_<session>_<NNN>.txt`

`marks.tsv` in the session folder lists every mark with its time and note.

## Staging scenes (the inbox)

`IW4L_CONSOLE_INBOX=<file>` makes the game run every line appended to that file
as a console command. `playtest.ps1` sets it to `iw4l-artifacts/inbox.txt`.
Lines already in the file at launch are ignored; truncating the file starts over.

```powershell
Add-Content iw4l-artifacts\inbox.txt 'set sv_cheats 1; god; give weapon/t5:ray_gun_zm'
```

Each line is logged as `inbox: > <line>`; what it prints follows in
`iw4l-artifacts/logs/latest.log`. Recipes for reaching a system are in
[`BO1-ZOMBIES.md`](BO1-ZOMBIES.md).
