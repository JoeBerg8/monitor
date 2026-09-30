---
name: monitor
description: Control the locally installed monitor activity recorder when the user asks to start, pause, inspect, diagnose, export, or uninstall it.
---

<!-- managed-by: monitor -->

# monitor

Use the installed `monitor` CLI. Run `monitor instructions` for the current
command and privacy contract.

Starting the recorder requires an explicit user request. Status checks,
diagnostics, and reading the command instructions do not start recording.
Never hide the recorder state, bypass desktop permission prompts, or claim a
capture source works unless `monitor status` or `monitor doctor` confirms it.

Screenshots can contain sensitive visible information. The recorder does not
store typed text or clipboard contents.

Prefer JSON output when another program or agent will consume the result:

- Start or resume: `monitor start --json`
- Pause: `monitor stop --json`
- Inspect: `monitor status --json`
- Diagnose: `monitor doctor --json`
- Export ordered events: `monitor export`
- Uninstall: `monitor uninstall`
