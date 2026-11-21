#!/usr/bin/env python3
import argparse
import http.client
import http.server
import json
import os
import re
import shutil
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import urllib.parse

ROLE = "first-party Screenroom test receiver, not a certified third-party device"

GROUP = "239.255.255.250"

SSDP_PORT = 1900

DEVICE_TYPE = "urn:schemas-upnp-org:device:MediaRenderer:1"

AV_TYPE = "urn:schemas-upnp-org:service:AVTransport:1"

RC_TYPE = "urn:schemas-upnp-org:service:RenderingControl:1"

NO_MEDIA = "NO_MEDIA_PRESENT"

STOPPED = "STOPPED"

PLAYING = "PLAYING"

PAUSED = "PAUSED_PLAYBACK"

MAX_FETCH_BYTES = 512 * 1024 * 1024

FETCH_TIMEOUT = 60.0

MAX_BODY_BYTES = 256 * 1024

SOAP_ENVELOPE_OPEN = (
    '<?xml version="1.0" encoding="utf-8"?>'
    '<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" '
    's:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body>'
)


def local_address():
    probe = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        probe.connect(("192.0.2.1", 9))
        return probe.getsockname()[0]
    finally:
        probe.close()


def redact(url):
    if not url:
        return ""
    try:
        parsed = urllib.parse.urlsplit(url)
    except ValueError:
        return "<unparsable address>"
    parts = parsed.path.split("/")
    if len(parts) > 2 and parts[1] == "stream" and parts[2]:
        parts[2] = "<redacted>"
    return urllib.parse.urlunsplit((parsed.scheme, parsed.netloc, "/".join(parts), "", ""))


def hhmmss(seconds):
    total = max(0, int(seconds))
    return f"{total // 3600:02d}:{(total % 3600) // 60:02d}:{total % 60:02d}"


def parse_hhmmss(text):
    parts = text.strip().split(":")
    try:
        values = [float(part) for part in parts]
    except ValueError:
        return None
    if len(values) == 3:
        return values[0] * 3600 + values[1] * 60 + values[2]
    if len(values) == 2:
        return values[0] * 60 + values[1]
    if len(values) == 1:
        return values[0]
    return None


def xml_escape(text):
    return (
        text.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
    )


def action_from(body):
    match = re.search(r"<u:([A-Za-z0-9]+)[\s>]", body)
    return match.group(1) if match else ""


def find_tag(body, name):
    match = re.search(rf"<{name}[^>]*>(.*?)</{name}>", body, re.DOTALL)
    return match.group(1).strip() if match else ""


def tool(name):
    found = shutil.which(name)
    if found:
        return found
    for directory in ("/opt/homebrew/bin", "/usr/local/bin"):
        candidate = os.path.join(directory, name)
        if os.access(candidate, os.X_OK):
            return candidate
    return ""


class FetchError(Exception):
    pass


class Receiver:
    def __init__(self, name, advertise, status_path, log_path, pull_dir):
        self.name = name
        self.advertise = advertise
        self.status_path = status_path
        self.log_path = log_path
        self.pull_dir = pull_dir
        self.lock = threading.RLock()
        self.state = NO_MEDIA
        self.uri = ""
        self.metadata = ""
        self.duration = 0.0
        self.base_position = 0.0
        self.playing_since = None
        self.volume = 40
        self.http_port = None
        self.listening = False
        self.closed_at = None
        self.pulled_path = ""
        self.pulled_bytes = 0
        self.pulled_status = None
        self.pulled_probe = {}
        self.pulled_content_range = ""
        self.fetch_error = ""
        self.actions = {}
        self.started = time.time()
        self._log_file = open(self.log_path, "a", encoding="utf-8")
        self._log_lock = threading.Lock()

    def log(self, message):
        stamp = time.strftime("%Y-%m-%dT%H:%M:%S", time.gmtime())
        line = f"{stamp}Z {message}\n"
        with self._log_lock:
            self._log_file.write(line)
            self._log_file.flush()
            sys.stdout.write(line)
            sys.stdout.flush()

    def note_action(self, action):
        with self.lock:
            self.actions[action] = self.actions.get(action, 0) + 1

    def position(self):
        with self.lock:
            base = self.base_position
            since = self.playing_since
        if since is None:
            return base
        advanced = base + (time.monotonic() - since)
        with self.lock:
            duration = self.duration
        return min(advanced, duration) if duration > 0 else advanced

    def snapshot(self):
        with self.lock:
            return {
                "role": ROLE,
                "name": self.name,
                "pid": os.getpid(),
                "listening": self.listening,
                "ssdp_port": SSDP_PORT,
                "http_port": self.http_port,
                "advertise": self.advertise,
                "state": self.state,
                "position_seconds": round(self.position(), 3),
                "duration_seconds": round(self.duration, 3),
                "current_uri": redact(self.uri),
                "volume": self.volume,
                "fetched": {
                    "http_status": self.pulled_status,
                    "content_range": self.pulled_content_range,
                    "bytes": self.pulled_bytes,
                    "path": self.pulled_path,
                    "ffprobe": self.pulled_probe,
                    "error": self.fetch_error,
                },
                "actions": dict(sorted(self.actions.items())),
                "started_at": self.started,
                "closed_at": self.closed_at,
            }

    def write_status(self):
        payload = self.snapshot()
        directory = os.path.dirname(self.status_path) or "."
        os.makedirs(directory, exist_ok=True)
        temporary = f"{self.status_path}.tmp"
        with open(temporary, "w", encoding="utf-8") as handle:
            json.dump(payload, handle, indent=2, sort_keys=True)
            handle.write("\n")
        os.replace(temporary, self.status_path)
        return payload

    def description(self):
        host = self.advertise
        port = self.http_port
        av_control = f"http://{host}:{port}/control/av"
        rc_control = f"http://{host}:{port}/control/rc"
        return (
            '<?xml version="1.0" encoding="utf-8"?>'
            '<root xmlns="urn:schemas-upnp-org:device-1-0">'
            "<specVersion><major>1</major><minor>0</minor></specVersion>"
            "<device>"
            f"<deviceType>{DEVICE_TYPE}</deviceType>"
            f"<friendlyName>{xml_escape(self.name)}</friendlyName>"
            "<manufacturer>Screenroom test harness</manufacturer>"
            "<modelName>scripts/dev-receiver</modelName>"
            "<UDN>uuid:screenroom-test-receiver</UDN>"
            "<serviceList>"
            "<service>"
            f"<serviceType>{AV_TYPE}</serviceType>"
            "<serviceId>urn:upnp-org:serviceId:AVTransport</serviceId>"
            f"<controlURL>{av_control}</controlURL>"
            f"<eventSubURL>http://{host}:{port}/event/av</eventSubURL>"
            f"<SCPDURL>http://{host}:{port}/AVTransport.xml</SCPDURL>"
            "</service>"
            "<service>"
            f"<serviceType>{RC_TYPE}</serviceType>"
            "<serviceId>urn:upnp-org:serviceId:RenderingControl</serviceId>"
            f"<controlURL>{rc_control}</controlURL>"
            f"<eventSubURL>http://{host}:{port}/event/rc</eventSubURL>"
            f"<SCPDURL>http://{host}:{port}/RenderingControl.xml</SCPDURL>"
            "</service>"
            "</serviceList>"
            "</device>"
            "</root>"
        )

    def scpd_av(self):
        return (
            '<?xml version="1.0" encoding="utf-8"?>'
            '<scpd xmlns="urn:schemas-upnp-org:service-1-0">'
            "<specVersion><major>1</major><minor>0</minor></specVersion>"
            "<actionList>"
            "<action><name>SetAVTransportURI</name></action>"
            "<action><name>Play</name></action>"
            "<action><name>Pause</name></action>"
            "<action><name>Stop</name></action>"
            "<action><name>Seek</name></action>"
            "<action><name>GetTransportInfo</name></action>"
            "<action><name>GetPositionInfo</name></action>"
            "<action><name>GetMediaInfo</name></action>"
            "</actionList>"
            "</scpd>"
        )

    def scpd_rc(self):
        return (
            '<?xml version="1.0" encoding="utf-8"?>'
            '<scpd xmlns="urn:schemas-upnp-org:service-1-0">'
            "<specVersion><major>1</major><minor>0</minor></specVersion>"
            "<actionList>"
            "<action><name>GetVolume</name></action>"
            "<action><name>SetVolume</name></action>"
            "</actionList>"
            "</scpd>"
        )

    def play_failure(self, description):
        with self.lock:
            self.state = STOPPED
            self.playing_since = None
            self.fetch_error = description
        self.write_status()
        self.log(f"play failed: {description}")
        return description

    def pull_media(self, uri):
        parsed = urllib.parse.urlsplit(uri)
        if parsed.scheme != "http" or not parsed.hostname:
            raise FetchError("the media address is not an http address")
        target = parsed.path or "/"
        if parsed.query:
            target = f"{target}?{parsed.query}"
        port = parsed.port or 80
        self.log(f"fetching {redact(uri)} with a range request")
        connection = http.client.HTTPConnection(parsed.hostname, port, timeout=FETCH_TIMEOUT)
        try:
            connection.request(
                "GET",
                target,
                headers={
                    "Range": "bytes=0-",
                    "User-Agent": "ScreenroomTestReceiver/1.0",
                    "Accept": "*/*",
                    "Connection": "close",
                },
            )
            response = connection.getresponse()
            status = response.status
            content_range = response.getheader("Content-Range") or ""
            if status not in (200, 206):
                raise FetchError(f"the media endpoint answered HTTP {status}")
            expected = response.getheader("Content-Length")
            os.makedirs(self.pull_dir, exist_ok=True)
            suffix = os.path.splitext(parsed.path)[1] or ".bin"
            handle = tempfile.NamedTemporaryFile(
                dir=self.pull_dir, prefix="pulled-", suffix=suffix, delete=False
            )
            written = 0
            try:
                with handle:
                    while True:
                        chunk = response.read(65536)
                        if not chunk:
                            break
                        written += len(chunk)
                        if written > MAX_FETCH_BYTES:
                            raise FetchError("the media is larger than the fetch limit")
                        handle.write(chunk)
            except FetchError:
                os.unlink(handle.name)
                raise
            except OSError as error:
                try:
                    os.unlink(handle.name)
                except OSError:
                    pass
                raise FetchError(f"the transfer broke: {error}") from error
            if expected is not None and written != int(expected):
                raise FetchError(f"expected {expected} bytes and received {written}")
            self.pulled_path = handle.name
            self.pulled_bytes = written
            self.pulled_status = status
            self.pulled_content_range = content_range
            self.log(f"fetched {written} bytes with HTTP {status} into {handle.name}")
        except FetchError:
            raise
        except (OSError, http.client.HTTPException) as error:
            raise FetchError(f"the media endpoint failed: {error}") from error
        finally:
            connection.close()
        self.probe_media(handle.name)

    def probe_media(self, path):
        binary = tool("ffprobe")
        if not binary:
            self.pulled_probe = {"error": "ffprobe was not found"}
            self.log("ffprobe was not found, so the fetched bytes are unverified")
            return
        command = [
            binary,
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
            path,
        ]
        try:
            completed = subprocess.run(
                command, capture_output=True, timeout=60, check=False
            )
        except (OSError, subprocess.SubprocessError) as error:
            self.pulled_probe = {"error": f"ffprobe could not run: {error}"}
            self.log("ffprobe could not run")
            return
        if completed.returncode != 0:
            self.pulled_probe = {
                "error": "ffprobe rejected the fetched file",
                "stderr": completed.stderr.decode("utf-8", "replace")[:400],
            }
            self.log("ffprobe rejected the fetched file")
            self.write_status()
            return
        try:
            report = json.loads(completed.stdout.decode("utf-8", "replace"))
        except ValueError:
            self.pulled_probe = {"error": "ffprobe output was not JSON"}
            return
        video = next(
            (stream for stream in report.get("streams", []) if stream.get("codec_type") == "video"),
            {},
        )
        audio = next(
            (stream for stream in report.get("streams", []) if stream.get("codec_type") == "audio"),
            {},
        )
        container = report.get("format", {})
        duration = 0.0
        try:
            duration = float(container.get("duration", 0.0))
        except (TypeError, ValueError):
            duration = 0.0
        self.pulled_probe = {
            "format_name": container.get("format_name"),
            "duration_seconds": round(duration, 3),
            "size_bytes": int(container.get("size", self.pulled_bytes) or 0),
            "video_codec": video.get("codec_name"),
            "width": video.get("width"),
            "height": video.get("height"),
            "audio_codec": audio.get("codec_name"),
        }
        if duration > 0:
            with self.lock:
                self.duration = duration
        self.log(
            "ffprobe accepted the fetched file: "
            f"{self.pulled_probe.get('format_name')} "
            f"{self.pulled_probe.get('video_codec')} "
            f"{self.pulled_probe.get('width')}x{self.pulled_probe.get('height')} "
            f"{self.pulled_probe.get('duration_seconds')}s"
        )

    def av_action(self, action, body):
        self.note_action(action)
        self.log(f"avtransport {action}")
        if action == "SetAVTransportURI":
            uri = find_tag(body, "CurrentURI")
            if not uri:
                return self.fault(402, "the receiver needs a media address")
            with self.lock:
                self.uri = uri
                self.metadata = find_tag(body, "CurrentURIMetaData")
                self.state = STOPPED
                self.base_position = 0.0
                self.playing_since = None
                self.pulled_path = ""
                self.pulled_bytes = 0
                self.pulled_status = None
                self.pulled_content_range = ""
                self.pulled_probe = {}
                self.fetch_error = ""
            self.write_status()
            return self.action_reply(action, "")
        if action == "Play":
            with self.lock:
                uri = self.uri
                current = self.state
                already = bool(self.pulled_path)
            if not uri:
                return self.fault(701, "no media has been set on this receiver")
            if current == PAUSED:
                with self.lock:
                    self.playing_since = time.monotonic()
                    self.state = PLAYING
                self.write_status()
                return self.action_reply(action, "")
            if not already:
                try:
                    self.pull_media(uri)
                except FetchError as error:
                    self.play_failure(str(error))
                    return self.fault(716, f"the receiver could not play the media: {error}")
            with self.lock:
                self.state = PLAYING
                self.base_position = 0.0
                self.playing_since = time.monotonic()
            self.write_status()
            return self.action_reply(action, "")
        if action == "Pause":
            with self.lock:
                if self.state != PLAYING:
                    return self.fault(701, "the receiver is not playing")
                self.base_position = self.position()
                self.playing_since = None
                self.state = PAUSED
            self.write_status()
            return self.action_reply(action, "")
        if action == "Stop":
            with self.lock:
                self.state = STOPPED
                self.base_position = 0.0
                self.playing_since = None
            self.write_status()
            return self.action_reply(action, "")
        if action == "Seek":
            target = parse_hhmmss(find_tag(body, "Target"))
            if target is None:
                return self.fault(711, "the seek target is not a time")
            with self.lock:
                if self.state not in (PLAYING, PAUSED):
                    return self.fault(701, "the receiver has no media to seek in")
                limit = self.duration if self.duration > 0 else target
                self.base_position = min(target, limit)
                self.playing_since = time.monotonic() if self.state == PLAYING else None
            self.write_status()
            return self.action_reply(action, "")
        if action == "GetTransportInfo":
            with self.lock:
                self.check_end(self.duration)
                state = self.state
            return self.action_reply(
                action,
                f"<CurrentTransportState>{state}</CurrentTransportState>"
                "<CurrentTransportStatus>OK</CurrentTransportStatus>"
                "<CurrentSpeed>1</CurrentSpeed>",
            )
        if action == "GetPositionInfo":
            with self.lock:
                self.check_end(self.duration)
                state = self.state
                uri = self.uri
                metadata = self.metadata
                duration = self.duration
                position = self.position()
            return self.action_reply(
                action,
                "<Track>1</Track>"
                f"<TrackDuration>{hhmmss(duration)}</TrackDuration>"
                f"<TrackMetaData>{metadata}</TrackMetaData>"
                f"<TrackURI>{xml_escape(uri)}</TrackURI>"
                f"<RelTime>{hhmmss(position)}</RelTime>"
                f"<AbsTime>{hhmmss(position)}</AbsTime>"
                f"<RelCount>{int(position)}</RelCount>"
                "<AbsCount>0</AbsCount>",
            )
        if action == "GetMediaInfo":
            with self.lock:
                uri = self.uri
                metadata = self.metadata
                duration = self.duration
                state = self.state
            tracks = 0 if state == NO_MEDIA else 1
            return self.action_reply(
                action,
                f"<NrTracks>{tracks}</NrTracks>"
                f"<MediaDuration>{hhmmss(duration)}</MediaDuration>"
                f"<CurrentURI>{xml_escape(uri)}</CurrentURI>"
                f"<CurrentURIMetaData>{metadata}</CurrentURIMetaData>"
                "<NextURI></NextURI><NextURIMetaData></NextURIMetaData>"
                "<PlayMedium>NETWORK</PlayMedium>"
                "<RecordMedium>NOT_IMPLEMENTED</RecordMedium>"
                "<WriteStatus>NOT_IMPLEMENTED</WriteStatus>",
            )
        return self.fault(401, f"this test receiver does not implement {action}")

    def check_end(self, duration):
        if self.state != PLAYING or duration <= 0 or self.playing_since is None:
            return
        if self.base_position + (time.monotonic() - self.playing_since) >= duration:
            self.state = STOPPED
            self.base_position = 0.0
            self.playing_since = None

    def rc_action(self, action, body):
        self.note_action(action)
        self.log(f"renderingcontrol {action}")
        if action == "GetVolume":
            with self.lock:
                volume = self.volume
            return self.rc_reply(action, f"<CurrentVolume>{volume}</CurrentVolume>")
        if action == "SetVolume":
            wanted = find_tag(body, "DesiredVolume")
            try:
                volume = int(float(wanted))
            except (TypeError, ValueError):
                return self.fault(402, "the receiver needs a volume value")
            with self.lock:
                self.volume = max(0, min(100, volume))
                current = self.volume
            self.write_status()
            return self.rc_reply(action, f"<CurrentVolume>{current}</CurrentVolume>")
        return self.fault(401, f"this test receiver does not implement {action}")

    def action_reply(self, action, fields):
        return (
            f"{SOAP_ENVELOPE_OPEN}<u:{action}Response xmlns:u=\"{AV_TYPE}\">"
            f"{fields}</u:{action}Response></s:Body></s:Envelope>"
        )

    def rc_reply(self, action, fields):
        return (
            f"{SOAP_ENVELOPE_OPEN}<u:{action}Response xmlns:u=\"{RC_TYPE}\">"
            f"{fields}</u:{action}Response></s:Body></s:Envelope>"
        )

    def fault(self, code, description):
        self.note_action("Fault")
        body = (
            f"{SOAP_ENVELOPE_OPEN}<s:Fault>"
            "<faultcode>s:Client</faultcode>"
            "<faultstring>UPnPError</faultstring>"
            "<detail><UPnPError xmlns=\"urn:schemas-upnp-org:control-1-0\">"
            f"<errorCode>{code}</errorCode>"
            f"<errorDescription>{xml_escape(description)}</errorDescription>"
            "</UPnPError></detail>"
            "</s:Fault></s:Body></s:Envelope>"
        )
        return body


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    receiver = None

    def log_message(self, format, *args):
        return

    def respond(self, status, content_type, body, extra=None):
        payload = body.encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(payload)))
        self.send_header("Connection", "close")
        for name, value in (extra or {}).items():
            self.send_header(name, value)
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(payload)
        self.close_connection = True

    def do_GET(self):
        path = urllib.parse.urlsplit(self.path).path
        self.receiver.log(f"http GET {path}")
        if path == "/desc.xml":
            self.respond(200, 'text/xml; charset="utf-8"', self.receiver.description())
            return
        if path == "/AVTransport.xml":
            self.respond(200, 'text/xml; charset="utf-8"', self.receiver.scpd_av())
            return
        if path == "/RenderingControl.xml":
            self.respond(200, 'text/xml; charset="utf-8"', self.receiver.scpd_rc())
            return
        self.receiver.write_status()
        self.respond(404, "text/plain; charset=utf-8", "this test receiver serves no library\n")

    def do_HEAD(self):
        self.do_GET()

    def do_POST(self):
        path = urllib.parse.urlsplit(self.path).path
        self.receiver.log(f"http POST {path}")
        length = int(self.headers.get("Content-Length") or 0)
        if length < 0 or length > MAX_BODY_BYTES:
            self.respond(413, "text/plain; charset=utf-8", "request body too large\n")
            return
        body = self.rfile.read(length).decode("utf-8", "replace") if length else ""
        header = (self.headers.get("SOAPACTION") or "").strip().strip('"')
        if path == "/control/av":
            action = header.split("#")[-1] if "#" in header else action_from(body)
            if not action:
                self.respond(400, "text/plain; charset=utf-8", "no action was named\n")
                return
            reply = self.receiver.av_action(action, body)
            status = 500 if "<s:Fault>" in reply else 200
            self.respond(status, 'text/xml; charset="utf-8"', reply, {"EXT": ""})
            return
        if path == "/control/rc":
            action = header.split("#")[-1] if "#" in header else action_from(body)
            if not action:
                self.respond(400, "text/plain; charset=utf-8", "no action was named\n")
                return
            reply = self.receiver.rc_action(action, body)
            status = 500 if "<s:Fault>" in reply else 200
            self.respond(status, 'text/xml; charset="utf-8"', reply, {"EXT": ""})
            return
        self.respond(404, "text/plain; charset=utf-8", "this test receiver serves no library\n")


class Server(http.server.ThreadingHTTPServer):
    daemon_threads = True

    allow_reuse_address = True


def ssdp_reply(receiver, advertise, port, search_target):
    location = f"http://{advertise}:{port}/desc.xml"
    return (
        "HTTP/1.1 200 OK\r\n"
        f"CACHE-CONTROL: max-age=60\r\n"
        f"EXT:\r\n"
        f"LOCATION: {location}\r\n"
        f"SERVER: Darwin/UPnP/1.1 ScreenroomTestReceiver/1.0\r\n"
        f"FRIENDLYNAME: {receiver.name}\r\n"
        f"ST: {search_target}\r\n"
        f"USN: uuid:screenroom-test-receiver::{search_target}\r\n"
        "\r\n"
    )


def ssdp_loop(receiver, advertise, stop_event, multicast_joined):
    socket_in = socket.socket(socket.AF_INET, socket.SOCK_DGRAM, socket.IPPROTO_UDP)
    socket_in.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    try:
        socket_in.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEPORT, 1)
    except OSError:
        pass
    socket_in.bind(("", SSDP_PORT))
    if multicast_joined:
        membership = struct.pack(
            "4s4s", socket.inet_aton(GROUP), socket.inet_aton(advertise)
        )
        socket_in.setsockopt(socket.IPPROTO_IP, socket.IP_ADD_MEMBERSHIP, membership)
    socket_in.settimeout(0.5)
    socket_out = socket.socket(socket.AF_INET, socket.SOCK_DGRAM, socket.IPPROTO_UDP)
    socket_out.bind((advertise, 0))
    receiver.log(
        f"ssdp listening on udp/{SSDP_PORT} multicast {GROUP} on {advertise}; "
        f"http {advertise}:{receiver.http_port}"
    )
    while not stop_event.is_set():
        try:
            data, source = socket_in.recvfrom(4096)
        except socket.timeout:
            continue
        except OSError:
            break
        text = data.decode("utf-8", "replace")
        first = text.split("\r\n", 1)[0]
        if not first.upper().startswith("M-SEARCH"):
            continue
        search_target = ""
        for line in text.split("\r\n")[1:]:
            name, _, value = line.partition(":")
            if name.strip().upper() == "ST":
                search_target = value.strip()
                break
        if search_target not in (DEVICE_TYPE, "ssdp:all", "upnp:rootdevice"):
            receiver.log(f"ssdp ignored a search for {search_target or 'nothing'}")
            continue
        answer = ssdp_reply(receiver, advertise, receiver.http_port, search_target)
        try:
            socket_out.sendto(answer.encode("utf-8"), source)
        except OSError as error:
            receiver.log(f"ssdp could not answer {source[0]}: {error}")
            continue
        receiver.log(f"ssdp answered a {search_target} search from {source[0]}")
    socket_in.close()
    socket_out.close()
    receiver.log("ssdp listener closed")


def main():
    parser = argparse.ArgumentParser(
        description=f"Screenroom {ROLE}. It advertises, plays and reports; it certifies nothing."
    )
    parser.add_argument("--name", default="Screenroom Test Receiver (first-party)")
    parser.add_argument("--advertise", default="")
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument("--status", default="")
    parser.add_argument("--log", default="")
    parser.add_argument("--pull-dir", default="")
    parser.add_argument("--no-multicast", action="store_true")
    arguments = parser.parse_args()

    advertise = arguments.advertise or local_address()
    workdir = arguments.pull_dir or tempfile.mkdtemp(prefix="screenroom-receiver-")
    status_path = arguments.status or os.path.join(workdir, "receiver-status.json")
    log_path = arguments.log or os.path.join(workdir, "receiver.log")
    os.makedirs(workdir, exist_ok=True)

    receiver = Receiver(
        name=arguments.name[:80],
        advertise=advertise,
        status_path=status_path,
        log_path=log_path,
        pull_dir=os.path.join(workdir, "pulled"),
    )
    Handler.receiver = receiver

    try:
        server = Server((advertise, arguments.port), Handler)
    except OSError as error:
        receiver.log(f"cannot open the http listener on {advertise}:{arguments.port}: {error}")
        print(json.dumps({"role": ROLE, "listening": False, "error": str(error)}))
        receiver.write_status()
        return 2
    receiver.http_port = server.server_address[1]
    receiver.listening = True
    receiver.log(
        f"{ROLE}: name {receiver.name}; http {advertise}:{receiver.http_port}; "
        f"status {status_path}"
    )
    receiver.write_status()

    stop_event = threading.Event()

    def request_stop(signum, frame):
        stop_event.set()

    signal.signal(signal.SIGINT, request_stop)
    signal.signal(signal.SIGTERM, request_stop)

    server_thread = threading.Thread(target=server.serve_forever, daemon=True)
    server_thread.start()
    discovery_thread = threading.Thread(
        target=ssdp_loop,
        args=(receiver, advertise, stop_event, not arguments.no_multicast),
        daemon=True,
    )
    discovery_thread.start()

    while not stop_event.is_set():
        stop_event.wait(0.25)

    receiver.log("shutdown requested")
    server.shutdown()
    server.server_close()
    discovery_thread.join(timeout=5)
    with receiver.lock:
        receiver.listening = False
        receiver.closed_at = time.time()
        receiver.state = STOPPED if receiver.state == PLAYING else receiver.state
        receiver.playing_since = None
    receiver.write_status()
    receiver.log("http listener closed; receiver stopped")
    receiver._log_file.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
