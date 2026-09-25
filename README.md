# monitor

A small, local macOS recorder that writes app/window activity, mouse clicks,
scrolls, copy/paste/cut shortcuts, and periodic screenshots to disk.

## Build and run

1. Accept the installed Xcode license: `sudo xcodebuild -license`.
2. Open `monitor.xcodeproj` and run the `monitor` scheme.
3. Grant Accessibility, Input Monitoring, and Screen Recording permissions in
   **System Settings → Privacy & Security**. Restart the app if macOS requests it.

The app is an accessory process with no window or Dock icon. Use its menu-bar
icon to start or stop recording, or to quit. A red filled icon means recording;
an orange warning icon means recording is limited by missing permissions; an
outlined gray icon means paused. The menu identifies each missing permission
and links directly to its System Settings pane. Recording starts automatically
at launch. A second app instance exits without opening the database.

### Command line

Create the stable local signing identity once, then build and install:

```sh
./scripts/setup-local-signing.sh
./scripts/build-and-install.sh
open "/Applications/monitor.app"
```

The installed copy stays at `/Applications/monitor.app` and uses the same signing identity
across rebuilds, so macOS privacy permissions remain associated with it.
The identity is trusted only for this user and is not suitable for distributing
the app to another Mac.

To build without installing:

```sh
xcodebuild -project monitor.xcodeproj -scheme monitor \
  -configuration Debug -derivedDataPath build/monitor-derived-data build

open build/monitor-derived-data/Build/Products/Debug/monitor.app
```

Run tests with:

```sh
xcodebuild -project monitor.xcodeproj -scheme monitor \
  -destination 'platform=macOS,arch=arm64' \
  -derivedDataPath build/monitor-derived-data test
```

The test scheme disables capture startup so tests cannot write to the live
database or request privacy permissions.

### Privacy permissions and signing

Accessibility also authorizes the listen-only event tap, so Input Monitoring
does not need a separate entry when Accessibility is enabled.

Screen Recording permission is tied to the app's signing identity. This project
uses the local `monitor Local Code Signing` identity created by
`scripts/setup-local-signing.sh`; no Apple account is required. Keep launching
the installed copy at `/Applications/monitor.app`. The menu's **Request
Permissions Again…** item retries the system permission prompts.

## Data

The database is stored at:

```text
~/Library/Application Support/monitor/events.sqlite
```

Screenshots are stored beneath:

```text
~/Library/Application Support/monitor/screenshots/YYYY/MM/DD/
```

On the first renamed launch, existing data under
`~/Library/Application Support/FrictionRecorder` is moved here automatically.

Inspect the timeline with:

```sh
sqlite3 "$HOME/Library/Application Support/monitor/events.sqlite" \
  'SELECT * FROM events ORDER BY timestamp, id;'
```

Screenshots run every 15 seconds while the session is active, displays are
awake, and user input occurred within the last 60 seconds. There is no automatic
retention policy.
