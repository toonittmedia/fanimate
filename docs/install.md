# Installing and updating Fanimate

Every release on the [releases page](https://github.com/toonittmedia/fanimate/releases) has an installer for each desktop platform, a portable archive, and the browser build. Fanimate needs no account and no network connection.

## Installers

| Platform | File | Notes |
|---|---|---|
| Windows 10 and 11 (64-bit) | `Fanimate-<version>-windows-x64-setup.exe` | Installs for your user account only, so it needs no administrator rights. Start menu entry, optional desktop shortcut, uninstall from Settings › Apps |
| macOS 13 or later (Apple silicon) | `Fanimate-<version>-macos-arm64.dmg` | Drag Fanimate to Applications |
| Linux (any distribution, x86-64) | `Fanimate-<version>-linux-x86_64.AppImage` | Make it executable and run it |
| Debian, Ubuntu and derivatives | `fanimate_<version>_amd64.deb` | Installs to `/opt/fanimate`, with a menu entry |
| Flatpak | `Fanimate-<version>-linux-x86_64.flatpak` | `flatpak install --user Fanimate-*.flatpak` |
| Any (portable) | `fanimate-<version>-<platform>.zip` / `.tar.gz` | Unpack and run `fanimate`; nothing is installed |

Rhubarb (lip sync) comes with every package as a separate program. FFmpeg does not: see [More video formats with FFmpeg](#more-video-formats-with-ffmpeg) and the [video export legal note](legal/video-export.md).

### Not signed yet

Releases are not yet signed with an Apple Developer ID or a Windows code-signing certificate (planned before 1.0). Until then:

- **Windows** may show "Windows protected your PC" for the installer: choose **More info › Run anyway**.
- **macOS** says the app "cannot be opened because the developer cannot be verified": right-click Fanimate in Applications, choose **Open**, then **Open** again. This is needed once.

Each release lists SHA-256 checksums (`SHA256SUMS.txt`) to check a download against.

## More video formats with FFmpeg

Fanimate exports these with nothing else installed:

- **MP4 (H.264)** with sound, made by Windows' or macOS's own video encoder.
- **MP4 (AV1)**, **uncompressed AVI**, **animated GIF** and **animated PNG**.
- **PNG, TIFF and OpenEXR** picture sequences.

[FFmpeg](https://ffmpeg.org) is a free program that adds **H.265**, **WebM**, **MKV**, **ProRes 4444**, **ProRes 422**, **DNxHR** and **animated WebP**, and MP4 (H.264) on Linux. Fanimate finds it by itself once it is installed:

| System | How to install FFmpeg |
|---|---|
| Windows | Open Terminal (or Command Prompt) and run `winget install Gyan.FFmpeg`. Or download a build from [ffmpeg.org](https://ffmpeg.org/download.html), unzip it, and choose its `bin\ffmpeg.exe` in Fanimate (below). |
| macOS | With [Homebrew](https://brew.sh): `brew install ffmpeg` |
| Ubuntu, Debian | `sudo apt install ffmpeg` |
| Fedora | `sudo dnf install ffmpeg-free` (or `ffmpeg` from RPM Fusion for H.264 and H.265) |
| Arch | `sudo pacman -S ffmpeg` |

Then, in Fanimate, open **Edit › Preferences**. Under **Video formats** it says which FFmpeg it found and the formats it adds. If it says FFmpeg was not found:

- Choose **Look again** (needed after installing while Fanimate is open), or
- Choose **Choose FFmpeg…** and pick the `ffmpeg` program (`ffmpeg.exe` on Windows) yourself.

In **File › Export Video**, a format FFmpeg would add is marked "needs FFmpeg"; choosing it shows a **Set up FFmpeg…** button that opens the same place. The Flatpak can only use an FFmpeg inside your home folder, so choose one there.

From the command line, `fanimate-cli formats` lists every format and what makes it, and `fanimate-cli export` takes `--ffmpeg <path>`.

## Updates

Fanimate looks for a new version a few seconds after it starts, when **Check for updates automatically** is on (Edit › Preferences, or the first-run dialog). It asks GitHub for the list of releases and nothing else; no information about you or your work is sent. Turn it off to stay on one version, for example during a production. **Help › Check for Updates** checks at any time.

When there is a new version, a button appears at the right of the status bar. Nothing is downloaded until you choose **Download**, and nothing is installed until you choose **Restart and Update**:

- The download is checked against the release's signature, made by the release workflow with Fanimate's update key. A file that does not match is deleted, never run.
- Before installing, Fanimate asks you to save changes, and waits while the render queue is rendering.
- **Windows:** the new installer replaces Fanimate and starts it again. If anything fails, the installer puts the old version back.
- **macOS:** the new app is unpacked beside the old one and swapped in by renaming, so a failure leaves the old app in place.
- **AppImage:** the new file replaces the old one in a single rename.
- **Flatpak and .deb** update through your system's software tools; Fanimate tells you so instead of updating itself.
- **Portable copies** cannot replace themselves; Fanimate links to the download page instead.

**Skip this version** hides a version until a newer one appears.

## Working offline, and locking Fanimate down

Fanimate works fully without a network or an account, and sends nothing by itself. Two ways to be sure of it:

- **Work offline** (Edit › Preferences) stops update checks and, as they arrive, cloud services. Everything else works as before.
- **A policy file** sets the same for a whole computer, a school lab or a studio. It is a JSON file with any of these names (a name left out keeps its default):

  | Name | Default | Meaning |
  |---|---|---|
  | `offline` | `false` | Never use the network: no update checks, no cloud services |
  | `ai` | `true` | `false` turns off every AI tool, local ones included |
  | `cloud` | `true` | `false` turns off cloud services even when the network is allowed |

  ```json
  { "offline": true, "ai": false }
  ```

  Fanimate reads `fanimate-policy.json` beside the program (put it there to apply to everyone who uses the computer) and `policy.json` in the user's Fanimate folder (the folder with the settings). Where they disagree, the stricter one wins, and a user's own choice can add a restriction but never lift one. A policy file that cannot be read or understood is treated as the strictest policy, with a message naming the file, so a typo cannot silently allow what an administrator meant to forbid. When a policy turns **Work offline** on, Preferences shows it ticked and greyed out.

## Crash reports

If Fanimate closes unexpectedly, the next start first offers to recover your unsaved work (it is kept up to the last change), then asks whether to report the crash. **Report on GitHub…** opens a new issue in your browser with the report filled in (Fanimate's version, your system and graphics, what failed and where, and how many sessions on this computer ended in a crash), for you to read, edit and send. Fanimate never sends a report itself. Uncheck **Offer to report crashes** (also in Preferences) to stop being asked.

## Uninstalling

- **Windows:** Settings › Apps › Fanimate › Uninstall.
- **macOS:** move Fanimate from Applications to the Bin.
- **AppImage and portable copies:** delete the file or folder.
- **.deb:** `sudo apt remove fanimate`. **Flatpak:** `flatpak uninstall io.github.toonittmedia.Fanimate`.

Settings, crash recovery files and the render queue live in your user data folder (`%APPDATA%\Fanimate` on Windows, `~/Library/Application Support/Fanimate` on macOS, `~/.local/share/fanimate` on Linux); delete it to remove everything.
