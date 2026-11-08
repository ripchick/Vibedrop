# How VibeDrop gets sound into voice chats (and how you hear it locally)

VibeDrop deliberately uses **only mechanisms your operating system already provides**. The
project does not include, download, install or ship any audio driver or system audio component.
That is a hard project decision (see `CHANGELOG.md`, Unreleased): an experimental
Windows capture-APO route was evaluated and removed because it would have required a signed
system package and an own virtual-microphone driver — the exact kind of system modification
VibeDrop refuses to do.

## The two outputs

VibeDrop always plays a sound on **two routes at once** (independent volumes):

* **Main route** — your headphones/speakers. This is what *you* hear. It works on every
  Windows and Linux system with a normal audio output and needs no extra software.
* **Microphone route** — a second output that voice chats take as their microphone. What
  appears "on this route'' depends on how your OS routes it:

| Windows | Linux (PipeWire, packaged cable) |
|---|---|
| A **virtual audio cable**: you play into `<cable> Input`, the chat selects `<cable> Output` as its microphone. | A **virtual audio cable provisioned by the VibeDrop package**: you play into `vibedrop_route` (→ `VibeDrop_Route`), the chat selects `VibeDrop Microphone (Virtual)` as its microphone. See the Linux section below. |

## Windows: virtual audio cable (VB-CABLE or any equivalent)

Windows cannot send an application's sound into the *microphone* by itself. A virtual audio
cable driver provides a playback device ("CABLE Input") whose sound comes out of a matching
recording device ("CABLE Output"). Route diagram:

```
VibeDrop app --> decoder/mixer --> playback: "CABLE Input"
                                        |  (virtual cable driver)
recording: "CABLE Output"  <----------------------------------- Discord / game selects this as microphone
your real microphone --> VibeDrop voice input --+ (optional: mixed into the same route)
```

1. Install a cable **yourself** if you do not have one. Free options:
   [VB-CABLE](https://vb-audio.com/Cable/) (donationware), VoiceMeeter, Virtual Audio Cable.
   VibeDrop never downloads or launches installers for you; reboot if the installer asks.
   * Actual device names differ per cable and per system. VibeDrop detects virtual playback
     devices, marks them `(virtual)` and pairs a cable's input/output sides for you.
   * If no cable is installed, VibeDrop says exactly that ("No virtual audio cable was found")
     and local playback keeps working.
2. VibeDrop → Settings → *Send VibeDrop sounds to the microphone route* → pick the cable's
   playback side (e.g. **CABLE Input**).
3. Press **Test microphone route**: VibeDrop plays a short tone and listens on the matching
   recording side so you can see a concrete "Works"/"No signal" result before having friends
   listen to your debugging.
4. Discord/your game → voice settings → input device = the cable's recording side (e.g.
   **CABLE Output**).
5. To have people hear your **voice** too, either
   * let VibeDrop mix it: enable *Mix my microphone into the microphone route* and pick your
     real microphone — the voice goes **only** onto the microphone route, never into your
     headphones; or
   * mix it outside of VibeDrop (e.g. Windows "Listen to this device" → CABLE Input). If you do
     that, keep VibeDrop's voice switch **off**, or your voice will be doubled.
6. Discord noise suppression ("Standard"/Krisp) and automatic gain can eat short soundboard
   cues; if sounds get cut, disable those two in Discord.
7. The cable's recording device is never offered to VibeDrop as *your* voice microphone — that
   would create a feedback loop; protection is built in.

Status on the reference Windows machine: the tone arrives on the cable's capture endpoint and
an independent recording client verifies its spectrum (`examples/level3_cable_loopback.rs`,
logs in `build/logs/`). Voice+tones mixture requires the final human confirmation by ear.

## Linux: PipeWire / PulseAudio

On PipeWire systems the VibeDrop **package** provisions a virtual cable with the
same two ends as VB-CABLE — no manual commands, survives reboots:

* `VibeDrop_Route` — the playback side ("CABLE Input"). A null sink created by
  `/usr/share/pipewire/pipewire.conf.d/90-vibedrop-cable.conf`; ALSA applications
  (VibeDrop) play into it via the `vibedrop_route` pcm from `/etc/asound.conf`
  (needs `pipewire-alsa`, a hard dependency of every VibeDrop package).
* `VibeDrop Microphone (Virtual)` — the recording side ("CABLE Output"). The same
  config file starts a `pw-loopback` helper (`/usr/lib/vibedrop/virtual-mic.sh`)
  that copies the sink's monitor to the capture ports of that source node.
  Voice chats simply select it as their microphone — the "Monitor of
  VibeDrop_Route_..." source also still exists, but the named microphone is the
  intended pick.
* Both cable nodes carry `node.priority.session = -1000`, so they can never be
  elected the system's default output or input (a default flip once swallowed
  all desktop audio into the cable — observed and fixed in the field).
* On PulseAudio-only systems (no PipeWire) the classic recipe is
  `pactl load-module module-null-sink` + `module-loopback`.

Implementation notes for packagers and debugging (why the config looks like
this — all three quirks were hit in the field):

* PipeWire processes `context.modules` before `context.objects`, so a
  config-level module-loopback starts before the sink exists; wireplumber then
  links its capture stream to the system default device — which once wired the
  user's real microphone into the cable. The sink is therefore created via
  `context.objects`, and the loopback runs from `context.exec` (after the
  whole configuration is loaded); the helper dies together with the pipewire
  service cgroup, so restarts never leave orphans.
* wireplumber 0.4.x resolves a capture stream's `node.target` to a sink only
  when the stream sets `stream.capture.sink = true`; without that flag the
  loopback silently records the default microphone.
* On PipeWire 1.0.x a loopback playback stream with
  `media.class = Audio/Source/Virtual` gets broken ports (a single FR-only
  input port and no capture ports at all); a plain `Audio/Source` works, and
  the description still advertises "(Virtual)".

**Honest status:** verified end to end on the reference system (Linux Mint
22.3, PipeWire 1.0.5, wireplumber 0.4.17): a test tone played into
`vibedrop_route` was recorded back from `VibeDrop Microphone (Virtual)` with a
healthy signal level. The older manual recipes (per-user `pactl`/`pw-cli`
commands) are no longer needed; on a non-standard setup check `pactl info`
and `pw-dump` first. Details and uncovered areas are tracked in the README
("Known limitations").
