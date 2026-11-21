# Dev receiver — Screenroom casting verification

`scripts/dev-receiver/` holds a standalone DLNA/UPnP **MediaRenderer test double**.
It exists so the real Screenroom casting modules (`casting`, `cast_stream`,
`cast_session`) can be driven over real sockets against a receiver that is not
compiled into the test binary.

**It is a first-party test receiver, not a certified device.** A session against it
is evidence that the code path works end to end. It is never evidence that a
third-party renderer, a television or any hardware plays Screenroom's stream. The
F19 device gate stays closed until a real receiver answers and a film is watched
on it.

## Files

| Path | Purpose |
| --- | --- |
| `receiver.py` | The receiver: SSDP, HTTP description, SCPD documents, AVTransport and RenderingControl, media pull, ffprobe self-check, JSON status, clean shutdown. Python 3 standard library only. |
| `run-verification.sh` | Builds the probe, generates a rights-cleared test clip, runs a full session and a dropped session, and asserts every claim from the two status/evidence files. |

The probe that drives the app's own modules is
[`apps/desktop/examples/cast_lan_probe.rs`](../../apps/desktop/examples/cast_lan_probe.rs).

## Run the whole verification

```bash
export CARGO_TARGET_DIR="$PWD/../target"
export SCREENROOM_SKIP_REBUILD=1
export SCREENROOM_ALLOW_FOREGROUND=0

scripts/dev-receiver/run-verification.sh /tmp/screenroom-cast-verify
```

It prints one `PASS`/`FAIL` line per claim and exits non-zero if any fails. The
work directory keeps `receiver.log`, `receiver-status.json`, `probe.log`,
`evidence/probe-full.json` and `evidence-dropped/probe-dropped.json`; nothing is
written outside it and the generated clip is `ffmpeg` lavfi output, not user media.

## Run the receiver on its own

```bash
python3 scripts/dev-receiver/receiver.py \
  --name "Screenroom Test Receiver (first-party)" \
  --status /tmp/receiver-status.json \
  --log /tmp/receiver.log \
  --pull-dir /tmp/receiver-work
```

| Flag | Meaning |
| --- | --- |
| `--name` | Friendly name, clamped to 80 characters like a real device. |
| `--advertise` | LAN IPv4 address to publish in `LOCATION` and the control URLs. Defaults to the address the default route would use. |
| `--port` | HTTP port; `0` picks a free one. |
| `--status` | JSON status file, rewritten after every request and state change. |
| `--log` | Text log. Stream URLs are redacted before they reach it. |
| `--pull-dir` | Where the fetched media and its status file live. |
| `--no-multicast` | Bind UDP 1900 without joining 239.255.255.250 (unicast M-SEARCH only). |

Stop it with `SIGTERM` or `SIGINT`. It closes both listeners, writes
`"listening": false` with a close timestamp, and exits 0.

## What it implements

- SSDP: binds UDP 1900, joins 239.255.255.250 on the advertised interface, answers
  M-SEARCH for `urn:schemas-upnp-org:device:MediaRenderer:1`, `ssdp:all` and
  `upnp:rootdevice` with a unicast `HTTP/1.1 200 OK` whose `LOCATION` is on the LAN.
- `/desc.xml`, `/AVTransport.xml`, `/RenderingControl.xml`.
- AVTransport: `SetAVTransportURI`, `Play`, `Pause`, `Stop`, `Seek`,
  `GetTransportInfo`, `GetPositionInfo`, `GetMediaInfo`.
- RenderingControl: `GetVolume`, `SetVolume`.
- On `Play` it fetches the current URI with `Range: bytes=0-`, requires a 2xx
  answer, writes the bytes to a local file, and runs `ffprobe` over them. Its own
  JSON status carries the HTTP status, `Content-Range`, byte count and the parsed
  probe result, so a session can compare the served file with the source.
- A failed fetch is reported as SOAP fault 716 and the transport drops to
  `STOPPED`, rather than claiming to play.

## What it deliberately does not do

No library, no browsing, no `ContentDirectory`, no eventing (`SUBSCRIBE`), no
transcoding, no authentication, no discovery of other devices. It serves exactly
three documents and two control endpoints, and every other path is a 404. Those
absences are part of the point: Screenroom's streaming endpoint must be shown to
expose one file and no library, and this receiver must not be able to hide that.
